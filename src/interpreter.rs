use std::{
    collections::HashSet,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{debug, info, instrument};

use crate::error::EngineError;

type Result<T> = std::result::Result<T, EngineError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterpreterMetadata {
    pub executable: PathBuf,
    pub version: String,
    pub version_info: (u32, u32, u32),
    pub prefix: PathBuf,
    pub implementation: String,
    pub platform: String,
}

pub struct InterpreterScanner;

impl InterpreterScanner {
    #[instrument(skip(workspace_root))]
    pub async fn discover_all(workspace_root: &Path) -> Vec<InterpreterMetadata> {
        let mut candidates = CandidateSet::default();

        for env_name in [".venv", "venv", "env"] {
            let env_path = workspace_root.join(env_name);
            if env_path.is_dir() {
                if let Some(python_bin) = Self::python_in_env(&env_path) {
                    candidates.insert(python_bin);
                }
            }
        }

        match Self::discover_conda_environments().await {
            Ok(paths) => candidates.extend(paths),
            Err(e) => debug!(error = %e, "Conda discovery skipped"),
        }

        // 3. pyenv versions
        match Self::discover_pyenv_versions().await {
            Ok(paths) => candidates.extend(paths),
            Err(e) => debug!(error = %e, "pyenv discovery skipped"),
        }

        for name in ["python3", "python"] {
            if let Ok(path) = which::which(name) {
                if !Self::is_windows_store_stub(&path) {
                    candidates.insert(path);
                }
            }
        }

        let handles: Vec<_> = candidates
            .ordered
            .into_iter()
            .map(|candidate| {
                tokio::spawn(async move {
                    let result = Self::validate_interpreter(&candidate).await;
                    (candidate, result)
                })
            })
            .collect();

        let mut valid_interpreters = Vec::new();
        for handle in handles {
            match handle.await {
                Ok((candidate, Ok(metadata))) => {
                    info!(path = ?candidate, version = %metadata.version, "Discovered valid Python interpreter");
                    valid_interpreters.push(metadata);
                }
                Ok((candidate, Err(e))) => {
                    debug!(path = ?candidate, error = %e, "Skipping invalid or incompatible interpreter");
                }
                Err(join_err) => {
                    debug!(error = %join_err, "Interpreter validation task failed");
                }
            }
        }

        valid_interpreters
    }

    fn python_in_env(env_path: &Path) -> Option<PathBuf> {
        let relative: &[&str] = if cfg!(windows) {
            &["Scripts/python.exe", "python.exe"]
        } else {
            &["bin/python", "bin/python3"]
        };

        relative
            .iter()
            .map(|rel| env_path.join(rel))
            .find(|path| path.is_file())
    }

    fn is_windows_store_stub(path: &Path) -> bool {
        cfg!(windows)
            && path
                .components()
                .any(|c| c.as_os_str().eq_ignore_ascii_case("WindowsApps"))
    }

    async fn discover_conda_environments() -> Result<Vec<PathBuf>> {
        let conda = std::env::var_os("CONDA_EXE").unwrap_or_else(|| OsString::from("conda"));

        let mut cmd = Command::new(conda);
        cmd.args(["env", "list", "--json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        let output = tokio::time::timeout(Duration::from_secs(5), cmd.output())
            .await
            .map_err(|_| EngineError::Config("Conda command timed out".into()))??;

        if !output.status.success() {
            return Err(EngineError::Config("Conda command failed".into()));
        }

        #[derive(Deserialize)]
        struct CondaEnvList {
            envs: Vec<String>,
        }

        let parsed: CondaEnvList = serde_json::from_slice(&output.stdout)?;

        Ok(parsed
            .envs
            .into_iter()
            .filter_map(|env| Self::python_in_env(Path::new(&env)))
            .collect())
    }

    async fn discover_pyenv_versions() -> Result<Vec<PathBuf>> {
        let pyenv_root = match std::env::var_os("PYENV_ROOT") {
            Some(root) => PathBuf::from(root),
            None => dirs::home_dir()
                .ok_or_else(|| EngineError::Config("Home directory not found".into()))?
                .join(".pyenv"),
        };

        let versions_dir = [
            pyenv_root.join("versions"),
            pyenv_root.join("pyenv-win").join("versions"),
        ]
        .into_iter()
        .find(|dir| dir.is_dir());

        let mut paths = Vec::new();

        if let Some(versions_dir) = versions_dir {
            let mut entries = tokio::fs::read_dir(versions_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                if let Some(py_bin) = Self::python_in_env(&entry.path()) {
                    paths.push(py_bin);
                }
            }
        }

        Ok(paths)
    }

    #[instrument(skip(python_path))]
    pub async fn validate_interpreter(python_path: &Path) -> Result<InterpreterMetadata> {
        let inspection_script = r#"
import sys, json
data = {
    "executable": sys.executable,
    "version": ".".join(map(str, sys.version_info[:3])),
    "version_info": list(sys.version_info[:3]),
    "prefix": sys.prefix,
    "implementation": sys.implementation.name,
    "platform": sys.platform,
}
print(json.dumps(data))
"#;

        let mut cmd = Command::new(python_path);
        cmd.arg("-I")
            .arg("-c")
            .arg(inspection_script)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)

        let output = tokio::time::timeout(Duration::from_secs(3), cmd.output())
            .await
            .map_err(|_| EngineError::ValidationFailed {
                path: python_path.display().to_string(),
                reason: "Inspection timed out".into(),
            })??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(EngineError::ValidationFailed {
                path: python_path.display().to_string(),
                reason: format!("Exit code non-zero: {}", stderr),
            });
        }

        let raw: InterpreterMetadataRaw = serde_json::from_slice(&output.stdout)?;

        Ok(InterpreterMetadata {
            executable: PathBuf::from(raw.executable),
            version: raw.version,
            version_info: (
                raw.version_info.first().copied().unwrap_or(0),
                raw.version_info.get(1).copied().unwrap_or(0),
                raw.version_info.get(2).copied().unwrap_or(0),
            ),
            prefix: PathBuf::from(raw.prefix),
            implementation: raw.implementation,
            platform: raw.platform,
        })
    }
}

#[derive(Deserialize)]
struct InterpreterMetadataRaw {
    executable: String,
    version: String,
    version_info: Vec<u32>,
    prefix: String,
    implementation: String,
    platform: String,
}

#[derive(Default)]
struct CandidateSet {
    seen: HashSet<PathBuf>,
    ordered: Vec<PathBuf>,
}

impl CandidateSet {
    fn insert(&mut self, path: PathBuf) {
        if self.seen.insert(path.clone()) {
            self.ordered.push(path);
        }
    }

    fn extend<I: IntoIterator<Item = PathBuf>>(&mut self, iter: I) {
        for path in iter {
            self.insert(path);
        }
    }
}