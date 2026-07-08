use std::collections::HashMap;
use std::process::Child;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use lazycargo_search::SearchState;

use crate::build_history::BuildHistory;
use crate::dep_tree::DepNode;
use crate::metadata::ProjectInfo;
use crate::target_analyzer::DiskSnapshot;
use crate::ui::runner::OutputLine;
use crate::ui::{BuildCoreTab, DependenciesTab, Focus, FocusPanel, InputMode, WorkspaceTab};

pub(crate) struct WorkspaceModel {
    pub(crate) project: ProjectInfo,
    pub(crate) disk: DiskSnapshot,
    pub(crate) disk_receiver: Option<Receiver<DiskSnapshot>>,
    pub(crate) build_history: BuildHistory,
    pub(crate) diagnostics: Vec<String>,
}

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
    pub(crate) new_project_name: String,
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
    pub(crate) tree_selected: usize,
    pub(crate) tree_expanded: HashMap<String, bool>,
}

pub(crate) struct ProcessState {
    pub(crate) child: Option<Child>,
    pub(crate) command: String,
    pub(crate) start: Instant,
    pub(crate) slot: OutputSlot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum OutputSlot {
    WorkspaceCrateInfo,
    WorkspaceMetrics,
    WorkspaceTarget,
    BuildConfig,
    BuildLive,
    DepsFeatures,
    DepsTree,
    SearchDetail,
}

pub(crate) struct ContextOutput {
    pub(crate) lines: Vec<String>,
    pub(crate) stream_rx: Option<mpsc::Receiver<OutputLine>>,
    pub(crate) scroll: usize,
    pub(crate) tree_nodes: Vec<DepNode>,
}

impl ContextOutput {
    pub(crate) fn new() -> Self {
        Self {
            lines: Vec::new(),
            stream_rx: None,
            scroll: 0,
            tree_nodes: Vec::new(),
        }
    }

    pub(crate) fn with_lines(lines: Vec<String>) -> Self {
        Self {
            lines,
            stream_rx: None,
            scroll: 0,
            tree_nodes: Vec::new(),
        }
    }

    pub(crate) fn drain_stream(&mut self) {
        let Some(rx) = self.stream_rx.take() else {
            return;
        };
        let mut disconnected = false;
        loop {
            match rx.try_recv() {
                Ok(OutputLine::Stdout(line)) | Ok(OutputLine::Stderr(line)) => {
                    self.lines.push(line);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
        if !disconnected {
            self.stream_rx = Some(rx);
        }
    }
}

pub(crate) struct SearchModel {
    pub(crate) state: SearchState,
}
