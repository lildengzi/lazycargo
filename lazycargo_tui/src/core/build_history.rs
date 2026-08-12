use std::cmp::Reverse;
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::core::store::StoreError;

#[derive(Debug, Clone)]
pub struct BuildHistory {
    pub entries: Vec<BuildEntry>,
    pub db_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BuildEntry {
    pub timestamp: DateTime<Local>,
    pub command: String,
    pub package: Option<String>,
    pub duration_ms: u64,
    pub success: bool,
    pub target_triple: String,
    pub rustc_version: String,
    pub crate_timings: Vec<CrateTiming>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CrateTiming {
    pub name: String,
    pub duration_ms: u64,
    pub is_build: bool,
}

#[derive(Debug, Clone)]
pub struct CommandStats {
    pub command: String,
    pub count: usize,
    pub average_ms: u64,
}

impl BuildHistory {
    pub fn load() -> Self {
        let db_path = history_path();
        let entries = fs::read_to_string(&db_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<BuildEntry>>(&text).ok())
            .unwrap_or_default();
        Self { entries, db_path }
    }

    pub fn save(&self) -> Result<(), StoreError> {
        crate::core::store::atomic_write_json(&self.db_path, &self.entries)
    }

    pub fn add_entry(&mut self, entry: BuildEntry, limit: usize) -> Result<(), StoreError> {
        self.entries.insert(0, entry);
        self.entries.truncate(limit.max(1));
        self.save()
    }

    pub fn daily_stats(&self, days: u32) -> Vec<CommandStats> {
        let cutoff = Local::now() - chrono::Duration::days(days as i64);
        let mut values = std::collections::BTreeMap::<String, Vec<u64>>::new();
        for entry in self
            .entries
            .iter()
            .filter(|entry| entry.timestamp >= cutoff)
        {
            let command = command_kind(&entry.command).to_owned();
            values.entry(command).or_default().push(entry.duration_ms);
        }
        values
            .into_iter()
            .map(|(command, values)| {
                let count = values.len();
                let total = values.into_iter().sum::<u64>();
                CommandStats {
                    command,
                    count,
                    average_ms: total / count.max(1) as u64,
                }
            })
            .collect()
    }

    pub fn slowest_crates(&self, n: usize) -> Vec<CrateTiming> {
        let today = Local::now().date_naive();
        let mut values = std::collections::BTreeMap::<String, u64>::new();
        for entry in self
            .entries
            .iter()
            .filter(|entry| entry.timestamp.date_naive() == today)
        {
            for timing in &entry.crate_timings {
                *values.entry(timing.name.clone()).or_default() += timing.duration_ms;
            }
        }
        let mut timings = values
            .into_iter()
            .map(|(name, duration_ms)| CrateTiming {
                name,
                duration_ms,
                is_build: true,
            })
            .collect::<Vec<_>>();
        timings.sort_by_key(|timing| Reverse(timing.duration_ms));
        timings.truncate(n);
        timings
    }
}

pub fn command_kind(command: &str) -> &str {
    if command.contains(" cargo check") || command.starts_with("cargo check") {
        "check"
    } else if command.contains(" cargo build") || command.starts_with("cargo build") {
        "build"
    } else if command.contains(" cargo test") || command.starts_with("cargo test") {
        "test"
    } else {
        "other"
    }
}

pub fn is_recordable_command(command: &str) -> bool {
    matches!(command_kind(command), "check" | "build" | "test")
}

pub fn format_duration_ms(duration_ms: u64) -> String {
    if duration_ms >= 1_000 {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    } else {
        format!("{duration_ms}ms")
    }
}

/// 读取最新 `cargo build --timings` 输出的 crate 耗时（按耗时降序，截断 50 条）。
pub fn latest_crate_timings(root: &std::path::Path) -> Vec<CrateTiming> {
    let timings_dir = root.join("target").join("cargo-timings");
    let Ok(entries) = fs::read_dir(timings_dir) else {
        return Vec::new();
    };
    let latest = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path);
    let Some(path) = latest else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let mut timings = Vec::new();
    collect_crate_timings(&value, &mut timings);
    timings.sort_by_key(|timing| Reverse(timing.duration_ms));
    timings.truncate(50);
    timings
}

fn collect_crate_timings(value: &serde_json::Value, timings: &mut Vec<CrateTiming>) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                collect_crate_timings(item, timings);
            }
        }
        serde_json::Value::Object(map) => {
            let name = map
                .get("name")
                .or_else(|| map.get("crate"))
                .or_else(|| map.get("target"))
                .and_then(serde_json::Value::as_str);
            let duration = map
                .get("duration_ms")
                .and_then(serde_json::Value::as_u64)
                .or_else(|| {
                    map.get("duration")
                        .and_then(serde_json::Value::as_f64)
                        .map(|value| (value * 1000.0) as u64)
                });
            if let (Some(name), Some(duration_ms)) = (name, duration) {
                timings.push(CrateTiming {
                    name: name.to_owned(),
                    duration_ms,
                    is_build: true,
                });
            }
            for value in map.values() {
                collect_crate_timings(value, timings);
            }
        }
        _ => {}
    }
}

fn history_path() -> PathBuf {
    crate::core::store::history_path()
}
