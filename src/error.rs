use thiserror::Error;

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("I/O error encountered: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Executable lookup failed: {0}")]
    Which(#[from] which::Error),

    #[error("Interpreter validation failed for '{path}': {reason}")]
    ValidationFailed { path: String, reason: String },

    #[error("Process execution timed out after {seconds} seconds")]
    Timeout { seconds: u64 },
    
    #[error("Process terminated with non-zero exit code: {code:?}")]
    ExecutionFailed { code: Option<i32>, stderr: String },

    #[error("Environment configuration error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;
