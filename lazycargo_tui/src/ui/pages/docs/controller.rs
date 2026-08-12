use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::core::model::CoreState;
use crate::ui::controller::{DocsView, MouseState, Page};
use crate::ui::pages::docs::view::{docs_markdown, render_docs_page};

/// DocsPage 持有的视图状态；内容来自 core 的 DocsReadme/DocsFallback slot。
#[allow(dead_code)]
pub(crate) struct DocsPage {
    pub view: DocsView,
    visible_rows: Cell<usize>,
}

impl DocsPage {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            view: DocsView::default(),
            visible_rows: Cell::new(0),
        }
    }

    fn scroll_docs(&mut self, core: &CoreState, delta: isize) {
        let Some(markdown) = docs_markdown(core) else {
            return;
        };
        let content_len = markdown.lines().count();
        let visible = self.visible_rows.get().max(1);
        let max_scroll = content_len.saturating_sub(visible);
        let current = self.view.scroll;
        let next = if delta.is_negative() {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            current.saturating_add(delta as usize).min(max_scroll)
        };
        self.view.scroll = next;
    }
}

impl Page for DocsPage {
    fn handle_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Char('j') | KeyCode::Down => self.scroll_docs(core, 1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_docs(core, -1),
            KeyCode::PageDown => self.scroll_docs(core, 10),
            KeyCode::PageUp => self.scroll_docs(core, -10),
            _ => {}
        }
        true
    }

    fn handle_tick(&mut self, core: &mut CoreState) {
        core.poll_docs(core.config.output_max_lines);
    }

    fn render(&self, core: &CoreState, frame: &mut Frame<'_>, area: Rect) -> MouseState {
        let mut mouse_state = MouseState::default();
        render_docs_page(
            core,
            &self.view,
            &self.visible_rows,
            frame,
            area,
            &mut mouse_state,
        );
        mouse_state
    }

    fn title(&self) -> &'static str {
        "[Docs]"
    }
}
