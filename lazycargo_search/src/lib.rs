use std::time::Duration;

use serde::Deserialize;

/// A single row returned by `cargo search`.
#[derive(Debug, Clone)]
pub struct CrateSearchResult {
    pub name: String,
    pub author: Option<String>,
    pub version: String,
    pub description: String,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub repository: Option<String>,
    pub downloads: Option<u64>,
    pub recent_downloads: Option<u64>,
    pub updated_at: Option<String>,
}

#[derive(Debug)]
pub struct CrateSearchError(String);

impl std::fmt::Display for CrateSearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CrateSearchError {}

/// Link slots exposed by the search detail page.
#[derive(Debug, Clone, Copy)]
pub enum SearchLinkTarget {
    Crates,
    Docs,
    Repository,
}

/// Raw `cargo info` execution data used to enrich a selected crate.
pub struct CrateInfoReport<'a> {
    pub command: &'a str,
    pub duration_secs: f32,
    pub status: Option<String>,
    pub lines: &'a [String],
    pub timeout: bool,
    pub error: Option<String>,
}

/// State and pure helpers for the crates.io search page.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    pub expanded: bool,
    pub selected: usize,
    pub results: Vec<CrateSearchResult>,
    empty_detail: Vec<String>,
    info_name: Option<String>,
    info_detail: Vec<String>,
}

impl SearchState {
    /// Drop cached `cargo info` output for the current result set.
    pub fn clear_info(&mut self) {
        self.info_name = None;
        self.info_detail.clear();
    }

    /// Replace the visible search results and reset selection state.
    pub fn set_results(&mut self, results: Vec<CrateSearchResult>) {
        self.results = results;
        self.selected = 0;
        self.empty_detail = if self.results.is_empty() {
            vec![
                "no matching crates".to_owned(),
                "results are filtered by package name or description".to_owned(),
            ]
        } else {
            Vec::new()
        };
        self.clear_info();
    }

    /// Show a non-result detail message such as search failure or timeout.
    pub fn set_empty_detail(&mut self, detail: Vec<String>) {
        self.results.clear();
        self.selected = 0;
        self.empty_detail = detail;
        self.clear_info();
    }

    /// Return the currently selected search result, if any.
    pub fn selected_result(&self) -> Option<&CrateSearchResult> {
        self.results.get(self.selected)
    }

    /// Format the left-list rows. The TUI adds focus and selection styling.
    pub fn result_items(&self) -> Vec<String> {
        if self.results.is_empty() {
            return vec!["no results yet".to_owned()];
        }

        self.results
            .iter()
            .map(|result| {
                format!(
                    "{:<24} {:<10} {:>8} {}",
                    result.name,
                    result.version,
                    result
                        .downloads
                        .map(format_count)
                        .unwrap_or_else(|| "-".to_owned()),
                    result.source_label()
                )
            })
            .collect()
    }

    /// Return a short command-like summary for the status line.
    pub fn selected_summary(&self) -> String {
        if self.results.is_empty() {
            return "cargo search <query> --limit 100".to_owned();
        }

        self.selected_result()
            .map(|result| format!("cargo add {}", result.name))
            .unwrap_or_else(|| "no crate selected".to_owned())
    }

    /// Return the right-pane detail for the selected crate.
    pub fn selected_detail(&self) -> Vec<String> {
        let Some(result) = self.selected_result() else {
            if self.empty_detail.is_empty() {
                return vec![
                    "enter or s: search crates.io".to_owned(),
                    "select a result, then press a to preview cargo add".to_owned(),
                ];
            }

            return self.empty_detail.clone();
        };

        if self.info_name.as_deref() == Some(result.name.as_str()) {
            return self.info_detail.clone();
        }

        base_search_detail(result)
    }

    /// Cache expanded `cargo info` output for a crate.
    pub fn set_info_detail(&mut self, name: String, detail: Vec<String>) {
        self.info_name = Some(name);
        self.info_detail = detail;
    }

    /// Fill the author field for the currently selected result after `cargo info`.
    pub fn set_selected_author(&mut self, author: String) {
        if let Some(result) = self.results.get_mut(self.selected) {
            result.author = Some(author);
        }
    }

    /// Return the best crates.io URL for the selected crate.
    pub fn crates_url(&self) -> Option<String> {
        let result = self.selected_result()?;
        self.info_url_for(result, "crates.io:")
            .or_else(|| Some(format!("https://crates.io/crates/{}", result.name)))
    }

