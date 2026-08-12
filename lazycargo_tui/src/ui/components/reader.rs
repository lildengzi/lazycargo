use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use super::style::panel_block;

/// 用 tui-markdown 渲染 markdown 为带样式 Text，Paragraph + 纵向 scroll 展示。
///
/// 内部按渲染行数钳制 scroll，避免滚动越界时整屏空白；标题边框复用 panel_block。
pub(crate) fn render_reader(
    frame: &mut Frame<'_>,
    area: Rect,
    markdown: &str,
    scroll: usize,
    title: &str,
) {
    let text = tui_markdown::from_str(markdown);
    let max_scroll = text.height().saturating_sub(1);
    let offset = scroll.min(max_scroll).min(u16::MAX as usize) as u16;
    let paragraph = Paragraph::new(text)
        .block(panel_block(title, true))
        .wrap(Wrap { trim: false })
        .scroll((offset, 0));
    frame.render_widget(paragraph, area);
}
