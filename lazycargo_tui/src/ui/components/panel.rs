use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{List, ListItem};
use ratatui::Frame;

use super::style::panel_block;

pub(crate) fn render_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    lines: &[String],
    selected: usize,
    focused: bool,
    title: &str,
    filter: &str,
) -> usize {
    let lines = apply_filter(lines, filter, focused);
    let visible_rows = area.height.saturating_sub(2).max(1) as usize;
    let offset = list_offset(selected, visible_rows, lines.len());
    let items = lines
        .iter()
        .skip(offset)
        .take(visible_rows)
        .enumerate()
        .map(|(index, line)| {
            let real_index = offset + index;
            let style = if focused && real_index == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(line.clone())).style(style)
        })
        .collect::<Vec<_>>();

    let widget = List::new(items).block(panel_block(title, focused));
    frame.render_widget(widget, area);
    offset
}

pub(crate) fn apply_filter(lines: &[String], filter: &str, active: bool) -> Vec<String> {
    if !active || filter.is_empty() {
        return lines.to_vec();
    }
    lines
        .iter()
        .filter(|line| line.to_lowercase().contains(&filter.to_lowercase()))
        .cloned()
        .collect()
}

pub(crate) fn list_offset(selected: usize, visible_rows: usize, len: usize) -> usize {
    if len <= visible_rows {
        return 0;
    }

    selected.saturating_sub(visible_rows.saturating_sub(1))
}
