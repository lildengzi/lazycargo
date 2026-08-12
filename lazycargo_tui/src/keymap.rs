use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::controller::{DependenciesTab, Focus, WorkspaceTab};

pub(crate) struct NormalKeyContext {
    pub(crate) search_expanded: bool,
    pub(crate) focus: Focus,
    pub(crate) ws_tab: WorkspaceTab,
    pub(crate) deps_tab: DependenciesTab,
}

pub(crate) enum NormalKeyAction {
    BackFromSearch,
    Quit,
    OpenKeys,
    FocusNext,
    FocusOutput,
    SwitchTab(isize),
    ToggleCopyMode,
    FocusDigit(char),
    ScrollRight(isize),
    OutputUp,
    OutputDown,
    MoveUp,
    MoveDown,
    Activate,
    OpenFilter,
    RefreshTarget,
    DryRunCleanTarget,
    CleanTarget,
    CargoCheck,
    CargoBuild,
    TreeOffline,
    TreeWithFetch,
    InverseTreeOffline,
    InverseTreeWithFetch,
    PreviewAdd,
    OpenCrates,
    OpenDocs,
    OpenRepository,
    CopySearchDetail,
    CollapseTree,
    ToggleTree,
    OpenSearch,
    Noop,
}

pub(crate) enum TextInputAction {
    Cancel,
    Submit,
    Backspace,
    Push(char),
    Noop,
}

pub(crate) enum ProjectNewConfirmAction {
    Yes,
    No,
    Noop,
}

pub(crate) struct HelpSection {
    pub(crate) title: &'static str,
    pub(crate) entries: &'static [HelpEntry],
}

pub(crate) struct HelpEntry {
    pub(crate) key: &'static str,
    pub(crate) label: &'static str,
}

pub(crate) const CLOSE_KEYS: &str = "Esc/x/Enter";
pub(crate) const SEARCH_STATUS_HINT: &str = "Enter search, Esc results, q back, x keys";
pub(crate) const FILTER_STATUS_HINT: &str = "  Enter apply, Esc cancel";
pub(crate) const COPY_STATUS_HINT: &str = "drag select, m mouse, x keys";
pub(crate) const INIT_PROJECT_PROMPT: &str = "initialize Cargo project here? ";

pub(crate) const HELP_SECTIONS: &[HelpSection] = &[
    HelpSection {
        title: "Navigation",
        entries: &[
            HelpEntry {
                key: "1 / 2 / 3",
                label: "focus workspace / build / deps",
            },
            HelpEntry {
                key: "0 / click",
                label: "focus right waterfall",
            },
            HelpEntry {
                key: "Tab",
                label: "cycle focus",
            },
            HelpEntry {
                key: "j/k arrows",
                label: "move selection or scroll focused pane",
            },
            HelpEntry {
                key: "PgUp/PgDn",
                label: "scroll right waterfall",
            },
        ],
    },
    HelpSection {
        title: "Tabs",
        entries: &[
            HelpEntry {
                key: "[ / ]",
                label: "switch Detail / Output / Tree / Metrics",
            },
            HelpEntry {
                key: "click tab",
                label: "switch right tab",
            },
        ],
    },
    HelpSection {
        title: "Actions",
        entries: &[
            HelpEntry {
                key: "Enter",
                label: "run selected action or inspect",
            },
            HelpEntry {
                key: "c / b",
                label: "cargo check / build",
            },
            HelpEntry {
                key: "t / i",
                label: "offline tree / inverse tree",
            },
            HelpEntry {
                key: "T / I",
                label: "tree / inverse tree with fetch",
            },
            HelpEntry {
                key: "s",
                label: "open search page",
            },
            HelpEntry {
                key: "/",
                label: "filter current panel",
            },
            HelpEntry {
                key: "m",
                label: "toggle terminal copy mode",
            },
            HelpEntry {
                key: "q",
                label: "back or quit",
            },
        ],
    },
    HelpSection {
        title: "Search",
        entries: &[
            HelpEntry {
                key: "Enter",
                label: "search input or inspect result",
            },
            HelpEntry {
                key: "a",
                label: "preview cargo add",
            },
            HelpEntry {
                key: "o/d/g",
                label: "open crates/docs/repo",
            },
            HelpEntry {
                key: "y",
                label: "copy selected detail",
            },
        ],
    },
];

pub(crate) fn is_ctrl_c(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL)
}

