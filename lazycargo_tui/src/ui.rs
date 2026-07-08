use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use ansi_to_tui::IntoText as _;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::{Frame, Terminal};

use crate::build_history::{is_recordable_command, BuildEntry, BuildHistory, CrateTiming};
use crate::cargo_task::{
    CargoTask, CargoTaskKind, CommandSpec, FeatureSelection, Profile, TaskScope,
};
use crate::dep_tree;
use crate::metadata::ProjectInfo;
use crate::state::{
    ContextOutput, NavigationState, OutputSlot, ProcessState, SearchModel, SelectionState,
    WorkspaceModel,
};
use crate::target_analyzer::{self, DiskSnapshot};
use crate::util::format_bytes;
use lazycargo_search::{
    crate_info_detail, extract_crate_author, parse_crate_search_results, search_crates_registry,
    search_error_detail, search_timeout_detail, CrateInfoReport, SearchLinkTarget, SearchState,
};

mod dashboard;
pub(crate) mod runner;
mod terminal_support;

use dashboard::{
    apply_filter, build_items, dependency_items_for, list_offset, output_lines, workspace_items,
};
use runner::{command_output_with_timeout, extract_diagnostics, spawn_streaming, split_output};
use terminal_support::{copy_to_clipboard, first_url, open_url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Focus {
    Build,
    Dependencies,
    Search,
    Workspace,
    CommandLog,
    Output,
}

impl Focus {
    fn from_digit(value: char) -> Self {
        match value {
            '1' => Self::Workspace,
            '2' => Self::Build,
            '3' => Self::Dependencies,
            '0' => Self::Output,
            _ => Self::Workspace,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Workspace => Self::Build,
            Self::Build => Self::Dependencies,
            Self::Dependencies => Self::Output,
            Self::Search => Self::Workspace,
            Self::Output => Self::Workspace,
            Self::CommandLog => Self::Workspace,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Build => "[2]-Build Core",
            Self::Dependencies => "[3]-Dependencies",
            Self::Search => "[Search]",
            Self::Workspace => "[1]-Workspace",
            Self::CommandLog => "[?]-Keys",
            Self::Output => "[0]-Output / Detail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputMode {
    Normal,
    Filter,
    CrateSearch,
    ProjectNew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusPanel {
    Workspace,
    BuildCore,
    Dependencies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceTab {
    CrateInfo,
    Metrics,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildCoreTab {
    TaskConfig,
    LiveOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DependenciesTab {
    Features,
    DependencyTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextTab {
    Workspace(WorkspaceTab),
    Build(BuildCoreTab),
    Dependencies(DependenciesTab),
}

impl FocusPanel {
    fn title(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::BuildCore => "Build Core",
            Self::Dependencies => "Dependencies",
        }
    }
}

impl ContextTab {
    fn label(self) -> &'static str {
        match self {
            Self::Workspace(WorkspaceTab::CrateInfo) => "Crate Info",
            Self::Workspace(WorkspaceTab::Metrics) => "Metrics",
            Self::Workspace(WorkspaceTab::Target) => "Target",
            Self::Build(BuildCoreTab::TaskConfig) => "Task Config",
            Self::Build(BuildCoreTab::LiveOutput) => "Live Output",
            Self::Dependencies(DependenciesTab::Features) => "Features",
            Self::Dependencies(DependenciesTab::DependencyTree) => "Dependency Tree",
        }
    }
}

struct UiText {
    keys_title: &'static str,
    version_label: &'static str,
    status_label: &'static str,
    close_keys: &'static str,
}

fn ui_text() -> UiText {
    UiText {
        keys_title: "Keys",
        version_label: "Version",
        status_label: "status",
        close_keys: "Esc/x/Enter",
    }
}

struct HistoryEntry {
    command: String,
    success: bool,
    duration: Duration,
}

#[derive(Default)]
struct MouseState {
    panel_areas: Vec<(Focus, Rect, usize)>,
    link_areas: Vec<(Rect, String)>,
    tab_areas: Vec<(Rect, ContextTab)>,
    right_scrollbar_area: Option<Rect>,
    right_scrollbar_content_len: usize,
    right_scrollbar_visible_rows: usize,
}

struct App {
    workspace: WorkspaceModel,
    navigation: NavigationState,
    selection: SelectionState,
    process: ProcessState,
    output_store: HashMap<OutputSlot, ContextOutput>,
    search: SearchModel,
    history: Vec<HistoryEntry>,
}

impl App {
    fn new(project: ProjectInfo) -> Self {
        let command_preview = "cargo check".to_owned();
        let disk = DiskSnapshot::pending(&project.packages);
        let disk_receiver = Some(load_disk_snapshot_async(project.clone()));
        let output = project_health_snapshot(&project, &disk);
        let mut output_store = HashMap::new();
        output_store.insert(OutputSlot::BuildLive, ContextOutput::with_lines(output));
        output_store.insert(
            OutputSlot::BuildConfig,
            ContextOutput::with_lines(vec!["no build/check run yet".to_owned()]),
        );
        output_store.insert(
            OutputSlot::DepsFeatures,
            ContextOutput::with_lines(vec![
                "select dependency, then press t for tree or i for inverse tree".to_owned(),
            ]),
        );
        output_store.insert(
            OutputSlot::DepsTree,
            ContextOutput::with_lines(vec!["no dependency tree yet".to_owned()]),
        );
        output_store.insert(
            OutputSlot::SearchDetail,
            ContextOutput::with_lines(vec!["no package action yet".to_owned()]),
        );
        Self {
            workspace: WorkspaceModel {
                project,
                disk,
                disk_receiver,
                build_history: BuildHistory::load(),
                diagnostics: Vec::new(),
            },
            navigation: NavigationState {
                focus: Focus::Workspace,
                current_focus: FocusPanel::Workspace,
                input_mode: InputMode::Normal,
                ws_tab: WorkspaceTab::CrateInfo,
                build_tab: BuildCoreTab::TaskConfig,
                deps_tab: DependenciesTab::Features,
                menu_open: false,
                menu_selected: 0,
                filter: String::new(),
                new_project_name: String::new(),
                search_return_focus: Focus::Workspace,
                command_preview,
                message: "ready".to_owned(),
                last_status: "ready".to_owned(),
                copy_mode: false,
            },
            selection: SelectionState {
                workspace_selected: 0,
                dependency_selected: 0,
                build_selected: 0,
                tree_selected: 0,
                tree_expanded: HashMap::new(),
            },
            process: ProcessState {
                child: None,
                command: String::new(),
                start: Instant::now(),
                slot: OutputSlot::BuildLive,
            },
            output_store,
            search: SearchModel {
                state: SearchState::default(),
            },
            history: Vec::new(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.navigation.menu_open {
            return self.handle_menu_key(key);
        }

        match self.navigation.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Filter => self.handle_filter_key(key),
            InputMode::CrateSearch => self.handle_crate_search_key(key),
            InputMode::ProjectNew => self.handle_project_new_key(key),
        }
    }

    fn poll_disk_snapshot(&mut self) {
        let Some(receiver) = &self.workspace.disk_receiver else {
            return;
        };
        let Ok(snapshot) = receiver.try_recv() else {
            return;
        };
        self.workspace.disk = snapshot;
        self.workspace.disk_receiver = None;
        if self.navigation.last_status == "ready" {
            self.navigation.last_status = "disk snapshot ready".to_owned();
        }
    }

    fn refresh_disk_snapshot(&mut self) {
        self.workspace.disk_receiver =
            Some(load_disk_snapshot_async(self.workspace.project.clone()));
        self.navigation.message = "refreshing target analysis".to_owned();
        self.navigation.last_status = "target refresh".to_owned();
    }

    fn clean_target_stale(&mut self, dry_run: bool) {
        let root = Path::new(&self.workspace.project.workspace_root);
        match target_analyzer::clean_stale(root, dry_run) {
            Ok(lines) => {
                let output = std::iter::once(if dry_run {
                    "Target clean dry-run".to_owned()
                } else {
                    "Target clean stale".to_owned()
                })
                .chain(std::iter::once(String::new()))
                .chain(lines.clone())
                .collect();
                self.set_slot_lines(OutputSlot::WorkspaceTarget, output);
                self.navigation.ws_tab = WorkspaceTab::Target;
                self.navigation.current_focus = FocusPanel::Workspace;
                self.set_focus(Focus::Output);
                self.navigation.message = if dry_run {
                    format!("dry-run: {} stale artifacts", lines.len())
                } else {
                    format!("cleaned: {} stale artifacts", lines.len())
                };
                self.navigation.last_status = "target clean".to_owned();
                if !dry_run {
                    self.refresh_disk_snapshot();
                }
            }
            Err(error) => {
                self.navigation.message = format!("target clean failed: {error}");
                self.navigation.last_status = "target clean failed".to_owned();
            }
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, mouse_state: &MouseState) {
        if self.navigation.copy_mode {
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.mouse_over_focus(mouse_state, mouse.column, mouse.row)
                    == Some(Focus::Output)
                {
                    self.scroll_right(-3);
                    return;
                }
            }
            MouseEventKind::ScrollDown => {
                if self.mouse_over_focus(mouse_state, mouse.column, mouse.row)
                    == Some(Focus::Output)
                {
                    self.scroll_right(3);
                    return;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.scrollbar_to_row(mouse_state, mouse.column, mouse.row) {
                    return;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.scrollbar_to_row(mouse_state, mouse.column, mouse.row);
                return;
            }
            _ => return,
        }

        if let Some((_, url)) = self
            .link_areas(mouse_state)
            .iter()
            .find(|(area, _)| contains(*area, mouse.column, mouse.row))
            .cloned()
        {
            self.open_url_message(&url);
            return;
        }

        if let Some((_, tab)) = self
            .tab_areas(mouse_state)
            .iter()
            .find(|(area, _)| contains(*area, mouse.column, mouse.row))
            .copied()
        {
            self.apply_context_tab(tab);
            self.set_focus(Focus::Output);
            self.navigation.message = format!("selected {} tab", tab.label());
            return;
        }

        if let Some((focus, area, offset)) = self
            .panel_areas(mouse_state)
            .iter()
            .find(|(_, area, _)| contains(*area, mouse.column, mouse.row))
            .copied()
        {
            if self.search.state.expanded
                && focus == Focus::Search
                && area.height <= 3
                && mouse.row < area.y.saturating_add(area.height)
            {
                self.navigation.input_mode = InputMode::CrateSearch;
                self.navigation.message = "search input focused".to_owned();
                return;
            }

            self.set_focus(focus);
            if focus == Focus::Output {
                self.navigation.message = "focused right detail".to_owned();
                return;
            }
            let row = offset + mouse.row.saturating_sub(area.y).saturating_sub(1) as usize;
            self.select_row(focus, row);
            if focus == Focus::Search {
                self.reset_slot_scroll(OutputSlot::SearchDetail);
            }
            self.navigation.message = format!("focused {}", focus.title());
        }
    }

    fn link_areas<'a>(&self, mouse_state: &'a MouseState) -> &'a [(Rect, String)] {
        &mouse_state.link_areas
    }

    fn tab_areas<'a>(&self, mouse_state: &'a MouseState) -> &'a [(Rect, ContextTab)] {
        &mouse_state.tab_areas
    }

    fn panel_areas<'a>(&self, mouse_state: &'a MouseState) -> &'a [(Focus, Rect, usize)] {
        &mouse_state.panel_areas
    }

    fn mouse_over_focus(&self, mouse_state: &MouseState, column: u16, row: u16) -> Option<Focus> {
        mouse_state
            .panel_areas
            .iter()
            .find(|(_, area, _)| contains(*area, column, row))
            .map(|(focus, _, _)| *focus)
    }

    fn set_focus(&mut self, focus: Focus) {
        self.navigation.focus = focus;
        match focus {
            Focus::Workspace => self.navigation.current_focus = FocusPanel::Workspace,
            Focus::Build => self.navigation.current_focus = FocusPanel::BuildCore,
            Focus::Dependencies => self.navigation.current_focus = FocusPanel::Dependencies,
            _ => {}
        }
    }

    fn active_context_tabs(&self) -> Vec<ContextTab> {
        match self.navigation.current_focus {
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
        match self.navigation.current_focus {
            FocusPanel::Workspace => ContextTab::Workspace(self.navigation.ws_tab),
            FocusPanel::BuildCore => ContextTab::Build(self.navigation.build_tab),
            FocusPanel::Dependencies => ContextTab::Dependencies(self.navigation.deps_tab),
        }
    }

    fn active_output_slot(&self) -> OutputSlot {
        if self.search.state.expanded
            && matches!(self.navigation.focus, Focus::Search | Focus::Output)
        {
            return OutputSlot::SearchDetail;
        }
        match self.navigation.current_focus {
            FocusPanel::Workspace => match self.navigation.ws_tab {
                WorkspaceTab::CrateInfo => OutputSlot::WorkspaceCrateInfo,
                WorkspaceTab::Metrics => OutputSlot::WorkspaceMetrics,
                WorkspaceTab::Target => OutputSlot::WorkspaceTarget,
            },
            FocusPanel::BuildCore => match self.navigation.build_tab {
                BuildCoreTab::TaskConfig => OutputSlot::BuildConfig,
                BuildCoreTab::LiveOutput => OutputSlot::BuildLive,
            },
            FocusPanel::Dependencies => match self.navigation.deps_tab {
                DependenciesTab::Features => OutputSlot::DepsFeatures,
                DependenciesTab::DependencyTree => OutputSlot::DepsTree,
            },
        }
    }

    fn output_for(&mut self, slot: OutputSlot) -> &mut ContextOutput {
        self.output_store
            .entry(slot)
            .or_insert_with(ContextOutput::new)
    }

    fn output_lines_for(&self, slot: OutputSlot) -> Vec<String> {
        self.output_store
            .get(&slot)
            .map(|ctx| ctx.lines.clone())
            .unwrap_or_default()
    }

    fn set_slot_lines(&mut self, slot: OutputSlot, lines: Vec<String>) {
        let ctx = self.output_for(slot);
        ctx.lines = lines;
        ctx.stream_rx = None;
        ctx.scroll = 0;
        if slot != OutputSlot::DepsTree {
            ctx.tree_nodes.clear();
        }
    }

    fn reset_slot_scroll(&mut self, slot: OutputSlot) {
        self.output_for(slot).scroll = 0;
    }

    fn apply_context_tab(&mut self, tab: ContextTab) {
        match tab {
            ContextTab::Workspace(tab) => {
                self.navigation.current_focus = FocusPanel::Workspace;
                self.navigation.ws_tab = tab;
            }
            ContextTab::Build(tab) => {
                self.navigation.current_focus = FocusPanel::BuildCore;
                self.navigation.build_tab = tab;
            }
            ContextTab::Dependencies(tab) => {
                self.navigation.current_focus = FocusPanel::Dependencies;
                self.navigation.deps_tab = tab;
            }
        }
        self.reset_slot_scroll(self.active_output_slot());
    }

    fn switch_context_tab(&mut self, delta: isize) {
        let tabs = self.active_context_tabs();
        if tabs.is_empty() {
            return;
        }
        let current = self.active_context_tab();
        let index = tabs.iter().position(|tab| *tab == current).unwrap_or(0);
        let next = (index as isize + delta).rem_euclid(tabs.len() as isize) as usize;
        self.apply_context_tab(tabs[next]);
    }

    fn open_search(&mut self) {
        if self.navigation.focus != Focus::Search {
            self.navigation.search_return_focus = self.navigation.focus;
        }
        self.set_focus(Focus::Search);
        self.search.state.expanded = true;
        self.reset_slot_scroll(OutputSlot::SearchDetail);
        self.navigation.input_mode = InputMode::CrateSearch;
    }

    fn scroll_right(&mut self, delta: isize) {
        self.set_focus(Focus::Output);
        let slot = self.active_output_slot();
        let scroll = {
            let ctx = self.output_for(slot);
            ctx.scroll = (ctx.scroll as isize + delta).max(0) as usize;
            ctx.scroll
        };
        self.navigation.message = format!("right detail scroll: {scroll}");
    }

    fn scrollbar_to_row(&mut self, mouse_state: &MouseState, column: u16, row: u16) -> bool {
        let Some(area) = mouse_state.right_scrollbar_area else {
            return false;
        };
        if !contains(area, column, row) {
            return false;
        }

        let max_scroll = self
            .right_scrollbar_content_len(mouse_state)
            .saturating_sub(mouse_state.right_scrollbar_visible_rows);
        if max_scroll == 0 {
            return true;
        }

        let relative = row.saturating_sub(area.y) as usize;
        let track = area.height.saturating_sub(1).max(1) as usize;
        let scroll = (relative * max_scroll / track).min(max_scroll);
        self.output_for(self.active_output_slot()).scroll = scroll;
        self.set_focus(Focus::Output);
        self.navigation.message = format!("right detail scroll: {scroll}");
        true
    }

    fn right_scrollbar_content_len(&self, mouse_state: &MouseState) -> usize {
        mouse_state.right_scrollbar_content_len
    }

    fn toggle_copy_mode(&mut self) {
        let next = !self.navigation.copy_mode;
        let result = if next {
            execute!(io::stdout(), DisableMouseCapture)
        } else {
            execute!(io::stdout(), EnableMouseCapture)
        };

        match result {
            Ok(()) => {
                self.navigation.copy_mode = next;
                if self.navigation.copy_mode {
                    self.set_focus(Focus::Output);
                    self.navigation.message =
                        "copy mode: terminal mouse selection enabled; press m to restore"
                            .to_owned();
                    self.navigation.last_status = "copy mode".to_owned();
                } else {
                    self.navigation.message = "mouse interaction restored".to_owned();
                    self.navigation.last_status = "mouse mode".to_owned();
                }
            }
            Err(error) => {
                self.navigation.message = format!("failed to toggle copy mode: {error}");
                self.navigation.last_status = "copy mode failed".to_owned();
            }
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.kill_running_child()
            }
            KeyCode::Char('q') if self.search.state.expanded => {
                self.search.state.expanded = false;
                self.set_focus(self.navigation.search_return_focus);
                self.navigation.message = "back from search".to_owned();
            }
            KeyCode::Esc if self.search.state.expanded => {
                self.search.state.expanded = false;
                self.set_focus(self.navigation.search_return_focus);
                self.navigation.message = "back from search".to_owned();
            }
            KeyCode::Char('q') => return false,
            KeyCode::Char('x') => {
                self.navigation.menu_open = true;
                self.navigation.menu_selected = 0;
            }
            KeyCode::Tab => self.set_focus(self.navigation.focus.next()),
            KeyCode::BackTab => self.set_focus(Focus::Output),
            KeyCode::Char(']') => self.switch_context_tab(1),
            KeyCode::Char('[') => self.switch_context_tab(-1),
            KeyCode::Char('m') => self.toggle_copy_mode(),
            KeyCode::Char('0') => self.set_focus(Focus::Output),
            KeyCode::Char(value @ ('1'..='3')) => self.set_focus(Focus::from_digit(value)),
            KeyCode::PageUp => self.scroll_right(-10),
            KeyCode::PageDown => self.scroll_right(10),
            KeyCode::Up | KeyCode::Char('k') if self.navigation.focus == Focus::Output => {
                if self.navigation.deps_tab == DependenciesTab::DependencyTree
                    && self.navigation.current_focus == FocusPanel::Dependencies
                {
                    self.move_tree_selection(-1);
                } else {
                    self.scroll_right(-1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') if self.navigation.focus == Focus::Output => {
                if self.navigation.deps_tab == DependenciesTab::DependencyTree
                    && self.navigation.current_focus == FocusPanel::Dependencies
                {
                    self.move_tree_selection(1);
                } else {
                    self.scroll_right(1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Enter => self.activate_selection(),
            KeyCode::Char('/') => {
                self.navigation.input_mode = InputMode::Filter;
                self.navigation.filter.clear();
                self.navigation.message = format!("filter {}: ", self.navigation.focus.title());
            }
            KeyCode::Char('r') if self.navigation.ws_tab == WorkspaceTab::Target => {
                self.refresh_disk_snapshot()
            }
            KeyCode::Char('d') if self.navigation.ws_tab == WorkspaceTab::Target => {
                self.clean_target_stale(true)
            }
            KeyCode::Char('c') if self.navigation.ws_tab == WorkspaceTab::Target => {
                self.clean_target_stale(false)
            }
            KeyCode::Char('c') => self.run_cargo(Focus::Build, &["check"]),
            KeyCode::Char('b') => self.run_cargo(Focus::Build, &["build"]),
            KeyCode::Char('t') => self.run_tree(),
            KeyCode::Char('i') => self.run_inverse_tree(),
            KeyCode::Char('a') => self.preview_add(),
            KeyCode::Char('o') if self.search.state.expanded => {
                self.open_search_link(SearchLinkTarget::Crates)
            }
            KeyCode::Char('d') if self.search.state.expanded => {
                self.open_search_link(SearchLinkTarget::Docs)
            }
            KeyCode::Char('g') if self.search.state.expanded => {
                self.open_search_link(SearchLinkTarget::Repository)
            }
            KeyCode::Char('y') if self.search.state.expanded => self.copy_search_detail(),
            KeyCode::Left | KeyCode::Char('h')
                if self.navigation.deps_tab == DependenciesTab::DependencyTree =>
            {
                self.collapse_selected_tree_node()
            }
            KeyCode::Right | KeyCode::Char('l')
                if self.navigation.deps_tab == DependenciesTab::DependencyTree =>
            {
                self.toggle_selected_tree_node()
            }
            KeyCode::Char('s') => {
                self.open_search();
                self.navigation.message = "search crates".to_owned();
            }
            _ => {}
        }

        true
    }

    fn handle_menu_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') | KeyCode::Enter => self.navigation.menu_open = false,
            _ => {}
        }

        true
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.navigation.input_mode = InputMode::Normal;
                self.navigation.filter.clear();
                self.navigation.message = "filter cancelled".to_owned();
            }
            KeyCode::Enter => {
                self.navigation.input_mode = InputMode::Normal;
                self.navigation.message = format!(
                    "filter applied to {}: {}",
                    self.navigation.focus.title(),
                    self.navigation.filter
                );
            }
            KeyCode::Backspace => {
                self.navigation.filter.pop();
            }
            KeyCode::Char(value) => self.navigation.filter.push(value),
            _ => {}
        }

        true
    }

    fn handle_crate_search_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.navigation.input_mode = InputMode::Normal;
                self.navigation.message = "search input blurred".to_owned();
            }
            KeyCode::Enter => {
                let query = self.search.state.query.trim().to_owned();
                self.navigation.input_mode = InputMode::Normal;
                if query.is_empty() {
                    self.navigation.message = "empty crate search".to_owned();
                } else {
                    self.search_crates(&query);
                }
            }
            KeyCode::Backspace => {
                self.search.state.query.pop();
            }
            KeyCode::Char(value) => self.search.state.query.push(value),
            _ => {}
        }

        true
    }

    fn handle_project_new_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.navigation.input_mode = InputMode::Normal;
                self.navigation.new_project_name.clear();
                self.navigation.message = "cargo new cancelled".to_owned();
            }
            KeyCode::Enter => {
                let name = self.navigation.new_project_name.trim().to_owned();
                self.navigation.input_mode = InputMode::Normal;
                if name.is_empty() {
                    self.navigation.message = "cargo new requires a project name".to_owned();
                } else {
                    self.navigation.new_project_name.clear();
                    self.run_cargo(Focus::Build, &["new", &name]);
                }
            }
            KeyCode::Backspace => {
                self.navigation.new_project_name.pop();
            }
            KeyCode::Char(value) => self.navigation.new_project_name.push(value),
            _ => {}
        }

        true
    }

    fn open_project_new_input(&mut self) {
        self.navigation.input_mode = InputMode::ProjectNew;
        self.navigation.new_project_name.clear();
        self.set_focus(Focus::Build);
        self.navigation.build_tab = BuildCoreTab::TaskConfig;
        self.navigation.command_preview = "cargo new <name>".to_owned();
        self.navigation.message = "new project name: ".to_owned();
        self.set_slot_lines(
            OutputSlot::BuildConfig,
            vec![
                "Create new Cargo project".to_owned(),
                String::new(),
                "$ cargo new <name>".to_owned(),
                String::new(),
                "Type a project directory name in the status bar, then press Enter.".to_owned(),
                "Esc cancels without creating anything.".to_owned(),
            ],
        );
    }

    fn preview(&mut self, command: &str) {
        self.navigation.command_preview = command.to_owned();
        self.navigation.message = format!("preview: {command}");
    }

    fn move_selection(&mut self, delta: isize) {
        let len = match self.navigation.focus {
            Focus::Workspace => workspace_items(&self.workspace.project).len(),
            Focus::Dependencies => {
                dependency_items_for(&self.workspace.project, self.selection.workspace_selected)
                    .len()
            }
            Focus::Search => self.search.state.result_items().len(),
            Focus::Build => build_items().len(),
            _ => 0,
        };

        if len == 0 {
            return;
        }

        let selected = self.selected_mut(self.navigation.focus);
        *selected = ((*selected as isize + delta).rem_euclid(len as isize)) as usize;
        if self.navigation.focus == Focus::Search {
            self.reset_slot_scroll(OutputSlot::SearchDetail);
        }
        if self.navigation.focus == Focus::Workspace {
            self.sync_workspace_selection();
        }
        if self.navigation.focus == Focus::Dependencies {
            self.navigation.deps_tab = DependenciesTab::Features;
            self.reset_slot_scroll(OutputSlot::DepsFeatures);
        }
    }

    fn select_row(&mut self, focus: Focus, row: usize) {
        let len = match focus {
            Focus::Workspace => workspace_items(&self.workspace.project).len(),
            Focus::Dependencies => {
                dependency_items_for(&self.workspace.project, self.selection.workspace_selected)
                    .len()
            }
            Focus::Search => self.search.state.result_items().len(),
            Focus::Build => build_items().len(),
            _ => 0,
        };

        if len > 0 {
            *self.selected_mut(focus) = row.min(len.saturating_sub(1));
            if focus == Focus::Workspace {
                self.sync_workspace_selection();
            }
            if focus == Focus::Dependencies {
                self.navigation.deps_tab = DependenciesTab::Features;
                self.reset_slot_scroll(OutputSlot::DepsFeatures);
            }
        }
    }

    fn sync_workspace_selection(&mut self) {
        self.selection.dependency_selected = 0;
        self.reset_slot_scroll(self.active_output_slot());
        self.navigation.deps_tab = DependenciesTab::Features;
        self.set_slot_lines(
            OutputSlot::DepsFeatures,
            vec![format!(
                "workspace scope changed: {}",
                self.selected_scope_label()
            )],
        );
        self.set_slot_lines(
            OutputSlot::DepsTree,
            vec!["dependency tree not loaded for current scope".to_owned()],
        );
    }

    fn selected_scope_label(&self) -> String {
        if self.selection.workspace_selected == 0 {
            "workspace".to_owned()
        } else {
            self.workspace
                .project
                .packages
                .get(self.selection.workspace_selected.saturating_sub(1))
                .map(|package| format!("package {package}"))
                .unwrap_or_else(|| "workspace".to_owned())
        }
    }

    fn selected_mut(&mut self, focus: Focus) -> &mut usize {
        match focus {
            Focus::Workspace => &mut self.selection.workspace_selected,
            Focus::Dependencies => &mut self.selection.dependency_selected,
            Focus::Search => &mut self.search.state.selected,
            Focus::Build => &mut self.selection.build_selected,
            _ => &mut self.selection.workspace_selected,
        }
    }

    fn activate_selection(&mut self) {
        match self.navigation.focus {
            Focus::Workspace => {
                if let Some(scope) =
                    workspace_items(&self.workspace.project).get(self.selection.workspace_selected)
                {
                    self.preview(&format!("scope: {scope}"));
                }
            }
            Focus::Dependencies => self.inspect_dependency(),
            Focus::Output if self.navigation.deps_tab == DependenciesTab::DependencyTree => {
                self.toggle_selected_tree_node()
            }
            Focus::Search => {
                if self.navigation.input_mode == InputMode::CrateSearch
                    || self.search.state.results.is_empty()
                {
                    self.search.state.expanded = true;
                    self.navigation.input_mode = InputMode::CrateSearch;
                    self.navigation.message = "search crates".to_owned();
                } else {
                    self.inspect_selected_crate();
                }
            }
            Focus::Build => match build_items()
                .get(self.selection.build_selected)
                .map(|item| item.key)
            {
                Some("check") => self.run_cargo(Focus::Build, &["check"]),
                Some("build") => self.run_cargo(Focus::Build, &["build"]),
                Some("test") => self.run_cargo(Focus::Build, &["test"]),
                Some("run") => self.run_cargo(Focus::Build, &["run"]),
                Some("release") => self.run_cargo(Focus::Build, &["build", "--release"]),
                Some("clippy") => self.run_cargo(Focus::Build, &["clippy", "--all-targets"]),
                Some("doc") => self.run_cargo(Focus::Build, &["doc", "--no-deps"]),
                Some("update") => self.run_cargo(Focus::Build, &["update"]),
                Some("clean") => self.run_cargo(Focus::Build, &["clean"]),
                Some("new") => self.open_project_new_input(),
                Some("timings") => self.run_cargo(Focus::Build, &["build", "--timings"]),
                Some("diagnostics") => self.navigation.build_tab = BuildCoreTab::LiveOutput,
                _ => {}
            },
            _ => {}
        }
    }

    fn run_cargo(&mut self, detail_focus: Focus, args: &[&str]) {
        if self.process.child.is_some() {
            self.navigation.message = format!("already running: {}", self.process.command);
            return;
        }

        let spec = self.build_cargo_command(args);
        let command = spec.display();
        let slot = match detail_focus {
            Focus::Build => OutputSlot::BuildLive,
            Focus::Dependencies => OutputSlot::DepsTree,
            Focus::Search => OutputSlot::SearchDetail,
            _ => self.active_output_slot(),
        };
        self.navigation.command_preview = command.clone();
        self.navigation.message = format!("running: {command}");
        self.set_focus(detail_focus);
        if detail_focus == Focus::Build {
            self.navigation.build_tab = BuildCoreTab::LiveOutput;
        } else if detail_focus == Focus::Dependencies {
            self.navigation.deps_tab = DependenciesTab::DependencyTree;
        }

        {
            let ctx = self.output_for(slot);
            ctx.lines.clear();
            ctx.lines.push(format!("$ {command}"));
            ctx.scroll = 0;
            ctx.stream_rx = None;
            ctx.tree_nodes.clear();
        }

        match spawn_streaming(&spec.program, &spec.args, &[("CARGO_TERM_COLOR", "always")]) {
            Ok((child, rx)) => {
                self.process.child = Some(child);
                self.process.command = command;
                self.process.start = Instant::now();
                self.process.slot = slot;
                self.output_for(slot).stream_rx = Some(rx);
            }
            Err(error) => {
                self.navigation.last_status = "error".to_owned();
                self.set_slot_lines(slot, vec![format!("failed to run {command}: {error}")]);
                self.workspace.diagnostics = vec![format!("runner error: {error}")];
                self.navigation.message = format!("failed: {command}");
            }
        }
    }

    fn drain_all_streams(&mut self) {
        for ctx in self.output_store.values_mut() {
            ctx.drain_stream();
        }

        let Some(child) = &mut self.process.child else {
            return;
        };

        match child.try_wait() {
            Ok(Some(status)) => {
                let Some(child) = self.process.child.take() else {
                    return;
                };
                drop(child);
                let command = std::mem::take(&mut self.process.command);
                let duration = self.process.start.elapsed();
                let slot = self.process.slot;
                self.output_for(slot).drain_stream();
                self.output_for(slot).stream_rx = None;
                self.finish_cargo_output(slot, command, duration, status);
            }
            Ok(None) => {}
            Err(error) => {
                self.navigation.message = format!("wait failed: {error}");
                self.navigation.last_status = "wait failed".to_owned();
                self.process.child = None;
                self.process.command.clear();
            }
        }
    }

    fn finish_cargo_output(
        &mut self,
        slot: OutputSlot,
        command: String,
        duration: Duration,
        status: ExitStatus,
    ) {
        let success = status.success();
        self.navigation.last_status = if success {
            format!("ok {:.2}s", duration.as_secs_f32())
        } else {
            format!("failed {:.2}s", duration.as_secs_f32())
        };
        let lines = {
            let ctx = self.output_for(slot);
            ctx.lines.push(String::new());
            ctx.lines.push(format!("exit: {status}"));
            ctx.lines
                .push(format!("duration: {:.2}s", duration.as_secs_f32()));
            ctx.lines.clone()
        };
        if slot == OutputSlot::BuildLive {
            self.workspace.diagnostics = extract_diagnostics(&lines);
        }
        self.history.insert(
            0,
            HistoryEntry {
                command: command.clone(),
                success,
                duration,
            },
        );
        self.history.truncate(20);
        self.record_build_history(&command, duration, success);
        if slot == OutputSlot::DepsTree {
            self.update_dependency_tree_from_output();
        }
        self.navigation.message = format!("finished: {command}");
    }

    fn record_build_history(&mut self, command: &str, duration: Duration, success: bool) {
        if !is_recordable_command(command) {
            return;
        }
        let entry = BuildEntry {
            timestamp: chrono::Local::now(),
            command: command.to_owned(),
            package: self.selected_package(),
            duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
            success,
            target_triple: std::env::consts::ARCH.to_owned(),
            rustc_version: self.workspace.project.rustc_version.clone(),
            crate_timings: latest_crate_timings(Path::new(&self.workspace.project.workspace_root)),
        };
        if let Err(error) = self.workspace.build_history.add_entry(entry) {
            self.workspace
                .diagnostics
                .push(format!("failed to save build history: {error}"));
        }
    }

    fn update_dependency_tree_from_output(&mut self) {
        let lines = self.output_lines_for(OutputSlot::DepsTree);
        let nodes = dep_tree::parse_tree_output(&lines, &self.selection.tree_expanded);
        if nodes.is_empty() {
            self.output_for(OutputSlot::DepsTree).lines.insert(
                0,
                "structured tree parse unavailable; showing raw cargo tree output".to_owned(),
            );
            return;
        }
        let visible_len = dep_tree::flatten_visible(&nodes).len();
        self.selection.tree_selected = self
            .selection
            .tree_selected
            .min(visible_len.saturating_sub(1));
        let selected = self.selection.tree_selected;
        let ctx = self.output_for(OutputSlot::DepsTree);
        ctx.tree_nodes = nodes;
        dep_tree::apply_selected(&mut ctx.tree_nodes, selected);
    }

    fn move_tree_selection(&mut self, delta: isize) {
        let len = self
            .output_store
            .get(&OutputSlot::DepsTree)
            .map(|ctx| dep_tree::flatten_visible(&ctx.tree_nodes).len())
            .unwrap_or(0);
        if len == 0 {
            self.scroll_right(delta);
            return;
        }
        self.selection.tree_selected = (self.selection.tree_selected as isize + delta)
            .clamp(0, len.saturating_sub(1) as isize)
            as usize;
        let selected = self.selection.tree_selected;
        dep_tree::apply_selected(
            &mut self.output_for(OutputSlot::DepsTree).tree_nodes,
            selected,
        );
        self.navigation.message = format!("tree node {}", self.selection.tree_selected + 1);
    }

    fn toggle_selected_tree_node(&mut self) {
        let selected = self.selection.tree_selected;
        let toggled = {
            let ctx = self.output_for(OutputSlot::DepsTree);
            dep_tree::toggle_node(&mut ctx.tree_nodes, selected).map(|key| {
                let expanded = dep_tree::flatten_visible(&ctx.tree_nodes)
                    .get(selected)
                    .map(|node| node.expanded)
                    .unwrap_or(false);
                (key, expanded)
            })
        };
        if let Some((key, expanded)) = toggled {
            self.selection.tree_expanded.insert(key, expanded);
            self.navigation.message = "tree node toggled".to_owned();
        }
    }

    fn collapse_selected_tree_node(&mut self) {
        let selected = self.selection.tree_selected;
        let Some(key) = dep_tree::set_node_expanded(
            &mut self.output_for(OutputSlot::DepsTree).tree_nodes,
            selected,
            false,
        ) else {
            return;
        };
        self.selection.tree_expanded.insert(key, false);
        self.navigation.message = "tree node collapsed".to_owned();
    }

    fn kill_running_child(&mut self) {
        let Some(mut child) = self.process.child.take() else {
            return;
        };

        let _ = child.kill();
        let _ = child.wait();
        self.navigation.message = "killed".to_owned();
        self.navigation.last_status = "killed".to_owned();
        let slot = self.process.slot;
        let command = std::mem::take(&mut self.process.command);
        let ctx = self.output_for(slot);
        ctx.drain_stream();
        ctx.stream_rx = None;
        ctx.lines.push(format!("killed: {command}"));
        self.process.command.clear();
    }

    fn build_cargo_command(&self, args: &[&str]) -> CommandSpec {
        let Some(kind) = self.cargo_task_kind(args) else {
            return CommandSpec {
                program: "cargo".into(),
                args: self.scoped_args(args).into_iter().map(Into::into).collect(),
            };
        };
        let mut task = CargoTask {
            kind,
            scope: self.cargo_task_scope(args),
            features: FeatureSelection::default(),
            profile: if args.contains(&"--release") {
                Profile::Release
            } else {
                Profile::Dev
            },
            target: None,
            extra_args: Vec::new(),
        };
        if args == ["doc", "--no-deps"] {
            task.extra_args.push("--no-deps".to_owned());
        }
        if args == ["build", "--timings"] {
            task.extra_args.push("--timings".to_owned());
        }
        task.to_command()
    }

    fn cargo_task_kind(&self, args: &[&str]) -> Option<CargoTaskKind> {
        match args.first().copied()? {
            "check" => Some(CargoTaskKind::Check),
            "build" => Some(CargoTaskKind::Build),
            "run" => Some(CargoTaskKind::Run {
                bin: None,
                args: Vec::new(),
            }),
            "test" => Some(CargoTaskKind::Test {
                filter: None,
                nocapture: false,
            }),
            "clippy" => Some(CargoTaskKind::Clippy),
            "doc" => Some(CargoTaskKind::Doc),
            "update" => Some(CargoTaskKind::Update { package: None }),
            _ => None,
        }
    }

    fn cargo_task_scope(&self, args: &[&str]) -> TaskScope {
        let command = args.first().copied().unwrap_or_default();
        if matches!(command, "run" | "update") {
            return self
                .selected_package()
                .map(TaskScope::Package)
                .unwrap_or(TaskScope::CurrentPackage);
        }
        self.selected_package()
            .map(TaskScope::Package)
            .unwrap_or(TaskScope::Workspace)
    }

    #[allow(dead_code)]
    fn scoped_args(&self, args: &[&str]) -> Vec<String> {
        let mut result = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let Some(command) = result.first().map(String::as_str) else {
            return result;
        };

        if command == "new" {
            return result;
        }

        if matches!(command, "run" | "update" | "clean") {
            if let Some(package) = self.selected_package() {
                result.push("-p".to_owned());
                result.push(package);
            }
            return result;
        }

        match self.selected_package() {
            Some(package) => {
                result.push("-p".to_owned());
                result.push(package);
            }
            None => result.push("--workspace".to_owned()),
        }

        result
    }

    fn selected_package(&self) -> Option<String> {
        if self.selection.workspace_selected == 0 {
            None
        } else {
            self.workspace
                .project
                .packages
                .get(self.selection.workspace_selected.saturating_sub(1))
                .cloned()
        }
    }

    fn search_crates(&mut self, query: &str) {
        let command = format!("crates.io api search {query}");
        self.navigation.command_preview = command.clone();
        self.navigation.message = format!("searching crates: {query}");
        self.navigation.focus = Focus::Search;
        self.search.state.expanded = true;

        let started = Instant::now();
        match search_crates_registry(query) {
            Ok(results) => {
                let duration = started.elapsed();
                let lines = vec![
                    format!("$ {command}"),
                    format!("duration: {:.2}s", duration.as_secs_f32()),
                    format!("results: {}", results.len()),
                    String::new(),
                    "source: https://crates.io/api/v1/crates".to_owned(),
                ];
                self.search.state.set_results(results);
                self.set_slot_lines(OutputSlot::SearchDetail, lines);
                self.navigation.last_status = format!("search ok {:.2}s", duration.as_secs_f32());
                self.history.insert(
                    0,
                    HistoryEntry {
                        command,
                        success: true,
                        duration,
                    },
                );
                self.history.truncate(20);
                self.navigation.message = format!("searched crates: {query}");
                return;
            }
            Err(error) => {
                self.navigation.message =
                    format!("api search failed, trying cargo search: {error}");
            }
        }

        let command = format!("cargo search {query} --limit 100");
        self.navigation.command_preview = command.clone();
        let started = Instant::now();
        let output = command_output_with_timeout(
            "cargo",
            &["search", query, "--limit", "100"],
            Duration::from_secs(10),
        );
        let duration = started.elapsed();

        match output {
            Ok(Some(output)) => {
                let mut lines = Vec::new();
                lines.push(format!("$ {command}"));
                lines.push(format!("exit: {}", output.status));
                lines.push(format!("duration: {:.2}s", duration.as_secs_f32()));
                lines.push(String::new());
                lines.extend(split_output(&output.stdout));
                lines.extend(split_output(&output.stderr));

                let success = output.status.success();
                self.navigation.last_status = if success {
                    format!("search ok {:.2}s", duration.as_secs_f32())
                } else {
                    format!("search failed {:.2}s", duration.as_secs_f32())
                };
                if success {
                    self.search
                        .state
                        .set_results(parse_crate_search_results(&lines, query));
                } else {
                    self.search.state.set_empty_detail(lines.clone());
                }
                self.set_slot_lines(OutputSlot::SearchDetail, lines.clone());
                self.history.insert(
                    0,
                    HistoryEntry {
                        command,
                        success,
                        duration,
                    },
                );
                self.history.truncate(20);
                self.navigation.message = format!("searched crates: {query}");
            }
            Ok(None) => {
                let lines = search_timeout_detail(&command, duration.as_secs_f32());
                self.navigation.last_status = "search timeout".to_owned();
                self.search.state.set_empty_detail(lines.clone());
                self.set_slot_lines(OutputSlot::SearchDetail, lines.clone());
                self.history.insert(
                    0,
                    HistoryEntry {
                        command,
                        success: false,
                        duration,
                    },
                );
                self.history.truncate(20);
                self.navigation.message = format!("search timed out: {query}");
            }
            Err(error) => {
                self.navigation.last_status = "search error".to_owned();
                let lines = search_error_detail(&command, &error.to_string());
                self.search.state.set_empty_detail(lines.clone());
                self.set_slot_lines(OutputSlot::SearchDetail, lines);
                self.navigation.message = format!("crate search failed: {query}");
            }
        }
    }

    fn inspect_dependency(&mut self) {
        let Some(dependency) = selected_dependency_name(
            &self.workspace.project,
            self.selection.workspace_selected,
            self.selection.dependency_selected,
        ) else {
            self.set_slot_lines(
                OutputSlot::DepsFeatures,
                vec!["no dependency selected".to_owned()],
            );
            return;
        };

        self.navigation.command_preview = format!("cargo tree -i {dependency}");
        self.set_slot_lines(
            OutputSlot::DepsFeatures,
            vec![
                format!("dependency: {dependency}"),
                String::new(),
                "enter: inspect".to_owned(),
                "t: cargo tree".to_owned(),
                "i: cargo tree -i <dependency>".to_owned(),
                "a: preview cargo add".to_owned(),
            ],
        );
        self.navigation.message = format!("selected dependency: {dependency}");
        self.navigation.deps_tab = DependenciesTab::Features;
    }

    fn run_tree(&mut self) {
        self.run_cargo(Focus::Dependencies, &["tree"]);
        self.navigation.deps_tab = DependenciesTab::DependencyTree;
    }

    fn run_inverse_tree(&mut self) {
        let Some(dependency) = selected_dependency_name(
            &self.workspace.project,
            self.selection.workspace_selected,
            self.selection.dependency_selected,
        ) else {
            self.set_slot_lines(
                OutputSlot::DepsFeatures,
                vec!["select a dependency first".to_owned()],
            );
            return;
        };

        self.run_cargo(Focus::Dependencies, &["tree", "-i", &dependency]);
        self.navigation.deps_tab = DependenciesTab::DependencyTree;
    }

    fn preview_add(&mut self) {
        let selected_name = self
            .search
            .state
            .selected_result()
            .map(|result| result.name.as_str())
            .or_else(|| {
                let query = self.search.state.query.trim();
                (!query.is_empty()).then_some(query)
            })
            .unwrap_or("<crate>");
        let command = match self.selected_package() {
            Some(package) => format!("cargo add {selected_name} -p {package}"),
            None => format!("cargo add {selected_name}"),
        };
        self.preview(&command);
        let detail = self
            .search
            .state
            .selected_detail()
            .into_iter()
            .chain(vec![String::new(), command])
            .collect();
        self.set_slot_lines(OutputSlot::SearchDetail, detail);
    }

    fn inspect_selected_crate(&mut self) {
        let Some(result) = self.search.state.selected_result().cloned() else {
            self.set_slot_lines(
                OutputSlot::SearchDetail,
                vec!["no crate selected".to_owned()],
            );
            return;
        };

        let command = format!("cargo info {}", result.name);
        self.navigation.command_preview = command.clone();
        self.navigation.message = format!("inspecting crate: {}", result.name);
        self.search.state.expanded = true;

        let started = Instant::now();
        let output =
            command_output_with_timeout("cargo", &["info", &result.name], Duration::from_secs(8));
        let duration = started.elapsed();

        let detail;
        match output {
            Ok(Some(output)) => {
                let mut lines = split_output(&output.stdout);
                lines.extend(split_output(&output.stderr));
                let success = output.status.success();
                self.navigation.last_status = if success {
                    format!("info ok {:.2}s", duration.as_secs_f32())
                } else {
                    format!("info failed {:.2}s", duration.as_secs_f32())
                };
                if let Some(author) = extract_crate_author(&lines) {
                    self.search.state.set_selected_author(author);
                }
                detail = crate_info_detail(
                    &result,
                    &CrateInfoReport {
                        command: &command,
                        duration_secs: duration.as_secs_f32(),
                        status: Some(output.status.to_string()),
                        lines: &lines,
                        timeout: false,
                        error: None,
                    },
                );
                self.record_crate_inspection(command.clone(), duration, success);
            }
            Ok(None) => {
                self.navigation.last_status = "info timeout".to_owned();
                detail = crate_info_detail(
                    &result,
                    &CrateInfoReport {
                        command: &command,
                        duration_secs: duration.as_secs_f32(),
                        status: None,
                        lines: &[],
                        timeout: true,
                        error: None,
                    },
                );
                self.record_crate_inspection(command.clone(), duration, false);
            }
            Err(error) => {
                self.navigation.last_status = "info error".to_owned();
                detail = crate_info_detail(
                    &result,
                    &CrateInfoReport {
                        command: &command,
                        duration_secs: duration.as_secs_f32(),
                        status: None,
                        lines: &[],
                        timeout: false,
                        error: Some(error.to_string()),
                    },
                );
                self.record_crate_inspection(command.clone(), duration, false);
            }
        }

        self.search
            .state
            .set_info_detail(result.name.clone(), detail.clone());
        self.set_slot_lines(OutputSlot::SearchDetail, detail);
        self.navigation.message = format!("inspected crate: {}", result.name);
    }

    fn record_crate_inspection(&mut self, command: String, duration: Duration, success: bool) {
        self.history.insert(
            0,
            HistoryEntry {
                command,
                success,
                duration,
            },
        );
        self.history.truncate(20);
    }

    fn open_search_link(&mut self, target: SearchLinkTarget) {
        let url = self.search.state.url_for(target);

        let Some(url) = url else {
            self.navigation.message = SearchState::unavailable_message(target).to_owned();
            return;
        };

        match open_url(&url) {
            Ok(()) => self.open_url_message_success(&url),
            Err(error) => self.open_url_message_error(error),
        }
    }

    fn open_url_message(&mut self, url: &str) {
        match open_url(url) {
            Ok(()) => self.open_url_message_success(url),
            Err(error) => self.open_url_message_error(error),
        }
    }

    fn open_url_message_success(&mut self, url: &str) {
        self.navigation.message = format!("opened: {url}");
        self.navigation.last_status = "opened link".to_owned();
    }

    fn open_url_message_error(&mut self, error: io::Error) {
        self.navigation.message = format!("failed to open link: {error}");
        self.navigation.last_status = "open link failed".to_owned();
    }

    fn copy_search_detail(&mut self) {
        let text = self.search.state.selected_detail().join("\n");
        match copy_to_clipboard(&text) {
            Ok(()) => {
                self.navigation.message = "copied search detail".to_owned();
                self.navigation.last_status = "copied".to_owned();
            }
            Err(error) => {
                self.navigation.message = format!("copy failed: {error}");
                self.navigation.last_status = "copy failed".to_owned();
            }
        }
    }
}

