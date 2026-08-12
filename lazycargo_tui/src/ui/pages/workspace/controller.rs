use std::cell::Cell;
use std::io;
use std::path::Path;
use std::process::ExitStatus;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyEvent, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::core::build_history::{is_recordable_command, latest_crate_timings, BuildEntry};
use crate::core::command::{
    is_project_init_command, CargoTask, CargoTaskKind, CommandSpec, FeatureSelection, Profile,
    TaskScope,
};
use crate::core::dep_tree;
use crate::core::model::{CoreState, OutputSlot};
use crate::core::process::extract_diagnostics;
use crate::core::project::ProjectInfo;
use crate::core::target_analyzer::{self, analyze_target_async, DiskSnapshot};
use crate::core::task::{info_progress_detail, run_info_job, SearchJobConfig};
use crate::core::util::format_bytes;
use crate::ui::components::panel::render_panel;
use crate::ui::components::scrollbar::render_scrollbar;
use crate::ui::components::style::output_line_to_lines;
use crate::ui::controller::{
    contains, focus_under, link_under, panel_under, tab_under, BuildCoreTab, ContextTab,
    DependenciesTab, Focus, FocusPanel, InputMode, MouseState, Page, WorkspaceTab, WorkspaceView,
};
use crate::ui::keymap::{self, NormalKeyAction, NormalKeyContext, TextInputAction};
use crate::ui::pages::workspace::view::{
    build_items, dependency_items_for, output_lines, scope_label, selected_dependency_name,
    workspace_items,
};
use crate::ui::terminal_support::{copy_to_clipboard, open_url};
use crate::ui::HistoryEntry;
use lazycargo_search::{SearchLinkTarget, SearchState};

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

pub(crate) struct WorkspacePage {
    pub view: WorkspaceView,
    pub nav: WorkspaceNav,
    pub history: Vec<HistoryEntry>,
    visible_rows: Cell<usize>,
}

