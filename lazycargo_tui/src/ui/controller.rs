use std::collections::HashMap;

use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::core::model::CoreState;

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
    pub(crate) fn from_digit(value: char) -> Self {
        match value {
            '1' => Self::Workspace,
            '2' => Self::Build,
            '3' => Self::Dependencies,
            '0' => Self::Output,
            _ => Self::Workspace,
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Workspace => Self::Build,
            Self::Build => Self::Dependencies,
            Self::Dependencies => Self::Output,
            Self::Search => Self::Workspace,
            Self::Output => Self::Workspace,
            Self::CommandLog => Self::Workspace,
        }
    }

    pub(crate) fn title(self) -> &'static str {
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
    ProjectNewConfirm,
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
pub(crate) enum ContextTab {
    Workspace(WorkspaceTab),
    Build(BuildCoreTab),
    Dependencies(DependenciesTab),
}

impl FocusPanel {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::BuildCore => "Build Core",
            Self::Dependencies => "Dependencies",
        }
    }
}

impl ContextTab {
    pub(crate) fn label(self) -> &'static str {
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

#[derive(Default)]
pub(crate) struct MouseState {
    pub panel_areas: Vec<(Focus, Rect, usize)>,
    pub link_areas: Vec<(Rect, String)>,
    pub tab_areas: Vec<(Rect, ContextTab)>,
    pub right_scrollbar_area: Option<Rect>,
    pub right_scrollbar_content_len: usize,
    pub right_scrollbar_visible_rows: usize,
}

pub(crate) trait Page {
    fn handle_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool;
    fn handle_tick(&mut self, _core: &mut CoreState) {}
    fn render(&self, core: &CoreState, frame: &mut Frame<'_>, area: Rect) -> MouseState;
    fn handle_mouse(&mut self, _core: &mut CoreState, _mouse: MouseEvent, _state: &MouseState) {}
}

pub(crate) struct WorkspaceView {
    pub ws_tab: WorkspaceTab,
    pub build_tab: BuildCoreTab,
    pub deps_tab: DependenciesTab,
    pub selected: SelectedIndex,
    pub tree_expanded: HashMap<String, bool>,
    pub search_return_focus: Focus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SelectedIndex {
    pub workspace: usize,
    pub dependency: usize,
    pub build: usize,
    pub target_crate: usize,
    pub tree: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DocsView {
    pub scroll: usize,
}

impl Default for WorkspaceView {
    fn default() -> Self {
        Self {
            ws_tab: WorkspaceTab::CrateInfo,
            build_tab: BuildCoreTab::TaskConfig,
            deps_tab: DependenciesTab::Features,
            selected: SelectedIndex::default(),
            tree_expanded: HashMap::new(),
            search_return_focus: Focus::Workspace,
        }
    }
}

pub(crate) fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

pub(crate) fn link_under(state: &MouseState, column: u16, row: u16) -> Option<(Rect, String)> {
    state
        .link_areas
        .iter()
        .find(|(area, _)| contains(*area, column, row))
        .cloned()
}

pub(crate) fn tab_under(state: &MouseState, column: u16, row: u16) -> Option<(Rect, ContextTab)> {
    state
        .tab_areas
        .iter()
        .find(|(area, _)| contains(*area, column, row))
        .copied()
}

pub(crate) fn panel_under(
    state: &MouseState,
    column: u16,
    row: u16,
) -> Option<(Focus, Rect, usize)> {
    state
        .panel_areas
        .iter()
        .find(|(_, area, _)| contains(*area, column, row))
        .copied()
}

pub(crate) fn focus_under(state: &MouseState, column: u16, row: u16) -> Option<Focus> {
    panel_under(state, column, row).map(|(focus, _, _)| focus)
}
