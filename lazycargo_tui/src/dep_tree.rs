use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DepNode {
    pub name: String,
    pub version: String,
    pub children: Vec<DepNode>,
    pub expanded: bool,
    pub depth: usize,
    pub is_duplicate: bool,
    pub other_versions: Vec<String>,
    pub is_conflict: bool,
    pub features: Vec<String>,
    pub feature_source: Vec<String>,
    pub dependency_type: String,
    pub is_selected: bool,
}

impl DepNode {
    fn key(&self) -> String {
        format!("{}@{}:{}", self.name, self.version, self.depth)
    }
}

pub fn parse_tree_output(output: &[String], expanded: &HashMap<String, bool>) -> Vec<DepNode> {
    let mut parsed = output
        .iter()
        .filter_map(|line| parse_line(line))
        .collect::<Vec<_>>();
    if parsed.is_empty() {
        return Vec::new();
    }

    let mut roots = Vec::<DepNode>::new();
    let mut stack = Vec::<DepNode>::new();
    for mut node in parsed.drain(..) {
        node.depth = node.depth.min(stack.len());
        node.expanded = expanded.get(&node.key()).copied().unwrap_or(node.depth < 2);
        while stack.len() > node.depth {
            let child = stack.pop().expect("stack length checked");
            if let Some(parent) = stack.last_mut() {
                parent.children.push(child);
            } else {
                roots.push(child);
            }
        }
        stack.push(node);
    }
    while let Some(child) = stack.pop() {
        if let Some(parent) = stack.last_mut() {
            parent.children.push(child);
        } else {
            roots.push(child);
        }
    }

    mark_duplicates(&mut roots);
    roots
}

pub fn flatten_visible(nodes: &[DepNode]) -> Vec<DepNode> {
    let mut visible = Vec::new();
    for node in nodes {
        flatten_node(node, &mut visible);
    }
    visible
}

pub fn toggle_node(nodes: &mut [DepNode], visible_index: usize) -> Option<String> {
    let mut index = 0;
    toggle_node_inner(nodes, visible_index, &mut index)
}

pub fn set_node_expanded(
    nodes: &mut [DepNode],
    visible_index: usize,
    expanded: bool,
) -> Option<String> {
    let mut index = 0;
    set_node_expanded_inner(nodes, visible_index, expanded, &mut index)
}

pub fn apply_selected(nodes: &mut [DepNode], selected: usize) {
    let mut index = 0;
    apply_selected_inner(nodes, selected, &mut index);
}

pub fn node_detail(node: Option<&DepNode>) -> Vec<String> {
    let Some(node) = node else {
        return vec!["no dependency node selected".to_owned()];
    };
    let mut lines = vec![
        "Dependency node".to_owned(),
        String::new(),
        format!("name: {}", node.name),
        format!("version: {}", node.version),
        format!("type: {}", node.dependency_type),
        format!("duplicate: {}", node.is_duplicate),
        format!("conflict: {}", node.is_conflict),
    ];
    if !node.other_versions.is_empty() {
        lines.push(format!(
            "other versions: {}",
            node.other_versions.join(", ")
        ));
    }
    lines.extend([
        String::new(),
        "Features".to_owned(),
        if node.features.is_empty() {
            "  <unavailable from cargo tree output>".to_owned()
        } else {
            format!("  {}", node.features.join(", "))
        },
        String::new(),
        "Actions".to_owned(),
        "  Enter/l/right: expand or collapse".to_owned(),
        "  h/left: collapse".to_owned(),
    ]);
    lines
}

pub fn render_tree_lines(nodes: &[DepNode], selected: usize) -> Vec<String> {
    let mut visible = flatten_visible(nodes);
    for (index, node) in visible.iter_mut().enumerate() {
        node.is_selected = index == selected;
    }
    visible
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            let marker = if node.children.is_empty() {
                "   "
            } else if node.expanded {
                "[-]"
            } else {
                "[+]"
            };
            let duplicate = if node.is_duplicate {
                format!(" (also {} versions)", node.other_versions.len())
            } else {
                String::new()
            };
            let conflict = if node.is_conflict { " !" } else { "" };
            let selected_marker = if index == selected { ">" } else { " " };
            format!(
                "{selected_marker} {}{marker} {} {}{}{}",
                "  ".repeat(node.depth),
                node.name,
                node.version,
                duplicate,
                conflict
            )
        })
        .collect()
}

fn flatten_node(node: &DepNode, visible: &mut Vec<DepNode>) {
    visible.push(node.clone());
    if node.expanded {
        for child in &node.children {
            flatten_node(child, visible);
        }
    }
}

fn toggle_node_inner(
    nodes: &mut [DepNode],
    visible_index: usize,
    index: &mut usize,
) -> Option<String> {
    for node in nodes {
        if *index == visible_index {
            node.expanded = !node.expanded;
            return Some(node.key());
        }
        *index += 1;
        if node.expanded {
            if let Some(key) = toggle_node_inner(&mut node.children, visible_index, index) {
                return Some(key);
            }
        }
    }
    None
}

fn set_node_expanded_inner(
    nodes: &mut [DepNode],
    visible_index: usize,
    expanded: bool,
    index: &mut usize,
) -> Option<String> {
    for node in nodes {
        if *index == visible_index {
            node.expanded = expanded;
            return Some(node.key());
        }
        *index += 1;
        if node.expanded {
            if let Some(key) =
                set_node_expanded_inner(&mut node.children, visible_index, expanded, index)
            {
                return Some(key);
            }
        }
    }
    None
}

fn apply_selected_inner(nodes: &mut [DepNode], selected: usize, index: &mut usize) {
    for node in nodes {
        node.is_selected = *index == selected;
        *index += 1;
        if node.expanded {
            apply_selected_inner(&mut node.children, selected, index);
        }
    }
}

