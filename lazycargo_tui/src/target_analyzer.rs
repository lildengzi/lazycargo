use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::util::format_bytes;

#[derive(Debug, Clone)]
pub struct DiskSnapshot {
    pub total_size: u64,
    pub total_label: String,
    pub by_profile: Vec<ProfileDiskInfo>,
    pub by_crate: Vec<CrateDiskInfo>,
    pub stale_size: u64,
    pub stale_label: String,
    pub last_updated: Instant,
}

#[derive(Debug, Clone)]
pub struct ProfileDiskInfo {
    pub profile: String,
    pub size: u64,
    pub label: String,
    pub file_count: usize,
    pub last_modified: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateDiskInfo {
    pub name: String,
    pub size: u64,
    pub label: String,
    pub is_local: bool,
    pub profile: String,
}

#[derive(Debug, Clone)]
pub struct PackageDiskInfo {
    pub name: String,
    pub source_size: String,
    pub target_cache: String,
}

impl DiskSnapshot {
    pub fn pending(package_names: &[String]) -> Self {
        Self {
            total_size: 0,
            total_label: "calculating...".to_owned(),
            by_profile: Vec::new(),
            by_crate: package_names
                .iter()
                .map(|name| CrateDiskInfo {
                    name: name.clone(),
                    size: 0,
                    label: "calculating...".to_owned(),
                    is_local: true,
                    profile: "workspace".to_owned(),
                })
                .collect(),
            stale_size: 0,
            stale_label: "calculating...".to_owned(),
            last_updated: Instant::now(),
        }
    }

