use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid json in {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lazycargo")
        .join("config.json")
}

pub fn history_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lazycargo")
        .join("build_history.json")
}

pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| StoreError::Write {
            path: path.to_owned(),
            source,
        })?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|source| StoreError::Json {
        path: path.to_owned(),
        source,
    })?;
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or_else(|| Path::new(".")))
        .map_err(|source| StoreError::Write {
            path: path.to_owned(),
            source,
        })?;
    tmp.write_all(text.as_bytes())
        .map_err(|source| StoreError::Write {
            path: path.to_owned(),
            source,
        })?;
    tmp.persist(path).map_err(|error| StoreError::Write {
        path: path.to_owned(),
        source: error.error,
    })?;
    Ok(())
}
