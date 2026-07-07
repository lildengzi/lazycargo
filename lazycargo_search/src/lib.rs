/// A single row returned by `cargo search`.
#[derive(Debug, Clone)]
pub struct CrateSearchResult {
    pub name: String,
    pub author: Option<String>,
    pub version: String,
    pub description: String,
}

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
            .map(|result| format!("{:<36} {}", result.name, result.version))
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
            .or_else(|| Some(format!("https://docs.rs/{}", result.name)))
    }

    /// Return the repository URL discovered from cached `cargo info` output.
    pub fn repository_url(&self) -> Option<String> {
        let result = self.selected_result()?;
        self.info_url_for(result, "repository:")
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
                "repository link unavailable; press enter to run cargo info first"
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
    if let Some(author) = &result.author {
        lines.push(format!("author: {author}"));
        lines.push(String::new());
    }
    lines.extend([
        "Links".to_owned(),
        format!("  crates.io: https://crates.io/crates/{}", result.name),
        format!("  docs.rs: https://docs.rs/{}", result.name),
        String::new(),
        "Actions".to_owned(),
        "  enter: inspect with cargo info".to_owned(),
        "  a: preview cargo add for selected crate".to_owned(),
        "  o: open crates.io".to_owned(),
        "  d: open docs".to_owned(),
        "  g: open repository after cargo info".to_owned(),
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

    let mut detail = base_search_detail(&result);
    detail.push(String::new());
    detail.push(format!("$ {}", report.command));
    detail.push(format!("duration: {:.2}s", report.duration_secs));

    if let Some(status) = &report.status {
        detail.push(format!("exit: {status}"));
        detail.push(String::new());
        detail.extend(extract_crate_info_lines(report.lines));
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
    lines.iter().find_map(|line| {
        line.trim()
            .strip_prefix("authors:")
            .map(str::trim)
            .filter(|author| !author.is_empty())
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
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
        state.set_results(vec![CrateSearchResult {
            name: "demo".to_owned(),
            author: None,
            version: "1.2.3".to_owned(),
            description: "demo crate".to_owned(),
        }]);
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
}
