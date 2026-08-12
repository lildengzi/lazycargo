use std::collections::HashMap;

use crate::ui::controller::{
    BuildCoreTab, DependenciesTab, Focus, FocusPanel, InputMode, WorkspaceTab,
};

pub(crate) struct NavigationState {
    pub(crate) focus: Focus,
    pub(crate) current_focus: FocusPanel,
    pub(crate) input_mode: InputMode,
    pub(crate) ws_tab: WorkspaceTab,
    pub(crate) build_tab: BuildCoreTab,
    pub(crate) deps_tab: DependenciesTab,
    pub(crate) menu_open: bool,
    pub(crate) menu_selected: usize,
    pub(crate) filter: String,
    pub(crate) search_return_focus: Focus,
    pub(crate) command_preview: String,
    pub(crate) message: String,
    pub(crate) last_status: String,
    pub(crate) copy_mode: bool,
}

pub(crate) struct SelectionState {
    pub(crate) workspace_selected: usize,
    pub(crate) dependency_selected: usize,
    pub(crate) build_selected: usize,
    pub(crate) target_crate_selected: usize,
    pub(crate) tree_selected: usize,
    pub(crate) tree_expanded: HashMap<String, bool>,
}
