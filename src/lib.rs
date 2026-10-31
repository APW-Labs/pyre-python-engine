pub mod error;
pub mod interpreter;
pub mod supervisor;

pub use error::{EngineError, Result};
pub use interpreter::{InterpreterMetadata, InterpreterScanner};
pub use supervisor::{ProcessOutput, ProcessSupervisor};

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::{collections::HashMap, time::Duration};

    #[tokio::test]
    async fn test_interpreter_scanner_fallback() {
        let dir = tempdir().unwrap();
        let interpreters = InterpreterScanner::discover_all(dir.path()).await;
        println!("Discovered interpreters: {:?}", interpreters);
    }

    #[tokio::test]
    async fn test_process_supervisor_streaming() {
        let python = which::which("python3")
            .or_else(|_| which::which("python"))
            .expect("Python must be installed for testing");

        let dir = tempdir().unwrap();
        let script_path = dir.path().join("test_script.py");
        
        tokio::fs::write(
            &script_path,
            "import sys\nprint('OUT:Hello APW')\nsys.stderr.write('ERR:Warning APW\\n')\nsys.exit(0)",
        )
        .await
        .unwrap();

        let (mut supervisor, mut rx) = ProcessSupervisor::spawn(
            &python,
            &script_path,
            &[],
            &HashMap::new(),
            Some(dir.path()),
        )
        .await
        .unwrap();

        let mut outputs = Vec::new();
        while let Some(output) = rx.recv().await {
            outputs.push(output);
        }

        let code = supervisor.wait_with_timeout(Duration::from_secs(5)).await.unwrap();
        assert_eq!(code, 0);

        assert!(outputs.iter().any(|o| matches!(o, ProcessOutput::Stdout(s) if s.contains("OUT:Hello APW"))));
        assert!(outputs.iter().any(|o| matches!(o, ProcessOutput::Stderr(s) if s.contains("ERR:Warning APW"))));
    }
}