use std::io;
use std::process::Command;

use serde::Deserialize;

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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub fn load() -> io::Result<Self> {
        let output = Command::new("cargo")
            .args(["metadata", "--format-version", "1"])
            .output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(io::Error::other(format!("cargo metadata failed: {error}")));
        }

        let metadata =
            serde_json::from_slice::<CargoMetadata>(&output.stdout).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("failed to parse cargo metadata: {error}"),
                )
            })?;

        let workspace_packages = metadata
            .packages
            .iter()
            .filter(|package| metadata.workspace_members.contains(&package.id))
            .map(package_info)
            .collect::<Vec<_>>();

        let Some(root_package) = workspace_packages.first() else {
            return Ok(Self::default());
        };

        let dependency_packages = metadata
            .packages
            .iter()
            .filter(|package| !metadata.workspace_members.contains(&package.id))
            .map(|package| PackageFeatureInfo {
                id: package.id.clone(),
                name: package.name.clone(),
                version: package.version.clone(),
                features: sorted_features(&package.features),
                enabled_features: resolved_features(&metadata, &package.id),
            })
            .collect::<Vec<_>>();

        Ok(Self {
            name: root_package.name.clone(),
            version: root_package.version.clone(),
            workspace_root: metadata.workspace_root,
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
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_root: String,
    workspace_members: Vec<String>,
    resolve: Option<CargoResolve>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    version: String,
    rust_version: Option<String>,
    dependencies: Vec<CargoDependency>,
    targets: Vec<CargoTarget>,
    features: std::collections::BTreeMap<String, Vec<String>>,
    manifest_path: String,
}

#[derive(Debug, Deserialize)]
struct CargoDependency {
    name: String,
    req: String,
    kind: Option<String>,
    features: Vec<String>,
    optional: bool,
    uses_default_features: bool,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
    src_path: String,
}

#[derive(Debug, Deserialize)]
struct CargoResolve {
    nodes: Vec<CargoResolveNode>,
}

#[derive(Debug, Deserialize)]
struct CargoResolveNode {
    id: String,
    features: Vec<String>,
}

fn package_info(package: &CargoPackage) -> PackageInfo {
    PackageInfo {
        name: package.name.clone(),
        version: package.version.clone(),
        manifest_path: package.manifest_path.clone(),
        rust_version: package.rust_version.clone(),
        targets: package
            .targets
            .iter()
            .map(|target| TargetInfo {
                name: target.name.clone(),
                kind: target.kind.clone(),
                src_path: target.src_path.clone(),
            })
            .collect(),
        dependencies: package
            .dependencies
            .iter()
            .map(|dependency| DependencyInfo {
                name: dependency.name.clone(),
                req: dependency.req.clone(),
                kind: match dependency.kind.as_deref() {
                    Some("dev") => DependencyKind::Dev,
                    Some("build") => DependencyKind::Build,
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

fn resolved_features(metadata: &CargoMetadata, package_id: &str) -> Vec<String> {
    let mut features = metadata
        .resolve
        .as_ref()
        .and_then(|resolve| resolve.nodes.iter().find(|node| node.id == package_id))
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
