use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::keymap;

use super::dialog::centered_rect;

pub(crate) fn render_keys_dialog(frame: &mut Frame<'_>, status: &str) {
    let area = centered_rect(70, 64, frame.area());
    let lines = key_dialog_lines(status);
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

fn key_dialog_lines(status: &str) -> Vec<Line<'static>> {
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
        Span::raw(status.to_owned()),
    ]));
    lines
}

fn key_line(key: &'static str, label: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<12}"), Style::default().fg(Color::Green)),
        Span::raw(label),
    ])
}
