use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub(crate) fn render_search_input(frame: &mut Frame<'_>, area: Rect, query: &str, focused: bool) {
    let style = if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let input = if query.is_empty() {
        "type crate name, Enter to search".to_owned()
    } else {
        query.to_owned()
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
