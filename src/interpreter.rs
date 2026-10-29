use std::{collections::HashSet, path::{Path, PathBuf}};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument};


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterpreterMetadata {
    pub executable: PathBuf,
    pub version: String,
    pub version_info: (u32, u32, u32),
    pub prefix: PathBuf,
    pub implementation: String,
    pub platform: String
}

pub struct InterpreterScanner;

impl InterpreterScanner {
    /// scans a workspace root and system environments for valid python interpreters.
    #[instrument(skip(workspace_root))]
    pub async fn discover_all(workspace_root: &Path) -> Vec<InterpreterMetadata>{
        let mut candidates: HashSet<PathBuf> = HashSet::new();
        let mut valid_interpreters: Vec<InterpreterMetadata> = Vec::new();

        let local_envs: [&str; 3] = [".venv", "venv", "env"];
        for env_name in local_envs {
            let env_path = workspace_root.join(env_name);

            if env_path.is_dir() {
                if let Some(python_bin) = Self::get_venv_python_path(&env_path) {
                    if python_bin.is_file() {
                        candidates.insert(python_bin);
                    }
                }
            }
        }

        if let Ok(conda_interpreters) = Self::discover_conda_environments().await {
            candidates.extend(conda_interpreters);            
        }

        if let Ok(pyenv_interpreters) = Self::discover_pyenv_versions().await {
            candidates.extend(pyenv_interpreters);
        }

        for sys_bin in &["python3", "python"] {
            if let Ok(path) = which::which(sys_bin) {
                candidates.insert(path);
            }
        }

        for candidate in candidates {
            match Self::validate_interpreter(&candidate).await {
                Ok(metadata) => {
                    info!(path = ?candidate, version = %metadata.version, "Discovered valid Python interpreter");
                    valid_interpreters.push(metadata);
                }
                Err(e) => {
                    debug!(path = ?candidate, error = %e, "Skipping invalid or incompatible interpreter");
                }
            }
        }

        return valid_interpreters;
    }
}