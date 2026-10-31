use std::collections::HashMap;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{info, instrument, warn};

#[cfg(unix)]
use nix::{
    sys::signal::{killpg, Signal},
    unistd::Pid,
};

use crate::error::{EngineError, Result};

#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(unix)]
const TERMINATE_GRACE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub enum ProcessOutput {
    Stdout(String),
    Stderr(String),
}

pub struct ProcessSupervisor {
    child: Child,
    pid: u32,
}

impl ProcessSupervisor {
    #[instrument(skip(python_path, script_path, args, env_vars))]
    pub async fn spawn(
        python_path: &Path,
        script_path: &Path,
        args: &[String],
        env_vars: &HashMap<String, String>,
        working_dir: Option<&Path>,
    ) -> Result<(Self, mpsc::Receiver<ProcessOutput>)> {
        let mut cmd = Command::new(python_path);
        cmd.arg("-u")
            .arg(script_path)
            .args(args)
            .env("PYTHONIOENCODING", "utf-8")
            .envs(env_vars)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }

        #[cfg(unix)]
        cmd.process_group(0);

        #[cfg(windows)]
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);

        let mut child = cmd.spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| EngineError::Config("Failed to acquire child process ID".into()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Config("Failed to capture stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| EngineError::Config("Failed to capture stderr".into()))?;

        let (tx, rx) = mpsc::channel(1024);

        tokio::spawn(pump(stdout, tx.clone(), ProcessOutput::Stdout));
        tokio::spawn(pump(stderr, tx, ProcessOutput::Stderr));

        info!(pid, "Python process successfully spawned under supervisor");
        Ok((Self { child, pid }, rx))
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub async fn wait_with_timeout(&mut self, max_duration: Duration) -> Result<i32> {
        match timeout(max_duration, self.child.wait()).await {
            Ok(Ok(status)) => Ok(exit_code(status)),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => {
                warn!(
                    pid = self.pid,
                    "Execution timed out. Terminating process tree..."
                );
                self.terminate_tree().await?;
                Err(EngineError::Timeout {
                    seconds: max_duration.as_secs(),
                })
            }
        }
    }

    #[instrument(skip(self))]
    pub async fn terminate_tree(&mut self) -> Result<()> {
        let pid = self.pid;

        #[cfg(unix)]
        {
            let pgid = Pid::from_raw(pid as i32);
            let _ = killpg(pgid, Signal::SIGTERM);
            let _ = timeout(TERMINATE_GRACE, self.child.wait()).await;
            let _ = killpg(pgid, Signal::SIGKILL);
        }

        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .await;
        }

        let _ = self.child.wait().await;
        info!(pid, "Process tree terminated and cleaned up successfully");
        Ok(())
    }
}

impl Drop for ProcessSupervisor {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            #[cfg(unix)]
            {
                let _ = killpg(Pid::from_raw(self.pid as i32), Signal::SIGKILL);
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/T", "/PID", &self.pid.to_string()])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
            }
        }
    }
}

async fn pump<R, F>(reader: R, tx: mpsc::Sender<ProcessOutput>, wrap: F)
where
    R: AsyncRead + Unpin,
    F: Fn(String) -> ProcessOutput,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    let mut receiver_alive = true;

    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if !receiver_alive {
                    continue;
                }
                while matches!(buf.last(), Some(b'\n' | b'\r')) {
                    buf.pop();
                }
                let line = String::from_utf8_lossy(&buf).into_owned();
                if tx.send(wrap(line)).await.is_err() {
                    receiver_alive = false;
                }
            }
        }
    }
}

fn exit_code(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(-1)
}