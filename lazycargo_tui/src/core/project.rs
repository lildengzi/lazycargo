use std::fmt;
use std::process::Command;

use cargo_metadata::{DependencyKind as CargoDependencyKind, MetadataCommand, Package, Resolve};

#[derive(Debug)]
pub struct MetadataLoadError(String);

impl fmt::Display for MetadataLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MetadataLoadError {}

#[derive(Debug, Clone, Default)]
pub struct ProjectInfo {
    pub name: String,
    pub version: String,
    pub workspace_root: String,
    pub manifest_path: String,
    pub packages: Vec<String>,
    pub targets: Vec<TargetInfo>,
    pub dependencies: Vec<DependencyInfo>,
    pub features: Vec<String>,
    pub workspace_packages: Vec<PackageInfo>,
    pub dependency_packages: Vec<PackageFeatureInfo>,
    pub rustc_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub manifest_path: String,
    pub rust_version: Option<String>,
    pub targets: Vec<TargetInfo>,
    pub dependencies: Vec<DependencyInfo>,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetInfo {
    pub name: String,
    pub kind: Vec<String>,
    pub src_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyInfo {
    pub name: String,
    pub req: String,
    pub kind: DependencyKind,
    pub features: Vec<String>,
    pub optional: bool,
    pub uses_default_features: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFeatureInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub features: Vec<String>,
    pub enabled_features: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Normal,
    Dev,
    Build,
}

impl DependencyKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Dev => "dev",
            Self::Build => "build",
        }
    }
}

impl ProjectInfo {
    pub fn load() -> Result<Self, MetadataLoadError> {
        let metadata = MetadataCommand::new()
            .no_deps()
            .exec()
            .map_err(|error| MetadataLoadError(format!("cargo metadata failed: {error}")))?;

        if metadata.workspace_members.is_empty() {
            return Err(MetadataLoadError("no workspace members".to_owned()));
        }
        let workspace_member_ids = metadata
            .workspace_members
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        let workspace_packages = metadata
            .packages
            .iter()
            .filter(|package| workspace_member_ids.contains(&package.id.to_string()))
            .map(package_info)
            .collect::<Vec<_>>();

        let root_package = workspace_packages
            .first()
            .ok_or_else(|| MetadataLoadError("no workspace packages".to_owned()))?;

        let dependency_packages = metadata
            .packages
            .iter()
            .filter(|package| !workspace_member_ids.contains(&package.id.to_string()))
            .map(|package| PackageFeatureInfo {
                id: package.id.to_string(),
                name: package.name.clone(),
                version: package.version.to_string(),
                features: sorted_features(&package.features),
                enabled_features: enabled_features_for(metadata.resolve.as_ref(), &package.id),
            })
            .collect::<Vec<_>>();

        Ok(Self {
            name: root_package.name.clone(),
            version: root_package.version.clone(),
            workspace_root: metadata.workspace_root.to_string(),
            manifest_path: root_package.manifest_path.clone(),
            packages: workspace_packages
                .iter()
                .map(|package| package.name.clone())
                .collect(),
            targets: root_package.targets.clone(),
            dependencies: root_package.dependencies.clone(),
            features: root_package.features.clone(),
            workspace_packages,
            dependency_packages,
            rustc_version: rustc_version(),
        })
    }

    /// 当前目录无 Cargo.toml 时的降级项目信息（limited mode）。
    pub fn fallback() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let name = cwd
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("workspace")
            .to_owned();
        Self {
            name,
            version: "<no Cargo.toml>".to_owned(),
            workspace_root: cwd.to_string_lossy().into_owned(),
            manifest_path: cwd.join("Cargo.toml").to_string_lossy().into_owned(),
            rustc_version: "<unknown>".to_owned(),
            ..Self::default()
        }
    }
}

fn package_info(package: &Package) -> PackageInfo {
    PackageInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        manifest_path: package.manifest_path.to_string(),
        rust_version: package
            .rust_version
            .as_ref()
            .map(std::string::ToString::to_string),
        targets: package
            .targets
            .iter()
            .map(|target| TargetInfo {
                name: target.name.clone(),
                kind: target.kind.iter().map(ToString::to_string).collect(),
                src_path: target.src_path.to_string(),
            })
            .collect(),
        dependencies: package
            .dependencies
            .iter()
            .map(|dependency| DependencyInfo {
                name: dependency.name.clone(),
                req: dependency.req.to_string(),
                kind: match dependency.kind {
                    CargoDependencyKind::Development => DependencyKind::Dev,
                    CargoDependencyKind::Build => DependencyKind::Build,
                    _ => DependencyKind::Normal,
                },
                features: dependency.features.clone(),
                optional: dependency.optional,
                uses_default_features: dependency.uses_default_features,
            })
            .collect(),
        features: sorted_features(&package.features),
    }
}

fn enabled_features_for(
    resolve: Option<&Resolve>,
    package_id: &cargo_metadata::PackageId,
) -> Vec<String> {
    let mut features = resolve
        .and_then(|resolve| resolve.nodes.iter().find(|node| node.id == *package_id))
        .map(|node| node.features.clone())
        .unwrap_or_default();
    features.sort();
    features.dedup();
    features
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "<unknown>".to_owned())
}

fn sorted_features(features: &std::collections::BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut features = features.keys().cloned().collect::<Vec<_>>();
    features.sort();
    features
}
