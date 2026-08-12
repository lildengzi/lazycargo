use std::cell::Cell;
use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyEvent, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use ratatui::layout::Rect;
use ratatui::Frame;

use lazycargo_search::{SearchLinkTarget, SearchState};

use crate::core::model::{CoreState, OutputSlot};
use crate::core::task::{
    info_progress_detail, run_info_job, run_search_job, search_progress_detail, SearchJobConfig,
    SearchJobKind,
};
use crate::ui::controller::{
    contains, focus_under, link_under, panel_under, DependenciesTab, Focus, InputMode, MouseState,
    Page, WorkspaceTab,
};
use crate::ui::keymap::{self, NormalKeyAction, NormalKeyContext, TextInputAction};
use crate::ui::pages::search::view::render_search_page;
use crate::ui::terminal_support::{copy_to_clipboard, open_url};
use crate::ui::HistoryEntry;

/// SearchPage 持有的导航状态子集（从 `state::NavigationState` 复制，Task 15 由根 App 在调用前同步）。
pub(crate) struct SearchNav {
    pub focus: Focus,
    pub input_mode: InputMode,
    pub filter: String,
    pub message: String,
    pub last_status: String,
    pub copy_mode: bool,
    pub command_preview: String,
    pub search_return_focus: Focus,
    pub menu_open: bool,
    pub menu_selected: usize,
    /// 进入搜索时同步的 workspace 选中项，用于 `cargo add -p <package>` 预览。
    pub workspace_selected: usize,
}

impl Default for SearchNav {
    fn default() -> Self {
        Self {
            focus: Focus::Search,
            input_mode: InputMode::CrateSearch,
            filter: String::new(),
            message: "ready".to_owned(),
            last_status: "ready".to_owned(),
            copy_mode: false,
            command_preview: "cargo search".to_owned(),
            search_return_focus: Focus::Workspace,
            menu_open: false,
            menu_selected: 0,
            workspace_selected: 0,
        }
    }
}

pub(crate) struct SearchPage {
    pub nav: SearchNav,
    pub history: Vec<HistoryEntry>,
    visible_rows: Cell<usize>,
}

impl SearchPage {
    pub fn new() -> Self {
        Self {
            nav: SearchNav::default(),
            history: Vec::new(),
            visible_rows: Cell::new(0),
        }
    }

