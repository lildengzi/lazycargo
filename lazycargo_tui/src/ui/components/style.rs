use ansi_to_tui::IntoText as _;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

use crate::ui::terminal_support::first_url;

pub(crate) fn output_line_to_lines(line: &String) -> Vec<Line<'static>> {
    if line.contains('\u{1b}') {
        return line
            .into_text()
            .map(|text| text.lines)
            .unwrap_or_else(|_| vec![semantic_output_line(line)]);
    }
    vec![semantic_output_line(line)]
}

pub(crate) fn semantic_output_line(raw: &str) -> Line<'static> {
    let trimmed = raw.trim();
    let lower = trimmed.to_lowercase();

    if trimmed.is_empty() {
        return Line::from(String::new());
    }

    if let Some(line) = feature_marker_line(raw) {
        return line;
    }
    if lower.starts_with('$') {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Magenta),
        ));
    }
    if lower.starts_with("exit:") || lower.starts_with("duration:") {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Cyan),
        ));
    }
    if lower.contains("error")
        || lower.contains("failed")
        || lower.contains("panic")
        || lower.contains("exit status: 1")
    {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    if lower.contains("warning") || lower.contains("unused") {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if looks_like_tree_line(raw) {
        return tree_output_line(raw);
    }
    if first_url(raw).is_some() {
        return url_output_line(raw);
    }
    if is_section_heading(trimmed) {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(line) = fixed_label_line(raw) {
        return line;
    }
    if let Some(line) = search_action_line(raw) {
        return line;
    }
    if let Some((key, value)) = raw.split_once(':') {
        return key_value_line(key, value);
    }
    if lower.contains(" v") || lower.starts_with("version ") || lower.starts_with("version:") {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Gray),
        ));
    }
    if lower.contains('/') && (lower.starts_with("  ") || lower.starts_with('(')) {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Cyan),
        ));
    }
    Line::from(raw.to_owned())
}

pub(crate) fn panel_block(title: impl Into<String>, focused: bool) -> Block<'static> {
    let style = if focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    Block::default()
        .title(Span::styled(title.into(), style))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style)
}

fn feature_marker_line(raw: &str) -> Option<Line<'static>> {
    let marker_index = raw.find('[')?;
    let marker = raw.get(marker_index..marker_index.saturating_add(3))?;
    let style = match marker {
        "[x]" => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        "[-]" => Style::default().fg(Color::Yellow),
        "[ ]" => Style::default().fg(Color::DarkGray),
        _ => return None,
    };
    Some(Line::from(vec![
        Span::raw(raw[..marker_index].to_owned()),
        Span::styled(marker.to_owned(), style),
        Span::styled(raw[marker_index + 3..].to_owned(), style),
    ]))
}

fn is_section_heading(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "Actions"
            | "Build"
            | "Cargo metrics"
            | "Dependencies"
            | "Disk"
            | "Disk tracking"
            | "Effect"
            | "Feature state"
            | "Health snapshot"
            | "Hot paths"
            | "Links"
            | "Local path"
            | "Members"
            | "Package disk snapshot"
            | "Package identity"
            | "Project health snapshot"
            | "Targets"
            | "Workspace metrics"
            | "Workspace scope"
    )
}

fn fixed_label_line(raw: &str) -> Option<Line<'static>> {
    const LABELS: &[&str] = &[
        "crate",
        "description",
        "version",
        "downloads",
        "recent",
        "updated",
        "author",
        "crates.io",
        "docs.rs",
        "homepage",
        "repository",
        "status",
        "elapsed",
        "progress",
    ];
    let (label, value) = raw.split_once("  ")?;
    let label = label.trim();
    if !LABELS.contains(&label) {
        return None;
    }
    Some(Line::from(vec![
        Span::styled(
            format!("{label:<13} "),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.trim_start().to_owned(), value_style(value)),
    ]))
}

fn search_action_line(raw: &str) -> Option<Line<'static>> {
    let trimmed = raw.trim_start();
    let (key, label) = trimmed.split_once("  ")?;
    if !matches!(key, "enter" | "a" | "o/d/g" | "y" | "s") {
        return None;
    }
    Some(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("{key:<6}"),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(label.trim_start().to_owned()),
    ]))
}

fn key_value_line(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            key.to_owned(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(":"),
        Span::styled(value.to_owned(), value_style(value)),
    ])
}

fn value_style(value: &str) -> Style {
    if value.contains("http://") || value.contains("https://") {
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::UNDERLINED)
    } else if value.contains('/') {
        Style::default().fg(Color::Cyan)
    } else if value.contains("unknown") || value.contains("<") {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    }
}

fn url_output_line(raw: &str) -> Line<'static> {
    let mut spans = Vec::new();
    for part in raw.split_inclusive(' ') {
        let style = if part.starts_with("http://") || part.starts_with("https://") {
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
        };
        spans.push(Span::styled(part.to_owned(), style));
    }
    Line::from(spans)
}

fn looks_like_tree_line(raw: &str) -> bool {
    raw.contains("├")
        || raw.contains("└")
        || raw.contains("│")
        || raw.contains("──")
        || raw.contains(" (*)")
}

fn tree_output_line(raw: &str) -> Line<'static> {
    let split_at = raw
        .char_indices()
        .find(|(_, ch)| ch.is_alphanumeric() || *ch == '_' || *ch == '-')
        .map(|(index, _)| index)
        .unwrap_or(0);
    let (tree_prefix, rest) = raw.split_at(split_at);
    let mut spans = vec![Span::styled(
        tree_prefix.to_owned(),
        Style::default().fg(Color::DarkGray),
    )];
    for part in rest.split_inclusive(' ') {
        let style =
            if part.starts_with('v') || part.contains("(*)") || part.contains("(proc-macro)") {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
        spans.push(Span::styled(part.to_owned(), style));
    }
    Line::from(spans)
}