impl WorkspacePage {
    pub fn new() -> Self {
        Self {
            view: WorkspaceView::default(),
            nav: WorkspaceNav::default(),
            history: Vec::new(),
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
            NormalKeyAction::RefreshTarget => self.refresh_disk_snapshot(core),
            NormalKeyAction::DryRunCleanTarget => self.clean_target_stale(core, true),
            NormalKeyAction::CleanTarget => self.clean_target_stale(core, false),
            NormalKeyAction::CargoCheck => self.run_cargo(core, Focus::Build, &["check"]),
            NormalKeyAction::CargoBuild => self.run_cargo(core, Focus::Build, &["build"]),
            NormalKeyAction::TreeOffline => self.run_tree(core, false),
            NormalKeyAction::TreeWithFetch => self.run_tree(core, true),
            NormalKeyAction::InverseTreeOffline => self.run_inverse_tree(core, false),
            NormalKeyAction::InverseTreeWithFetch => self.run_inverse_tree(core, true),
            NormalKeyAction::PreviewAdd => self.preview_add(core),
            NormalKeyAction::OpenCrates => self.open_search_link(core, SearchLinkTarget::Crates),
            NormalKeyAction::OpenDocs => self.open_search_link(core, SearchLinkTarget::Docs),
            NormalKeyAction::OpenRepository => {
                self.open_search_link(core, SearchLinkTarget::Repository)
            }
            NormalKeyAction::CopySearchDetail => self.copy_search_detail(core),
            NormalKeyAction::OpenSearch => {
                self.open_search(core);
                self.nav.message = "search crates".to_owned();
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
            Focus::Build => match build_items()
                .get(self.view.selected.build)
                .map(|item| item.key)
            {
                Some("check") => self.run_cargo(core, Focus::Build, &["check"]),
                Some("build") => self.run_cargo(core, Focus::Build, &["build"]),
                Some("test") => self.run_cargo(core, Focus::Build, &["test"]),
                Some("run") => self.run_cargo(core, Focus::Build, &["run"]),
                Some("release") => self.run_cargo(core, Focus::Build, &["build", "--release"]),
                Some("clippy") => self.run_cargo(core, Focus::Build, &["clippy", "--all-targets"]),
                Some("doc") => self.run_cargo(core, Focus::Build, &["doc", "--no-deps"]),
                Some("update") => self.run_cargo(core, Focus::Build, &["update"]),
                Some("clean") => self.run_cargo(core, Focus::Build, &["clean"]),
                Some("timings") => self.run_cargo(core, Focus::Build, &["build", "--timings"]),
                Some("diagnostics") => self.view.build_tab = BuildCoreTab::LiveOutput,
                _ => {}
            },
            Focus::Search => {
                if self.nav.input_mode == InputMode::CrateSearch
                    || core.search.state.results.is_empty()
                {
                    core.search.state.expanded = true;
                    self.nav.input_mode = InputMode::CrateSearch;
                    self.nav.message = "search crates".to_owned();
                } else {
                    self.inspect_selected_crate(core);
                }
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

    pub(crate) fn kill_running_child(&mut self, core: &mut CoreState) -> bool {
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

    fn refresh_disk_snapshot(&mut self, core: &mut CoreState) {
        core.disk_receiver = Some(analyze_target_async(
            &core.project,
            core.config.target_stale_days,
        ));
        self.nav.message = "refreshing target analysis".to_owned();
        self.nav.last_status = "target refresh".to_owned();
    }

    fn poll_disk_snapshot(&mut self, core: &mut CoreState) {
        let Some(receiver) = &core.disk_receiver else {
            return;
        };
        let Ok(snapshot) = receiver.try_recv() else {
            return;
        };
        core.disk = snapshot;
        self.view.selected.target_crate = self
            .view
            .selected
            .target_crate
            .min(core.disk.by_crate.len().saturating_sub(1));
        core.disk_receiver = None;
        if self.nav.last_status == "ready" {
            self.nav.last_status = "disk snapshot ready".to_owned();
        }
    }

    fn clean_target_stale(&mut self, core: &mut CoreState, dry_run: bool) {
        let root = Path::new(&core.project.workspace_root);
        match target_analyzer::clean_stale(root, dry_run, core.config.target_stale_days) {
            Ok(report) => {
                let output = std::iter::once(if dry_run {
                    "Target clean dry-run".to_owned()
                } else {
                    "Target clean stale".to_owned()
                })
                .chain(std::iter::once(format!(
                    "threshold: {} days, artifacts: {}, total: {}",
                    core.config.target_stale_days,
                    report.artifact_count,
                    format_bytes(report.total_size)
                )))
                .chain(std::iter::once(String::new()))
                .chain(report.lines.clone())
                .collect();
                core.set_slot_lines(OutputSlot::WorkspaceTarget, output);
                self.view.ws_tab = WorkspaceTab::Target;
                self.nav.current_focus = FocusPanel::Workspace;
                self.set_focus(Focus::Output);
                self.nav.message = if dry_run {
                    format!("dry-run: {} stale artifacts", report.artifact_count)
                } else {
                    format!("cleaned: {} stale artifacts", report.artifact_count)
                };
                self.nav.last_status = "target clean".to_owned();
                if !dry_run {
                    self.refresh_disk_snapshot(core);
                }
            }
            Err(error) => {
                self.nav.message = format!("target clean failed: {error}");
                self.nav.last_status = "target clean failed".to_owned();
            }
        }
    }

    fn drain_all_streams(&mut self, core: &mut CoreState) {
        let max_lines = core.config.output_max_lines;
        if let Some(finish) = core.poll_process(max_lines) {
            self.finish_cargo_output(
                core,
                finish.slot,
                finish.command,
                finish.duration,
                finish.status,
            );
        }
    }

    fn finish_cargo_output(
        &mut self,
        core: &mut CoreState,
        slot: OutputSlot,
        command: String,
        duration: Duration,
        status: ExitStatus,
    ) {
        let success = status.success();
        self.nav.last_status = if success {
            format!("ok {:.2}s", duration.as_secs_f32())
        } else {
            format!("failed {:.2}s", duration.as_secs_f32())
        };
        let lines = {
            let ctx = core.context(slot);
            ctx.lines.push(String::new());
            ctx.lines.push(format!("exit: {status}"));
            ctx.lines
                .push(format!("duration: {:.2}s", duration.as_secs_f32()));
            ctx.lines.clone()
        };
        if slot == OutputSlot::BuildLive {
            core.diagnostics = extract_diagnostics(&lines);
        }
        self.history.insert(
            0,
            HistoryEntry {
                command: command.clone(),
                success,
                duration,
            },
        );
        self.history.truncate(core.config.command_history_limit);
        self.record_build_history(core, &command, duration, success);
        if success && is_project_init_command(&command) {
            self.reload_project_after_init(core);
        }
        if slot == OutputSlot::DepsTree {
            self.update_dependency_tree_from_output(core);
        }
        self.nav.message = format!("finished: {command}");
    }

    fn record_build_history(&mut self, core: &mut CoreState, command: &str, duration: Duration, success: bool) {
        if !is_recordable_command(command) {
            return;
        }
        let entry = BuildEntry {
            timestamp: chrono::Local::now(),
            command: command.to_owned(),
            package: self.selected_package(core),
            duration_ms: duration.as_millis().min(u128::from(u64::MAX)) as u64,
            success,
            target_triple: std::env::consts::ARCH.to_owned(),
            rustc_version: core.project.rustc_version.clone(),
            crate_timings: latest_crate_timings(Path::new(&core.project.workspace_root)),
        };
        if let Err(error) = core
            .history
            .add_entry(entry, core.config.build_history_limit)
        {
            core.diagnostics
                .push(format!("failed to save build history: {error}"));
        }
    }

    fn reload_project_after_init(&mut self, core: &mut CoreState) {
        match ProjectInfo::load() {
            Ok(project) => {
                core.project = project;
                self.view.selected.workspace = 0;
                self.view.selected.dependency = 0;
                core.disk = DiskSnapshot::pending(&core.project.packages);
                core.disk_receiver = Some(analyze_target_async(
                    &core.project,
                    core.config.target_stale_days,
                ));
                self.nav.message = "Cargo project initialized".to_owned();
                self.nav.last_status = "project ready".to_owned();
                self.nav.current_focus = FocusPanel::Workspace;
                self.view.ws_tab = WorkspaceTab::CrateInfo;
            }
            Err(error) => {
                self.nav.message = format!("initialized, but reload failed: {error}");
                self.nav.last_status = "reload failed".to_owned();
            }
        }
    }

    fn build_cargo_command(&self, core: &CoreState, args: &[&str]) -> CommandSpec {
        let Some(kind) = self.cargo_task_kind(args) else {
            return CommandSpec {
                program: "cargo".into(),
                args: self
                    .scoped_args(core, args)
                    .into_iter()
                    .map(Into::into)
                    .collect(),
            };
        };
        let mut task = CargoTask {
            kind,
            scope: self.cargo_task_scope(core, args),
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

    fn cargo_task_scope(&self, core: &CoreState, args: &[&str]) -> TaskScope {
        let command = args.first().copied().unwrap_or_default();
        if matches!(command, "run" | "update") {
            return self
                .selected_package(core)
                .map(TaskScope::Package)
                .unwrap_or(TaskScope::CurrentPackage);
        }
        self.selected_package(core)
            .map(TaskScope::Package)
            .unwrap_or(TaskScope::Workspace)
    }

    fn scoped_args(&self, core: &CoreState, args: &[&str]) -> Vec<String> {
        let mut result = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        let Some(command) = result.first().map(String::as_str) else {
            return result;
        };

        if matches!(command, "init" | "new") {
            return result;
        }

        if matches!(command, "run" | "update" | "clean") {
            if let Some(package) = self.selected_package(core) {
                result.push("-p".to_owned());
                result.push(package);
            }
            return result;
        }

        match self.selected_package(core) {
            Some(package) => {
                result.push("-p".to_owned());
                result.push(package);
            }
            None => result.push("--workspace".to_owned()),
        }

        result
    }

    fn selected_package(&self, core: &CoreState) -> Option<String> {
        if self.view.selected.workspace == 0 {
            None
        } else {
            core.project
                .packages
                .get(self.view.selected.workspace.saturating_sub(1))
                .cloned()
        }
    }

    fn run_cargo(&mut self, core: &mut CoreState, detail_focus: Focus, args: &[&str]) {
        if core.processes.child.is_some() {
            self.nav.message = format!("already running: {}", core.processes.command);
            return;
        }

        let spec = self.build_cargo_command(core, args);
        let command = spec.display();
        let slot = match detail_focus {
            Focus::Build => OutputSlot::BuildLive,
            Focus::Dependencies => OutputSlot::DepsTree,
            Focus::Search => OutputSlot::SearchDetail,
            _ => self.active_output_slot(core),
        };
        self.nav.command_preview = command.clone();
        self.nav.message = format!("running: {command}");
        self.set_focus(detail_focus);
        if detail_focus == Focus::Build {
            self.view.build_tab = BuildCoreTab::LiveOutput;
        } else if detail_focus == Focus::Dependencies {
            self.view.deps_tab = DependenciesTab::DependencyTree;
        }

        match core.spawn_command(&spec, slot) {
            Ok(()) => {}
            Err(_) => {
                self.nav.last_status = "error".to_owned();
                self.nav.message = format!("failed: {}", spec.display());
            }
        }
    }

    fn run_tree(&mut self, core: &mut CoreState, allow_fetch: bool) {
        if allow_fetch {
            self.run_cargo(core, Focus::Dependencies, &["tree", "-e", "features"]);
        } else {
            self.run_cargo(
                core,
                Focus::Dependencies,
                &["tree", "--offline", "-e", "features"],
            );
        }
        self.view.deps_tab = DependenciesTab::DependencyTree;
    }

    fn run_inverse_tree(&mut self, core: &mut CoreState, allow_fetch: bool) {
        let Some(dependency) = selected_dependency_name(
            &core.project,
            self.view.selected.workspace,
            self.view.selected.dependency,
        ) else {
            core.set_slot_lines(
                OutputSlot::DepsFeatures,
                vec!["select a dependency first".to_owned()],
            );
            return;
        };

        if allow_fetch {
            self.run_cargo(
                core,
                Focus::Dependencies,
                &["tree", "-e", "features", "-i", &dependency],
            );
        } else {
            self.run_cargo(
                core,
                Focus::Dependencies,
                &["tree", "--offline", "-e", "features", "-i", &dependency],
            );
        }
        self.view.deps_tab = DependenciesTab::DependencyTree;
    }

    /// tree 命令的产出解析，由 finish_cargo_output（DepsTree 分支）触发。
    fn update_dependency_tree_from_output(&mut self, core: &mut CoreState) {
        let lines = core.slot_lines(OutputSlot::DepsTree);
        let nodes = dep_tree::parse_tree_output(&lines, &self.view.tree_expanded);
        if nodes.is_empty() {
            core.context(OutputSlot::DepsTree).lines.insert(
                0,
                "structured tree parse unavailable; showing raw cargo tree output".to_owned(),
            );
            return;
        }
        let visible_len = dep_tree::flatten_visible(&nodes).len();
        self.view.selected.tree = self
            .view
            .selected
            .tree
            .min(visible_len.saturating_sub(1));
        let selected = self.view.selected.tree;
        let ctx = core.context(OutputSlot::DepsTree);
        ctx.tree_nodes = nodes;
        dep_tree::apply_selected(&mut ctx.tree_nodes, selected);
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
        self.preview(&command);
        let detail = core
            .search
            .state
            .selected_detail()
            .into_iter()
            .chain(vec![String::new(), command])
            .collect();
        core.set_slot_lines(OutputSlot::SearchDetail, detail);
    }

    fn open_search(&mut self, core: &mut CoreState) {
        if self.nav.focus != Focus::Search {
            self.view.search_return_focus = self.nav.focus;
        }
        self.set_focus(Focus::Search);
        core.search.state.expanded = true;
        self.reset_slot_scroll(core, OutputSlot::SearchDetail);
        self.nav.input_mode = InputMode::CrateSearch;
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

    fn search_job_config(&self, core: &CoreState) -> SearchJobConfig {
        SearchJobConfig {
            limit: core.config.search_limit,
            network_timeout: core.config.network_timeout(),
            info_timeout: core.config.cargo_info_timeout(),
        }
    }

    fn open_search_link(&mut self, core: &CoreState, target: SearchLinkTarget) {
        let url = core.search.state.url_for(target);

        let Some(url) = url else {
            self.nav.message = SearchState::unavailable_message(target).to_owned();
            return;
        };

        match open_url(&url) {
            Ok(()) => self.open_url_message_success(&url),
            Err(error) => self.open_url_message_error(error),
        }
    }

    fn open_url_message_success(&mut self, url: &str) {
        self.nav.message = format!("opened: {url}");
        self.nav.last_status = "opened link".to_owned();
    }

    fn open_url_message_error(&mut self, error: io::Error) {
        self.nav.message = format!("failed to open link: {error}");
        self.nav.last_status = "open link failed".to_owned();
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

        if let Some((_, tab)) = tab_under(mouse_state, mouse.column, mouse.row) {
            self.apply_context_tab(core, tab);
            self.set_focus(Focus::Output);
            self.nav.message = format!("selected {} tab", tab.label());
            return;
        }

        if let Some((focus, area, offset)) = panel_under(mouse_state, mouse.column, mouse.row) {
            self.set_focus(focus);
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

    fn scrollbar_to_row(&mut self, core: &mut CoreState, mouse_state: &MouseState, column: u16, row: u16) -> bool {
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
        let ctx = core.context(self.active_output_slot(core));
        ctx.scroll = scroll;
        ctx.follow_tail = scroll >= max_scroll;
        self.set_focus(Focus::Output);
        self.nav.message = format!("right detail scroll: {scroll}");
        true
    }

    fn select_row(&mut self, core: &mut CoreState, focus: Focus, row: usize) {
        let len = match focus {
            Focus::Workspace => workspace_items(&core.project).len(),
            Focus::Dependencies => {
                dependency_items_for(&core.project, self.view.selected.workspace).len()
            }
            Focus::Build => build_items().len(),
            _ => 0,
        };

        if len > 0 {
            *self.selected_mut(focus) = row.min(len.saturating_sub(1));
            if focus == Focus::Workspace {
                self.sync_workspace_selection(core);
            }
            if focus == Focus::Dependencies {
                self.view.deps_tab = DependenciesTab::Features;
                self.reset_slot_scroll(core, OutputSlot::DepsFeatures);
            }
        }
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
                // search 输入归 SearchPage、project-new-confirm 归 InitPage；
                // 路由由根 App 分派，此分支在 Workspace 路由下不会到达，防御性忽略。
                true
            }
        }
    }

    fn handle_tick(&mut self, core: &mut CoreState) {
        self.poll_disk_snapshot(core);
        self.drain_all_streams(core);
    }

    fn handle_mouse(&mut self, core: &mut CoreState, mouse: MouseEvent, state: &MouseState) {
        self.mouse_event(core, mouse, state);
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