    /// Return the best docs URL for the selected crate.
    pub fn docs_url(&self) -> Option<String> {
        let result = self.selected_result()?;
        self.info_url_for(result, "documentation:")
            .or_else(|| result.documentation.clone())
            .or_else(|| Some(format!("https://docs.rs/{}", result.name)))
    }

    /// Return the repository URL discovered from cached `cargo info` output.
    pub fn repository_url(&self) -> Option<String> {
        let result = self.selected_result()?;
        self.info_url_for(result, "repository:")
            .or_else(|| result.repository.clone())
    }

    /// Return the URL for a requested search detail link target.
    pub fn url_for(&self, target: SearchLinkTarget) -> Option<String> {
        match target {
            SearchLinkTarget::Crates => self.crates_url(),
            SearchLinkTarget::Docs => self.docs_url(),
            SearchLinkTarget::Repository => self.repository_url(),
        }
    }

    /// Human-readable message when a link target cannot be resolved.
    pub fn unavailable_message(target: SearchLinkTarget) -> &'static str {
        match target {
            SearchLinkTarget::Repository => {
                "repository link unavailable; press enter to run cargo info"
            }
            SearchLinkTarget::Crates | SearchLinkTarget::Docs => "no selected crate link",
        }
    }

    fn info_url_for(&self, result: &CrateSearchResult, prefix: &str) -> Option<String> {
        if self.info_name.as_deref() != Some(result.name.as_str()) {
            return None;
        }

        self.info_detail
            .iter()
            .find_map(|line| line.trim().strip_prefix(prefix).map(str::trim))
            .filter(|url| !url.is_empty())
            .map(str::to_owned)
    }
}

/// Build the default detail panel for a search result.
pub fn base_search_detail(result: &CrateSearchResult) -> Vec<String> {
    let mut lines = vec![
        format!("crate: {}", result.name),
        format!("version: {}", result.version),
        format!("description: {}", result.description),
        String::new(),
    ];
    if let Some(downloads) = result.downloads {
        lines.push(format!("downloads: {}", format_count(downloads)));
    }
    if let Some(downloads) = result.recent_downloads {
        lines.push(format!("recent downloads: {}", format_count(downloads)));
    }
    if let Some(updated_at) = &result.updated_at {
        lines.push(format!("updated: {updated_at}"));
    }
    if result.downloads.is_some()
        || result.recent_downloads.is_some()
        || result.updated_at.is_some()
    {
        lines.push(String::new());
    }
    if let Some(author) = &result.author {
        lines.push(format!("author: {author}"));
        lines.push(String::new());
    }
    lines.push("Links".to_owned());
    lines.push(format!(
        "  crates.io: https://crates.io/crates/{}",
        result.name
    ));
    let docs_url = result
        .documentation
        .clone()
        .unwrap_or_else(|| default_docs_url(&result.name));
    lines.push(format!("  docs.rs: {docs_url}"));
    if let Some(homepage) = &result.homepage {
        lines.push(format!("  homepage: {homepage}"));
    }
    if let Some(repository) = &result.repository {
        lines.push(format!("  repository: {repository}"));
    } else {
        lines.push("  repository: <not declared by crates.io>".to_owned());
    }
    lines.extend([
        String::new(),
        "Actions".to_owned(),
        "  enter: inspect with cargo info".to_owned(),
        "  a: preview cargo add for selected crate".to_owned(),
        "  o: open crates.io".to_owned(),
        "  d: open docs".to_owned(),
        "  g: open repository".to_owned(),
        "  y: copy this detail".to_owned(),
        "  s: search again".to_owned(),
    ]);
    lines
}

