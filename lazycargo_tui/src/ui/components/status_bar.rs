use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

#[allow(dead_code)]
pub(crate) fn render_status_bar(frame: &mut Frame<'_>, area: Rect, left: &str, right: &str) {
    let right_width = right.len() as u16;
    let right_x = area
        .x
        .saturating_add(area.width.saturating_sub(right_width));

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            left.to_owned(),
            Style::default().fg(Color::Green),
        ))),
        area,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            right.to_owned(),
            Style::default().fg(Color::Green),
        ))),
        Rect {
            x: right_x,
            y: area.y,
            width: right_width,
            height: 1,
        },
    );
}
