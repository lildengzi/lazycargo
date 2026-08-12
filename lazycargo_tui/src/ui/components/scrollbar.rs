use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::Frame;

pub(crate) fn render_scrollbar(
    frame: &mut Frame<'_>,
    area: Rect,
    offset: usize,
    content_len: usize,
    visible_rows: usize,
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
        .position(offset)
        .viewport_content_length(visible_rows);
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}