pub fn run() -> io::Result<()> {
    let (project, startup_message) = match ProjectInfo::load() {
        Ok(project) => (project, None),
        Err(error) => (
            fallback_project_info(),
            Some(format!("No Cargo.toml found; limited mode: {error}")),
        ),
    };
    let mut terminal = setup_terminal()?;
    let mut app = App::new(project);
    if let Some(message) = startup_message {
        app.navigation.last_status = "limited mode".to_owned();
        app.navigation.message = "limited mode: cargo new is available".to_owned();
        app.set_slot_lines(
            OutputSlot::BuildLive,
            vec![
                "Limited mode".to_owned(),
                String::new(),
                message,
                String::new(),
                "This directory is not a Cargo project yet.".to_owned(),
                "Use [2]-Build Core -> new project to run cargo new <name>.".to_owned(),
                "Search is also available with s.".to_owned(),
            ],
        );
    }
    let result = run_app(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    result
}

fn fallback_project_info() -> ProjectInfo {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let name = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace")
        .to_owned();
    ProjectInfo {
        name,
        version: "<no Cargo.toml>".to_owned(),
        workspace_root: cwd.to_string_lossy().into_owned(),
        manifest_path: cwd.join("Cargo.toml").to_string_lossy().into_owned(),
        rustc_version: "<unknown>".to_owned(),
        ..ProjectInfo::default()
    }
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    let mut mouse_state = MouseState::default();
    loop {
        app.poll_disk_snapshot();
        app.drain_all_streams();
        terminal.draw(|frame| {
            mouse_state = render(frame, app);
        })?;

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) => {
                    if !app.handle_key(key) {
                        break;
                    }
                }
                Event::Mouse(mouse) => app.handle_mouse(mouse, &mouse_state),
                _ => {}
            }
        }
    }

    Ok(())
}