/// Merge the default result detail with parsed `cargo info` metadata.
pub fn crate_info_detail(result: &CrateSearchResult, report: &CrateInfoReport<'_>) -> Vec<String> {
    let mut result = result.clone();
    if result.author.is_none() {
        result.author = extract_crate_author(report.lines);
    }
    if let Some(homepage) = extract_prefixed_value(report.lines, "homepage:") {
        result.homepage = Some(homepage);
    }
    if let Some(documentation) = extract_prefixed_value(report.lines, "documentation:") {
        result.documentation = Some(documentation);
    }
    if let Some(repository) = extract_prefixed_value(report.lines, "repository:") {
        result.repository = Some(repository);
    }

    let mut detail = base_search_detail(&result);
    detail.push(String::new());
    detail.push(format!("$ {}", report.command));
    detail.push(format!("duration: {:.2}s", report.duration_secs));

    if let Some(status) = &report.status {
        detail.push(format!("exit: {status}"));
        detail.push(String::new());
        detail.extend(
            extract_crate_info_lines(report.lines)
                .into_iter()
                .filter(|line| !is_link_or_author_line(line)),
        );
    } else if report.timeout {
        detail.push("cargo info timed out after 8s".to_owned());
        detail.push(
            "likely cause: crates.io registry update, network, or proxy is unavailable".to_owned(),
        );
        detail.push("try again after fixing Cargo network/proxy settings".to_owned());
    } else if let Some(error) = &report.error {
        detail.push(format!("failed to run cargo info: {error}"));
    }

    detail
}

/// Build a detail message for a timed-out crate search.
pub fn search_timeout_detail(command: &str, duration_secs: f32) -> Vec<String> {
    vec![
        format!("$ {command}"),
        format!("duration: {duration_secs:.2}s"),
        String::new(),
        "cargo search timed out after 10s".to_owned(),
        "likely cause: crates.io, network, or proxy is unavailable".to_owned(),
    ]
}

/// Build a detail message for a failed crate search process.
pub fn search_error_detail(command: &str, error: &str) -> Vec<String> {
    vec![format!("failed to run {command}: {error}")]
}

/// Search crates.io through the first-party HTTP API.
pub fn search_crates_registry(query: &str) -> Result<Vec<CrateSearchResult>, CrateSearchError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("lazycargo (https://github.com/lildengzi/lazycargo)")
        .build()
        .map_err(|error| CrateSearchError(format!("failed to build HTTP client: {error}")))?;

    let response = client
        .get("https://crates.io/api/v1/crates")
        .query(&[("q", query), ("per_page", "100")])
        .send()
        .map_err(|error| CrateSearchError(format!("crates.io request failed: {error}")))?;

    if !response.status().is_success() {
        return Err(CrateSearchError(format!(
            "crates.io returned HTTP {}",
            response.status()
        )));
    }

    let payload = response
        .json::<CratesApiResponse>()
        .map_err(|error| CrateSearchError(format!("invalid crates.io response: {error}")))?;

    let mut results = payload
        .crates
        .into_iter()
        .map(CrateSearchResult::from)
        .collect::<Vec<_>>();
    results.sort_by_key(|result| search_rank(result, &query.to_lowercase()));
    Ok(results)
}

/// Parse `cargo search` output, filter by package name or description, and rank matches.
pub fn parse_crate_search_results(lines: &[String], query: &str) -> Vec<CrateSearchResult> {
    let query = query.trim().to_lowercase();

    let mut results = lines
        .iter()
        .filter_map(|line| {
            if line.starts_with('$')
                || line.starts_with("duration:")
                || line.starts_with("exit:")
                || line.trim().is_empty()
            {
                return None;
            }

            let (name, rest) = line.split_once(" = ")?;
            let (version_part, description_part) = rest.split_once(" # ").unwrap_or((rest, ""));
            let version = version_part.trim().trim_matches('"');
            if name.trim().is_empty() || version.is_empty() {
                return None;
            }

            let name = name.trim();
            let description = description_part.trim();
            if !query.is_empty()
                && !name.to_lowercase().contains(&query)
                && !description.to_lowercase().contains(&query)
            {
                return None;
            }

            Some(CrateSearchResult {
                name: name.to_owned(),
                author: None,
                version: version.to_owned(),
                description: description.to_owned(),
                homepage: None,
                documentation: None,
                repository: None,
                downloads: None,
                recent_downloads: None,
                updated_at: None,
            })
        })
        .collect::<Vec<_>>();

    results.sort_by_key(|result| search_rank(result, &query));
    results
}

fn search_rank(result: &CrateSearchResult, query: &str) -> (u8, String) {
    let name = result.name.to_lowercase();
    let description = result.description.to_lowercase();
    let rank = if name == query {
        0
    } else if name.starts_with(query) {
        1
    } else if name.contains(query) {
        2
    } else if description.contains(query) {
        3
    } else {
        4
    };

    (rank, name)
}

#[derive(Debug, Deserialize)]
struct CratesApiResponse {
    crates: Vec<CratesApiCrate>,
}

