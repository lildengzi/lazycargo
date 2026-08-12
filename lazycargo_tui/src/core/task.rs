use std::time::{Duration, Instant};

use lazycargo_search::{
    crate_info_detail, extract_crate_author, parse_crate_search_results, search_crates_registry,
    search_error_detail, search_timeout_detail, CrateInfoReport, CrateSearchResult,
};

use crate::core::process::{run_captured, split_output};
use crate::core::util::{animated_progress_bar, progress_bar};

#[derive(Debug, Clone)]
pub struct SearchJobConfig {
    pub limit: usize,
    pub network_timeout: Duration,
    pub info_timeout: Duration,
}

pub struct SearchJobResult {
    pub kind: SearchJobKind,
    pub command: String,
    pub duration: Duration,
    pub success: bool,
    pub results: Option<Vec<CrateSearchResult>>,
    pub detail: Vec<String>,
    pub message: String,
    pub status: String,
}

pub enum SearchJobKind {
    Search,
    Info {
        name: String,
        author: Option<String>,
    },
}

pub fn run_search_job(query: String, config: SearchJobConfig) -> SearchJobResult {
    let command = format!("crates.io api search {query}");
    let started = Instant::now();
    if let Ok(results) = search_crates_registry(&query, config.network_timeout, config.limit) {
        let duration = started.elapsed();
        let detail = vec![
            format!("$ {command}"),
            format!("duration: {:.2}s", duration.as_secs_f32()),
            format!("results: {}", results.len()),
            format!("progress: {}", progress_bar(1.0, 20)),
            String::new(),
            "source: https://crates.io/api/v1/crates".to_owned(),
        ];
        return SearchJobResult {
            kind: SearchJobKind::Search,
            command,
            duration,
            success: true,
            results: Some(results),
            detail,
            message: format!("searched crates: {query}"),
            status: format!("search ok {:.2}s", duration.as_secs_f32()),
        };
    }

    let limit = config.limit.clamp(1, 100).to_string();
    let command = format!("cargo search {query} --limit {limit}");
    let started = Instant::now();
    let output = run_captured(
        "cargo",
        &["search", &query, "--limit", &limit],
        config.network_timeout,
    );
    let duration = started.elapsed();

    match output {
        Ok(Some(output)) => {
            let mut detail = Vec::new();
            detail.push(format!("$ {command}"));
            detail.push(format!("exit: {}", output.status));
            detail.push(format!("duration: {:.2}s", duration.as_secs_f32()));
            detail.push(format!("progress: {}", progress_bar(1.0, 20)));
            detail.push(String::new());
            detail.extend(split_output(&output.stdout));
            detail.extend(split_output(&output.stderr));

            let success = output.status.success();
            let results = success.then(|| parse_crate_search_results(&detail, &query));
            SearchJobResult {
                kind: SearchJobKind::Search,
                command,
                duration,
                success,
                results,
                detail,
                message: format!("searched crates: {query}"),
                status: if success {
                    format!("search ok {:.2}s", duration.as_secs_f32())
                } else {
                    format!("search failed {:.2}s", duration.as_secs_f32())
                },
            }
        }
        Ok(None) => {
            let detail = search_timeout_detail(&command, duration.as_secs_f32());
            SearchJobResult {
                kind: SearchJobKind::Search,
                command,
                duration,
                success: false,
                results: None,
                detail,
                message: format!("search timed out: {query}"),
                status: "search timeout".to_owned(),
            }
        }
        Err(error) => {
            let detail = search_error_detail(&command, &error.to_string());
            SearchJobResult {
                kind: SearchJobKind::Search,
                command,
                duration,
                success: false,
                results: None,
                detail,
                message: format!("crate search failed: {query}"),
                status: "search error".to_owned(),
            }
        }
    }
}

pub fn run_info_job(result: CrateSearchResult, config: SearchJobConfig) -> SearchJobResult {
    let command = format!("cargo info {}", result.name);
    let started = Instant::now();
    let output = run_captured("cargo", &["info", &result.name], config.info_timeout);
    let duration = started.elapsed();

    match output {
        Ok(Some(output)) => {
            let mut lines = split_output(&output.stdout);
            lines.extend(split_output(&output.stderr));
            let success = output.status.success();
            let author = extract_crate_author(&lines);
            let detail = crate_info_detail(
                &result,
                &CrateInfoReport {
                    command: &command,
                    duration_secs: duration.as_secs_f32(),
                    status: Some(output.status.to_string()),
                    lines: &lines,
                    timeout: false,
                    error: None,
                },
            );
            SearchJobResult {
                kind: SearchJobKind::Info {
                    name: result.name.clone(),
                    author,
                },
                command,
                duration,
                success,
                results: None,
                detail,
                message: format!("inspected crate: {}", result.name),
                status: if success {
                    format!("info ok {:.2}s", duration.as_secs_f32())
                } else {
                    format!("info failed {:.2}s", duration.as_secs_f32())
                },
            }
        }
        Ok(None) => {
            let detail = crate_info_detail(
                &result,
                &CrateInfoReport {
                    command: &command,
                    duration_secs: duration.as_secs_f32(),
                    status: None,
                    lines: &[],
                    timeout: true,
                    error: None,
                },
            );
            SearchJobResult {
                kind: SearchJobKind::Info {
                    name: result.name.clone(),
                    author: None,
                },
                command,
                duration,
                success: false,
                results: None,
                detail,
                message: format!("cargo info timed out: {}", result.name),
                status: "info timeout".to_owned(),
            }
        }
        Err(error) => {
            let detail = crate_info_detail(
                &result,
                &CrateInfoReport {
                    command: &command,
                    duration_secs: duration.as_secs_f32(),
                    status: None,
                    lines: &[],
                    timeout: false,
                    error: Some(error.to_string()),
                },
            );
            SearchJobResult {
                kind: SearchJobKind::Info {
                    name: result.name.clone(),
                    author: None,
                },
                command,
                duration,
                success: false,
                results: None,
                detail,
                message: format!("crate info failed: {}", result.name),
                status: "info error".to_owned(),
            }
        }
    }
}

pub fn search_progress_detail(query: &str, elapsed: Duration) -> Vec<String> {
    vec![
        format!("searching crates: {query}"),
        format!("elapsed: {:.1}s", elapsed.as_secs_f32()),
        format!(
            "progress: {}",
            animated_progress_bar(elapsed.as_millis() / 160, 20)
        ),
        String::new(),
        "source: crates.io API, fallback cargo search".to_owned(),
    ]
}

pub fn info_progress_detail(name: &str, elapsed: Duration) -> Vec<String> {
    vec![
        format!("crate: {name}"),
        format!("status: running cargo info"),
        format!("elapsed: {:.1}s", elapsed.as_secs_f32()),
        format!(
            "progress: {}",
            animated_progress_bar(elapsed.as_millis() / 160, 20)
        ),
        String::new(),
        "cargo info may update registry metadata on first run".to_owned(),
    ]
}
