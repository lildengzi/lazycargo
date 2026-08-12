use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEvent, MouseButton, MouseEvent,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};

use crate::core::build_history::{is_recordable_command, BuildEntry, CrateTiming};
use crate::core::command::{
    CargoTask, CargoTaskKind, CommandSpec, FeatureSelection, Profile, TaskScope,
};
use crate::core::config::AppConfig;
use crate::core::dep_tree;
use crate::core::project::ProjectInfo;
use crate::core::model::{ContextOutput, CoreState, OutputSlot};
use crate::core::process::{extract_diagnostics, spawn_streaming};
use crate::core::target_analyzer::{self, DiskSnapshot};
use crate::core::task::{
    info_progress_detail, run_info_job, run_search_job, search_progress_detail, SearchJobConfig,
    SearchJobKind,
};
use crate::core::util::format_bytes;
use crate::keymap::{
    self, NormalKeyAction, NormalKeyContext, ProjectNewConfirmAction, TextInputAction,
};
use crate::state::{NavigationState, SelectionState};
use lazycargo_search::{SearchLinkTarget, SearchState};

mod components;
pub mod controller;
mod pages;

use controller::*;

mod dashboard;
mod layout;
mod style;
mod terminal_support;

use dashboard::{build_items, dependency_items_for, output_lines, workspace_items};
use layout::{
    render_command_log, render_menu, render_panel, render_project_new_confirm, render_scrollbar,
    render_search_input,
};
use style::{output_line_to_lines, panel_block, semantic_output_line};
use terminal_support::{copy_to_clipboard, first_url, open_url};

struct HistoryEntry {
    command: String,
    success: bool,
    duration: Duration,
}

struct App {
    core: CoreState,
    navigation: NavigationState,
    selection: SelectionState,
    history: Vec<HistoryEntry>,
}

impl App {
    fn new(project: ProjectInfo, config: AppConfig) -> Self {
        let command_preview = "cargo check".to_owned();
        let disk = DiskSnapshot::pending(&project.packages);
        let disk_receiver = Some(load_disk_snapshot_async(
            project.clone(),
            config.target_stale_days,
        ));
        let output = project_health_snapshot(&project, &disk);
        let mut core = CoreState::new(project.clone(), config, disk);
        core.disk_receiver = disk_receiver;
        core.output.insert(OutputSlot::BuildLive, ContextOutput::with_lines(output));
        core.output.insert(
            OutputSlot::BuildConfig,
            ContextOutput::with_lines(vec!["no build/check run yet".to_owned()]),
        );
        core.output.insert(
            OutputSlot::DepsFeatures,
            ContextOutput::with_lines(vec![
                "select dependency, then press t/i for offline tree or T/I to allow fetch"
                    .to_owned(),
            ]),
        );
        core.output.insert(
            OutputSlot::DepsTree,
            ContextOutput::with_lines(vec!["no dependency tree yet".to_owned()]),
        );
        core.output.insert(
            OutputSlot::SearchDetail,
            ContextOutput::with_lines(vec!["no package action yet".to_owned()]),
        );
        Self {
            core,
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
                target_crate_selected: 0,
                tree_selected: 0,
                tree_expanded: HashMap::new(),
            },
            history: Vec::new(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if keymap::is_ctrl_c(key) {
            if self.core.processes.child.is_some() {
                self.kill_running_child();
                return true;
            }
            return false;
        }

        if self.navigation.menu_open {
            return self.handle_menu_key(key);
        }

        match self.navigation.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Filter => self.handle_filter_key(key),
            InputMode::CrateSearch => self.handle_crate_search_key(key),
            InputMode::ProjectNewConfirm => self.handle_project_new_confirm_key(key),
        }
    }

    fn poll_disk_snapshot(&mut self) {
        let Some(receiver) = &self.core.disk_receiver else {
            return;
        };
        let Ok(snapshot) = receiver.try_recv() else {
            return;
        };
        self.core.disk = snapshot;
        self.selection.target_crate_selected = self
            .selection
            .target_crate_selected
            .min(self.core.disk.by_crate.len().saturating_sub(1));
        self.core.disk_receiver = None;
        if self.navigation.last_status == "ready" {
            self.navigation.last_status = "disk snapshot ready".to_owned();
        }
    }