fn render(frame: &mut Frame<'_>, app: &mut App) -> MouseState {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(root[0]);

    let mut mouse_state = MouseState::default();
    if app.search.state.expanded {
        render_search_page(frame, app, root[0], &mut mouse_state);
    } else {
        render_left(frame, app, main[0], &mut mouse_state);
        mouse_state.panel_areas.push((Focus::Output, main[1], 0));
        render_output(frame, app, main[1], &mut mouse_state);
    }
    mouse_state
        .panel_areas
        .push((Focus::CommandLog, root[1], 0));
    render_command_log(frame, app, root[1]);
    if app.navigation.menu_open {
        render_menu(frame, app);
    }
    mouse_state
}

fn render_left(frame: &mut Frame<'_>, app: &App, area: Rect, mouse_state: &mut MouseState) {
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
        app,
        chunks[0],
        Focus::Workspace,
        workspace_items(&app.workspace.project),
        app.selection.workspace_selected,
    );
    mouse_state
        .panel_areas
        .push((Focus::Workspace, chunks[0], offset));
    let offset = render_panel(
        frame,
        app,
        chunks[1],
        Focus::Build,
        build_items()
            .into_iter()
            .map(|item| item.label.to_owned())
            .collect(),
        app.selection.build_selected,
    );
    mouse_state
        .panel_areas
        .push((Focus::Build, chunks[1], offset));
    let offset = render_panel(
        frame,
        app,
        chunks[2],
        Focus::Dependencies,
        dependency_items_for(&app.workspace.project, app.selection.workspace_selected),
        app.selection.dependency_selected,
    );
    mouse_state
        .panel_areas
        .push((Focus::Dependencies, chunks[2], offset));
}