    pub fn package(&self, name: &str) -> Option<PackageDiskInfo> {
        let target_size = self
            .by_crate
            .iter()
            .filter(|item| item.name == name)
            .map(|item| item.size)
            .sum::<u64>();
        (target_size > 0).then(|| PackageDiskInfo {
            name: name.to_owned(),
            source_size: "<unknown>".to_owned(),
            target_cache: format_bytes(target_size),
        })
    }
}

pub fn analyze_target(root: &Path, local_crates: &[String], stale_days: u64) -> DiskSnapshot {
    let target = root.join("target");
    let local = local_crates
        .iter()
        .map(|name| normalize_crate_name(name))
        .collect::<HashSet<_>>();
    let mut total_size = 0;
    let mut stale_size = 0;
    let mut profiles = BTreeMap::<String, ProfileAccumulator>::new();
    let mut crates = BTreeMap::<(String, String), u64>::new();
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(stale_days.max(1) * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    if !target.exists() {
        return DiskSnapshot {
            total_size: 0,
            total_label: "0 B".to_owned(),
            by_profile: Vec::new(),
            by_crate: Vec::new(),
            stale_size: 0,
            stale_label: "0 B".to_owned(),
            last_updated: Instant::now(),
        };
    }

    for entry in WalkDir::new(&target).into_iter().filter_map(Result::ok) {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }

        let path = entry.path();
        let size = metadata.len();
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let profile = profile_for_path(&target, path).unwrap_or_else(|| "other".to_owned());
        let profile_entry = profiles.entry(profile.clone()).or_default();
        profile_entry.size += size;
        profile_entry.file_count += 1;
        profile_entry.last_modified = profile_entry.last_modified.max(modified);
        total_size += size;

        if modified < cutoff && is_cleanable_artifact(path) {
            stale_size += size;
        }

        if let Some(crate_name) = crate_name_for_artifact(path) {
            *crates.entry((profile, crate_name)).or_default() += size;
        }
    }

    let mut by_profile = profiles
        .into_iter()
        .map(|(profile, info)| ProfileDiskInfo {
            profile,
            size: info.size,
            label: format_bytes(info.size),
            file_count: info.file_count,
            last_modified: DateTime::<Local>::from(info.last_modified),
        })
        .collect::<Vec<_>>();
    by_profile.sort_by_key(|profile| Reverse(profile.size));

    let mut by_crate = crates
        .into_iter()
        .map(|((profile, name), size)| {
            let normalized = normalize_crate_name(&name);
            CrateDiskInfo {
                name,
                size,
                label: format_bytes(size),
                is_local: local.contains(&normalized),
                profile,
            }
        })
        .collect::<Vec<_>>();
    by_crate.sort_by_key(|item| Reverse(item.size));

    DiskSnapshot {
        total_size,
        total_label: format_bytes(total_size),
        by_profile,
        by_crate,
        stale_size,
        stale_label: format_bytes(stale_size),
        last_updated: Instant::now(),
    }
}

pub struct CleanStaleReport {
    pub lines: Vec<String>,
    pub artifact_count: usize,
    pub total_size: u64,
}

pub fn clean_stale(
    root: &Path,
    dry_run: bool,
    stale_days: u64,
) -> Result<CleanStaleReport, String> {
    let target = root.join("target");
    if !target.exists() {
        return Ok(CleanStaleReport {
            lines: vec!["target directory does not exist".to_owned()],
            artifact_count: 0,
            total_size: 0,
        });
    }

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(stale_days.max(1) * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut cleaned = Vec::new();
    let mut artifact_count = 0;
    let mut total_size = 0;

    for entry in WalkDir::new(&target).into_iter() {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = entry.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            continue;
        }
        let path = entry.path();
        if !is_cleanable_artifact(path) {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if modified >= cutoff {
            continue;
        }
        artifact_count += 1;
        total_size += metadata.len();
        cleaned.push(format!(
            "{} {}",
            format_bytes(metadata.len()),
            path.display()
        ));
        if !dry_run {
            fs::remove_file(path).map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }

    if cleaned.is_empty() {
        cleaned.push(format!(
            "no stale .rmeta/.rlib artifacts older than {} days",
            stale_days.max(1)
        ));
    }
    Ok(CleanStaleReport {
        artifact_count,
        lines: cleaned,
        total_size,
    })
}

struct ProfileAccumulator {
    size: u64,
    file_count: usize,
    last_modified: SystemTime,
}

impl Default for ProfileAccumulator {
    fn default() -> Self {
        Self {
            size: 0,
            file_count: 0,
            last_modified: SystemTime::UNIX_EPOCH,
        }
    }
}

fn profile_for_path(target: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(target).ok()?;
    let parts = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    match parts.as_slice() {
        [profile, ..] if is_profile(profile) => Some(profile.clone()),
        [_triple, profile, ..] if is_profile(profile) => Some(profile.clone()),
        [first, ..] => Some(first.clone()),
        _ => None,
    }
}

fn is_profile(value: &str) -> bool {
    matches!(value, "debug" | "release" | "bench" | "test")
}

fn crate_name_for_artifact(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_string_lossy();
    if !matches!(extension.as_ref(), "d" | "rmeta" | "rlib") {
        return None;
    }
    let stem = path.file_stem()?.to_string_lossy();
    let stem = stem
        .strip_prefix("lib")
        .or_else(|| stem.strip_prefix("build_script_build-"))
        .unwrap_or(&stem);
    let base = stem.split('-').next().unwrap_or(stem);
    (!base.is_empty()).then(|| base.replace('_', "-"))
}

fn is_cleanable_artifact(path: &Path) -> bool {
    path.extension()
        .map(|extension| {
            let extension = extension.to_string_lossy();
            matches!(extension.as_ref(), "rmeta" | "rlib")
        })
        .unwrap_or(false)
}

fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "lazycargo-target-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn analyze_missing_target_returns_empty_snapshot() {
        let root = temp_root("missing");

        let snapshot = analyze_target(&root, &["app".to_owned()], 7);

        assert_eq!(snapshot.total_size, 0);
        assert_eq!(snapshot.stale_size, 0);
        assert!(snapshot.by_crate.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clean_missing_target_reports_no_artifacts() {
        let root = temp_root("clean-missing");

        let report = clean_stale(&root, false, 7).unwrap();

        assert_eq!(report.artifact_count, 0);
        assert_eq!(report.total_size, 0);
        assert_eq!(report.lines, ["target directory does not exist"]);

        let _ = fs::remove_dir_all(root);
    }
}
