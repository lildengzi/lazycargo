#[derive(Debug, Clone)]
pub struct CrateSearchResult {
    pub name: String,
    pub author: Option<String>,
    pub version: String,
    pub description: String,
}

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
    pub fn clear_info(&mut self) {
        self.info_name = None;
        self.info_detail.clear();
    }

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

    pub fn set_empty_detail(&mut self, detail: Vec<String>) {
        self.results.clear();
        self.selected = 0;
        self.empty_detail = detail;
        self.clear_info();
    }

    pub fn selected_result(&self) -> Option<&CrateSearchResult> {
        self.results.get(self.selected)
    }

    pub fn result_items(&self) -> Vec<String> {
        if self.results.is_empty() {
            return vec!["no results yet".to_owned()];
        }

        self.results
            .iter()
            .map(|result| format!("{:<36} {}", result.name, result.version))
            .collect()
    }

    pub fn selected_summary(&self) -> String {
        if self.results.is_empty() {
            return "cargo search <query> --limit 100".to_owned();
        }

        self.selected_result()
            .map(|result| format!("cargo add {}", result.name))
            .unwrap_or_else(|| "no crate selected".to_owned())
    }

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

    pub fn set_info_detail(&mut self, name: String, detail: Vec<String>) {
        self.info_name = Some(name);
        self.info_detail = detail;
    }

    pub fn set_selected_author(&mut self, author: String) {
        if let Some(result) = self.results.get_mut(self.selected) {
            result.author = Some(author);
        }
    }

    pub fn crates_url(&self) -> Option<String> {
        let result = self.selected_result()?;
        self.info_url_for(result, "crates.io:")
            .or_else(|| Some(format!("https://crates.io/crates/{}", result.name)))
    }

    pub fn docs_url(&self) -> Option<String> {
        let result = self.selected_result()?;
        self.info_url_for(result, "documentation:")
            .or_else(|| Some(format!("https://docs.rs/{}", result.name)))
    }

    pub fn repository_url(&self) -> Option<String> {
        let result = self.selected_result()?;
        self.info_url_for(result, "repository:")
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

pub fn extract_crate_author(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        line.trim()
            .strip_prefix("authors:")
            .map(str::trim)
            .filter(|author| !author.is_empty())
            .map(str::to_owned)
    })
}