fn render_search_page(
    frame: &mut Frame<'_>,
    app: &mut App,
    area: Rect,
    mouse_state: &mut MouseState,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(chunks[0]);

    render_search_input(frame, app, left[0]);
    mouse_state.panel_areas.push((Focus::Search, left[0], 0));

    let offset = render_panel(
        frame,
        app,
        left[1],
        Focus::Search,
        app.search.state.result_items(),
        app.search.state.selected,
    );
    mouse_state
        .panel_areas
        .push((Focus::Search, left[1], offset));
    mouse_state.panel_areas.push((
        Focus::Output,
        chunks[1],
        app.output_store
            .get(&OutputSlot::SearchDetail)
            .map(|ctx| ctx.scroll)
            .unwrap_or(0),
    ));

    let detail_lines = app.search.state.selected_detail();
    let detail_len = detail_lines.len();
    let visible_rows = chunks[1].height.saturating_sub(2) as usize;
    let detail_offset = app
        .output_store
        .get(&OutputSlot::SearchDetail)
        .map(|ctx| ctx.scroll)
        .unwrap_or(0)
        .min(detail_lines.len().saturating_sub(visible_rows));
    for (index, line) in detail_lines
        .iter()
        .skip(detail_offset)
        .take(visible_rows)
        .enumerate()
    {
        if let Some(url) = first_url(line) {
            let row = chunks[1].y.saturating_add(1 + index as u16);
            let area = Rect {
                x: chunks[1].x.saturating_add(1),
                y: row,
                width: chunks[1].width.saturating_sub(2),
                height: 1,
            };
            mouse_state.link_areas.push((area, url.to_owned()));
        }
    }

    let lines = detail_lines
        .into_iter()
        .skip(detail_offset)
        .take(visible_rows)
        .map(Line::from)
        .collect::<Vec<_>>();
    let detail_title = if app.navigation.copy_mode {
        "Search Detail  [copy mode]"
    } else {
        "Search Detail"
    };
    let widget = Paragraph::new(lines)
        .block(panel_block(
            detail_title,
            app.navigation.focus == Focus::Output,
        ))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, chunks[1]);

    let scrollbar_area = chunks[1].inner(Margin {
        vertical: 1,
        horizontal: 0,
    });
    mouse_state.right_scrollbar_area = (detail_len > visible_rows).then_some(scrollbar_area);
    mouse_state.right_scrollbar_content_len = detail_len;
    mouse_state.right_scrollbar_visible_rows = visible_rows;
    render_scrollbar(
        frame,
        scrollbar_area,
        detail_len,
        visible_rows,
        detail_offset,
    );
}

