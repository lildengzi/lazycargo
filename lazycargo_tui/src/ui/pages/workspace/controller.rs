use std::cell::Cell;
use std::io;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture, KeyEvent};
use crossterm::execute;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::core::dep_tree;
use crate::core::model::{CoreState, OutputSlot};
use crate::ui::components::panel::render_panel;
use crate::ui::components::scrollbar::render_scrollbar;
use crate::ui::components::style::output_line_to_lines;
use crate::ui::controller::{
    BuildCoreTab, ContextTab, DependenciesTab, Focus, FocusPanel, InputMode, MouseState, Page,
    WorkspaceTab, WorkspaceView,
};
use crate::ui::keymap::{self, NormalKeyAction, NormalKeyContext, TextInputAction};
use crate::ui::pages::workspace::view::{
    build_items, dependency_items_for, output_lines, scope_label, selected_dependency_name,
    workspace_items,
};
use crate::ui::HistoryEntry;

/// WorkspacePage 持有的导航状态子集（从 `state::NavigationState` 复制，Task 15 由根 App 在调用前同步）。
pub(crate) struct WorkspaceNav {
    pub focus: Focus,
    pub current_focus: FocusPanel,
    pub input_mode: InputMode,
    pub filter: String,
    pub message: String,
    pub last_status: String,
    pub menu_open: bool,
    pub menu_selected: usize,
    pub copy_mode: bool,
    pub command_preview: String,
}

impl Default for WorkspaceNav {
    fn default() -> Self {
        Self {
            focus: Focus::Workspace,
            current_focus: FocusPanel::Workspace,
            input_mode: InputMode::Normal,
            filter: String::new(),
            message: "ready".to_owned(),
            last_status: "ready".to_owned(),
            menu_open: false,
            menu_selected: 0,
            copy_mode: false,
            command_preview: "cargo check".to_owned(),
        }
    }
}

/// handle_tick/poll 产生的副作用在 render 前 flush 到状态栏。
#[derive(Default)]
pub(crate) struct PendingState {
    #[allow(dead_code)]
    pub message: Option<String>,
}

#[allow(dead_code)]
pub(crate) struct WorkspacePage {
    pub view: WorkspaceView,
    pub nav: WorkspaceNav,
    pub history: Vec<HistoryEntry>,
    pub pending: PendingState,
    visible_rows: Cell<usize>,
}