#[derive(Debug, Deserialize)]
struct CratesApiCrate {
    name: String,
    description: Option<String>,
    max_version: String,
    max_stable_version: Option<String>,
    homepage: Option<String>,
    documentation: Option<String>,
    repository: Option<String>,
    downloads: u64,
    recent_downloads: Option<u64>,
    updated_at: Option<String>,
}

impl From<CratesApiCrate> for CrateSearchResult {
    fn from(value: CratesApiCrate) -> Self {
        Self {
            name: value.name,
            author: None,
            version: value.max_stable_version.unwrap_or(value.max_version),
            description: value.description.unwrap_or_default(),
            homepage: non_empty(value.homepage),
            documentation: non_empty(value.documentation),
            repository: non_empty(value.repository),
            downloads: Some(value.downloads),
            recent_downloads: value.recent_downloads,
            updated_at: value.updated_at,
        }
    }
}

impl CrateSearchResult {
    fn source_label(&self) -> String {
        if let Some(author) = &self.author {
            return format!("author:{author}");
        }
        self.repository
            .as_deref()
            .or(self.homepage.as_deref())
            .and_then(repository_owner)
            .map(|owner| format!("repo:{owner}"))
            .unwrap_or_else(|| "repo:-".to_owned())
    }
}

fn repository_owner(url: &str) -> Option<String> {
    let cleaned = url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("git@");
    let cleaned = cleaned.strip_prefix("github.com:").unwrap_or(cleaned);
    let mut parts = cleaned.split('/');
    let host = parts.next()?;
    if !host.eq_ignore_ascii_case("github.com")
        && !host.eq_ignore_ascii_case("gitlab.com")
        && !host.eq_ignore_ascii_case("codeberg.org")
    {
        return None;
    }
    parts
        .next()
        .filter(|owner| !owner.is_empty())
        .map(str::to_owned)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn default_docs_url(name: &str) -> String {
    format!("https://docs.rs/{name}")
}

fn format_count(value: u64) -> String {
    const UNITS: &[&str] = &["", "K", "M", "B"];
    let mut value = value as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}", value as u64)
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}

/// Keep the high-signal metadata lines from `cargo info` output.
pub fn extract_crate_info_lines(lines: &[String]) -> Vec<String> {
    let mut result = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let lower = trimmed.to_lowercase();
        if lower.starts_with("repository:")
            || lower.starts_with("homepage:")
            || lower.starts_with("documentation:")
            || lower.starts_with("authors:")
            || lower.starts_with("crates.io:")
            || lower.starts_with("license:")
            || lower.starts_with("rust-version:")
            || lower.starts_with("features:")
        {
            result.push(trimmed.to_owned());
        }
    }

    if result.is_empty() {
        result
            .push("cargo info did not return repository/homepage/documentation fields".to_owned());
    }

    result
}

/// Extract the authors field from `cargo info` output.
pub fn extract_crate_author(lines: &[String]) -> Option<String> {
    extract_prefixed_value(lines, "authors:")
}

