use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::process::Command;

#[derive(Debug, Error)]
pub enum VenvError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Python execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Invalid path: {0}")]
    InvalidPath(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VenvMetadata {
    pub venv_path: PathBuf,
    pub python_executable: PathBuf,
}

pub struct VenvManager;

impl VenvManager {
    pub async fn create_venv(
        project_root: &Path,
        venv_name: &str,
        python_executable: Option<&str>,
    ) -> Result<VenvMetadata, VenvError> {
        let venv_path = project_root.join(venv_name);
        let py_bin = python_executable.unwrap_or("python3");

        let status = Command::new(py_bin)
            .arg("-m")
            .arg("venv")
            .arg(&venv_path)
            .status()
            .await?;

        if !status.success() {
            return Err(VenvError::ExecutionFailed(format!(
                "Failed to create venv with exit code: {:?}",
                status.code()
            )));
        }

        let executable_path = Self::get_venv_python_path(&venv_path);

        Ok(VenvMetadata {
            venv_path,
            python_executable: executable_path,
        })
    }

    pub fn get_venv_python_path(venv_path: &Path) -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            venv_path.join("Scripts").join("python.exe")
        }
        #[cfg(not(target_os = "windows"))]
        {
            venv_path.join("bin").join("python")
        }
    }
}