fn parse_line(line: &str) -> Option<DepNode> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('$')
        || trimmed.starts_with("exit:")
        || trimmed.starts_with("duration:")
    {
        return None;
    }
    let (depth, rest) = strip_tree_prefix(line)?;
    parse_package(rest.trim()).map(|(name, version)| DepNode {
        name,
        version,
        children: Vec::new(),
        expanded: depth < 2,
        depth,
        is_duplicate: false,
        other_versions: Vec::new(),
        is_conflict: false,
        features: Vec::new(),
        feature_source: Vec::new(),
        dependency_type: "normal".to_owned(),
        is_selected: false,
    })
}

fn strip_tree_prefix(line: &str) -> Option<(usize, &str)> {
    let mut depth = 0;
    let mut rest = line;
    loop {
        if let Some(next) = rest.strip_prefix("├── ") {
            return Some((depth + 1, next));
        }
        if let Some(next) = rest.strip_prefix("└── ") {
            return Some((depth + 1, next));
        }
        if let Some(next) = rest.strip_prefix("│   ") {
            depth += 1;
            rest = next;
            continue;
        }
        if let Some(next) = rest.strip_prefix("    ") {
            depth += 1;
            rest = next;
            continue;
        }
        if depth == 0 {
            return Some((0, rest.trim()));
        }
        return None;
    }
}

fn parse_package(rest: &str) -> Option<(String, String)> {
    let cleaned = rest
        .split(" (*)")
        .next()
        .unwrap_or(rest)
        .split(" (proc-macro)")
        .next()
        .unwrap_or(rest);
    let mut parts = cleaned.split_whitespace();
    let name = parts.next()?.to_owned();
    if name == "[build-dependencies]" || name == "[dev-dependencies]" {
        return None;
    }
    if !is_crate_name(&name) {
        return None;
    }
    let version = parts.find_map(parse_version_token)?;
    Some((name, version))
}

fn parse_version_token(token: &str) -> Option<String> {
    let version = token.strip_prefix('v')?;
    is_version_like(version).then(|| version.to_owned())
}

fn is_crate_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

fn is_version_like(version: &str) -> bool {
    let mut parts = version.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    !first.is_empty()
        && first.chars().all(|character| character.is_ascii_digit())
        && parts.all(|part| {
            !part.is_empty()
                && part.chars().all(|character| {
                    character.is_ascii_alphanumeric() || character == '-' || character == '+'
                })
        })
}

fn mark_duplicates(nodes: &mut [DepNode]) {
    let mut versions = BTreeMap::<String, Vec<String>>::new();
    collect_versions(nodes, &mut versions);
    let versions = versions
        .into_iter()
        .map(|(name, mut values)| {
            values.sort();
            values.dedup();
            (name, values)
        })
        .collect::<BTreeMap<_, _>>();
    apply_duplicates(nodes, &versions, &mut Vec::new());
}

fn collect_versions(nodes: &[DepNode], versions: &mut BTreeMap<String, Vec<String>>) {
    for node in nodes {
        versions
            .entry(node.name.clone())
            .or_default()
            .push(node.version.clone());
        collect_versions(&node.children, versions);
    }
}

fn apply_duplicates(
    nodes: &mut [DepNode],
    versions: &BTreeMap<String, Vec<String>>,
    path: &mut Vec<(String, String)>,
) {
    for node in nodes {
        let all = versions.get(&node.name).cloned().unwrap_or_default();
        node.other_versions = all
            .iter()
            .filter(|version| **version != node.version)
            .cloned()
            .collect();
        node.is_duplicate = !node.other_versions.is_empty();
        node.is_conflict = path
            .iter()
            .any(|(name, version)| name == &node.name && version != &node.version);
        path.push((node.name.clone(), node.version.clone()));
        apply_duplicates(&mut node.children, versions, path);
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cargo_tree_names_versions_and_depths() {
        let lines = vec![
            "lazycargo v0.1.1 (/tmp/lazycargo/lazycargo_tui)".to_owned(),
            "├── ansi-to-tui v8.0.1".to_owned(),
            "│   ├── nom v8.0.0".to_owned(),
            "│   │   └── memchr v2.8.2".to_owned(),
            "└── arboard v3.6.1".to_owned(),
        ];

        let nodes = parse_tree_output(&lines, &HashMap::new());
        let visible = flatten_visible(&nodes);

        assert_eq!(visible[0].name, "lazycargo");
        assert_eq!(visible[0].version, "0.1.1");
        assert_eq!(visible[0].depth, 0);
        assert_eq!(visible[1].name, "ansi-to-tui");
        assert_eq!(visible[1].version, "8.0.1");
        assert_eq!(visible[1].depth, 1);
        assert_eq!(visible[2].name, "nom");
        assert_eq!(visible[2].depth, 2);
        assert_eq!(visible[3].name, "arboard");
    }

    #[test]
    fn skips_lines_without_crate_names() {
        let lines = vec![
            "lazycargo v0.1.1 (/tmp/lazycargo/lazycargo_tui)".to_owned(),
            "├── ansi-to-tui v8.0.1".to_owned(),
            "│   │   [build-dependencies]".to_owned(),
            "│   │   2.8.2".to_owned(),
            "│   │   └── memchr v2.8.2".to_owned(),
        ];

        let nodes = parse_tree_output(&lines, &HashMap::new());
        let visible = flatten_visible(&nodes);

        assert!(visible.iter().all(|node| node.name != "2.8.2"));
        assert!(visible.iter().any(|node| node.name == "memchr"));
    }
}