    fn handle_normal_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool {
        let context = NormalKeyContext {
            search_expanded: core.search.state.expanded,
            focus: self.nav.focus,
            ws_tab: WorkspaceTab::CrateInfo,
            deps_tab: DependenciesTab::Features,
        };
        match keymap::normal_key_action(key, context) {
            NormalKeyAction::BackFromSearch => {
                core.search.state.expanded = false;
                self.nav.focus = self.nav.search_return_focus;
                self.nav.message = "back from search".to_owned();
            }
            NormalKeyAction::Quit => return false,
            NormalKeyAction::OpenKeys => {
                self.nav.menu_open = true;
                self.nav.menu_selected = 0;
            }
            NormalKeyAction::FocusNext => self.nav.focus = self.nav.focus.next(),
            NormalKeyAction::FocusOutput => self.nav.focus = Focus::Output,
            NormalKeyAction::ScrollRight(delta) => self.scroll_right(core, delta),
            NormalKeyAction::OutputUp => self.scroll_right(core, -1),
            NormalKeyAction::OutputDown => self.scroll_right(core, 1),
            NormalKeyAction::MoveUp => self.move_selection(core, -1),
            NormalKeyAction::MoveDown => self.move_selection(core, 1),
            NormalKeyAction::Activate => self.activate_selection(core),
            NormalKeyAction::OpenFilter => {
                self.nav.input_mode = InputMode::Filter;
                self.nav.filter.clear();
                self.nav.message = format!("filter {}: ", self.nav.focus.title());
            }
            NormalKeyAction::ToggleCopyMode => self.toggle_copy_mode(),
            NormalKeyAction::PreviewAdd => self.preview_add(core),
            NormalKeyAction::OpenCrates => self.open_search_link(core, SearchLinkTarget::Crates),
            NormalKeyAction::OpenDocs => self.open_search_link(core, SearchLinkTarget::Docs),
            NormalKeyAction::OpenDocsInline => self.open_search_docs_inline(core),
            NormalKeyAction::OpenRepository => {
                self.open_search_link(core, SearchLinkTarget::Repository);
            }
            NormalKeyAction::CopySearchDetail => self.copy_search_detail(core),
            NormalKeyAction::OpenSearch => {
                self.open_search(core);
                self.nav.message = "search crates".to_owned();
            }
            NormalKeyAction::FocusDigit(value) => self.nav.focus = Focus::from_digit(value),
            _ => {
                // cargo/target/tree 动作属于 Workspace 页，搜索页不处理
            }
        }

        true
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> bool {
        match keymap::text_input_action(key) {
            TextInputAction::Cancel => {
                self.nav.input_mode = InputMode::Normal;
                self.nav.filter.clear();
                self.nav.message = "filter cancelled".to_owned();
            }
            TextInputAction::Submit => {
                self.nav.input_mode = InputMode::Normal;
                self.nav.message = format!(
                    "filter applied to {}: {}",
                    self.nav.focus.title(),
                    self.nav.filter
                );
            }
            TextInputAction::Backspace => {
                self.nav.filter.pop();
            }
            TextInputAction::Push(value) => self.nav.filter.push(value),
            TextInputAction::Noop => {}
        }

        true
    }

    fn handle_crate_search_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool {
        match keymap::text_input_action(key) {
            TextInputAction::Cancel => {
                self.nav.input_mode = InputMode::Normal;
                self.nav.message = "search input blurred".to_owned();
            }
            TextInputAction::Submit => {
                let query = core.search.state.query.trim().to_owned();
                self.nav.input_mode = InputMode::Normal;
                if query.is_empty() {
                    self.nav.message = "empty crate search".to_owned();
                } else {
                    self.search_crates(core, &query);
                }
            }
            TextInputAction::Backspace => {
                core.search.state.query.pop();
            }
            TextInputAction::Push(value) => core.search.state.query.push(value),
            TextInputAction::Noop => {}
        }

        true
    }

    /// 排空搜索任务结果；返回 true 表示有任务在本次调用完成（调用方需要同步 nav）。
    pub(crate) fn poll_search_job(&mut self, core: &mut CoreState) -> bool {
        if core.search_receiver.is_some() {
            let elapsed = core
                .search_started
                .map(|started| started.elapsed())
                .unwrap_or_default();
            if let Some(name) = self.search_info_pending_name() {
                core.search
                    .state
                    .set_selected_detail(name.clone(), info_progress_detail(&name, elapsed));
            } else {
                core.search
                    .state
                    .set_empty_detail(search_progress_detail(&core.search.state.query, elapsed));
            }
        }
        let Some(receiver) = &core.search_receiver else {
            return false;
        };
        let Ok(result) = receiver.try_recv() else {
            return false;
        };
        core.search_receiver = None;
        core.search_started = None;
        match result.kind {
            SearchJobKind::Search => {
                if let Some(results) = result.results {
                    core.search.state.set_results(results);
                } else {
                    core.search.state.set_empty_detail(result.detail.clone());
                }
            }
            SearchJobKind::Info { name, author } => {
                if let Some(author) = author {
                    core.search.state.set_selected_author(author);
                }
                core.search
                    .state
                    .set_info_detail(name, result.detail.clone());
                self.record_crate_inspection(
                    core,
                    result.command.clone(),
                    result.duration,
                    result.success,
                );
            }
        }
        core.set_slot_lines(OutputSlot::SearchDetail, result.detail);
        self.nav.last_status = result.status;
        self.nav.message = result.message;
        self.history.insert(
            0,
            HistoryEntry {
                command: result.command,
                success: result.success,
                duration: result.duration,
            },
        );
        self.history.truncate(core.config.command_history_limit);
        true
    }

    fn search_info_pending_name(&self) -> Option<String> {
        let command = &self.nav.command_preview;
        command
            .strip_prefix("cargo info ")
            .map(str::to_owned)
            .filter(|name| !name.is_empty())
    }

    fn search_crates(&mut self, core: &mut CoreState, query: &str) {
        if core.search_receiver.is_some() {
            self.nav.message = "search already running".to_owned();
            return;
        }
        let query = query.to_owned();
        let command = format!("crates.io api search {query}");
        self.nav.command_preview = command.clone();
        self.nav.message = format!("searching crates: {query}");
        self.nav.focus = Focus::Search;
        core.search.state.expanded = true;
        core.search
            .state
            .set_empty_detail(search_progress_detail(&query, Duration::from_secs(0)));
        core.set_slot_lines(
            OutputSlot::SearchDetail,
            search_progress_detail(&query, Duration::from_secs(0)),
        );
        let (tx, rx) = mpsc::channel();
        core.search_receiver = Some(rx);
        core.search_started = Some(Instant::now());
        let config = self.search_job_config(core);
        thread::spawn(move || {
            let _ = tx.send(run_search_job(query, config));
        });
    }

    fn search_job_config(&self, core: &CoreState) -> SearchJobConfig {
        SearchJobConfig {
            limit: core.config.search_limit,
            network_timeout: core.config.network_timeout(),
            info_timeout: core.config.cargo_info_timeout(),
        }
    }

    fn inspect_selected_crate(&mut self, core: &mut CoreState) {
        if core.search_receiver.is_some() {
            self.nav.message = "search task already running".to_owned();
            return;
        }
        let Some(result) = core.search.state.selected_result().cloned() else {
            core.set_slot_lines(
                OutputSlot::SearchDetail,
                vec!["no crate selected".to_owned()],
            );
            return;
        };

        let command = format!("cargo info {}", result.name);
        self.nav.command_preview = command.clone();
        self.nav.message = format!("inspecting crate: {}", result.name);
        core.search.state.expanded = true;
        let detail = info_progress_detail(&result.name, Duration::from_secs(0));
        core.search
            .state
            .set_selected_detail(result.name.clone(), detail.clone());
        core.set_slot_lines(OutputSlot::SearchDetail, detail);
        let (tx, rx) = mpsc::channel();
        core.search_receiver = Some(rx);
        core.search_started = Some(Instant::now());
        let config = self.search_job_config(core);
        thread::spawn(move || {
            let _ = tx.send(run_info_job(result, config));
        });
    }

    fn record_crate_inspection(
        &mut self,
        core: &CoreState,
        command: String,
        duration: Duration,
        success: bool,
    ) {
        self.history.insert(
            0,
            HistoryEntry {
                command,
                success,
                duration,
            },
        );
        self.history.truncate(core.config.command_history_limit);
    }

    fn preview_add(&mut self, core: &mut CoreState) {
        let selected_name = core
            .search
            .state
            .selected_result()
            .map(|result| result.name.as_str())
            .or_else(|| {
                let query = core.search.state.query.trim();
                (!query.is_empty()).then_some(query)
            })
            .unwrap_or("<crate>");
        let command = match self.selected_package(core) {
            Some(package) => format!("cargo add {selected_name} -p {package}"),
            None => format!("cargo add {selected_name}"),
        };
        self.nav.command_preview = command.clone();
        self.nav.message = format!("preview: {command}");
        let detail = core
            .search
            .state
            .selected_detail()
            .into_iter()
            .chain(vec![String::new(), command])
            .collect();
        core.set_slot_lines(OutputSlot::SearchDetail, detail);
    }

    fn selected_package(&self, core: &CoreState) -> Option<String> {
        if self.nav.workspace_selected == 0 {
            None
        } else {
            core.project
                .packages
                .get(self.nav.workspace_selected.saturating_sub(1))
                .cloned()
        }
    }

    fn open_search_link(&mut self, core: &CoreState, target: SearchLinkTarget) {
        let url = core.search.state.url_for(target);

        let Some(url) = url else {
            self.nav.message = SearchState::unavailable_message(target).to_owned();
            return;
        };

        match open_url(&url) {
            Ok(()) => {
                self.nav.message = format!("opened: {url}");
                self.nav.last_status = "opened link".to_owned();
            }
            Err(error) => {
                self.nav.message = format!("failed to open link: {error}");
                self.nav.last_status = "open link failed".to_owned();
            }
        }
    }

    /// 搜索选中 crate `d`：在 TUI 内嵌阅读 docs.rs README，路由由根 App 切到 DocsPage。
    fn open_search_docs_inline(&mut self, core: &mut CoreState) {
        let Some(result) = core.search.state.selected_result().cloned() else {
            self.nav.message = "no crate selected".to_owned();
            return;
        };
        core.open_docs(&result.name, &result.version, core.config.network_timeout());
        core.docs_open = true;
        self.nav.message = format!("reading docs for {} {}...", result.name, result.version);
    }

    fn copy_search_detail(&mut self, core: &CoreState) {
        let text = core.search.state.selected_detail().join("\n");
        match copy_to_clipboard(&text) {
            Ok(()) => {
                self.nav.message = "copied search detail".to_owned();
                self.nav.last_status = "copied".to_owned();
            }
            Err(error) => {
                self.nav.message = format!("copy failed: {error}");
                self.nav.last_status = "copy failed".to_owned();
            }
        }
    }

    fn open_search(&mut self, core: &mut CoreState) {
        if self.nav.focus != Focus::Search {
            self.nav.search_return_focus = self.nav.focus;
        }
        self.nav.focus = Focus::Search;
        core.search.state.expanded = true;
        core.context(OutputSlot::SearchDetail).scroll = 0;
        self.nav.input_mode = InputMode::CrateSearch;
    }

    fn scroll_right(&mut self, core: &mut CoreState, delta: isize) {
        self.nav.focus = Focus::Output;
        let content_len = core.search.state.selected_detail().len();
        let scroll = {
            let ctx = core.context(OutputSlot::SearchDetail);
            let visible_rows = self.visible_rows.get().max(1);
            let max_scroll = content_len.saturating_sub(visible_rows);
            let current = if ctx.follow_tail {
                max_scroll
            } else {
                ctx.scroll.min(max_scroll)
            };
            let next = if delta.is_negative() {
                current.saturating_sub(delta.unsigned_abs())
            } else {
                current.saturating_add(delta as usize).min(max_scroll)
            };
            ctx.scroll = next;
            ctx.follow_tail = next >= max_scroll;
            if delta < 0 && max_scroll > 0 {
                ctx.follow_tail = false;
            }
            ctx.scroll
        };
        self.nav.message = format!("right detail scroll: {scroll}");
    }

    fn move_selection(&mut self, core: &mut CoreState, delta: isize) {
        let len = core.search.state.result_items().len();
        if len == 0 {
            return;
        }
        core.search.state.selected =
            ((core.search.state.selected as isize + delta).rem_euclid(len as isize)) as usize;
        core.context(OutputSlot::SearchDetail).scroll = 0;
    }

    fn activate_selection(&mut self, core: &mut CoreState) {
        if self.nav.input_mode == InputMode::CrateSearch || core.search.state.results.is_empty() {
            core.search.state.expanded = true;
            self.nav.input_mode = InputMode::CrateSearch;
            self.nav.message = "search crates".to_owned();
        } else {
            self.inspect_selected_crate(core);
        }
    }

    fn toggle_copy_mode(&mut self) {
        let next = !self.nav.copy_mode;
        let result = if next {
            execute!(io::stdout(), DisableMouseCapture)
        } else {
            execute!(io::stdout(), EnableMouseCapture)
        };

        match result {
            Ok(()) => {
                self.nav.copy_mode = next;
                if self.nav.copy_mode {
                    self.nav.focus = Focus::Output;
                    self.nav.message =
                        "copy mode: terminal mouse selection enabled; press m to restore"
                            .to_owned();
                    self.nav.last_status = "copy mode".to_owned();
                } else {
                    self.nav.message = "mouse interaction restored".to_owned();
                    self.nav.last_status = "mouse mode".to_owned();
                }
            }
            Err(error) => {
                self.nav.message = format!("failed to toggle copy mode: {error}");
                self.nav.last_status = "copy mode failed".to_owned();
            }
        }
    }

    fn select_row(&mut self, core: &mut CoreState, focus: Focus, row: usize) {
        let len = match focus {
            Focus::Search => core.search.state.result_items().len(),
            _ => 0,
        };
        if len > 0 {
            core.search.state.selected = row.min(len.saturating_sub(1));
            core.context(OutputSlot::SearchDetail).scroll = 0;
        }
    }

    fn mouse_event(&mut self, core: &mut CoreState, mouse: MouseEvent, mouse_state: &MouseState) {
        if self.nav.copy_mode {
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if focus_under(mouse_state, mouse.column, mouse.row) == Some(Focus::Output) {
                    self.scroll_right(core, -3);
                    return;
                }
            }
            MouseEventKind::ScrollDown => {
                if focus_under(mouse_state, mouse.column, mouse.row) == Some(Focus::Output) {
                    self.scroll_right(core, 3);
                    return;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.scrollbar_to_row(core, mouse_state, mouse.column, mouse.row) {
                    return;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.scrollbar_to_row(core, mouse_state, mouse.column, mouse.row);
                return;
            }
            _ => return,
        }

        if let Some((_, url)) = link_under(mouse_state, mouse.column, mouse.row) {
            self.open_url_message(&url);
            return;
        }

        if let Some((focus, area, offset)) = panel_under(mouse_state, mouse.column, mouse.row) {
            if core.search.state.expanded
                && focus == Focus::Search
                && area.height <= 3
                && mouse.row < area.y.saturating_add(area.height)
            {
                self.nav.input_mode = InputMode::CrateSearch;
                self.nav.message = "search input focused".to_owned();
                return;
            }
            self.nav.focus = focus;
            if focus == Focus::Output {
                self.nav.message = "focused right detail".to_owned();
                return;
            }
            let row = offset + mouse.row.saturating_sub(area.y).saturating_sub(1) as usize;
            self.select_row(core, focus, row);
            self.nav.message = format!("focused {}", focus.title());
        }
    }

    fn open_url_message(&mut self, url: &str) {
        match open_url(url) {
            Ok(()) => {
                self.nav.message = format!("opened: {url}");
                self.nav.last_status = "opened link".to_owned();
            }
            Err(error) => {
                self.nav.message = format!("failed to open link: {error}");
                self.nav.last_status = "open link failed".to_owned();
            }
        }
    }

    fn scrollbar_to_row(
        &mut self,
        core: &mut CoreState,
        mouse_state: &MouseState,
        column: u16,
        row: u16,
    ) -> bool {
        let Some(area) = mouse_state.right_scrollbar_area else {
            return false;
        };
        if !contains(area, column, row) {
            return false;
        }

        let max_scroll = mouse_state
            .right_scrollbar_content_len
            .saturating_sub(mouse_state.right_scrollbar_visible_rows);
        if max_scroll == 0 {
            return true;
        }

        let relative = row.saturating_sub(area.y) as usize;
        let track = area.height.saturating_sub(1).max(1) as usize;
        let scroll = (relative * max_scroll / track).min(max_scroll);
        let ctx = core.context(OutputSlot::SearchDetail);
        ctx.scroll = scroll;
        ctx.follow_tail = scroll >= max_scroll;
        self.nav.focus = Focus::Output;
        self.nav.message = format!("right detail scroll: {scroll}");
        true
    }
}

impl Page for SearchPage {
    fn handle_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool {
        if keymap::is_ctrl_c(key) {
            // ctrl-c 由根 App 在路由前统一处理（kill_running_child 归 WorkspacePage），
            // 此处仅防御性拦截，不会真正退出。
            return true;
        }

        if self.nav.menu_open {
            if keymap::menu_closes(key) {
                self.nav.menu_open = false;
            }
            return true;
        }

        match self.nav.input_mode {
            InputMode::CrateSearch => self.handle_crate_search_key(core, key),
            InputMode::Filter => self.handle_filter_key(key),
            InputMode::Normal => self.handle_normal_key(core, key),
            InputMode::ProjectNewConfirm => {
                // project-new-confirm 由 InitPage 处理，搜索路由下不会到达；
                // 此处防御性忽略。
                true
            }
        }
    }

    fn handle_tick(&mut self, core: &mut CoreState) {
        self.poll_search_job(core);
    }

    fn handle_mouse(&mut self, core: &mut CoreState, mouse: MouseEvent, state: &MouseState) {
        self.mouse_event(core, mouse, state);
    }

    fn render(&self, core: &CoreState, frame: &mut Frame<'_>, area: Rect) -> MouseState {
        let mut mouse_state = MouseState::default();
        render_search_page(
            core,
            &self.nav,
            &self.visible_rows,
            frame,
            area,
            &mut mouse_state,
        );
        mouse_state
    }

    fn title(&self) -> &'static str {
        "[Search]"
    }
}
