use std::cmp::Reverse;
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

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

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.db_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let text =
            serde_json::to_string_pretty(&self.entries).map_err(|error| error.to_string())?;
        fs::write(&self.db_path, text).map_err(|error| error.to_string())
    }

    pub fn add_entry(&mut self, entry: BuildEntry) -> Result<(), String> {
        self.entries.insert(0, entry);
        self.entries.truncate(500);
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

fn history_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lazycargo")
        .join("build_history.json")
}