pub(crate) fn menu_closes(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc | KeyCode::Char('x') | KeyCode::Enter)
}

pub(crate) fn normal_key_action(key: KeyEvent, context: NormalKeyContext) -> NormalKeyAction {
    match key.code {
        KeyCode::Char('q') if context.search_expanded => NormalKeyAction::BackFromSearch,
        KeyCode::Esc if context.search_expanded => NormalKeyAction::BackFromSearch,
        KeyCode::Char('q') => NormalKeyAction::Quit,
        KeyCode::Char('x') => NormalKeyAction::OpenKeys,
        KeyCode::Tab => NormalKeyAction::FocusNext,
        KeyCode::BackTab => NormalKeyAction::FocusOutput,
        KeyCode::Char(']') => NormalKeyAction::SwitchTab(1),
        KeyCode::Char('[') => NormalKeyAction::SwitchTab(-1),
        KeyCode::Char('m') => NormalKeyAction::ToggleCopyMode,
        KeyCode::Char('0') => NormalKeyAction::FocusOutput,
        KeyCode::Char(value @ ('1'..='3')) => NormalKeyAction::FocusDigit(value),
        KeyCode::PageUp => NormalKeyAction::ScrollRight(-10),
        KeyCode::PageDown => NormalKeyAction::ScrollRight(10),
        KeyCode::Up | KeyCode::Char('k') if context.focus == Focus::Output => {
            NormalKeyAction::OutputUp
        }
        KeyCode::Down | KeyCode::Char('j') if context.focus == Focus::Output => {
            NormalKeyAction::OutputDown
        }
        KeyCode::Up | KeyCode::Char('k') => NormalKeyAction::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => NormalKeyAction::MoveDown,
        KeyCode::Enter => NormalKeyAction::Activate,
        KeyCode::Char('/') => NormalKeyAction::OpenFilter,
        KeyCode::Char('r') if context.ws_tab == WorkspaceTab::Target => {
            NormalKeyAction::RefreshTarget
        }
        KeyCode::Char('d') if context.ws_tab == WorkspaceTab::Target => {
            NormalKeyAction::DryRunCleanTarget
        }
        KeyCode::Char('c') if context.ws_tab == WorkspaceTab::Target => {
            NormalKeyAction::CleanTarget
        }
        KeyCode::Char('c') => NormalKeyAction::CargoCheck,
        KeyCode::Char('b') => NormalKeyAction::CargoBuild,
        KeyCode::Char('t') => NormalKeyAction::TreeOffline,
        KeyCode::Char('T') => NormalKeyAction::TreeWithFetch,
        KeyCode::Char('i') => NormalKeyAction::InverseTreeOffline,
        KeyCode::Char('I') => NormalKeyAction::InverseTreeWithFetch,
        KeyCode::Char('a') => NormalKeyAction::PreviewAdd,
        KeyCode::Char('o') if context.search_expanded => NormalKeyAction::OpenCrates,
        KeyCode::Char('d') if context.search_expanded => NormalKeyAction::OpenDocs,
        KeyCode::Char('g') if context.search_expanded => NormalKeyAction::OpenRepository,
        KeyCode::Char('y') if context.search_expanded => NormalKeyAction::CopySearchDetail,
        KeyCode::Left | KeyCode::Char('h')
            if context.deps_tab == DependenciesTab::DependencyTree =>
        {
            NormalKeyAction::CollapseTree
        }
        KeyCode::Right | KeyCode::Char('l')
            if context.deps_tab == DependenciesTab::DependencyTree =>
        {
            NormalKeyAction::ToggleTree
        }
        KeyCode::Char('s') => NormalKeyAction::OpenSearch,
        _ => NormalKeyAction::Noop,
    }
}

pub(crate) fn text_input_action(key: KeyEvent) -> TextInputAction {
    match key.code {
        KeyCode::Esc => TextInputAction::Cancel,
        KeyCode::Enter => TextInputAction::Submit,
        KeyCode::Backspace => TextInputAction::Backspace,
        KeyCode::Char(value) => TextInputAction::Push(value),
        _ => TextInputAction::Noop,
    }
}

pub(crate) fn project_new_confirm_action(key: KeyEvent) -> ProjectNewConfirmAction {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => ProjectNewConfirmAction::Yes,
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => ProjectNewConfirmAction::No,
        _ => ProjectNewConfirmAction::Noop,
    }
}
