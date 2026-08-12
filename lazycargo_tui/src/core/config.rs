use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::store::{self, StoreError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub output_max_lines: usize,
    pub command_history_limit: usize,
    pub build_history_limit: usize,
    pub search_limit: usize,
    pub network_timeout_secs: u64,
    pub cargo_info_timeout_secs: u64,
    pub target_stale_days: u64,
    pub target_top_crates: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            output_max_lines: 20_000,
            command_history_limit: 50,
            build_history_limit: 500,
            search_limit: 100,
            network_timeout_secs: 10,
            cargo_info_timeout_secs: 8,
            target_stale_days: 7,
            target_top_crates: 20,
        }
    }
}

impl AppConfig {
    pub fn load_or_create() -> Result<Self, StoreError> {
        let path = store::config_path();
        if !path.exists() {
            let config = Self::default();
            save_config_to_path(&path, &config)?;
            return Ok(config);
        }

        let text = std::fs::read_to_string(&path).map_err(|source| StoreError::Read {
            path: path.clone(),
            source,
        })?;
        let mut config =
            serde_json::from_str::<Self>(&text).map_err(|source| StoreError::Json {
                path: path.clone(),
                source,
            })?;
        config.normalize();
        Ok(config)
    }

    pub fn config_path() -> PathBuf {
        store::config_path()
    }

    pub fn network_timeout(&self) -> Duration {
        Duration::from_secs(self.network_timeout_secs.max(1))
    }

    pub fn cargo_info_timeout(&self) -> Duration {
        Duration::from_secs(self.cargo_info_timeout_secs.max(1))
    }

    fn normalize(&mut self) {
        self.output_max_lines = self.output_max_lines.max(200);
        self.command_history_limit = self.command_history_limit.max(1);
        self.build_history_limit = self.build_history_limit.max(1);
        self.search_limit = self.search_limit.clamp(1, 100);
        self.network_timeout_secs = self.network_timeout_secs.max(1);
        self.cargo_info_timeout_secs = self.cargo_info_timeout_secs.max(1);
        self.target_stale_days = self.target_stale_days.max(1);
        self.target_top_crates = self.target_top_crates.max(1);
    }
}

fn save_config_to_path(path: &Path, config: &AppConfig) -> Result<(), StoreError> {
    store::atomic_write_json(path, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_stable() {
        let config = AppConfig::default();

        assert_eq!(config.output_max_lines, 20_000);
        assert_eq!(config.search_limit, 100);
        assert_eq!(config.target_stale_days, 7);
    }

    #[test]
    fn missing_json_fields_use_defaults() {
        let config = serde_json::from_str::<AppConfig>(r#"{"search_limit": 5}"#).unwrap();

        assert_eq!(config.search_limit, 5);
        assert_eq!(
            config.output_max_lines,
            AppConfig::default().output_max_lines
        );
    }

    #[test]
    fn normalizes_unsafe_values() {
        let mut config = AppConfig {
            output_max_lines: 0,
            command_history_limit: 0,
            build_history_limit: 0,
            search_limit: 500,
            network_timeout_secs: 0,
            cargo_info_timeout_secs: 0,
            target_stale_days: 0,
            target_top_crates: 0,
        };

        config.normalize();

        assert_eq!(config.output_max_lines, 200);
        assert_eq!(config.search_limit, 100);
        assert_eq!(config.target_stale_days, 1);
        assert_eq!(config.network_timeout_secs, 1);
    }
}
