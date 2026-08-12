use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

use crate::core::project::ProjectInfo;

#[derive(Debug, Error)]
pub enum DocsError {
    #[error("docs.rs request failed: {0}")]
    Request(String),
    #[error("docs.rs returned HTTP {0}")]
    Http(String),
    #[error("invalid crates.io response: {0}")]
    Parse(String),
    #[error("no documentation found for {0}")]
    NotFound(String),
}

pub fn docs_url(name: &str, version: &str) -> String {
    format!("https://docs.rs/{name}/{version}")
}

pub fn docs_root_url(name: &str) -> String {
    format!("https://docs.rs/{name}")
}

fn local_doc_path_unchecked(workspace_root: &str, name: &str) -> PathBuf {
    Path::new(workspace_root)
        .join("target")
        .join("doc")
        .join(name)
        .join("index.html")
}

pub fn local_doc_path(project: &ProjectInfo, name: &str) -> Option<PathBuf> {
    let path = local_doc_path_unchecked(&project.workspace_root, name);
    path.is_file().then_some(path)
}

fn get(url: &str, timeout: Duration) -> Result<String, DocsError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent("lazycargo (https://github.com/lildengzi/lazycargo)")
        .build()
        .map_err(|error| DocsError::Request(error.to_string()))?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| DocsError::Request(error.to_string()))?;
    if !response.status().is_success() {
        return Err(DocsError::Http(response.status().to_string()));
    }
    response
        .text()
        .map_err(|error| DocsError::Request(error.to_string()))
}

pub fn fetch_readme(name: &str, version: &str, timeout: Duration) -> Result<String, DocsError> {
    get(
        &format!("https://docs.rs/crate/{name}/{version}/source/README.md"),
        timeout,
    )
}

pub fn fetch_description(name: &str, timeout: Duration) -> Result<String, DocsError> {
    let body = get(&format!("https://crates.io/api/v1/crates/{name}"), timeout)?;
    #[derive(serde::Deserialize)]
    struct CrateResponse {
        version: Version,
    }
    #[derive(serde::Deserialize)]
    struct Version {
        description: Option<String>,
    }
    let payload: CrateResponse = serde_json::from_str(&body)
        .map_err(|error| DocsError::Parse(error.to_string()))?;
    payload
        .version
        .description
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| DocsError::NotFound(name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_url_combines_name_and_version() {
        assert_eq!(docs_url("serde", "1.0.228"), "https://docs.rs/serde/1.0.228");
    }

    #[test]
    fn local_doc_path_resolves_under_target_doc() {
        let mut project = ProjectInfo::default();
        project.workspace_root = std::env::temp_dir().to_string_lossy().into_owned();
        assert_eq!(local_doc_path(&project, "serde"), None);
    }

    #[test]
    fn local_doc_path_layout_is_under_target_doc() {
        assert_eq!(
            local_doc_path_unchecked("/workspace", "serde"),
            PathBuf::from("/workspace/target/doc/serde/index.html")
        );
    }
}
