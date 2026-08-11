use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::Frame;

use crate::keymap;

use super::dashboard::{apply_filter, list_offset};
use super::style::panel_block;
use super::{App, Focus, InputMode};

pub(super) fn render_search_input(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let style = if app.navigation.input_mode == InputMode::CrateSearch {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let input = if app.search.state.query.is_empty() {
        "type crate name, Enter to search".to_owned()
    } else {
        app.search.state.query.clone()
    };
    let widget = Paragraph::new(Line::from(input))
        .block(
            Block::default()
                .title(Span::styled("Search", style))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(style),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);
}

pub(super) fn render_panel(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    focus: Focus,
    lines: Vec<String>,
    selected: usize,
) -> usize {
    let lines = apply_filter(lines, &app.navigation.filter, app.navigation.focus == focus);
    let visible_rows = area.height.saturating_sub(2).max(1) as usize;
    let offset = list_offset(selected, visible_rows, lines.len());
    let items = lines
        .into_iter()
        .skip(offset)
        .take(visible_rows)
        .enumerate()
        .map(|(index, line)| {
            let real_index = offset + index;
            let style = if app.navigation.focus == focus && real_index == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(line)).style(style)
        })
        .collect::<Vec<_>>();

    let widget = List::new(items).block(panel_block(focus.title(), app.navigation.focus == focus));
    frame.render_widget(widget, area);
    offset
}

pub(super) fn render_scrollbar(
    frame: &mut Frame<'_>,
    area: Rect,
    content_len: usize,
    visible_rows: usize,
    scroll_offset: usize,
) {
    if content_len <= visible_rows {
        return;
    }
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .thumb_symbol("█")
        .track_style(Style::default().fg(Color::DarkGray))
        .thumb_style(Style::default().fg(Color::Green));
    let mut scrollbar_state = ScrollbarState::new(content_len)
        .position(scroll_offset)
        .viewport_content_length(visible_rows);
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

pub(super) fn render_menu(frame: &mut Frame<'_>, app: &App) {
    let area = centered_rect(70, 64, frame.area());
    let lines = key_dialog_lines(app);
    let widget = Paragraph::new(lines).block(
        Block::default()
            .title(Span::styled(
                "Keys",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Green)),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

pub(super) fn render_project_new_confirm(frame: &mut Frame<'_>) {
    let area = centered_rect(54, 28, frame.area());
    let lines = vec![
        Line::from(vec![Span::styled(
            "No Cargo project found",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from("This directory has no Cargo.toml."),
        Line::from("Initialize it as a Cargo project now?"),
        Line::from(""),
        Line::from(vec![
            Span::styled("Y", Style::default().fg(Color::Green)),
            Span::raw(" yes, run cargo init    "),
            Span::styled("N / Esc", Style::default().fg(Color::Red)),
            Span::raw(" no"),
        ]),
    ];
    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Green)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

pub(super) fn render_command_log(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let version_text = format!(" v{}", env!("CARGO_PKG_VERSION"));
    let version_width = version_text.len() as u16;
    let version_x = area
        .x
        .saturating_add(area.width.saturating_sub(version_width));

    let line = match app.navigation.input_mode {
        InputMode::CrateSearch => Line::from(vec![
            Span::styled("search: ", Style::default().fg(Color::Yellow)),
            Span::raw(keymap::SEARCH_STATUS_HINT),
            Span::styled(
                format!("  {}", app.navigation.last_status),
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Filter => Line::from(vec![
            Span::styled("filter: ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.navigation.filter),
            Span::styled(
                keymap::FILTER_STATUS_HINT,
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::ProjectNewConfirm => Line::from(vec![
            Span::styled("no Cargo.toml: ", Style::default().fg(Color::Yellow)),
            Span::raw(keymap::INIT_PROJECT_PROMPT),
            Span::styled("y", Style::default().fg(Color::Green)),
            Span::raw("/"),
            Span::styled("n", Style::default().fg(Color::Green)),
        ]),
        InputMode::Normal if app.navigation.copy_mode => Line::from(vec![
            Span::styled("copy: ", Style::default().fg(Color::Yellow)),
            Span::raw(keymap::COPY_STATUS_HINT),
            Span::styled(
                format!("  {}", app.navigation.last_status),
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Normal if app.search.state.expanded => Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::raw(": inspect, "),
            Span::styled("a", Style::default().fg(Color::Green)),
            Span::raw(": add, "),
            Span::styled("q", Style::default().fg(Color::Green)),
            Span::raw(": back, "),
            Span::styled("x", Style::default().fg(Color::Green)),
            Span::raw(": keys"),
            Span::styled(
                format!("  {}", app.navigation.last_status),
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Normal => Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::raw(": run/inspect, "),
            Span::styled("s", Style::default().fg(Color::Green)),
            Span::raw(": search, "),
            Span::styled("[ ]", Style::default().fg(Color::Green)),
            Span::raw(": tabs, "),
            Span::styled("x", Style::default().fg(Color::Green)),
            Span::raw(": keys, "),
            Span::styled("q", Style::default().fg(Color::Green)),
            Span::raw(": quit"),
            Span::styled(
                format!("  {}", app.navigation.last_status),
                Style::default().fg(Color::Green),
            ),
        ]),
    };

    frame.render_widget(Paragraph::new(line), area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            version_text,
            Style::default().fg(Color::Green),
        ))),
        Rect {
            x: version_x,
            y: area.y,
            width: version_width,
            height: 1,
        },
    );
}

pub(super) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn key_dialog_lines(app: &App) -> Vec<Line<'static>> {
    let version = env!("CARGO_PKG_VERSION");
    let mut lines = Vec::new();
    for section in keymap::HELP_SECTIONS {
        lines.push(Line::from(vec![Span::styled(
            section.title,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
        lines.extend(
            section
                .entries
                .iter()
                .map(|entry| key_line(entry.key, entry.label)),
        );
    }
    lines.push(Line::from(vec![
        Span::styled("Version  ", Style::default().fg(Color::Yellow)),
        Span::raw(version.to_owned()),
        Span::raw("    "),
        Span::styled(keymap::CLOSE_KEYS, Style::default().fg(Color::Green)),
        Span::raw(" close"),
    ]));
    lines.push(Line::from(vec![
        Span::styled("status  ", Style::default().fg(Color::Yellow)),
        Span::raw(app.navigation.last_status.clone()),
    ]));
    lines
}

fn key_line(key: &'static str, label: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<12}"), Style::default().fg(Color::Green)),
        Span::raw(label),
    ])
}
