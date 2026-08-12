use std::cell::Cell;

use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::core::model::{CoreState, OutputSlot};
use crate::ui::components::reader::render_reader;
use crate::ui::components::style::panel_block;
use crate::ui::controller::{DocsView, MouseState};

/// 当前打开文档的 markdown 全文（README 优先，失败回退描述；均无则 None）。
pub(crate) fn docs_markdown(core: &CoreState) -> Option<String> {
    let readme = core.slot_lines(OutputSlot::DocsReadme);
    if !readme.is_empty() {
        return Some(readme.join("\n"));
    }
    let fallback = core.slot_lines(OutputSlot::DocsFallback);
    if !fallback.is_empty() {
        return Some(fallback.join("\n"));
    }
    None
}

pub(crate) fn render_docs_page(
    core: &CoreState,
    view: &DocsView,
    visible_rows: &Cell<usize>,
    frame: &mut Frame<'_>,
    area: Rect,
    _mouse_state: &mut MouseState,
) {
    visible_rows.set(area.height.saturating_sub(2) as usize);
    let title = format!("[Docs] {} {}", core.docs.name, core.docs.version);
    match docs_markdown(core) {
        Some(markdown) => render_reader(frame, area, &markdown, view.scroll, &title),
        None => {
            let widget =
                Paragraph::new("no docs loaded — press a key to open docs from a dependency")
                    .block(panel_block(title, true));
            frame.render_widget(widget, area);
        }
    }
}
