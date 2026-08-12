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

/// 定位本地构建产出的文档首页。workspace 构建 → `target/doc/index.html`；
/// 单包构建 → `target/doc/<package>/index.html`（缺失时回退根 index.html）。
pub fn local_docs_index(workspace_root: &str, package_name: Option<&str>) -> Option<PathBuf> {
    let doc_root = Path::new(workspace_root).join("target").join("doc");
    match package_name {
        Some(name) => {
            let package_index = doc_root.join(name).join("index.html");
            if package_index.is_file() {
                Some(package_index)
            } else {
                (doc_root.join("index.html").is_file()).then_some(doc_root.join("index.html"))
            }
        }
        None => {
            let index = doc_root.join("index.html");
            index.is_file().then_some(index)
        }
    }
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
    let payload: CrateResponse =
        serde_json::from_str(&body).map_err(|error| DocsError::Parse(error.to_string()))?;
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
        assert_eq!(
            docs_url("serde", "1.0.228"),
            "https://docs.rs/serde/1.0.228"
        );
    }

    #[test]
    fn local_doc_path_resolves_under_target_doc() {
        let project = ProjectInfo {
            workspace_root: std::env::temp_dir().to_string_lossy().into_owned(),
            ..ProjectInfo::default()
        };
        assert_eq!(local_doc_path(&project, "serde"), None);
    }

    #[test]
    fn local_doc_path_layout_is_under_target_doc() {
        assert_eq!(
            local_doc_path_unchecked("/workspace", "serde"),
            PathBuf::from("/workspace/target/doc/serde/index.html")
        );
    }

    #[test]
    fn local_docs_index_prefers_package_index_for_single_package() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("target").join("doc");
        std::fs::create_dir_all(doc.join("serde")).unwrap();
        std::fs::write(doc.join("serde").join("index.html"), "docs").unwrap();
        std::fs::write(doc.join("index.html"), "root").unwrap();
        let found = local_docs_index(dir.path().to_str().unwrap(), Some("serde")).unwrap();
        assert_eq!(found, doc.join("serde").join("index.html"));
    }

    #[test]
    fn local_docs_index_falls_back_to_root_index_for_single_package() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("target").join("doc");
        std::fs::create_dir_all(&doc).unwrap();
        std::fs::write(doc.join("index.html"), "root").unwrap();
        let found = local_docs_index(dir.path().to_str().unwrap(), Some("serde")).unwrap();
        assert_eq!(found, doc.join("index.html"));
    }

    #[test]
    fn local_docs_index_uses_root_index_for_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("target").join("doc");
        std::fs::create_dir_all(&doc).unwrap();
        std::fs::write(doc.join("index.html"), "root").unwrap();
        let found = local_docs_index(dir.path().to_str().unwrap(), None).unwrap();
        assert_eq!(found, doc.join("index.html"));
    }

    #[test]
    fn local_docs_index_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            local_docs_index(dir.path().to_str().unwrap(), Some("serde")),
            None
        );
    }
}
