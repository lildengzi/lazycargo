use std::fs::{self, File};
use std::io;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    Block, BorderType, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::{Frame, Terminal};

use crate::metadata::ProjectInfo;
use lazycargo_search::{
    crate_info_detail, extract_crate_author, parse_crate_search_results, search_error_detail,
    search_timeout_detail, CrateInfoReport, SearchLinkTarget, SearchState,
};

mod dashboard;
mod runner;
mod terminal_support;

use dashboard::{
    apply_filter, build_items, dependency_items_for, list_offset, output_lines, workspace_items,
};
use runner::{command_output_with_timeout, extract_diagnostics, split_output};
use terminal_support::{copy_to_clipboard, first_url, open_url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
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
enum InputMode {
    Normal,
    Filter,
    CrateSearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusPanel {
    Workspace,
    BuildCore,
    Dependencies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceTab {
    CrateInfo,
    Metrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildCoreTab {
    TaskConfig,
    LiveOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DependenciesTab {
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
            Self::Build(BuildCoreTab::TaskConfig) => "Task Config",
            Self::Build(BuildCoreTab::LiveOutput) => "Live Output",
            Self::Dependencies(DependenciesTab::Features) => "Features",
            Self::Dependencies(DependenciesTab::DependencyTree) => "Dependency Tree",
        }
    }
}

struct HistoryEntry {
    command: String,
    success: bool,
    duration: Duration,
}

struct DiskSnapshot {
    target_size: String,
    packages: Vec<PackageDiskInfo>,
}

struct PackageDiskInfo {
    name: String,
    source_size: String,
    target_cache: String,
}

struct App {
    project: ProjectInfo,
    disk: DiskSnapshot,
    focus: Focus,
    current_focus: FocusPanel,
    input_mode: InputMode,
    ws_tab: WorkspaceTab,
    build_tab: BuildCoreTab,
    deps_tab: DependenciesTab,
    menu_open: bool,
    menu_selected: usize,
    filter: String,
    search: SearchState,
    search_return_focus: Focus,
    command_preview: String,
    message: String,
    output: Vec<String>,
    build_detail: Vec<String>,
    dependency_detail: Vec<String>,
    tree_detail: Vec<String>,
    package_detail: Vec<String>,
    diagnostics: Vec<String>,
    history: Vec<HistoryEntry>,
    last_status: String,
    panel_areas: Vec<(Focus, Rect, usize)>,
    link_areas: Vec<(Rect, String)>,
    tab_areas: Vec<(Rect, ContextTab)>,
    right_scrollbar_area: Option<Rect>,
    right_scrollbar_content_len: usize,
    right_scrollbar_visible_rows: usize,
    copy_mode: bool,
    right_scroll: usize,
    workspace_selected: usize,
    dependency_selected: usize,
    build_selected: usize,
    running_child: Option<Child>,
    running_command: String,
    running_start: Instant,
    running_focus: Focus,
    running_stdout_path: Option<PathBuf>,
    running_stderr_path: Option<PathBuf>,
}

impl App {
    fn new(project: ProjectInfo) -> Self {
        let command_preview = "cargo check".to_owned();
        let disk = DiskSnapshot::load(&project);
        let output = project_health_snapshot(&project, &disk);
        Self {
            project,
            disk,
            focus: Focus::Workspace,
            current_focus: FocusPanel::Workspace,
            input_mode: InputMode::Normal,
            ws_tab: WorkspaceTab::CrateInfo,
            build_tab: BuildCoreTab::TaskConfig,
            deps_tab: DependenciesTab::Features,
            menu_open: false,
            menu_selected: 0,
            filter: String::new(),
            search: SearchState::default(),
            search_return_focus: Focus::Workspace,
            command_preview,
            message: "ready".to_owned(),
            output,
            build_detail: vec!["no build/check run yet".to_owned()],
            dependency_detail: vec![
                "select dependency, then press t for tree or i for inverse tree".to_owned(),
            ],
            tree_detail: vec!["no dependency tree yet".to_owned()],
            package_detail: vec!["no package action yet".to_owned()],
            diagnostics: Vec::new(),
            history: Vec::new(),
            last_status: "ready".to_owned(),
            panel_areas: Vec::new(),
            link_areas: Vec::new(),
            tab_areas: Vec::new(),
            right_scrollbar_area: None,
            right_scrollbar_content_len: 0,
            right_scrollbar_visible_rows: 0,
            copy_mode: false,
            right_scroll: 0,
            workspace_selected: 0,
            dependency_selected: 0,
            build_selected: 0,
            running_child: None,
            running_command: String::new(),
            running_start: Instant::now(),
            running_focus: Focus::Build,
            running_stdout_path: None,
            running_stderr_path: None,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if self.menu_open {
            return self.handle_menu_key(key);
        }

        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Filter => self.handle_filter_key(key),
            InputMode::CrateSearch => self.handle_crate_search_key(key),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.copy_mode {
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.mouse_over_focus(mouse.column, mouse.row) == Some(Focus::Output) {
                    self.scroll_right(-3);
                    return;
                }
            }
            MouseEventKind::ScrollDown => {
                if self.mouse_over_focus(mouse.column, mouse.row) == Some(Focus::Output) {
                    self.scroll_right(3);
                    return;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.scrollbar_to_row(mouse.column, mouse.row) {
                    return;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.scrollbar_to_row(mouse.column, mouse.row);
                return;
            }
            _ => return,
        }

        if let Some((_, url)) = self
            .link_areas
            .iter()
            .find(|(area, _)| contains(*area, mouse.column, mouse.row))
            .cloned()
        {
            self.open_url_message(&url);
            return;
        }

        if let Some((_, tab)) = self
            .tab_areas
            .iter()
            .find(|(area, _)| contains(*area, mouse.column, mouse.row))
            .copied()
        {
            self.apply_context_tab(tab);
            self.set_focus(Focus::Output);
            self.message = format!("selected {} tab", tab.label());
            return;
        }

        if let Some((focus, area, offset)) = self
            .panel_areas
            .iter()
            .find(|(_, area, _)| contains(*area, mouse.column, mouse.row))
            .copied()
        {
            if self.search.expanded
                && focus == Focus::Search
                && area.height <= 3
                && mouse.row < area.y.saturating_add(area.height)
            {
                self.input_mode = InputMode::CrateSearch;
                self.message = "search input focused".to_owned();
                return;
            }

            self.set_focus(focus);
            if focus == Focus::Output {
                self.message = "focused right detail".to_owned();
                return;
            }
            let row = offset + mouse.row.saturating_sub(area.y).saturating_sub(1) as usize;
            self.select_row(focus, row);
            if focus == Focus::Search {
                self.right_scroll = 0;
            }
            self.message = format!("focused {}", focus.title());
        }
    }

    fn mouse_over_focus(&self, column: u16, row: u16) -> Option<Focus> {
        self.panel_areas
            .iter()
            .find(|(_, area, _)| contains(*area, column, row))
            .map(|(focus, _, _)| *focus)
    }

    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        match focus {
            Focus::Workspace => self.current_focus = FocusPanel::Workspace,
            Focus::Build => self.current_focus = FocusPanel::BuildCore,
            Focus::Dependencies => self.current_focus = FocusPanel::Dependencies,
            _ => {}
        }
    }

    fn active_context_tabs(&self) -> Vec<ContextTab> {
        match self.current_focus {
            FocusPanel::Workspace => vec![
                ContextTab::Workspace(WorkspaceTab::CrateInfo),
                ContextTab::Workspace(WorkspaceTab::Metrics),
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
        match self.current_focus {
            FocusPanel::Workspace => ContextTab::Workspace(self.ws_tab),
            FocusPanel::BuildCore => ContextTab::Build(self.build_tab),
            FocusPanel::Dependencies => ContextTab::Dependencies(self.deps_tab),
        }
    }

    fn apply_context_tab(&mut self, tab: ContextTab) {
        match tab {
            ContextTab::Workspace(tab) => {
                self.current_focus = FocusPanel::Workspace;
                self.ws_tab = tab;
            }
            ContextTab::Build(tab) => {
                self.current_focus = FocusPanel::BuildCore;
                self.build_tab = tab;
            }
            ContextTab::Dependencies(tab) => {
                self.current_focus = FocusPanel::Dependencies;
                self.deps_tab = tab;
            }
        }
        self.right_scroll = 0;
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
        if self.focus != Focus::Search {
            self.search_return_focus = self.focus;
        }
        self.set_focus(Focus::Search);
        self.search.expanded = true;
        self.right_scroll = 0;
        self.input_mode = InputMode::CrateSearch;
    }

    fn scroll_right(&mut self, delta: isize) {
        self.set_focus(Focus::Output);
        let max_scroll = self
            .right_scrollbar_content_len
            .saturating_sub(self.right_scrollbar_visible_rows);
        self.right_scroll = (self.right_scroll as isize + delta)
            .max(0)
            .min(max_scroll as isize) as usize;
        self.message = format!("right detail scroll: {}", self.right_scroll);
    }

    fn scrollbar_to_row(&mut self, column: u16, row: u16) -> bool {
        let Some(area) = self.right_scrollbar_area else {
            return false;
        };
        if !contains(area, column, row) {
            return false;
        }

        let max_scroll = self
            .right_scrollbar_content_len
            .saturating_sub(self.right_scrollbar_visible_rows);
        if max_scroll == 0 {
            return true;
        }

        let relative = row.saturating_sub(area.y) as usize;
        let track = area.height.saturating_sub(1).max(1) as usize;
        self.right_scroll = (relative * max_scroll / track).min(max_scroll);
        self.set_focus(Focus::Output);
        self.message = format!("right detail scroll: {}", self.right_scroll);
        true
    }

    fn toggle_copy_mode(&mut self) {
        let next = !self.copy_mode;
        let result = if next {
            execute!(io::stdout(), DisableMouseCapture)
        } else {
            execute!(io::stdout(), EnableMouseCapture)
        };

        match result {
            Ok(()) => {
                self.copy_mode = next;
                if self.copy_mode {
                    self.set_focus(Focus::Output);
                    self.message =
                        "copy mode: terminal mouse selection enabled; press m to restore"
                            .to_owned();
                    self.last_status = "copy mode".to_owned();
                } else {
                    self.message = "mouse interaction restored".to_owned();
                    self.last_status = "mouse mode".to_owned();
                }
            }
            Err(error) => {
                self.message = format!("failed to toggle copy mode: {error}");
                self.last_status = "copy mode failed".to_owned();
            }
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.kill_running_child()
            }
            KeyCode::Char('q') if self.search.expanded => {
                self.search.expanded = false;
                self.set_focus(self.search_return_focus);
                self.message = "back from search".to_owned();
            }
            KeyCode::Esc if self.search.expanded => {
                self.search.expanded = false;
                self.set_focus(self.search_return_focus);
                self.message = "back from search".to_owned();
            }
            KeyCode::Char('q') => return false,
            KeyCode::Char('x') => {
                self.menu_open = true;
                self.menu_selected = 0;
            }
            KeyCode::Tab => self.set_focus(self.focus.next()),
            KeyCode::BackTab => self.set_focus(Focus::Output),
            KeyCode::Char(']') => self.switch_context_tab(1),
            KeyCode::Char('[') => self.switch_context_tab(-1),
            KeyCode::Char('m') => self.toggle_copy_mode(),
            KeyCode::Char('0') => self.set_focus(Focus::Output),
            KeyCode::Char(value @ ('1'..='3')) => self.set_focus(Focus::from_digit(value)),
            KeyCode::PageUp => self.scroll_right(-10),
            KeyCode::PageDown => self.scroll_right(10),
            KeyCode::Up | KeyCode::Char('k') if self.focus == Focus::Output => {
                self.scroll_right(-1)
            }
            KeyCode::Down | KeyCode::Char('j') if self.focus == Focus::Output => {
                self.scroll_right(1)
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Enter => self.activate_selection(),
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Filter;
                self.filter.clear();
                self.message = format!("filter {}: ", self.focus.title());
            }
            KeyCode::Char('c') => self.run_cargo(Focus::Build, &["check"]),
            KeyCode::Char('b') => self.run_cargo(Focus::Build, &["build"]),
            KeyCode::Char('t') => self.run_tree(),
            KeyCode::Char('i') => self.run_inverse_tree(),
            KeyCode::Char('a') => self.preview_add(),
            KeyCode::Char('o') if self.search.expanded => {
                self.open_search_link(SearchLinkTarget::Crates)
            }
            KeyCode::Char('d') if self.search.expanded => {
                self.open_search_link(SearchLinkTarget::Docs)
            }
            KeyCode::Char('g') if self.search.expanded => {
                self.open_search_link(SearchLinkTarget::Repository)
            }
            KeyCode::Char('y') if self.search.expanded => self.copy_search_detail(),
            KeyCode::Char('s') => {
                self.open_search();
                self.message = "search crates".to_owned();
            }
            _ => {}
        }

        true
    }

    fn handle_menu_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Char('x') | KeyCode::Enter => self.menu_open = false,
            _ => {}
        }

        true
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.filter.clear();
                self.message = "filter cancelled".to_owned();
            }
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                self.message = format!("filter applied to {}: {}", self.focus.title(), self.filter);
            }
            KeyCode::Backspace => {
                self.filter.pop();
            }
            KeyCode::Char(value) => self.filter.push(value),
            _ => {}
        }

        true
    }

    fn handle_crate_search_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.message = "search input blurred".to_owned();
            }
            KeyCode::Enter => {
                let query = self.search.query.trim().to_owned();
                self.input_mode = InputMode::Normal;
                if query.is_empty() {
                    self.message = "empty crate search".to_owned();
                } else {
                    self.search_crates(&query);
                }
            }
            KeyCode::Backspace => {
                self.search.query.pop();
            }
            KeyCode::Char(value) => self.search.query.push(value),
            _ => {}
        }

        true
    }

    fn preview(&mut self, command: &str) {
        self.command_preview = command.to_owned();
        self.message = format!("preview: {command}");
    }

    fn move_selection(&mut self, delta: isize) {
        let len = match self.focus {
            Focus::Workspace => workspace_items(&self.project).len(),
            Focus::Dependencies => {
                dependency_items_for(&self.project, self.workspace_selected).len()
            }
            Focus::Search => self.search.result_items().len(),
            Focus::Build => build_items().len(),
            _ => 0,
        };

        if len == 0 {
            return;
        }

        let selected = self.selected_mut(self.focus);
        *selected = ((*selected as isize + delta).rem_euclid(len as isize)) as usize;
        if self.focus == Focus::Search {
            self.right_scroll = 0;
        }
        if self.focus == Focus::Workspace {
            self.sync_workspace_selection();
        }
        if self.focus == Focus::Dependencies {
            self.deps_tab = DependenciesTab::Features;
            self.right_scroll = 0;
        }
    }

    fn select_row(&mut self, focus: Focus, row: usize) {
        let len = match focus {
            Focus::Workspace => workspace_items(&self.project).len(),
            Focus::Dependencies => {
                dependency_items_for(&self.project, self.workspace_selected).len()
            }
            Focus::Search => self.search.result_items().len(),
            Focus::Build => build_items().len(),
            _ => 0,
        };

        if len > 0 {
            *self.selected_mut(focus) = row.min(len.saturating_sub(1));
            if focus == Focus::Workspace {
                self.sync_workspace_selection();
            }
            if focus == Focus::Dependencies {
                self.deps_tab = DependenciesTab::Features;
                self.right_scroll = 0;
            }
        }
    }

    fn sync_workspace_selection(&mut self) {
        self.dependency_selected = 0;
        self.right_scroll = 0;
        self.deps_tab = DependenciesTab::Features;
        self.dependency_detail = vec![format!(
            "workspace scope changed: {}",
            self.selected_scope_label()
        )];
        self.tree_detail = vec!["dependency tree not loaded for current scope".to_owned()];
    }

    fn selected_scope_label(&self) -> String {
        if self.workspace_selected == 0 {
            "workspace".to_owned()
        } else {
            self.project
                .packages
                .get(self.workspace_selected.saturating_sub(1))
                .map(|package| format!("package {package}"))
                .unwrap_or_else(|| "workspace".to_owned())
        }
    }

    fn selected_mut(&mut self, focus: Focus) -> &mut usize {
        match focus {
            Focus::Workspace => &mut self.workspace_selected,
            Focus::Dependencies => &mut self.dependency_selected,
            Focus::Search => &mut self.search.selected,
            Focus::Build => &mut self.build_selected,
            _ => &mut self.workspace_selected,
        }
    }

    fn activate_selection(&mut self) {
        match self.focus {
            Focus::Workspace => {
                if let Some(scope) = workspace_items(&self.project).get(self.workspace_selected) {
                    self.preview(&format!("scope: {scope}"));
                }
            }
            Focus::Dependencies => self.inspect_dependency(),
            Focus::Search => {
                if self.input_mode == InputMode::CrateSearch || self.search.results.is_empty() {
                    self.search.expanded = true;
                    self.input_mode = InputMode::CrateSearch;
                    self.message = "search crates".to_owned();
                } else {
                    self.inspect_selected_crate();
                }
            }
            Focus::Build => match build_items().get(self.build_selected).map(|item| item.key) {
                Some("check") => self.run_cargo(Focus::Build, &["check"]),
                Some("build") => self.run_cargo(Focus::Build, &["build"]),
                Some("test") => self.run_cargo(Focus::Build, &["test"]),
                Some("run") => self.run_cargo(Focus::Build, &["run"]),
                Some("release") => self.run_cargo(Focus::Build, &["build", "--release"]),
                Some("clippy") => self.run_cargo(Focus::Build, &["clippy", "--all-targets"]),
                Some("doc") => self.run_cargo(Focus::Build, &["doc", "--no-deps"]),
                Some("update") => self.run_cargo(Focus::Build, &["update"]),
                Some("clean") => self.run_cargo(Focus::Build, &["clean"]),
                Some("timings") => self.run_cargo(Focus::Build, &["build", "--timings"]),
                Some("diagnostics") => self.build_tab = BuildCoreTab::LiveOutput,
                _ => {}
            },
            _ => {}
        }
    }

    fn run_cargo(&mut self, detail_focus: Focus, args: &[&str]) {
        if self.running_child.is_some() {
            self.message = format!("already running: {}", self.running_command);
            return;
        }

        let args = self.scoped_args(args);
        let command = format!("cargo {}", args.join(" "));
        self.command_preview = command.clone();
        self.message = format!("running: {command}");
        self.output = vec![format!("Running: {command}...")];
        self.set_focus(detail_focus);
        if detail_focus == Focus::Build {
            self.build_tab = BuildCoreTab::LiveOutput;
        }
        self.right_scroll = 0;

        let (stdout_path, stderr_path) = cargo_output_paths();
        let stdout_file = match File::create(&stdout_path) {
            Ok(file) => file,
            Err(error) => {
                self.last_status = "error".to_owned();
                self.output = vec![format!("failed to create cargo stdout log: {error}")];
                self.set_detail(detail_focus, self.output.clone());
                self.message = format!("failed: {command}");
                return;
            }
        };
        let stderr_file = match File::create(&stderr_path) {
            Ok(file) => file,
            Err(error) => {
                let _ = fs::remove_file(&stdout_path);
                self.last_status = "error".to_owned();
                self.output = vec![format!("failed to create cargo stderr log: {error}")];
                self.set_detail(detail_focus, self.output.clone());
                self.message = format!("failed: {command}");
                return;
            }
        };

        match Command::new("cargo")
            .env("CARGO_TERM_COLOR", "always")
            .args(&args)
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .spawn()
        {
            Ok(child) => {
                self.running_child = Some(child);
                self.running_command = command;
                self.running_start = Instant::now();
                self.running_focus = detail_focus;
                self.running_stdout_path = Some(stdout_path);
                self.running_stderr_path = Some(stderr_path);
            }
            Err(error) => {
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                self.last_status = "error".to_owned();
                self.output = vec![format!("failed to run {command}: {error}")];
                self.set_detail(detail_focus, self.output.clone());
                self.diagnostics = vec![format!("runner error: {error}")];
                self.message = format!("failed: {command}");
            }
        }
    }

    fn poll_running_child(&mut self) {
        let Some(child) = &mut self.running_child else {
            return;
        };

        match child.try_wait() {
            Ok(Some(status)) => {
                let Some(child) = self.running_child.take() else {
                    return;
                };
                drop(child);
                let command = std::mem::take(&mut self.running_command);
                let duration = self.running_start.elapsed();
                let detail_focus = self.running_focus;
                let stdout_path = self.running_stdout_path.take();
                let stderr_path = self.running_stderr_path.take();
                let stdout = self.read_running_output(stdout_path);
                let stderr = self.read_running_output(stderr_path);
                self.finish_cargo_output(detail_focus, command, duration, status, stdout, stderr);
            }
            Ok(None) => {}
            Err(error) => {
                self.message = format!("wait failed: {error}");
                self.last_status = "wait failed".to_owned();
                self.running_child = None;
                self.running_command.clear();
                self.clear_running_output_files();
            }
        }
    }

    fn read_running_output(&mut self, path: Option<PathBuf>) -> Vec<u8> {
        let Some(path) = path else {
            return Vec::new();
        };
        let bytes = fs::read(&path).unwrap_or_else(|error| {
            self.diagnostics.push(format!(
                "failed to read cargo output {}: {error}",
                path.display()
            ));
            Vec::new()
        });
        let _ = fs::remove_file(path);
        bytes
    }

    fn finish_cargo_output(
        &mut self,
        detail_focus: Focus,
        command: String,
        duration: Duration,
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    ) {
        let mut lines = Vec::new();
        lines.push(format!("$ {command}"));
        lines.push(format!("exit: {status}"));
        lines.push(format!("duration: {:.2}s", duration.as_secs_f32()));
        lines.push(String::new());
        lines.extend(split_output(&stdout));
        lines.extend(split_output(&stderr));

        let success = status.success();
        self.last_status = if success {
            format!("ok {:.2}s", duration.as_secs_f32())
        } else {
            format!("failed {:.2}s", duration.as_secs_f32())
        };
        self.diagnostics = extract_diagnostics(&lines);
        self.output = lines;
        self.set_detail(detail_focus, self.output.clone());
        self.history.insert(
            0,
            HistoryEntry {
                command: command.clone(),
                success,
                duration,
            },
        );
        self.history.truncate(20);
        self.message = format!("finished: {command}");
    }

    fn kill_running_child(&mut self) {
        let Some(mut child) = self.running_child.take() else {
            return;
        };

        let _ = child.kill();
        let _ = child.wait();
        self.message = "killed".to_owned();
        self.last_status = "killed".to_owned();
        self.output = vec![format!("killed: {}", self.running_command)];
        self.set_detail(self.running_focus, self.output.clone());
        self.running_command.clear();
        self.clear_running_output_files();
    }

    fn clear_running_output_files(&mut self) {
        if let Some(path) = self.running_stdout_path.take() {
            let _ = fs::remove_file(path);
        }
        if let Some(path) = self.running_stderr_path.take() {
            let _ = fs::remove_file(path);
        }
    }

    fn scoped_args(&self, args: &[&str]) -> Vec<String> {
        let mut result = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let Some(command) = result.first().map(String::as_str) else {
            return result;
        };

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
        if self.workspace_selected == 0 {
            None
        } else {
            self.project
                .packages
                .get(self.workspace_selected.saturating_sub(1))
                .cloned()
        }
    }

    fn set_detail(&mut self, focus: Focus, lines: Vec<String>) {
        match focus {
            Focus::Build => self.build_detail = lines,
            Focus::Dependencies => {
                self.dependency_detail = lines.clone();
                self.tree_detail = lines;
            }
            Focus::Search => self.package_detail = lines,
            _ => self.output = lines,
        }
    }

    fn search_crates(&mut self, query: &str) {
        let command = format!("cargo search {query} --limit 100");
        self.command_preview = command.clone();
        self.message = format!("searching crates: {query}");
        self.focus = Focus::Search;
        self.search.expanded = true;

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
                self.last_status = if success {
                    format!("search ok {:.2}s", duration.as_secs_f32())
                } else {
                    format!("search failed {:.2}s", duration.as_secs_f32())
                };
                if success {
                    self.search
                        .set_results(parse_crate_search_results(&lines, query));
                } else {
                    self.search.set_empty_detail(lines.clone());
                }
                self.package_detail = lines.clone();
                self.output = lines;
                self.history.insert(
                    0,
                    HistoryEntry {
                        command,
                        success,
                        duration,
                    },
                );
                self.history.truncate(20);
                self.message = format!("searched crates: {query}");
            }
            Ok(None) => {
                let lines = search_timeout_detail(&command, duration.as_secs_f32());
                self.last_status = "search timeout".to_owned();
                self.search.set_empty_detail(lines.clone());
                self.package_detail = lines.clone();
                self.output = lines;
                self.history.insert(
                    0,
                    HistoryEntry {
                        command,
                        success: false,
                        duration,
                    },
                );
                self.history.truncate(20);
                self.message = format!("search timed out: {query}");
            }
            Err(error) => {
                self.last_status = "search error".to_owned();
                let lines = search_error_detail(&command, &error.to_string());
                self.search.set_empty_detail(lines.clone());
                self.package_detail = lines;
                self.output = self.package_detail.clone();
                self.message = format!("crate search failed: {query}");
            }
        }
    }

    fn inspect_dependency(&mut self) {
        let Some(dependency) = selected_dependency_name(
            &self.project,
            self.workspace_selected,
            self.dependency_selected,
        ) else {
            self.dependency_detail = vec!["no dependency selected".to_owned()];
            return;
        };

        self.command_preview = format!("cargo tree -i {dependency}");
        self.dependency_detail = vec![
            format!("dependency: {dependency}"),
            String::new(),
            "enter: inspect".to_owned(),
            "t: cargo tree".to_owned(),
            "i: cargo tree -i <dependency>".to_owned(),
            "a: preview cargo add".to_owned(),
        ];
        self.message = format!("selected dependency: {dependency}");
        self.deps_tab = DependenciesTab::Features;
    }

    fn run_tree(&mut self) {
        self.run_cargo(Focus::Dependencies, &["tree"]);
        self.tree_detail = self.output.clone();
        self.deps_tab = DependenciesTab::DependencyTree;
    }

    fn run_inverse_tree(&mut self) {
        let Some(dependency) = selected_dependency_name(
            &self.project,
            self.workspace_selected,
            self.dependency_selected,
        ) else {
            self.dependency_detail = vec!["select a dependency first".to_owned()];
            return;
        };

        self.run_cargo(Focus::Dependencies, &["tree", "-i", &dependency]);
        self.tree_detail = self.output.clone();
        self.deps_tab = DependenciesTab::DependencyTree;
    }

    fn preview_add(&mut self) {
        let selected_name = self
            .search
            .selected_result()
            .map(|result| result.name.as_str())
            .or_else(|| {
                let query = self.search.query.trim();
                (!query.is_empty()).then_some(query)
            })
            .unwrap_or("<crate>");
        let command = match self.selected_package() {
            Some(package) => format!("cargo add {selected_name} -p {package}"),
            None => format!("cargo add {selected_name}"),
        };
        self.preview(&command);
        self.package_detail = self
            .search
            .selected_detail()
            .into_iter()
            .chain(vec![String::new(), command])
            .collect();
    }

    fn inspect_selected_crate(&mut self) {
        let Some(result) = self.search.selected_result().cloned() else {
            self.package_detail = vec!["no crate selected".to_owned()];
            return;
        };

        let command = format!("cargo info {}", result.name);
        self.command_preview = command.clone();
        self.message = format!("inspecting crate: {}", result.name);
        self.search.expanded = true;

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
                self.last_status = if success {
                    format!("info ok {:.2}s", duration.as_secs_f32())
                } else {
                    format!("info failed {:.2}s", duration.as_secs_f32())
                };
                if let Some(author) = extract_crate_author(&lines) {
                    self.search.set_selected_author(author);
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
                self.output = lines;
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
            Ok(None) => {
                self.last_status = "info timeout".to_owned();
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
                self.output = detail.clone();
                self.history.insert(
                    0,
                    HistoryEntry {
                        command,
                        success: false,
                        duration,
                    },
                );
                self.history.truncate(20);
            }
            Err(error) => {
                self.last_status = "info error".to_owned();
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
                self.output = detail.clone();
            }
        }

        self.search
            .set_info_detail(result.name.clone(), detail.clone());
        self.package_detail = detail;
        self.message = format!("inspected crate: {}", result.name);
    }

    fn open_search_link(&mut self, target: SearchLinkTarget) {
        let url = self.search.url_for(target);

        let Some(url) = url else {
            self.message = SearchState::unavailable_message(target).to_owned();
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
        self.message = format!("opened: {url}");
        self.last_status = "opened link".to_owned();
    }

    fn open_url_message_error(&mut self, error: io::Error) {
        self.message = format!("failed to open link: {error}");
        self.last_status = "open link failed".to_owned();
    }

    fn copy_search_detail(&mut self) {
        let text = self.search.selected_detail().join("\n");
        match copy_to_clipboard(&text) {
            Ok(()) => {
                self.message = "copied search detail".to_owned();
                self.last_status = "copied".to_owned();
            }
            Err(error) => {
                self.message = format!("copy failed: {error}");
                self.last_status = "copy failed".to_owned();
            }
        }
    }
}