    fn poll_search_job(&mut self) {
        if self.core.search_receiver.is_some() {
            let elapsed = self.core
                .search_started
                .map(|started| started.elapsed())
                .unwrap_or_default();
            if let Some(name) = self.search_info_pending_name() {
                self.core.search
                    .state
                    .set_selected_detail(name.clone(), info_progress_detail(&name, elapsed));
            } else {
                self.core.search
                    .state
                    .set_empty_detail(search_progress_detail(&self.core.search.state.query, elapsed));
            }
        }
        let Some(receiver) = &self.core.search_receiver else {
            return;
        };
        let Ok(result) = receiver.try_recv() else {
            return;
        };
        self.core.search_receiver = None;
        self.core.search_started = None;
        match result.kind {
            SearchJobKind::Search => {
                if let Some(results) = result.results {
                    self.core.search.state.set_results(results);
                } else {
                    self.core.search.state.set_empty_detail(result.detail.clone());
                }
            }
            SearchJobKind::Info { name, author } => {
                if let Some(author) = author {
                    self.core.search.state.set_selected_author(author);
                }
                self.core.search
                    .state
                    .set_info_detail(name, result.detail.clone());
                self.record_crate_inspection(
                    result.command.clone(),
                    result.duration,
                    result.success,
                );
            }
        }
        self.core.set_slot_lines(OutputSlot::SearchDetail, result.detail);
        self.navigation.last_status = result.status;
        self.navigation.message = result.message;
        self.history.insert(
            0,
            HistoryEntry {
                command: result.command,
                success: result.success,
                duration: result.duration,
            },
        );
        self.history.truncate(self.core.config.command_history_limit);
    }

    fn search_info_pending_name(&self) -> Option<String> {
        let command = &self.navigation.command_preview;
        command
            .strip_prefix("cargo info ")
            .map(str::to_owned)
            .filter(|name| !name.is_empty())
    }

    fn refresh_disk_snapshot(&mut self) {
        self.core.disk_receiver = Some(load_disk_snapshot_async(
            self.core.project.clone(),
            self.core.config.target_stale_days,
        ));
        self.navigation.message = "refreshing target analysis".to_owned();
        self.navigation.last_status = "target refresh".to_owned();
    }