fn render_search_input(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let style = if app.navigation.input_mode == InputMode::CrateSearch {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let input = if app.search.state.query.is_empty() {
        "type crate name, Enter to search".to_owned()
    } else {
        app.search.state.query.clone()
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

fn render_panel(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    focus: Focus,
    lines: Vec<String>,
    selected: usize,
) -> usize {
    let lines = apply_filter(lines, &app.navigation.filter, app.navigation.focus == focus);
    let visible_rows = area.height.saturating_sub(2).max(1) as usize;
    let offset = list_offset(selected, visible_rows, lines.len());
    let items = lines
        .into_iter()
        .skip(offset)
        .take(visible_rows)
        .enumerate()
        .map(|(index, line)| {
            let real_index = offset + index;
            let style = if app.navigation.focus == focus && real_index == selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(line)).style(style)
        })
        .collect::<Vec<_>>();

    let widget = List::new(items).block(panel_block(focus.title(), app.navigation.focus == focus));
    frame.render_widget(widget, area);
    offset
}

fn render_output(frame: &mut Frame<'_>, app: &mut App, area: Rect, mouse_state: &mut MouseState) {
    let visible_rows = area.height.saturating_sub(2) as usize;
    let lines = output_lines(app);
    let slot = app.active_output_slot();
    let offset = app
        .output_store
        .get(&slot)
        .map(|ctx| ctx.scroll)
        .unwrap_or(0)
        .min(lines.len().saturating_sub(visible_rows));
    let rendered_lines: Vec<Line<'static>> = lines
        .iter()
        .skip(offset)
        .take(visible_rows)
        .flat_map(output_line_to_lines)
        .collect();

    let block = output_block(app, area, mouse_state);
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
    render_scrollbar(frame, scrollbar_area, lines.len(), visible_rows, offset);
}

fn render_scrollbar(
    frame: &mut Frame<'_>,
    area: Rect,
    content_len: usize,
    visible_rows: usize,
    scroll_offset: usize,
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
        .position(scroll_offset)
        .viewport_content_length(visible_rows);
    frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

fn output_block(app: &mut App, area: Rect, mouse_state: &mut MouseState) -> Block<'static> {
    let focused = app.navigation.focus == Focus::Output;
    let border_style = if focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let title = embedded_tab_title(app);
    mouse_state.tab_areas = embedded_tab_areas(app, area);

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
}

fn embedded_tab_title(app: &App) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(Span::styled(
        format!(" {} ", app.navigation.current_focus.title()),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    for (index, tab) in app.active_context_tabs().into_iter().enumerate() {
        let style = if tab == app.active_context_tab() {
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
    if app.navigation.copy_mode {
        spans.push(Span::styled(
            " [copy mode] ",
            Style::default().fg(Color::Yellow),
        ));
    }
    Line::from(spans)
}

fn embedded_tab_areas(app: &App, area: Rect) -> Vec<(Rect, ContextTab)> {
    let mut areas = Vec::new();
    let mut x = area
        .x
        .saturating_add(1 + app.navigation.current_focus.title().len() as u16 + 2);
    for (index, tab) in app.active_context_tabs().into_iter().enumerate() {
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

fn render_menu(frame: &mut Frame<'_>, app: &mut App) {
    let text = ui_text();
    let area = centered_rect(70, 64, frame.area());
    let lines = key_dialog_lines(app);
    let widget = Paragraph::new(lines).block(
        Block::default()
            .title(Span::styled(
                text.keys_title,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Green)),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(widget, area);
}

fn key_dialog_lines(app: &App) -> Vec<Line<'static>> {
    let text = ui_text();
    let version = env!("CARGO_PKG_VERSION");
    vec![
        Line::from(vec![Span::styled(
            "Navigation",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        key_line("1 / 2 / 3", "focus workspace / build / deps"),
        key_line("0 / click", "focus right waterfall"),
        key_line("Tab", "cycle focus"),
        key_line("j/k arrows", "move selection or scroll focused pane"),
        key_line("PgUp/PgDn", "scroll right waterfall"),
        Line::from(vec![Span::styled(
            "Tabs",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        key_line("[ / ]", "switch Detail / Output / Tree / Metrics"),
        key_line("click tab", "switch right tab"),
        Line::from(vec![Span::styled(
            "Actions",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        key_line("Enter", "run selected action or inspect"),
        key_line("c / b", "cargo check / build"),
        key_line("t / i", "tree / inverse tree"),
        key_line("s", "open search page"),
        key_line("/", "filter current panel"),
        key_line("m", "toggle terminal copy mode"),
        key_line("q", "back or quit"),
        Line::from(vec![Span::styled(
            "Search",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        key_line("Enter", "search input or inspect result"),
        key_line("a", "preview cargo add"),
        key_line("o/d/g", "open crates/docs/repo"),
        key_line("y", "copy selected detail"),
        Line::from(vec![
            Span::styled(
                format!("{}  ", text.version_label),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(version.to_owned()),
            Span::raw("    "),
            Span::styled(text.close_keys, Style::default().fg(Color::Green)),
            Span::raw(" close"),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}  ", text.status_label),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(app.navigation.last_status.clone()),
        ]),
    ]
}

fn key_line(key: &'static str, label: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<12}"), Style::default().fg(Color::Green)),
        Span::raw(label),
    ])
}

fn render_command_log(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let version_text = format!(" v{}", env!("CARGO_PKG_VERSION"));
    let version_width = version_text.len() as u16;
    let version_x = area
        .x
        .saturating_add(area.width.saturating_sub(version_width));

    let line = match app.navigation.input_mode {
        InputMode::CrateSearch => Line::from(vec![
            Span::styled("search: ", Style::default().fg(Color::Yellow)),
            Span::raw("Enter search, Esc results, q back, x keys"),
            Span::styled(
                format!("  {}", app.navigation.last_status),
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Filter => Line::from(vec![
            Span::styled("filter: ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.navigation.filter),
            Span::styled(
                "  Enter apply, Esc cancel",
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::ProjectNew => Line::from(vec![
            Span::styled("new: ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.navigation.new_project_name),
            Span::styled(
                "  Enter create, Esc cancel",
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Normal if app.navigation.copy_mode => Line::from(vec![
            Span::styled("copy: ", Style::default().fg(Color::Yellow)),
            Span::raw("drag select, m mouse, x keys"),
            Span::styled(
                format!("  {}", app.navigation.last_status),
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Normal if app.search.state.expanded => Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::raw(": inspect, "),
            Span::styled("a", Style::default().fg(Color::Green)),
            Span::raw(": add, "),
            Span::styled("q", Style::default().fg(Color::Green)),
            Span::raw(": back, "),
            Span::styled("x", Style::default().fg(Color::Green)),
            Span::raw(": keys"),
            Span::styled(
                format!("  {}", app.navigation.last_status),
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Normal => Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::raw(": run/inspect, "),
            Span::styled("s", Style::default().fg(Color::Green)),
            Span::raw(": search, "),
            Span::styled("[ ]", Style::default().fg(Color::Green)),
            Span::raw(": tabs, "),
            Span::styled("x", Style::default().fg(Color::Green)),
            Span::raw(": keys, "),
            Span::styled("q", Style::default().fg(Color::Green)),
            Span::raw(": quit"),
            Span::styled(
                format!("  {}", app.navigation.last_status),
                Style::default().fg(Color::Green),
            ),
        ]),
    };

    frame.render_widget(Paragraph::new(line), area);
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

fn selected_dependency_name(
    project: &ProjectInfo,
    workspace_selected: usize,
    selected: usize,
) -> Option<String> {
    let dependencies = if workspace_selected == 0 {
        project
            .workspace_packages
            .first()
            .map(|package| package.dependencies.as_slice())
            .unwrap_or(project.dependencies.as_slice())
    } else {
        project
            .workspace_packages
            .get(workspace_selected.saturating_sub(1))
            .map(|package| package.dependencies.as_slice())
            .unwrap_or(project.dependencies.as_slice())
    };

    dependencies
        .get(selected)
        .map(|dependency| dependency.name.clone())
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);
    horizontal[1]
}

fn project_health_snapshot(project: &ProjectInfo, disk: &DiskSnapshot) -> Vec<String> {
    vec![
        "Project health snapshot".to_owned(),
        String::new(),
        format!("project: {} {}", project.name, project.version),
        format!("workspace root: {}", project.workspace_root),
        format!("rustc: {}", project.rustc_version),
        format!(
            "msrv: {}",
            project
                .workspace_packages
                .first()
                .and_then(|package| package.rust_version.as_deref())
                .unwrap_or("<not declared>")
        ),
        String::new(),
        "Disk".to_owned(),
        format!("  target total: {}", disk.total_label),
        format!("  workspace packages: {}", project.workspace_packages.len()),
        format!("  direct dependencies: {}", project.dependencies.len()),
        String::new(),
        "Hot paths".to_owned(),
        "  [1] Workspace: package source/target cache snapshot".to_owned(),
        "  [3] Dependencies: feature state tree and inverse tree".to_owned(),
        "  s: crates.io package search".to_owned(),
    ]
}

fn load_disk_snapshot_async(project: ProjectInfo) -> Receiver<DiskSnapshot> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let root = Path::new(&project.workspace_root);
        let _ = tx.send(target_analyzer::analyze_target(root, &project.packages));
    });
    rx
}

fn dir_size(path: &std::path::Path) -> io::Result<u64> {
    let mut total = 0;
    if !path.exists() {
        return Ok(0);
    }
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            total += dir_size(&entry.path())?;
        } else {
            total += metadata.len();
        }
    }
    Ok(total)
}

fn latest_crate_timings(root: &Path) -> Vec<CrateTiming> {
    let timings_dir = root.join("target").join("cargo-timings");
    let Ok(entries) = fs::read_dir(timings_dir) else {
        return Vec::new();
    };
    let latest = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path);
    let Some(path) = latest else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let mut timings = Vec::new();
    collect_crate_timings(&value, &mut timings);
    timings.sort_by_key(|timing| Reverse(timing.duration_ms));
    timings.truncate(50);
    timings
}

fn collect_crate_timings(value: &serde_json::Value, timings: &mut Vec<CrateTiming>) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                collect_crate_timings(item, timings);
            }
        }
        serde_json::Value::Object(map) => {
            let name = map
                .get("name")
                .or_else(|| map.get("crate"))
                .or_else(|| map.get("target"))
                .and_then(serde_json::Value::as_str);
            let duration = map
                .get("duration_ms")
                .and_then(serde_json::Value::as_u64)
                .or_else(|| {
                    map.get("duration")
                        .and_then(serde_json::Value::as_f64)
                        .map(|value| (value * 1000.0) as u64)
                });
            if let (Some(name), Some(duration_ms)) = (name, duration) {
                timings.push(CrateTiming {
                    name: name.to_owned(),
                    duration_ms,
                    is_build: true,
                });
            }
            for value in map.values() {
                collect_crate_timings(value, timings);
            }
        }
        _ => {}
    }
}

fn output_line_to_lines(line: &String) -> Vec<Line<'static>> {
    if line.contains('\u{1b}') {
        return line
            .into_text()
            .map(|text| text.lines)
            .unwrap_or_else(|_| vec![semantic_output_line(line)]);
    }
    vec![semantic_output_line(line)]
}

fn semantic_output_line(raw: &str) -> Line<'static> {
    let trimmed = raw.trim();
    let lower = trimmed.to_lowercase();

    if trimmed.is_empty() {
        return Line::from(String::new());
    }

    if let Some(line) = feature_marker_line(raw) {
        return line;
    }
    if lower.starts_with('$') {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Magenta),
        ));
    }
    if lower.starts_with("exit:") || lower.starts_with("duration:") {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Cyan),
        ));
    }
    if lower.contains("error")
        || lower.contains("failed")
        || lower.contains("panic")
        || lower.contains("exit status: 1")
    {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    if lower.contains("warning") || lower.contains("unused") {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if looks_like_tree_line(raw) {
        return tree_output_line(raw);
    }
    if first_url(raw).is_some() {
        return url_output_line(raw);
    }
    if is_section_heading(trimmed) {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if let Some((key, value)) = raw.split_once(':') {
        return key_value_line(key, value);
    }
    if lower.contains(" v") || lower.starts_with("version ") || lower.starts_with("version:") {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Gray),
        ));
    }
    if lower.contains('/') && (lower.starts_with("  ") || lower.starts_with('(')) {
        return Line::from(Span::styled(
            raw.to_owned(),
            Style::default().fg(Color::Cyan),
        ));
    }
    Line::from(raw.to_owned())
}

fn feature_marker_line(raw: &str) -> Option<Line<'static>> {
    let marker_index = raw.find('[')?;
    let marker = raw.get(marker_index..marker_index.saturating_add(3))?;
    let style = match marker {
        "[x]" => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        "[-]" => Style::default().fg(Color::Yellow),
        "[ ]" => Style::default().fg(Color::DarkGray),
        _ => return None,
    };
    Some(Line::from(vec![
        Span::raw(raw[..marker_index].to_owned()),
        Span::styled(marker.to_owned(), style),
        Span::styled(raw[marker_index + 3..].to_owned(), style),
    ]))
}

fn is_section_heading(trimmed: &str) -> bool {
    matches!(
        trimmed,
        "Actions"
            | "Build"
            | "Cargo metrics"
            | "Dependencies"
            | "Disk"
            | "Disk tracking"
            | "Effect"
            | "Feature state"
            | "Health snapshot"
            | "Hot paths"
            | "Links"
            | "Local path"
            | "Members"
            | "Package disk snapshot"
            | "Package identity"
            | "Project health snapshot"
            | "Targets"
            | "Workspace metrics"
            | "Workspace scope"
    )
}

fn key_value_line(key: &str, value: &str) -> Line<'static> {
    let value_style = if value.contains("http://") || value.contains("https://") {
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::UNDERLINED)
    } else if value.contains('/') {
        Style::default().fg(Color::Cyan)
    } else if value.contains("unknown") || value.contains("<") {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(
            key.to_owned(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(":"),
        Span::styled(value.to_owned(), value_style),
    ])
}

fn url_output_line(raw: &str) -> Line<'static> {
    let mut spans = Vec::new();
    for part in raw.split_inclusive(' ') {
        let style = if part.starts_with("http://") || part.starts_with("https://") {
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
        };
        spans.push(Span::styled(part.to_owned(), style));
    }
    Line::from(spans)
}

fn looks_like_tree_line(raw: &str) -> bool {
    raw.contains("├")
        || raw.contains("└")
        || raw.contains("│")
        || raw.contains("──")
        || raw.contains(" (*)")
}

fn tree_output_line(raw: &str) -> Line<'static> {
    let split_at = raw
        .char_indices()
        .find(|(_, ch)| ch.is_alphanumeric() || *ch == '_' || *ch == '-')
        .map(|(index, _)| index)
        .unwrap_or(0);
    let (tree_prefix, rest) = raw.split_at(split_at);
    let mut spans = vec![Span::styled(
        tree_prefix.to_owned(),
        Style::default().fg(Color::DarkGray),
    )];
    for part in rest.split_inclusive(' ') {
        let style =
            if part.starts_with('v') || part.contains("(*)") || part.contains("(proc-macro)") {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
        spans.push(Span::styled(part.to_owned(), style));
    }
    Line::from(spans)
}

fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn panel_block(title: impl Into<String>, focused: bool) -> Block<'static> {
    let style = if focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    Block::default()
        .title(Span::styled(title.into(), style))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(style)
}