pub fn run() -> io::Result<()> {
    let project = match ProjectInfo::load() {
        Ok(project) => project,
        Err(error) => {
            eprintln!("FATAL: {error}");
            std::process::exit(1);
        }
    };
    let mut terminal = setup_terminal()?;
    let mut app = App::new(project);
    let result = run_app(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    result
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
    loop {
        app.poll_running_child();
        terminal.draw(|frame| render(frame, app))?;

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) => {
                    if !app.handle_key(key) {
                        break;
                    }
                }
                Event::Mouse(mouse) => app.handle_mouse(mouse),
                _ => {}
            }
        }
    }

    Ok(())
}

fn render(frame: &mut Frame<'_>, app: &mut App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
        .split(root[0]);

    let mut panel_areas = Vec::new();
    app.tab_areas.clear();
    if app.search.expanded {
        app.right_scrollbar_area = None;
        app.right_scrollbar_content_len = 0;
        app.right_scrollbar_visible_rows = 0;
        render_search_page(frame, app, root[0], &mut panel_areas);
    } else {
        app.link_areas.clear();
        render_left(frame, app, main[0], &mut panel_areas);
        panel_areas.push((Focus::Output, main[1], 0));
        render_output(frame, app, main[1]);
    }
    panel_areas.push((Focus::CommandLog, root[1], 0));
    render_command_log(frame, app, root[1]);
    if app.menu_open {
        render_menu(frame, app);
    }
    app.panel_areas = panel_areas;
}

