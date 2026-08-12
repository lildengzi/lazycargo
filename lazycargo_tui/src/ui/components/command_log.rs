use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub(crate) fn render_command_log(frame: &mut Frame<'_>, lines: &[Line<'static>], area: Rect) {
    let version_text = format!(" v{}", env!("CARGO_PKG_VERSION"));
    let version_width = version_text.len() as u16;
    let version_x = area
        .x
        .saturating_add(area.width.saturating_sub(version_width));

    frame.render_widget(Paragraph::new(lines.to_vec()), area);
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
