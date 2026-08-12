use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

#[allow(dead_code)]
pub(crate) fn render_tab_bar(frame: &mut Frame<'_>, area: Rect, tabs: &[(&'static str, bool)]) {
    let mut spans = Vec::new();
    for (index, (label, active)) in tabs.iter().enumerate() {
        let style = if *active {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        if index > 0 {
            spans.push(Span::styled(" - ", Style::default().fg(Color::Gray)));
        }
        spans.push(Span::styled(*label, style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