fn render_left(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    panel_areas: &mut Vec<(Focus, Rect, usize)>,
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
        app,
        chunks[0],
        Focus::Workspace,
        workspace_items(&app.project),
        app.workspace_selected,
    );
    panel_areas.push((Focus::Workspace, chunks[0], offset));
    let offset = render_panel(
        frame,
        app,
        chunks[1],
        Focus::Build,
        build_items()
            .into_iter()
            .map(|item| item.label.to_owned())
            .collect(),
        app.build_selected,
    );
    panel_areas.push((Focus::Build, chunks[1], offset));
    let offset = render_panel(
        frame,
        app,
        chunks[2],
        Focus::Dependencies,
        dependency_items_for(&app.project, app.workspace_selected),
        app.dependency_selected,
    );
    panel_areas.push((Focus::Dependencies, chunks[2], offset));
}

fn render_search_page(
    frame: &mut Frame<'_>,
    app: &mut App,
    area: Rect,
    panel_areas: &mut Vec<(Focus, Rect, usize)>,
) {
    app.link_areas.clear();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(chunks[0]);

    render_search_input(frame, app, left[0]);
    panel_areas.push((Focus::Search, left[0], 0));

    let offset = render_panel(
        frame,
        app,
        left[1],
        Focus::Search,
        app.search.result_items(),
        app.search.selected,
    );
    panel_areas.push((Focus::Search, left[1], offset));
    panel_areas.push((Focus::Output, chunks[1], app.right_scroll));

    let detail_lines = app.search.selected_detail();
    let detail_len = detail_lines.len();
    let visible_rows = chunks[1].height.saturating_sub(2) as usize;
    let detail_offset = app
        .right_scroll
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
            app.link_areas.push((area, url.to_owned()));
        }
    }

    let lines = detail_lines
        .into_iter()
        .skip(detail_offset)
        .take(visible_rows)
        .map(Line::from)
        .collect::<Vec<_>>();
    let detail_title = if app.copy_mode {
        "Search Detail  [copy mode]"
    } else {
        "Search Detail"
    };
    let widget = Paragraph::new(lines)
        .block(panel_block(detail_title, app.focus == Focus::Output))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, chunks[1]);

    app.right_scrollbar_content_len = detail_len;
    app.right_scrollbar_visible_rows = visible_rows;
    let scrollbar_area = chunks[1].inner(Margin {
        vertical: 1,
        horizontal: 0,
    });
    app.right_scrollbar_area = (detail_len > visible_rows).then_some(scrollbar_area);
    if detail_len > visible_rows {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .track_style(Style::default().fg(Color::DarkGray))
            .thumb_style(Style::default().fg(Color::Green));
        let mut scrollbar_state = ScrollbarState::new(detail_len)
            .position(detail_offset)
            .viewport_content_length(visible_rows);
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn render_search_input(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let style = if app.input_mode == InputMode::CrateSearch {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let input = if app.search.query.is_empty() {
        "type crate name, Enter to search".to_owned()
    } else {
        app.search.query.clone()
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
    let lines = apply_filter(lines, &app.filter, app.focus == focus);
    let visible_rows = area.height.saturating_sub(2).max(1) as usize;
    let offset = list_offset(selected, visible_rows, lines.len());
    let items = lines
        .into_iter()
        .skip(offset)
        .take(visible_rows)
        .enumerate()
        .map(|(index, line)| {
            let real_index = offset + index;
            let style = if app.focus == focus && real_index == selected {
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

    let widget = List::new(items).block(panel_block(focus.title(), app.focus == focus));
    frame.render_widget(widget, area);
    offset
}

fn render_output(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let visible_rows = area.height.saturating_sub(2) as usize;
    let lines = output_lines(app);
    let offset = app
        .right_scroll
        .min(lines.len().saturating_sub(visible_rows));
    let rendered_lines: Vec<Line<'static>> = lines
        .iter()
        .skip(offset)
        .take(visible_rows)
        .flat_map(|line| {
            line.into_text()
                .map(|text| text.lines)
                .unwrap_or_else(|_| vec![Line::from(line.clone())])
        })
        .collect();

    let block = output_block(app, area);
    let widget = Paragraph::new(rendered_lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, area);

    app.right_scrollbar_content_len = lines.len();
    app.right_scrollbar_visible_rows = visible_rows;
    let scrollbar_area = area.inner(Margin {
        vertical: 1,
        horizontal: 0,
    });
    app.right_scrollbar_area = (lines.len() > visible_rows).then_some(scrollbar_area);
    if lines.len() > visible_rows {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("█")
            .track_style(Style::default().fg(Color::DarkGray))
            .thumb_style(Style::default().fg(Color::Green));
        let mut scrollbar_state = ScrollbarState::new(lines.len())
            .position(offset)
            .viewport_content_length(visible_rows);
        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn output_block(app: &mut App, area: Rect) -> Block<'static> {
    let focused = app.focus == Focus::Output;
    let border_style = if focused {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let title = embedded_tab_title(app);
    update_embedded_tab_areas(app, area);

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
}

fn embedded_tab_title(app: &App) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(Span::styled(
        format!(" {} ", app.current_focus.title()),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ));
    for (index, tab) in app.active_context_tabs().into_iter().enumerate() {
        let style = if tab == app.active_context_tab() {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("[{}] {}", index + 1, tab.label()),
            style,
        ));
        spans.push(Span::raw(" "));
    }
    if app.copy_mode {
        spans.push(Span::styled(
            " [copy mode] ",
            Style::default().fg(Color::Yellow),
        ));
    }
    Line::from(spans)
}

fn update_embedded_tab_areas(app: &mut App, area: Rect) {
    app.tab_areas.clear();
    let mut x = area
        .x
        .saturating_add(1 + app.current_focus.title().len() as u16 + 2);
    for (index, tab) in app.active_context_tabs().into_iter().enumerate() {
        let width = format!("[{}] {}", index + 1, tab.label()).len() as u16 + 2;
        if x >= area.x.saturating_add(area.width) {
            break;
        }
        app.tab_areas.push((
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
}

fn render_menu(frame: &mut Frame<'_>, app: &mut App) {
    let area = centered_rect(70, 64, frame.area());
    let version = env!("CARGO_PKG_VERSION");
    let donate_prefix = format!("Version  {version}    Donate  ");
    app.link_areas.push((
        Rect {
            x: area.x.saturating_add(1 + donate_prefix.len() as u16),
            y: area.y.saturating_add(23),
            width: "Bilibili".len() as u16,
            height: 1,
        },
        "https://www.bilibili.com".to_owned(),
    ));
    let widget = Paragraph::new(key_dialog_lines(app)).block(
        Block::default()
            .title(Span::styled(
                "Keys",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Green)),
    );
    frame.render_widget(widget, area);
}

fn key_dialog_lines(app: &App) -> Vec<Line<'static>> {
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
            Span::styled("Version  ", Style::default().fg(Color::Yellow)),
            Span::raw(version.to_owned()),
            Span::raw("    "),
            Span::styled("Donate  ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "Bilibili",
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            Span::raw("    "),
            Span::styled("Esc/x/Enter", Style::default().fg(Color::Green)),
            Span::raw(" close"),
        ]),
        Line::from(vec![
            Span::styled("status  ", Style::default().fg(Color::Yellow)),
            Span::raw(app.last_status.clone()),
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
    const DONATE_URL: &str = "https://www.bilibili.com";
    let donate_text = "Donate: Bilibili";
    let donate_width = donate_text.len() as u16;
    let donate_x = area
        .x
        .saturating_add(area.width.saturating_sub(donate_width));
    app.link_areas.push((
        Rect {
            x: donate_x,
            y: area.y,
            width: donate_width,
            height: 1,
        },
        DONATE_URL.to_owned(),
    ));

    let line = match app.input_mode {
        InputMode::CrateSearch => Line::from(vec![
            Span::styled("search: ", Style::default().fg(Color::Yellow)),
            Span::raw("Enter search, Esc results, q back, x keys"),
            Span::styled(
                format!("  {}", app.last_status),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  "),
            Span::styled(
                format!("v{}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Filter => Line::from(vec![
            Span::styled("filter: ", Style::default().fg(Color::Yellow)),
            Span::raw(&app.filter),
            Span::styled(
                "  Enter apply, Esc cancel",
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Normal if app.copy_mode => Line::from(vec![
            Span::styled("copy: ", Style::default().fg(Color::Yellow)),
            Span::raw("drag select, m mouse, x keys"),
            Span::styled(
                format!("  {}", app.last_status),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  "),
            Span::styled(
                format!("v{}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(Color::Green),
            ),
        ]),
        InputMode::Normal if app.search.expanded => Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::raw(": inspect, "),
            Span::styled("a", Style::default().fg(Color::Green)),
            Span::raw(": add, "),
            Span::styled("q", Style::default().fg(Color::Green)),
            Span::raw(": back, "),
            Span::styled("x", Style::default().fg(Color::Green)),
            Span::raw(": keys"),
            Span::styled(
                format!("  {}", app.last_status),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  "),
            Span::styled(
                format!("v{}", env!("CARGO_PKG_VERSION")),
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
                format!("  {}", app.last_status),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  "),
            Span::styled(
                format!("v{}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(Color::Green),
            ),
        ]),
    };

    frame.render_widget(Paragraph::new(line), area);
    let donate_line = Line::from(vec![
        Span::styled("Donate: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            "Bilibili",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::UNDERLINED),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(donate_line),
        Rect {
            x: donate_x,
            y: area.y,
            width: donate_width,
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

impl DiskSnapshot {
    fn load(project: &ProjectInfo) -> Self {
        let target_size = target_dir_size_label(project);
        let packages = project
            .workspace_packages
            .iter()
            .map(|package| PackageDiskInfo {
                name: package.name.clone(),
                source_size: package_source_size_label(&package.manifest_path),
                target_cache: package_target_cache_label(project, &package.name),
            })
            .collect();
        Self {
            target_size,
            packages,
        }
    }

    fn package(&self, name: &str) -> Option<&PackageDiskInfo> {
        self.packages.iter().find(|package| package.name == name)
    }
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
        format!("  target total: {}", disk.target_size),
        format!("  workspace packages: {}", project.workspace_packages.len()),
        format!("  direct dependencies: {}", project.dependencies.len()),
        String::new(),
        "Hot paths".to_owned(),
        "  [1] Workspace: package source/target cache snapshot".to_owned(),
        "  [3] Dependencies: feature state tree and inverse tree".to_owned(),
        "  s: crates.io package search".to_owned(),
    ]
}

fn package_source_size_label(manifest_path: &str) -> String {
    let manifest = std::path::Path::new(manifest_path);
    let Some(root) = manifest.parent() else {
        return "<unknown>".to_owned();
    };
    match dir_size(&root.join("src")) {
        Ok(bytes) => format_bytes(bytes),
        Err(_) => "<unknown>".to_owned(),
    }
}

fn package_target_cache_label(project: &ProjectInfo, package_name: &str) -> String {
    let target = std::path::Path::new(&project.workspace_root).join("target");
    let mut total = 0;
    for profile in ["debug", "release"] {
        let deps = target.join(profile).join("deps");
        total += prefixed_file_size(&deps, package_name).unwrap_or(0);
        total += prefixed_file_size(&target.join(profile), package_name).unwrap_or(0);
    }

    if total == 0 {
        "<not built or not attributable>".to_owned()
    } else {
        format_bytes(total)
    }
}

fn prefixed_file_size(path: &std::path::Path, package_name: &str) -> io::Result<u64> {
    let mut total = 0;
    if !path.exists() {
        return Ok(0);
    }
    let prefix = package_name.replace('-', "_");
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&prefix) {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}

fn target_dir_size_label(project: &ProjectInfo) -> String {
    let path = std::path::Path::new(&project.workspace_root).join("target");
    match dir_size(&path) {
        Ok(bytes) => format_bytes(bytes),
        Err(_) => "<unknown>".to_owned(),
    }
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

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn cargo_output_paths() -> (PathBuf, PathBuf) {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let pid = std::process::id();
    let base = format!("lazycargo-{pid}-{stamp}");
    let dir = std::env::temp_dir();
    (
        dir.join(format!("{base}.stdout")),
        dir.join(format!("{base}.stderr")),
    )
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
