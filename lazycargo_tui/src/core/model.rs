use std::collections::HashMap;
use std::process::Child;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

use lazycargo_search::SearchState;

use crate::core::build_history::BuildHistory;
use crate::core::config::AppConfig;
use crate::core::dep_tree::DepNode;
use crate::core::process::OutputLine;
use crate::core::project::ProjectInfo;
use crate::core::target_analyzer::DiskSnapshot;
use crate::core::task::SearchJobResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputSlot {
    WorkspaceCrateInfo,
    WorkspaceMetrics,
    WorkspaceTarget,
    BuildConfig,
    BuildLive,
    DepsFeatures,
    DepsTree,
    SearchDetail,
    DocsReadme,
    DocsFallback,
}

pub struct ContextOutput {
    pub lines: Vec<String>,
    pub stream_rx: Option<mpsc::Receiver<OutputLine>>,
    pub scroll: usize,
    pub follow_tail: bool,
    pub visible_rows: usize,
    pub tree_nodes: Vec<DepNode>,
}

impl ContextOutput {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            stream_rx: None,
            scroll: 0,
            follow_tail: false,
            visible_rows: 0,
            tree_nodes: Vec::new(),
        }
    }

    pub fn with_lines(lines: Vec<String>) -> Self {
        Self {
            lines,
            stream_rx: None,
            scroll: 0,
            follow_tail: false,
            visible_rows: 0,
            tree_nodes: Vec::new(),
        }
    }

    pub fn drain_stream(&mut self, max_lines: usize) {
        let Some(rx) = self.stream_rx.take() else {
            return;
        };
        let mut disconnected = false;
        let mut received = false;
        loop {
            match rx.try_recv() {
                Ok(OutputLine::Stdout(line)) | Ok(OutputLine::Stderr(line)) => {
                    self.lines.push(line);
                    received = true;
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
        self.trim_lines(max_lines);
        if received && self.follow_tail {
            self.scroll = usize::MAX;
        }
    }

    pub fn trim_lines(&mut self, max_lines: usize) {
        let max_lines = max_lines.max(1);
        let overflow = self.lines.len().saturating_sub(max_lines);
        if overflow == 0 {
            return;
        }
        self.lines.drain(..overflow);
        self.scroll = self.scroll.saturating_sub(overflow);
    }
}

pub struct ProcessState {
    pub child: Option<Child>,
    pub command: String,
    pub start: Instant,
    pub slot: OutputSlot,
}

pub struct SearchModel {
    pub state: SearchState,
}

pub struct CoreState {
    pub config: AppConfig,
    pub project: ProjectInfo,
    pub disk: DiskSnapshot,
    pub disk_receiver: Option<Receiver<DiskSnapshot>>,
    pub history: BuildHistory,
    pub processes: ProcessState,
    pub output: HashMap<OutputSlot, ContextOutput>,
    pub search: SearchModel,
    pub search_receiver: Option<Receiver<SearchJobResult>>,
    pub search_started: Option<Instant>,
    pub diagnostics: Vec<String>,
}

impl CoreState {
    pub fn new(project: ProjectInfo, config: AppConfig, disk: DiskSnapshot) -> Self {
        let mut output = HashMap::new();
        output.insert(OutputSlot::BuildLive, ContextOutput::with_lines(Vec::new()));
        Self {
            config,
            project,
            disk,
            disk_receiver: None,
            history: BuildHistory::load(),
            processes: ProcessState {
                child: None,
                command: String::new(),
                start: Instant::now(),
                slot: OutputSlot::BuildLive,
            },
            output,
            search: SearchModel {
                state: SearchState::default(),
            },
            search_receiver: None,
            search_started: None,
            diagnostics: Vec::new(),
        }
    }

    pub fn context(&mut self, slot: OutputSlot) -> &mut ContextOutput {
        self.output.entry(slot).or_insert_with(ContextOutput::new)
    }

    pub fn slot_lines(&self, slot: OutputSlot) -> Vec<String> {
        self.output
            .get(&slot)
            .map(|ctx| ctx.lines.clone())
            .unwrap_or_default()
    }

    pub fn set_slot_lines(&mut self, slot: OutputSlot, lines: Vec<String>) {
        let max_lines = self.config.output_max_lines;
        let ctx = self.context(slot);
        ctx.lines = lines;
        ctx.trim_lines(max_lines);
        ctx.stream_rx = None;
        ctx.scroll = 0;
        ctx.follow_tail = false;
        if slot != OutputSlot::DepsTree {
            ctx.tree_nodes.clear();
        }
    }

    pub fn drain_all_streams(&mut self, max_lines: usize) {
        for ctx in self.output.values_mut() {
            ctx.drain_stream(max_lines);
        }
    }
}
