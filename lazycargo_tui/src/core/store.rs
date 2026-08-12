use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("failed to read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("failed to write {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("invalid json in {path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },
}