    fn clean_target_stale(&mut self, dry_run: bool) {
        let root = Path::new(&self.core.project.workspace_root);
        match target_analyzer::clean_stale(root, dry_run, self.core.config.target_stale_days) {
            Ok(report) => {
                let output = std::iter::once(if dry_run {
                    "Target clean dry-run".to_owned()
                } else {
                    "Target clean stale".to_owned()
                })
                .chain(std::iter::once(format!(
                    "threshold: {} days, artifacts: {}, total: {}",
                    self.core.config.target_stale_days,
                    report.artifact_count,
                    format_bytes(report.total_size)
                )))
                .chain(std::iter::once(String::new()))
                .chain(report.lines.clone())
                .collect();
                self.core.set_slot_lines(OutputSlot::WorkspaceTarget, output);
                self.navigation.ws_tab = WorkspaceTab::Target;
                self.navigation.current_focus = FocusPanel::Workspace;
                self.set_focus(Focus::Output);
                self.navigation.message = if dry_run {
                    format!("dry-run: {} stale artifacts", report.artifact_count)
                } else {
                    format!("cleaned: {} stale artifacts", report.artifact_count)
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
            if self.core.search.state.expanded
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
        if self.core.search.state.expanded
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

    fn reset_slot_scroll(&mut self, slot: OutputSlot) {
        self.core.context(slot).scroll = 0;
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
        self.core.search.state.expanded = true;
        self.reset_slot_scroll(OutputSlot::SearchDetail);
        self.navigation.input_mode = InputMode::CrateSearch;
    }

    fn scroll_right(&mut self, delta: isize) {
        self.set_focus(Focus::Output);
        let slot = self.active_output_slot();
        let content_len = output_lines(self).len();
        let scroll = {
            let ctx = self.core.context(slot);
            let visible_rows = ctx.visible_rows.max(1);
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
        let ctx = self.core.context(self.active_output_slot());
        ctx.scroll = scroll;
        ctx.follow_tail = scroll >= max_scroll;
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
        let context = NormalKeyContext {
            search_expanded: self.core.search.state.expanded,
            focus: self.navigation.focus,
            ws_tab: self.navigation.ws_tab,
            deps_tab: self.navigation.deps_tab,
        };
        match keymap::normal_key_action(key, context) {
            NormalKeyAction::BackFromSearch => {
                self.core.search.state.expanded = false;
                self.set_focus(self.navigation.search_return_focus);
                self.navigation.message = "back from search".to_owned();
            }
            NormalKeyAction::Quit => return false,
            NormalKeyAction::OpenKeys => {
                self.navigation.menu_open = true;
                self.navigation.menu_selected = 0;
            }
            NormalKeyAction::FocusNext => self.set_focus(self.navigation.focus.next()),
            NormalKeyAction::FocusOutput => self.set_focus(Focus::Output),
            NormalKeyAction::SwitchTab(delta) => self.switch_context_tab(delta),
            NormalKeyAction::ToggleCopyMode => self.toggle_copy_mode(),
            NormalKeyAction::FocusDigit(value) => self.set_focus(Focus::from_digit(value)),
            NormalKeyAction::ScrollRight(delta) => self.scroll_right(delta),
            NormalKeyAction::OutputUp => {
                if self.navigation.current_focus == FocusPanel::Workspace
                    && self.navigation.ws_tab == WorkspaceTab::Target
                {
                    self.move_target_crate_selection(-1);
                } else if self.navigation.deps_tab == DependenciesTab::DependencyTree
                    && self.navigation.current_focus == FocusPanel::Dependencies
                {
                    self.move_tree_selection(-1);
                } else {
                    self.scroll_right(-1);
                }
            }
            NormalKeyAction::OutputDown => {
                if self.navigation.current_focus == FocusPanel::Workspace
                    && self.navigation.ws_tab == WorkspaceTab::Target
                {
                    self.move_target_crate_selection(1);
                } else if self.navigation.deps_tab == DependenciesTab::DependencyTree
                    && self.navigation.current_focus == FocusPanel::Dependencies
                {
                    self.move_tree_selection(1);
                } else {
                    self.scroll_right(1);
                }
            }
            NormalKeyAction::MoveUp => self.move_selection(-1),
            NormalKeyAction::MoveDown => self.move_selection(1),
            NormalKeyAction::Activate => self.activate_selection(),
            NormalKeyAction::OpenFilter => {
                self.navigation.input_mode = InputMode::Filter;
                self.navigation.filter.clear();
                self.navigation.message = format!("filter {}: ", self.navigation.focus.title());
            }
            NormalKeyAction::RefreshTarget => self.refresh_disk_snapshot(),
            NormalKeyAction::DryRunCleanTarget => self.clean_target_stale(true),
            NormalKeyAction::CleanTarget => self.clean_target_stale(false),
            NormalKeyAction::CargoCheck => self.run_cargo(Focus::Build, &["check"]),
            NormalKeyAction::CargoBuild => self.run_cargo(Focus::Build, &["build"]),
            NormalKeyAction::TreeOffline => self.run_tree(false),
            NormalKeyAction::TreeWithFetch => self.run_tree(true),
            NormalKeyAction::InverseTreeOffline => self.run_inverse_tree(false),
            NormalKeyAction::InverseTreeWithFetch => self.run_inverse_tree(true),
            NormalKeyAction::PreviewAdd => self.preview_add(),
            NormalKeyAction::OpenCrates => self.open_search_link(SearchLinkTarget::Crates),
            NormalKeyAction::OpenDocs => self.open_search_link(SearchLinkTarget::Docs),
            NormalKeyAction::OpenRepository => self.open_search_link(SearchLinkTarget::Repository),
            NormalKeyAction::CopySearchDetail => self.copy_search_detail(),
            NormalKeyAction::CollapseTree => self.collapse_selected_tree_node(),
            NormalKeyAction::ToggleTree => self.toggle_selected_tree_node(),
            NormalKeyAction::OpenSearch => {
                self.open_search();
                self.navigation.message = "search crates".to_owned();
            }
            NormalKeyAction::Noop => {}
        }

        true
    }

    fn handle_menu_key(&mut self, key: KeyEvent) -> bool {
        if keymap::menu_closes(key) {
            self.navigation.menu_open = false;
        }

        true
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> bool {
        match keymap::text_input_action(key) {
            TextInputAction::Cancel => {
                self.navigation.input_mode = InputMode::Normal;
                self.navigation.filter.clear();
                self.navigation.message = "filter cancelled".to_owned();
            }
            TextInputAction::Submit => {
                self.navigation.input_mode = InputMode::Normal;
                self.navigation.message = format!(
                    "filter applied to {}: {}",
                    self.navigation.focus.title(),
                    self.navigation.filter
                );
            }
            TextInputAction::Backspace => {
                self.navigation.filter.pop();
            }
            TextInputAction::Push(value) => self.navigation.filter.push(value),
            TextInputAction::Noop => {}
        }

        true
    }

    fn handle_crate_search_key(&mut self, key: KeyEvent) -> bool {
        match keymap::text_input_action(key) {
            TextInputAction::Cancel => {
                self.navigation.input_mode = InputMode::Normal;
                self.navigation.message = "search input blurred".to_owned();
            }
            TextInputAction::Submit => {
                let query = self.core.search.state.query.trim().to_owned();
                self.navigation.input_mode = InputMode::Normal;
                if query.is_empty() {
                    self.navigation.message = "empty crate search".to_owned();
                } else {
                    self.search_crates(&query);
                }
            }
            TextInputAction::Backspace => {
                self.core.search.state.query.pop();
            }
            TextInputAction::Push(value) => self.core.search.state.query.push(value),
            TextInputAction::Noop => {}
        }

        true
    }

    fn handle_project_new_confirm_key(&mut self, key: KeyEvent) -> bool {
        match keymap::project_new_confirm_action(key) {
            ProjectNewConfirmAction::Yes => {
                self.navigation.input_mode = InputMode::Normal;
                self.run_cargo(Focus::Build, &["init"]);
            }
            ProjectNewConfirmAction::No => {
                self.navigation.input_mode = InputMode::Normal;
                self.navigation.message = "project creation skipped".to_owned();
                self.navigation.last_status = "limited mode".to_owned();
            }
            ProjectNewConfirmAction::Noop => {}
        }

        true
    }

    fn preview(&mut self, command: &str) {
        self.navigation.command_preview = command.to_owned();
        self.navigation.message = format!("preview: {command}");
    }

    fn move_selection(&mut self, delta: isize) {
        let len = match self.navigation.focus {
            Focus::Workspace => workspace_items(&self.core.project).len(),
            Focus::Dependencies => {
                dependency_items_for(&self.core.project, self.selection.workspace_selected)
                    .len()
            }
            Focus::Search => self.core.search.state.result_items().len(),
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
            Focus::Workspace => workspace_items(&self.core.project).len(),
            Focus::Dependencies => {
                dependency_items_for(&self.core.project, self.selection.workspace_selected)
                    .len()
            }
            Focus::Search => self.core.search.state.result_items().len(),
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
        self.core.set_slot_lines(
            OutputSlot::DepsFeatures,
            vec![format!(
                "workspace scope changed: {}",
                self.selected_scope_label()
            )],
        );
        self.core.set_slot_lines(
            OutputSlot::DepsTree,
            vec!["dependency tree not loaded for current scope".to_owned()],
        );
    }

    fn selected_scope_label(&self) -> String {
        if self.selection.workspace_selected == 0 {
            "workspace".to_owned()
        } else {
            self.core
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
            Focus::Search => &mut self.core.search.state.selected,
            Focus::Build => &mut self.selection.build_selected,
            _ => &mut self.selection.workspace_selected,
        }
    }

    fn move_target_crate_selection(&mut self, delta: isize) {
        let len = self.core
            .disk
            .by_crate
            .len()
            .min(self.core.config.target_top_crates);
        if len == 0 {
            self.scroll_right(delta);
            return;
        }
        self.selection.target_crate_selected =
            (self.selection.target_crate_selected as isize + delta)
                .clamp(0, len.saturating_sub(1) as isize) as usize;
        self.navigation.message =
            format!("target crate {}", self.selection.target_crate_selected + 1);
    }

    fn inspect_target_crate(&mut self) {
        let Some(item) = self.core
            .disk
            .by_crate
            .get(self.selection.target_crate_selected)
        else {
            self.navigation.message = "no target crate selected".to_owned();
            return;
        };
        self.navigation.message = format!("target crate: {} {}", item.name, item.label);
        self.navigation.last_status = format!("target {}", item.label);
    }

    fn activate_selection(&mut self) {
        match self.navigation.focus {
            Focus::Workspace => {
                if let Some(scope) =
                    workspace_items(&self.core.project).get(self.selection.workspace_selected)
                {
                    self.preview(&format!("scope: {scope}"));
                }
            }
            Focus::Dependencies => self.inspect_dependency(),
            Focus::Output if self.navigation.deps_tab == DependenciesTab::DependencyTree => {
                self.toggle_selected_tree_node()
            }
            Focus::Output
                if self.navigation.current_focus == FocusPanel::Workspace
                    && self.navigation.ws_tab == WorkspaceTab::Target =>
            {
                self.inspect_target_crate()
            }
            Focus::Search => {
                if self.navigation.input_mode == InputMode::CrateSearch
                    || self.core.search.state.results.is_empty()
                {
                    self.core.search.state.expanded = true;
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
                Some("timings") => self.run_cargo(Focus::Build, &["build", "--timings"]),
                Some("diagnostics") => self.navigation.build_tab = BuildCoreTab::LiveOutput,
                _ => {}
            },
            _ => {}
        }
    }

    fn run_cargo(&mut self, detail_focus: Focus, args: &[&str]) {
        if self.core.processes.child.is_some() {
            self.navigation.message = format!("already running: {}", self.core.processes.command);
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
            let ctx = self.core.context(slot);
            ctx.lines.clear();
            ctx.lines.push(format!("$ {command}"));
            ctx.scroll = usize::MAX;
            ctx.follow_tail = true;
            ctx.stream_rx = None;
            ctx.tree_nodes.clear();
        }

        match spawn_streaming(&spec.program, &spec.args, &[("CARGO_TERM_COLOR", "always")]) {
            Ok((child, rx)) => {
                self.core.processes.child = Some(child);
                self.core.processes.command = command;
                self.core.processes.start = Instant::now();
                self.core.processes.slot = slot;
                self.core.context(slot).stream_rx = Some(rx);
            }
            Err(error) => {
                self.navigation.last_status = "error".to_owned();
                self.core.set_slot_lines(slot, vec![format!("failed to run {command}: {error}")]);
                self.core.diagnostics = vec![format!("runner error: {error}")];
                self.navigation.message = format!("failed: {command}");
            }
        }
    }

    fn drain_all_streams(&mut self) {
        let max_lines = self.core.config.output_max_lines;
        self.core.drain_all_streams(max_lines);

        let Some(child) = &mut self.core.processes.child else {
            return;
        };

        match child.try_wait() {
            Ok(Some(status)) => {
                let Some(child) = self.core.processes.child.take() else {
                    return;
                };
                drop(child);
                let command = std::mem::take(&mut self.core.processes.command);
                let duration = self.core.processes.start.elapsed();
                let slot = self.core.processes.slot;
                self.core.context(slot).drain_stream(max_lines);
                self.core.context(slot).stream_rx = None;
                self.finish_cargo_output(slot, command, duration, status);
            }
            Ok(None) => {}
            Err(error) => {
                self.navigation.message = format!("wait failed: {error}");
                self.navigation.last_status = "wait failed".to_owned();
                self.core.processes.child = None;
                self.core.processes.command.clear();
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
            let ctx = self.core.context(slot);
            ctx.lines.push(String::new());
            ctx.lines.push(format!("exit: {status}"));
            ctx.lines
                .push(format!("duration: {:.2}s", duration.as_secs_f32()));
            ctx.lines.clone()
        };
        if slot == OutputSlot::BuildLive {
            self.core.diagnostics = extract_diagnostics(&lines);
        }
        self.history.insert(
            0,
            HistoryEntry {
                command: command.clone(),
                success,
                duration,
            },
        );
        self.history.truncate(self.core.config.command_history_limit);
        self.record_build_history(&command, duration, success);
        if success && is_project_init_command(&command) {
            self.reload_project_after_init();
        }
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
            rustc_version: self.core.project.rustc_version.clone(),
            crate_timings: latest_crate_timings(Path::new(&self.core.project.workspace_root)),
        };
        if let Err(error) = self.core
            .history
            .add_entry(entry, self.core.config.build_history_limit)
        {
            self.core
                .diagnostics
                .push(format!("failed to save build history: {error}"));
        }
    }

    fn update_dependency_tree_from_output(&mut self) {
        let lines = self.core.slot_lines(OutputSlot::DepsTree);
        let nodes = dep_tree::parse_tree_output(&lines, &self.selection.tree_expanded);
        if nodes.is_empty() {
            self.core.context(OutputSlot::DepsTree).lines.insert(
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
        let ctx = self.core.context(OutputSlot::DepsTree);
        ctx.tree_nodes = nodes;
        dep_tree::apply_selected(&mut ctx.tree_nodes, selected);
    }

    fn move_tree_selection(&mut self, delta: isize) {
        let len = self.core
            .output
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
            &mut self.core.context(OutputSlot::DepsTree).tree_nodes,
            selected,
        );
        self.navigation.message = format!("tree node {}", self.selection.tree_selected + 1);
    }

    fn toggle_selected_tree_node(&mut self) {
        let selected = self.selection.tree_selected;
        let toggled = {
            let ctx = self.core.context(OutputSlot::DepsTree);
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
            &mut self.core.context(OutputSlot::DepsTree).tree_nodes,
            selected,
            false,
        ) else {
            return;
        };
        self.selection.tree_expanded.insert(key, false);
        self.navigation.message = "tree node collapsed".to_owned();
    }

    fn kill_running_child(&mut self) {
        let Some(mut child) = self.core.processes.child.take() else {
            return;
        };

        let _ = child.kill();
        let _ = child.wait();
        self.navigation.message = "killed".to_owned();
        self.navigation.last_status = "killed".to_owned();
        let slot = self.core.processes.slot;
        let command = std::mem::take(&mut self.core.processes.command);
        let max_lines = self.core.config.output_max_lines;
        let ctx = self.core.context(slot);
        ctx.drain_stream(max_lines);
        ctx.stream_rx = None;
        ctx.lines.push(format!("killed: {command}"));
        self.core.processes.command.clear();
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

        if matches!(command, "init" | "new") {
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
            self.core
                .project
                .packages
                .get(self.selection.workspace_selected.saturating_sub(1))
                .cloned()
        }
    }

    fn reload_project_after_init(&mut self) {
        match ProjectInfo::load() {
            Ok(project) => {
                self.core.project = project;
                self.selection.workspace_selected = 0;
                self.selection.dependency_selected = 0;
                self.core.disk = DiskSnapshot::pending(&self.core.project.packages);
                self.core.disk_receiver = Some(load_disk_snapshot_async(
                    self.core.project.clone(),
                    self.core.config.target_stale_days,
                ));
                self.navigation.message = "Cargo project initialized".to_owned();
                self.navigation.last_status = "project ready".to_owned();
                self.navigation.current_focus = FocusPanel::Workspace;
                self.navigation.ws_tab = WorkspaceTab::CrateInfo;
            }
            Err(error) => {
                self.navigation.message = format!("initialized, but reload failed: {error}");
                self.navigation.last_status = "reload failed".to_owned();
            }
        }
    }

    fn search_crates(&mut self, query: &str) {
        if self.core.search_receiver.is_some() {
            self.navigation.message = "search already running".to_owned();
            return;
        }
        let query = query.to_owned();
        let command = format!("crates.io api search {query}");
        self.navigation.command_preview = command.clone();
        self.navigation.message = format!("searching crates: {query}");
        self.navigation.focus = Focus::Search;
        self.core.search.state.expanded = true;
        self.core.search
            .state
            .set_empty_detail(search_progress_detail(&query, Duration::from_secs(0)));
        self.core.set_slot_lines(
            OutputSlot::SearchDetail,
            search_progress_detail(&query, Duration::from_secs(0)),
        );
        let (tx, rx) = mpsc::channel();
        self.core.search_receiver = Some(rx);
        self.core.search_started = Some(Instant::now());
        let config = self.search_job_config();
        thread::spawn(move || {
            let _ = tx.send(run_search_job(query, config));
        });
    }

    fn search_job_config(&self) -> SearchJobConfig {
        SearchJobConfig {
            limit: self.core.config.search_limit,
            network_timeout: self.core.config.network_timeout(),
            info_timeout: self.core.config.cargo_info_timeout(),
        }
    }

    fn inspect_dependency(&mut self) {
        let Some(dependency) = selected_dependency_name(
            &self.core.project,
            self.selection.workspace_selected,
            self.selection.dependency_selected,
        ) else {
            self.core.set_slot_lines(
                OutputSlot::DepsFeatures,
                vec!["no dependency selected".to_owned()],
            );
            return;
        };

        self.navigation.command_preview = format!("cargo tree -i {dependency}");
        self.core.set_slot_lines(
            OutputSlot::DepsFeatures,
            vec![format!("selected: {dependency}")],
        );
        self.navigation.message = format!("selected dependency: {dependency}");
        self.navigation.deps_tab = DependenciesTab::Features;
    }

    fn run_tree(&mut self, allow_fetch: bool) {
        if allow_fetch {
            self.run_cargo(Focus::Dependencies, &["tree", "-e", "features"]);
        } else {
            self.run_cargo(
                Focus::Dependencies,
                &["tree", "--offline", "-e", "features"],
            );
        }
        self.navigation.deps_tab = DependenciesTab::DependencyTree;
    }

    fn run_inverse_tree(&mut self, allow_fetch: bool) {
        let Some(dependency) = selected_dependency_name(
            &self.core.project,
            self.selection.workspace_selected,
            self.selection.dependency_selected,
        ) else {
            self.core.set_slot_lines(
                OutputSlot::DepsFeatures,
                vec!["select a dependency first".to_owned()],
            );
            return;
        };

        if allow_fetch {
            self.run_cargo(
                Focus::Dependencies,
                &["tree", "-e", "features", "-i", &dependency],
            );
        } else {
            self.run_cargo(
                Focus::Dependencies,
                &["tree", "--offline", "-e", "features", "-i", &dependency],
            );
        }
        self.navigation.deps_tab = DependenciesTab::DependencyTree;
    }

    fn preview_add(&mut self) {
        let selected_name = self.core
            .search
            .state
            .selected_result()
            .map(|result| result.name.as_str())
            .or_else(|| {
                let query = self.core.search.state.query.trim();
                (!query.is_empty()).then_some(query)
            })
            .unwrap_or("<crate>");
        let command = match self.selected_package() {
            Some(package) => format!("cargo add {selected_name} -p {package}"),
            None => format!("cargo add {selected_name}"),
        };
        self.preview(&command);
        let detail = self.core
            .search
            .state
            .selected_detail()
            .into_iter()
            .chain(vec![String::new(), command])
            .collect();
        self.core.set_slot_lines(OutputSlot::SearchDetail, detail);
    }

    fn inspect_selected_crate(&mut self) {
        if self.core.search_receiver.is_some() {
            self.navigation.message = "search task already running".to_owned();
            return;
        }
        let Some(result) = self.core.search.state.selected_result().cloned() else {
            self.core.set_slot_lines(
                OutputSlot::SearchDetail,
                vec!["no crate selected".to_owned()],
            );
            return;
        };

        let command = format!("cargo info {}", result.name);
        self.navigation.command_preview = command.clone();
        self.navigation.message = format!("inspecting crate: {}", result.name);
        self.core.search.state.expanded = true;
        let detail = info_progress_detail(&result.name, Duration::from_secs(0));
        self.core.search
            .state
            .set_selected_detail(result.name.clone(), detail.clone());
        self.core.set_slot_lines(OutputSlot::SearchDetail, detail);
        let (tx, rx) = mpsc::channel();
        self.core.search_receiver = Some(rx);
        self.core.search_started = Some(Instant::now());
        let config = self.search_job_config();
        thread::spawn(move || {
            let _ = tx.send(run_info_job(result, config));
        });
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
        self.history.truncate(self.core.config.command_history_limit);
    }

    fn open_search_link(&mut self, target: SearchLinkTarget) {
        let url = self.core.search.state.url_for(target);

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
        let text = self.core.search.state.selected_detail().join("\n");
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
    let (config, config_message) = match AppConfig::load_or_create() {
        Ok(config) => (
            config,
            Some(format!(
                "config: {}",
                AppConfig::config_path().to_string_lossy()
            )),
        ),
        Err(error) => (
            AppConfig::default(),
            Some(format!("config load failed, using defaults: {error}")),
        ),
    };
    let (project, startup_message) = match ProjectInfo::load() {
        Ok(project) => (project, None),
        Err(error) => (
            fallback_project_info(),
            Some(format!("No Cargo.toml found; limited mode: {error}")),
        ),
    };
    let mut terminal = setup_terminal()?;
    let mut app = App::new(project, config);
    if let Some(message) = config_message {
        app.navigation.message = message;
    }
    if let Some(message) = startup_message {
        app.navigation.last_status = "limited mode".to_owned();
        app.navigation.message = "no Cargo project: initialize here? y/n".to_owned();
        app.core.set_slot_lines(
            OutputSlot::BuildLive,
            vec![
                "Limited mode".to_owned(),
                String::new(),
                message,
                String::new(),
                "This directory is not a Cargo project yet.".to_owned(),
                "Press y to initialize the current directory with cargo init.".to_owned(),
                "Press n or Esc to stay in limited mode.".to_owned(),
                "Search is also available with s.".to_owned(),
            ],
        );
        app.navigation.input_mode = InputMode::ProjectNewConfirm;
        app.navigation.command_preview = "cargo init".to_owned();
    }
    let result = run_app(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    result
}

fn is_project_init_command(command: &str) -> bool {
    command == "cargo init" || command.starts_with("cargo init ")
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
        app.poll_search_job();
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
    if app.core.search.state.expanded {
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
    if app.navigation.input_mode == InputMode::ProjectNewConfirm {
        render_project_new_confirm(frame);
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
        workspace_items(&app.core.project),
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
        dependency_items_for(&app.core.project, app.selection.workspace_selected),
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
        app.core.search.state.result_items(),
        app.core.search.state.selected,
    );
    mouse_state
        .panel_areas
        .push((Focus::Search, left[1], offset));
    mouse_state.panel_areas.push((
        Focus::Output,
        chunks[1],
        app.core.output
            .get(&OutputSlot::SearchDetail)
            .map(|ctx| ctx.scroll)
            .unwrap_or(0),
    ));

    let detail_lines = app.core.search.state.selected_detail();
    let detail_len = detail_lines.len();
    let visible_rows = chunks[1].height.saturating_sub(2) as usize;
    app.core.context(OutputSlot::SearchDetail).visible_rows = visible_rows;
    let detail_offset = app.core
        .output
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
        .map(|line| semantic_output_line(&line))
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

fn render_output(frame: &mut Frame<'_>, app: &mut App, area: Rect, mouse_state: &mut MouseState) {
    let visible_rows = area.height.saturating_sub(2) as usize;
    let lines = output_lines(app);
    let slot = app.active_output_slot();
    let max_scroll = lines.len().saturating_sub(visible_rows);
    let offset = {
        let ctx = app.core.context(slot);
        ctx.visible_rows = visible_rows;
        if ctx.follow_tail {
            ctx.scroll = max_scroll;
        } else {
            ctx.scroll = ctx.scroll.min(max_scroll);
            if ctx.scroll >= max_scroll {
                ctx.follow_tail = true;
            }
        }
        ctx.scroll
    };
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

fn project_health_snapshot(project: &ProjectInfo, disk: &DiskSnapshot) -> Vec<String> {
    vec![
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

fn load_disk_snapshot_async(project: ProjectInfo, stale_days: u64) -> Receiver<DiskSnapshot> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let root = Path::new(&project.workspace_root);
        let _ = tx.send(target_analyzer::analyze_target(
            root,
            &project.packages,
            stale_days,
        ));
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

fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}