fn extract_prefixed_value(lines: &[String], prefix: &str) -> Option<String> {
    lines.iter().find_map(|line| {
        line.trim()
            .strip_prefix(prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn is_link_or_author_line(line: &str) -> bool {
    let lower = line.trim().to_lowercase();
    lower.starts_with("repository:")
        || lower.starts_with("homepage:")
        || lower.starts_with("documentation:")
        || lower.starts_with("crates.io:")
        || lower.starts_with("authors:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn result(name: &str) -> CrateSearchResult {
        CrateSearchResult {
            name: name.to_owned(),
            author: None,
            version: "1.2.3".to_owned(),
            description: format!("{name} crate"),
            homepage: None,
            documentation: None,
            repository: None,
            downloads: None,
            recent_downloads: None,
            updated_at: None,
        }
    }

    #[test]
    fn parse_search_results_filters_by_name_or_description() {
        let parsed = parse_crate_search_results(
            &lines(&[
                "$ cargo search hello --limit 100",
                "serde = \"1.0.228\" # generic serialization framework",
                "hello = \"0.1.0\" # tiny greeter",
                "world = \"0.2.0\" # says hello from examples",
                "duration: 0.12s",
            ]),
            "hello",
        );

        assert_eq!(
            parsed
                .iter()
                .map(|result| result.name.as_str())
                .collect::<Vec<_>>(),
            vec!["hello", "world"]
        );
    }

    #[test]
    fn parse_search_results_ranks_exact_then_prefix_then_description() {
        let parsed = parse_crate_search_results(
            &lines(&[
                "my-hello = \"0.3.0\" # command helpers",
                "hello-tools = \"0.2.0\" # utilities",
                "hello = \"0.1.0\" # exact",
                "greeter = \"0.4.0\" # hello output",
            ]),
            "hello",
        );

        assert_eq!(
            parsed
                .iter()
                .map(|result| result.name.as_str())
                .collect::<Vec<_>>(),
            vec!["hello", "hello-tools", "my-hello", "greeter"]
        );
    }

    #[test]
    fn extract_info_lines_keeps_link_and_metadata_fields() {
        let extracted = extract_crate_info_lines(&lines(&[
            "crate serde",
            "repository: https://github.com/serde-rs/serde",
            "documentation: https://docs.rs/serde",
            "license: MIT OR Apache-2.0",
            "unrelated text",
        ]));

        assert_eq!(
            extracted,
            vec![
                "repository: https://github.com/serde-rs/serde",
                "documentation: https://docs.rs/serde",
                "license: MIT OR Apache-2.0",
            ]
        );
    }

    #[test]
    fn extract_author_reads_cargo_info_authors_field() {
        assert_eq!(
            extract_crate_author(&lines(&["authors: Lazy Cargo Team"])),
            Some("Lazy Cargo Team".to_owned())
        );
        assert_eq!(extract_crate_author(&lines(&["authors: "])), None);
    }

    #[test]
    fn search_state_uses_cached_info_links_when_available() {
        let mut state = SearchState::default();
        state.set_results(vec![result("demo")]);
        state.set_info_detail(
            "demo".to_owned(),
            lines(&[
                "crate: demo",
                "documentation: https://example.invalid/docs",
                "repository: https://example.invalid/repo",
                "crates.io: https://example.invalid/crate",
            ]),
        );

        assert_eq!(
            state.url_for(SearchLinkTarget::Docs).as_deref(),
            Some("https://example.invalid/docs")
        );
        assert_eq!(
            state.url_for(SearchLinkTarget::Repository).as_deref(),
            Some("https://example.invalid/repo")
        );
        assert_eq!(
            state.url_for(SearchLinkTarget::Crates).as_deref(),
            Some("https://example.invalid/crate")
        );
    }

    #[test]
    fn search_state_uses_api_links_before_cargo_info() {
        let mut item = result("demo");
        item.documentation = Some("https://docs.rs/demo/latest/demo".to_owned());
        item.repository = Some("https://github.com/example/demo".to_owned());

        let mut state = SearchState::default();
        state.set_results(vec![item]);

        assert_eq!(
            state.url_for(SearchLinkTarget::Docs).as_deref(),
            Some("https://docs.rs/demo/latest/demo")
        );
        assert_eq!(
            state.url_for(SearchLinkTarget::Repository).as_deref(),
            Some("https://github.com/example/demo")
        );
    }

    #[test]
    fn result_items_include_version_downloads_and_repo_owner() {
        let mut item = result("demo");
        item.downloads = Some(24_000);
        item.repository = Some("https://github.com/example/demo".to_owned());

        let mut state = SearchState::default();
        state.set_results(vec![item]);

        let row = state.result_items().remove(0);
        assert!(row.contains("demo"));
        assert!(row.contains("1.2.3"));
        assert!(row.contains("24.0K"));
        assert!(row.contains("repo:example"));
    }

    #[test]
    fn cargo_info_detail_overrides_stale_api_links() {
        let mut item = result("demo");
        item.homepage = Some("https://github.com/old/demo".to_owned());
        item.repository = Some("https://github.com/old/demo".to_owned());

        let detail = crate_info_detail(
            &item,
            &CrateInfoReport {
                command: "cargo info demo",
                duration_secs: 0.2,
                status: Some("exit status: 0".to_owned()),
                lines: &lines(&[
                    "homepage: https://github.com/new/demo",
                    "repository: https://github.com/new/demo",
                    "documentation: https://docs.rs/demo",
                    "license: MIT",
                ]),
                timeout: false,
                error: None,
            },
        );

        assert!(detail
            .iter()
            .any(|line| line == "  repository: https://github.com/new/demo"));
        assert!(!detail.iter().any(|line| line.contains("github.com/old")));
        assert!(detail.iter().any(|line| line == "license: MIT"));
    }
}