impl WorkspacePage {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            view: WorkspaceView::default(),
            nav: WorkspaceNav::default(),
            history: Vec::new(),
            pending: PendingState::default(),
            visible_rows: Cell::new(0),
        }
    }

    fn handle_normal_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool {
        let context = NormalKeyContext {
            search_expanded: core.search.state.expanded,
            focus: self.nav.focus,
            ws_tab: self.view.ws_tab,
            deps_tab: self.view.deps_tab,
        };
        match keymap::normal_key_action(key, context) {
            NormalKeyAction::BackFromSearch => {
                core.search.state.expanded = false;
                self.set_focus(self.view.search_return_focus);
                self.nav.message = "back from search".to_owned();
            }
            NormalKeyAction::Quit => return false,
            NormalKeyAction::OpenKeys => {
                self.nav.menu_open = true;
                self.nav.menu_selected = 0;
            }
            NormalKeyAction::FocusNext => self.set_focus(self.nav.focus.next()),
            NormalKeyAction::FocusOutput => self.set_focus(Focus::Output),
            NormalKeyAction::SwitchTab(delta) => self.switch_context_tab(core, delta),
            NormalKeyAction::ToggleCopyMode => self.toggle_copy_mode(),
            NormalKeyAction::FocusDigit(value) => self.set_focus(Focus::from_digit(value)),
            NormalKeyAction::ScrollRight(delta) => self.scroll_right(core, delta),
            NormalKeyAction::OutputUp => {
                if self.nav.current_focus == FocusPanel::Workspace
                    && self.view.ws_tab == WorkspaceTab::Target
                {
                    self.move_target_crate_selection(core, -1);
                } else if self.view.deps_tab == DependenciesTab::DependencyTree
                    && self.nav.current_focus == FocusPanel::Dependencies
                {
                    self.move_tree_selection(core, -1);
                } else {
                    self.scroll_right(core, -1);
                }
            }
            NormalKeyAction::OutputDown => {
                if self.nav.current_focus == FocusPanel::Workspace
                    && self.view.ws_tab == WorkspaceTab::Target
                {
                    self.move_target_crate_selection(core, 1);
                } else if self.view.deps_tab == DependenciesTab::DependencyTree
                    && self.nav.current_focus == FocusPanel::Dependencies
                {
                    self.move_tree_selection(core, 1);
                } else {
                    self.scroll_right(core, 1);
                }
            }
            NormalKeyAction::MoveUp => self.move_selection(core, -1),
            NormalKeyAction::MoveDown => self.move_selection(core, 1),
            NormalKeyAction::Activate => self.activate_selection(core),
            NormalKeyAction::OpenFilter => {
                self.nav.input_mode = InputMode::Filter;
                self.nav.filter.clear();
                self.nav.message = format!("filter {}: ", self.nav.focus.title());
            }
            NormalKeyAction::CollapseTree => self.collapse_selected_tree_node(core),
            NormalKeyAction::ToggleTree => self.toggle_selected_tree_node(core),
            NormalKeyAction::RefreshTarget
            | NormalKeyAction::DryRunCleanTarget
            | NormalKeyAction::CleanTarget
            | NormalKeyAction::CargoCheck
            | NormalKeyAction::CargoBuild
            | NormalKeyAction::TreeOffline
            | NormalKeyAction::TreeWithFetch
            | NormalKeyAction::InverseTreeOffline
            | NormalKeyAction::InverseTreeWithFetch
            | NormalKeyAction::PreviewAdd
            | NormalKeyAction::OpenCrates
            | NormalKeyAction::OpenDocs
            | NormalKeyAction::OpenRepository
            | NormalKeyAction::CopySearchDetail
            | NormalKeyAction::OpenSearch => {
                // TODO(Task 15): core actions (cargo/process/search)
            }
            NormalKeyAction::Noop => {}
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

    fn set_focus(&mut self, focus: Focus) {
        self.nav.focus = focus;
        match focus {
            Focus::Workspace => self.nav.current_focus = FocusPanel::Workspace,
            Focus::Build => self.nav.current_focus = FocusPanel::BuildCore,
            Focus::Dependencies => self.nav.current_focus = FocusPanel::Dependencies,
            _ => {}
        }
    }

    fn switch_context_tab(&mut self, core: &mut CoreState, delta: isize) {
        let tabs = self.active_context_tabs();
        if tabs.is_empty() {
            return;
        }
        let current = self.active_context_tab();
        let index = tabs.iter().position(|tab| *tab == current).unwrap_or(0);
        let next = (index as isize + delta).rem_euclid(tabs.len() as isize) as usize;
        self.apply_context_tab(core, tabs[next]);
    }

    fn apply_context_tab(&mut self, core: &mut CoreState, tab: ContextTab) {
        match tab {
            ContextTab::Workspace(tab) => {
                self.nav.current_focus = FocusPanel::Workspace;
                self.view.ws_tab = tab;
            }
            ContextTab::Build(tab) => {
                self.nav.current_focus = FocusPanel::BuildCore;
                self.view.build_tab = tab;
            }
            ContextTab::Dependencies(tab) => {
                self.nav.current_focus = FocusPanel::Dependencies;
                self.view.deps_tab = tab;
            }
        }
        self.reset_slot_scroll(core, self.active_output_slot(core));
    }

    fn scroll_right(&mut self, core: &mut CoreState, delta: isize) {
        self.set_focus(Focus::Output);
        let slot = self.active_output_slot(core);
        let content_len = output_lines(
            core,
            &self.view,
            self.nav.current_focus,
            &self.nav.last_status,
            &self.history,
        )
        .len();
        let scroll = {
            let ctx = core.context(slot);
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
        let len = match self.nav.focus {
            Focus::Workspace => workspace_items(&core.project).len(),
            Focus::Dependencies => {
                dependency_items_for(&core.project, self.view.selected.workspace).len()
            }
            Focus::Build => build_items().len(),
            _ => 0,
        };

        if len == 0 {
            return;
        }

        let selected = self.selected_mut(self.nav.focus);
        *selected = ((*selected as isize + delta).rem_euclid(len as isize)) as usize;
        if self.nav.focus == Focus::Workspace {
            self.sync_workspace_selection(core);
        }
        if self.nav.focus == Focus::Dependencies {
            self.view.deps_tab = DependenciesTab::Features;
            self.reset_slot_scroll(core, OutputSlot::DepsFeatures);
        }
    }

    fn selected_mut(&mut self, focus: Focus) -> &mut usize {
        match focus {
            Focus::Workspace => &mut self.view.selected.workspace,
            Focus::Dependencies => &mut self.view.selected.dependency,
            Focus::Build => &mut self.view.selected.build,
            _ => &mut self.view.selected.workspace,
        }
    }

    fn sync_workspace_selection(&mut self, core: &mut CoreState) {
        self.view.selected.dependency = 0;
        self.reset_slot_scroll(core, self.active_output_slot(core));
        self.view.deps_tab = DependenciesTab::Features;
        core.set_slot_lines(
            OutputSlot::DepsFeatures,
            vec![format!(
                "workspace scope changed: {}",
                scope_label(&core.project, self.view.selected.workspace)
            )],
        );
        core.set_slot_lines(
            OutputSlot::DepsTree,
            vec!["dependency tree not loaded for current scope".to_owned()],
        );
    }

    fn move_target_crate_selection(&mut self, core: &mut CoreState, delta: isize) {
        let len = core.disk.by_crate.len().min(core.config.target_top_crates);
        if len == 0 {
            self.scroll_right(core, delta);
            return;
        }
        self.view.selected.target_crate = (self.view.selected.target_crate as isize + delta)
            .clamp(0, len.saturating_sub(1) as isize)
            as usize;
        self.nav.message = format!("target crate {}", self.view.selected.target_crate + 1);
    }

    fn inspect_target_crate(&mut self, core: &CoreState) {
        let Some(item) = core.disk.by_crate.get(self.view.selected.target_crate) else {
            self.nav.message = "no target crate selected".to_owned();
            return;
        };
        self.nav.message = format!("target crate: {} {}", item.name, item.label);
        self.nav.last_status = format!("target {}", item.label);
    }

    fn activate_selection(&mut self, core: &mut CoreState) {
        match self.nav.focus {
            Focus::Workspace => {
                if let Some(scope) =
                    workspace_items(&core.project).get(self.view.selected.workspace)
                {
                    self.preview(&format!("scope: {scope}"));
                }
            }
            Focus::Dependencies => self.inspect_dependency(core),
            Focus::Output if self.view.deps_tab == DependenciesTab::DependencyTree => {
                self.toggle_selected_tree_node(core);
            }
            Focus::Output
                if self.nav.current_focus == FocusPanel::Workspace
                    && self.view.ws_tab == WorkspaceTab::Target =>
            {
                self.inspect_target_crate(core);
            }
            Focus::Build => {
                // TODO(Task 15): run the selected build command (core action)
            }
            Focus::Search => {
                // TODO(Task 15): search page inspect / input focus
            }
            _ => {}
        }
    }

    fn preview(&mut self, command: &str) {
        self.nav.command_preview = command.to_owned();
        self.nav.message = format!("preview: {command}");
    }

    fn inspect_dependency(&mut self, core: &mut CoreState) {
        let Some(dependency) = selected_dependency_name(
            &core.project,
            self.view.selected.workspace,
            self.view.selected.dependency,
        ) else {
            core.set_slot_lines(
                OutputSlot::DepsFeatures,
                vec!["no dependency selected".to_owned()],
            );
            return;
        };

        self.nav.command_preview = format!("cargo tree -i {dependency}");
        core.set_slot_lines(
            OutputSlot::DepsFeatures,
            vec![format!("selected: {dependency}")],
        );
        self.nav.message = format!("selected dependency: {dependency}");
        self.view.deps_tab = DependenciesTab::Features;
    }

    fn move_tree_selection(&mut self, core: &mut CoreState, delta: isize) {
        let len = core
            .output
            .get(&OutputSlot::DepsTree)
            .map(|ctx| dep_tree::flatten_visible(&ctx.tree_nodes).len())
            .unwrap_or(0);
        if len == 0 {
            self.scroll_right(core, delta);
            return;
        }
        self.view.selected.tree = (self.view.selected.tree as isize + delta)
            .clamp(0, len.saturating_sub(1) as isize) as usize;
        let selected = self.view.selected.tree;
        dep_tree::apply_selected(&mut core.context(OutputSlot::DepsTree).tree_nodes, selected);
        self.nav.message = format!("tree node {}", self.view.selected.tree + 1);
    }

    fn toggle_selected_tree_node(&mut self, core: &mut CoreState) {
        let selected = self.view.selected.tree;
        let toggled = {
            let ctx = core.context(OutputSlot::DepsTree);
            dep_tree::toggle_node(&mut ctx.tree_nodes, selected).map(|key| {
                let expanded = dep_tree::flatten_visible(&ctx.tree_nodes)
                    .get(selected)
                    .map(|node| node.expanded)
                    .unwrap_or(false);
                (key, expanded)
            })
        };
        if let Some((key, expanded)) = toggled {
            self.view.tree_expanded.insert(key, expanded);
            self.nav.message = "tree node toggled".to_owned();
        }
    }

    fn collapse_selected_tree_node(&mut self, core: &mut CoreState) {
        let selected = self.view.selected.tree;
        let Some(key) = dep_tree::set_node_expanded(
            &mut core.context(OutputSlot::DepsTree).tree_nodes,
            selected,
            false,
        ) else {
            return;
        };
        self.view.tree_expanded.insert(key, false);
        self.nav.message = "tree node collapsed".to_owned();
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
                    self.set_focus(Focus::Output);
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

    fn kill_running_child(&mut self, core: &mut CoreState) -> bool {
        let Some(mut child) = core.processes.child.take() else {
            return false;
        };

        let _ = child.kill();
        let _ = child.wait();
        self.nav.message = "killed".to_owned();
        self.nav.last_status = "killed".to_owned();
        let slot = core.processes.slot;
        let command = std::mem::take(&mut core.processes.command);
        let max_lines = core.config.output_max_lines;
        let ctx = core.context(slot);
        ctx.drain_stream(max_lines);
        ctx.stream_rx = None;
        ctx.lines.push(format!("killed: {command}"));
        core.processes.command.clear();
        true
    }

    fn active_output_slot(&self, core: &CoreState) -> OutputSlot {
        if core.search.state.expanded && matches!(self.nav.focus, Focus::Search | Focus::Output) {
            return OutputSlot::SearchDetail;
        }
        match self.nav.current_focus {
            FocusPanel::Workspace => match self.view.ws_tab {
                WorkspaceTab::CrateInfo => OutputSlot::WorkspaceCrateInfo,
                WorkspaceTab::Metrics => OutputSlot::WorkspaceMetrics,
                WorkspaceTab::Target => OutputSlot::WorkspaceTarget,
            },
            FocusPanel::BuildCore => match self.view.build_tab {
                BuildCoreTab::TaskConfig => OutputSlot::BuildConfig,
                BuildCoreTab::LiveOutput => OutputSlot::BuildLive,
            },
            FocusPanel::Dependencies => match self.view.deps_tab {
                DependenciesTab::Features => OutputSlot::DepsFeatures,
                DependenciesTab::DependencyTree => OutputSlot::DepsTree,
            },
        }
    }

    fn reset_slot_scroll(&self, core: &mut CoreState, slot: OutputSlot) {
        core.context(slot).scroll = 0;
    }

    fn active_context_tabs(&self) -> Vec<ContextTab> {
        match self.nav.current_focus {
            FocusPanel::Workspace => vec![
                ContextTab::Workspace(WorkspaceTab::CrateInfo),
                ContextTab::Workspace(WorkspaceTab::Metrics),
                ContextTab::Workspace(WorkspaceTab::Target),
            ],
            FocusPanel::BuildCore => vec![
                ContextTab::Build(BuildCoreTab::TaskConfig),
                ContextTab::Build(BuildCoreTab::LiveOutput),
            ],
            FocusPanel::Dependencies => vec![
                ContextTab::Dependencies(DependenciesTab::Features),
                ContextTab::Dependencies(DependenciesTab::DependencyTree),
            ],
        }
    }

    fn active_context_tab(&self) -> ContextTab {
        match self.nav.current_focus {
            FocusPanel::Workspace => ContextTab::Workspace(self.view.ws_tab),
            FocusPanel::BuildCore => ContextTab::Build(self.view.build_tab),
            FocusPanel::Dependencies => ContextTab::Dependencies(self.view.deps_tab),
        }
    }

    fn render_left(
        &self,
        core: &CoreState,
        frame: &mut Frame<'_>,
        area: Rect,
        mouse_state: &mut MouseState,
    ) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(28),
                Constraint::Percentage(34),
                Constraint::Percentage(38),
            ])
            .split(area);

        let offset = render_panel(
            frame,
            chunks[0],
            &workspace_items(&core.project),
            self.view.selected.workspace,
            self.nav.focus == Focus::Workspace,
            Focus::Workspace.title(),
            &self.nav.filter,
        );
        mouse_state
            .panel_areas
            .push((Focus::Workspace, chunks[0], offset));
        let build_lines = build_items()
            .into_iter()
            .map(|item| item.label.to_owned())
            .collect::<Vec<_>>();
        let offset = render_panel(
            frame,
            chunks[1],
            &build_lines,
            self.view.selected.build,
            self.nav.focus == Focus::Build,
            Focus::Build.title(),
            &self.nav.filter,
        );
        mouse_state
            .panel_areas
            .push((Focus::Build, chunks[1], offset));
        let offset = render_panel(
            frame,
            chunks[2],
            &dependency_items_for(&core.project, self.view.selected.workspace),
            self.view.selected.dependency,
            self.nav.focus == Focus::Dependencies,
            Focus::Dependencies.title(),
            &self.nav.filter,
        );
        mouse_state
            .panel_areas
            .push((Focus::Dependencies, chunks[2], offset));
    }

    fn render_output(
        &self,
        core: &CoreState,
        frame: &mut Frame<'_>,
        area: Rect,
        mouse_state: &mut MouseState,
    ) {
        let visible_rows = area.height.saturating_sub(2) as usize;
        let lines = output_lines(
            core,
            &self.view,
            self.nav.current_focus,
            &self.nav.last_status,
            &self.history,
        );
        let slot = self.active_output_slot(core);
        let max_scroll = lines.len().saturating_sub(visible_rows);
        let offset = {
            let ctx = core.output.get(&slot);
            let follow_tail = ctx.map(|ctx| ctx.follow_tail).unwrap_or(false);
            let scroll = ctx.map(|ctx| ctx.scroll).unwrap_or(0);
            if follow_tail {
                max_scroll
            } else {
                scroll.min(max_scroll)
            }
        };
        let rendered_lines: Vec<Line<'static>> = lines
            .iter()
            .skip(offset)
            .take(visible_rows)
            .flat_map(output_line_to_lines)
            .collect();

        let block = self.output_block(area, mouse_state);
        let widget = Paragraph::new(rendered_lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(widget, area);

        let scrollbar_area = area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        });
        mouse_state.right_scrollbar_area = (lines.len() > visible_rows).then_some(scrollbar_area);
        mouse_state.right_scrollbar_content_len = lines.len();
        mouse_state.right_scrollbar_visible_rows = visible_rows;
        render_scrollbar(frame, scrollbar_area, offset, lines.len(), visible_rows);
        self.visible_rows.set(visible_rows);
    }

    fn output_block(&self, area: Rect, mouse_state: &mut MouseState) -> Block<'static> {
        let focused = self.nav.focus == Focus::Output;
        let border_style = if focused {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let title = self.embedded_tab_title();
        mouse_state.tab_areas = self.embedded_tab_areas(area);

        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style)
    }

    fn embedded_tab_title(&self) -> Line<'static> {
        let mut spans = Vec::new();
        spans.push(Span::styled(
            format!(" {} ", self.nav.current_focus.title()),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
        for (index, tab) in self.active_context_tabs().into_iter().enumerate() {
            let style = if tab == self.active_context_tab() {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            if index > 0 {
                spans.push(Span::styled(" - ", Style::default().fg(Color::Gray)));
            }
            spans.push(Span::styled(tab.label(), style));
        }
        if self.nav.copy_mode {
            spans.push(Span::styled(
                " [copy mode] ",
                Style::default().fg(Color::Yellow),
            ));
        }
        Line::from(spans)
    }

    fn embedded_tab_areas(&self, area: Rect) -> Vec<(Rect, ContextTab)> {
        let mut areas = Vec::new();
        let mut x = area
            .x
            .saturating_add(1 + self.nav.current_focus.title().len() as u16 + 2);
        for (index, tab) in self.active_context_tabs().into_iter().enumerate() {
            if index > 0 {
                x = x.saturating_add(3);
            }
            let width = tab.label().len() as u16;
            if x >= area.x.saturating_add(area.width) {
                break;
            }
            areas.push((
                Rect {
                    x,
                    y: area.y,
                    width: width.min(area.x.saturating_add(area.width).saturating_sub(x)),
                    height: 1,
                },
                tab,
            ));
            x = x.saturating_add(width);
        }
        areas
    }
}

impl Page for WorkspacePage {
    fn handle_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool {
        if keymap::is_ctrl_c(key) {
            return self.kill_running_child(core);
        }

        if self.nav.menu_open {
            if keymap::menu_closes(key) {
                self.nav.menu_open = false;
            }
            return true;
        }

        match self.nav.input_mode {
            InputMode::Normal => self.handle_normal_key(core, key),
            InputMode::Filter => self.handle_filter_key(key),
            InputMode::CrateSearch | InputMode::ProjectNewConfirm => {
                // TODO(Task 15): search input / project-init confirm owned by root App
                true
            }
        }
    }

    fn handle_tick(&mut self, core: &mut CoreState) {
        // core.poll_disk_snapshot(); 尚未迁入 CoreState（Task 15 提供后在此调用）
        let _ = core;
    }

    fn render(&self, core: &CoreState, frame: &mut Frame<'_>, area: Rect) -> MouseState {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
            .split(area);

        let mut mouse_state = MouseState::default();
        self.render_left(core, frame, main[0], &mut mouse_state);
        mouse_state.panel_areas.push((Focus::Output, main[1], 0));
        self.render_output(core, frame, main[1], &mut mouse_state);
        mouse_state
    }

    fn title(&self) -> &'static str {
        "[1]-Workspace"
    }
}
