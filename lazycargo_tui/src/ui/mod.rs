use std::io;
use std::time::Duration;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEvent, MouseEvent,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::{Frame, Terminal};

use crate::core::config::AppConfig;
use crate::core::model::{ContextOutput, CoreState, OutputSlot};
use crate::core::project::ProjectInfo;
use crate::core::target_analyzer::{analyze_target_async, DiskSnapshot};
use crate::keymap;

mod components;
pub mod controller;
mod pages;

use controller::*;

mod terminal_support;

use components::{command_log::render_command_log, menu::render_keys_dialog};

use pages::docs::controller::DocsPage;
use pages::init::InitPage;
use pages::search::controller::SearchPage;
use pages::workspace::controller::WorkspacePage;

#[derive(Clone)]
pub(crate) struct HistoryEntry {
    pub(crate) command: String,
    pub(crate) success: bool,
    pub(crate) duration: Duration,
}

/// 当前活动页面路由：Docs（TUI 内嵌文档阅读）→ Search（搜索展开）→ Init（无 Cargo 项目的初始化确认）→ Workspace。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Workspace,
    Search,
    Docs,
    Init,
}

/// 根 App 是页面装配器 + 事件循环接线：不持有 navigation/selection/history，
/// 全部由常驻的 `WorkspacePage`（及 `SearchPage`/`DocsPage`/`InitPage`）持有，路由切换时同步。
struct App {
    core: CoreState,
    page: WorkspacePage,
    search: SearchPage,
    docs: DocsPage,
    init: InitPage,
    route_prev: Route,
}

struct CommandBarState {
    input_mode: InputMode,
    filter: String,
    last_status: String,
    copy_mode: bool,
}

impl App {
    fn new(project: ProjectInfo, config: AppConfig) -> Self {
        let disk = DiskSnapshot::pending(&project.packages);
        let disk_receiver = Some(analyze_target_async(&project, config.target_stale_days));
        let output = project_health_snapshot(&project, &disk);
        let mut core = CoreState::new(project.clone(), config, disk);
        core.disk_receiver = disk_receiver;
        core.output
            .insert(OutputSlot::BuildLive, ContextOutput::with_lines(output));
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
            page: WorkspacePage::new(),
            search: SearchPage::new(),
            docs: DocsPage::new(),
            init: InitPage::new(),
            route_prev: Route::Workspace,
        }
    }

    fn route(&self) -> Route {
        if self.core.docs_open {
            Route::Docs
        } else if self.core.search.state.expanded {
            Route::Search
        } else if self.page.nav.input_mode == InputMode::ProjectNewConfirm {
            Route::Init
        } else {
            Route::Workspace
        }
    }

    /// 路由切换时同步各页面的导航状态子集。路由由 `core.docs_open` /
    /// `core.search.state.expanded` / `page.nav.input_mode` 决定，只可能发生在
    /// Workspace ↔ Search、Workspace ↔ Init、Workspace/Search ↔ Docs。
    fn sync_routes(&mut self) {
        let mut route = self.route();
        // Init 对话框被 InitPage 应答（y/n）后其 input_mode 回到 Normal，
        // 此时必须离开 Init 路由；route() 只看 page.nav.input_mode 会一直卡住。
        if route == Route::Init
            && self.route_prev == Route::Init
            && self.init.nav.input_mode != InputMode::ProjectNewConfirm
        {
            route = Route::Workspace;
        }
        if route == self.route_prev {
            return;
        }
        let previous = self.route_prev;
        match route {
            Route::Docs => self.enter_docs(),
            Route::Search => {
                if previous != Route::Docs {
                    self.enter_search();
                }
            }
            Route::Init => self.enter_init(),
            Route::Workspace => self.leave_to_workspace(),
        }
        self.route_prev = route;
    }

    fn enter_docs(&mut self) {
        self.docs.view.scroll = 0;
    }

    fn enter_search(&mut self) {
        self.search.nav.focus = self.page.nav.focus;
        self.search.nav.input_mode = self.page.nav.input_mode;
        self.search.nav.filter = self.page.nav.filter.clone();
        self.search.nav.message = self.page.nav.message.clone();
        self.search.nav.last_status = self.page.nav.last_status.clone();
        self.search.nav.copy_mode = self.page.nav.copy_mode;
        self.search.nav.command_preview = self.page.nav.command_preview.clone();
        self.search.nav.search_return_focus = self.page.view.search_return_focus;
        self.search.nav.menu_open = self.page.nav.menu_open;
        self.search.nav.menu_selected = self.page.nav.menu_selected;
        self.search.nav.workspace_selected = self.page.view.selected.workspace;
        self.search.history = self.page.history.clone();
    }

    fn enter_init(&mut self) {
        self.init.nav.input_mode = self.page.nav.input_mode;
        self.init.nav.message = self.page.nav.message.clone();
        self.init.nav.last_status = self.page.nav.last_status.clone();
    }

    fn leave_to_workspace(&mut self) {
        match self.route_prev {
            Route::Search => {
                self.page.nav.focus = self.search.nav.focus;
                self.page.nav.input_mode = self.search.nav.input_mode;
                self.page.nav.filter = self.search.nav.filter.clone();
                self.page.nav.message = self.search.nav.message.clone();
                self.page.nav.last_status = self.search.nav.last_status.clone();
                self.page.nav.copy_mode = self.search.nav.copy_mode;
                self.page.nav.command_preview = self.search.nav.command_preview.clone();
                self.page.view.search_return_focus = self.search.nav.search_return_focus;
                self.page.nav.menu_open = self.search.nav.menu_open;
                self.page.nav.menu_selected = self.search.nav.menu_selected;
                self.page.history = self.search.history.clone();
            }
            Route::Init => {
                let accepted = self.init.nav.accepted;
                self.page.nav.input_mode = self.init.nav.input_mode;
                self.page.nav.message = self.init.nav.message.clone();
                self.page.nav.last_status = self.init.nav.last_status.clone();
                if accepted {
                    self.page.nav.focus = Focus::Build;
                    self.page.nav.current_focus = FocusPanel::BuildCore;
                    self.page.view.build_tab = BuildCoreTab::LiveOutput;
                }
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> bool {
        if keymap::is_ctrl_c(key) {
            return self.page.kill_running_child(&mut self.core);
        }
        match self.route() {
            Route::Docs => self.docs.handle_key(&mut self.core, key),
            Route::Search => self.search.handle_key(&mut self.core, key),
            Route::Init => self.init.handle_key(&mut self.core, key),
            Route::Workspace => self.page.handle_key(&mut self.core, key),
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, mouse_state: &MouseState) {
        match self.route() {
            Route::Docs => {}
            Route::Search => self.search.handle_mouse(&mut self.core, mouse, mouse_state),
            Route::Init => {}
            Route::Workspace => self.page.handle_mouse(&mut self.core, mouse, mouse_state),
        }
    }

    fn command_bar_state(&self) -> CommandBarState {
        match self.route() {
            Route::Docs => CommandBarState {
                input_mode: InputMode::Normal,
                filter: String::new(),
                last_status: String::new(),
                copy_mode: false,
            },
            Route::Search => CommandBarState {
                input_mode: self.search.nav.input_mode,
                filter: self.search.nav.filter.clone(),
                last_status: self.search.nav.last_status.clone(),
                copy_mode: self.search.nav.copy_mode,
            },
            Route::Init => CommandBarState {
                input_mode: self.init.nav.input_mode,
                filter: String::new(),
                last_status: self.init.nav.last_status.clone(),
                copy_mode: false,
            },
            Route::Workspace => CommandBarState {
                input_mode: self.page.nav.input_mode,
                filter: self.page.nav.filter.clone(),
                last_status: self.page.nav.last_status.clone(),
                copy_mode: self.page.nav.copy_mode,
            },
        }
    }

    fn command_log_line(&self) -> Line<'static> {
        let state = self.command_bar_state();
        match state.input_mode {
            InputMode::Normal if self.core.docs_open => Line::from(vec![
                Span::styled("docs: ", Style::default().fg(Color::Yellow)),
                Span::raw("j/k scroll, q back, "),
                Span::styled("D", Style::default().fg(Color::Green)),
                Span::raw(" browser, "),
                Span::styled("x", Style::default().fg(Color::Green)),
                Span::raw(" keys"),
            ]),
            InputMode::CrateSearch => Line::from(vec![
                Span::styled("search: ", Style::default().fg(Color::Yellow)),
                Span::raw(keymap::SEARCH_STATUS_HINT),
                Span::styled(
                    format!("  {}", state.last_status),
                    Style::default().fg(Color::Green),
                ),
            ]),
            InputMode::Filter => Line::from(vec![
                Span::styled("filter: ", Style::default().fg(Color::Yellow)),
                Span::raw(state.filter),
                Span::styled(
                    keymap::FILTER_STATUS_HINT,
                    Style::default().fg(Color::Green),
                ),
            ]),
            InputMode::ProjectNewConfirm => Line::from(vec![
                Span::styled("no Cargo.toml: ", Style::default().fg(Color::Yellow)),
                Span::raw(keymap::INIT_PROJECT_PROMPT),
                Span::styled("y", Style::default().fg(Color::Green)),
                Span::raw("/"),
                Span::styled("n", Style::default().fg(Color::Green)),
            ]),
            InputMode::Normal if state.copy_mode => Line::from(vec![
                Span::styled("copy: ", Style::default().fg(Color::Yellow)),
                Span::raw(keymap::COPY_STATUS_HINT),
                Span::styled(
                    format!("  {}", state.last_status),
                    Style::default().fg(Color::Green),
                ),
            ]),
            InputMode::Normal if self.core.search.state.expanded => Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(": inspect, "),
                Span::styled("a", Style::default().fg(Color::Green)),
                Span::raw(": add, "),
                Span::styled("q", Style::default().fg(Color::Green)),
                Span::raw(": back, "),
                Span::styled("x", Style::default().fg(Color::Green)),
                Span::raw(": keys"),
                Span::styled(
                    format!("  {}", state.last_status),
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
                    format!("  {}", state.last_status),
                    Style::default().fg(Color::Green),
                ),
            ]),
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
            ProjectInfo::fallback(),
            Some(format!("No Cargo.toml found; limited mode: {error}")),
        ),
    };
    let mut terminal = setup_terminal()?;
    let mut app = App::new(project, config);
    if let Some(message) = config_message {
        app.page.nav.message = message;
    }
    if let Some(message) = startup_message {
        app.page.nav.last_status = "limited mode".to_owned();
        app.page.nav.message = "no Cargo project: initialize here? y/n".to_owned();
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
        app.page.nav.input_mode = InputMode::ProjectNewConfirm;
        app.page.nav.command_preview = "cargo init".to_owned();
    }
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
    let mut mouse_state = MouseState::default();
    loop {
        app.sync_routes();
        // tick：先轮询搜索任务（与旧 App 的 poll_search 先于 drain 的顺序一致），
        // 再处理磁盘快照 + 进程输出（磁盘先于搜索的状态栏文本会被覆盖，与旧行为一致）。
        let search_completed = app.search.poll_search_job(&mut app.core);
        if search_completed && app.route() != Route::Search {
            app.page.nav.message = app.search.nav.message.clone();
            app.page.nav.last_status = app.search.nav.last_status.clone();
            app.page.history = app.search.history.clone();
        }
        app.page.handle_tick(&mut app.core);
        if app.route() == Route::Docs {
            app.docs.handle_tick(&mut app.core);
        }
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

fn render(frame: &mut Frame<'_>, app: &App) -> MouseState {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let mut mouse_state = match app.route() {
        Route::Docs => app.docs.render(&app.core, frame, root[0]),
        Route::Search => app.search.render(&app.core, frame, root[0]),
        Route::Init => {
            let mouse = app.page.render(&app.core, frame, root[0]);
            app.init.render(&app.core, frame, frame.area());
            mouse
        }
        Route::Workspace => app.page.render(&app.core, frame, root[0]),
    };
    mouse_state
        .panel_areas
        .push((Focus::CommandLog, root[1], 0));
    render_command_log(frame, &[app.command_log_line()], root[1]);
    let (menu_open, menu_status) = match app.route() {
        Route::Docs => (false, String::new()),
        Route::Search => (app.search.nav.menu_open, app.search.nav.last_status.clone()),
        Route::Init => (false, app.init.nav.last_status.clone()),
        Route::Workspace => (app.page.nav.menu_open, app.page.nav.last_status.clone()),
    };
    if menu_open {
        render_keys_dialog(frame, &menu_status);
    }
    mouse_state
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

pub(crate) fn dir_size(path: &std::path::Path) -> io::Result<u64> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    use crate::core::project::{DependencyInfo, DependencyKind, PackageInfo, TargetInfo};
    use lazycargo_search::CrateSearchResult;

    fn sample_project() -> ProjectInfo {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let manifest = format!("{root}/Cargo.toml");
        let target = TargetInfo {
            name: "testapp".to_owned(),
            kind: vec!["lib".to_owned()],
            src_path: format!("{root}/src/lib.rs"),
        };
        let package = PackageInfo {
            name: "testapp".to_owned(),
            version: "0.1.0".to_owned(),
            manifest_path: manifest.clone(),
            rust_version: None,
            targets: vec![target.clone()],
            dependencies: vec![DependencyInfo {
                name: "serde".to_owned(),
                req: "1".to_owned(),
                kind: DependencyKind::Normal,
                features: Vec::new(),
                optional: false,
                uses_default_features: true,
            }],
            features: Vec::new(),
        };
        ProjectInfo {
            name: "testapp".to_owned(),
            version: "0.1.0".to_owned(),
            workspace_root: root,
            manifest_path: manifest,
            packages: vec!["testapp".to_owned()],
            targets: vec![target],
            dependencies: package.dependencies.clone(),
            features: Vec::new(),
            workspace_packages: vec![package],
            dependency_packages: Vec::new(),
            rustc_version: "rustc 1.88.0".to_owned(),
        }
    }

    /// App（装配器）渲染的主区域必须与直接渲染 WorkspacePage 一致，
    /// 验证路由/区域切分/命令栏不产生回归。
    #[test]
    fn workspace_page_render_matches_app_render() {
        let app = App::new(sample_project(), AppConfig::default());
        let width = 120;
        let height = 40;

        let mut app_mouse = MouseState::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                app_mouse = render(frame, &app);
            })
            .unwrap();

        let mut page_mouse = MouseState::default();
        let mut page_terminal = Terminal::new(TestBackend::new(width, height - 1)).unwrap();
        page_terminal
            .draw(|frame| {
                let area = frame.area();
                page_mouse = app.page.render(&app.core, frame, area);
            })
            .unwrap();

        let app_buffer = terminal.backend().buffer().clone();
        let page_buffer = page_terminal.backend().buffer().clone();
        for y in 0..height - 1 {
            for x in 0..width {
                assert_eq!(
                    app_buffer[(x, y)],
                    page_buffer[(x, y)],
                    "buffer mismatch at ({x},{y})"
                );
            }
        }

        let app_panels: Vec<_> = app_mouse
            .panel_areas
            .iter()
            .filter(|(focus, _, _)| *focus != Focus::CommandLog)
            .cloned()
            .collect();
        assert_eq!(app_panels, page_mouse.panel_areas);
        assert_eq!(app_mouse.link_areas, page_mouse.link_areas);
        assert_eq!(app_mouse.tab_areas, page_mouse.tab_areas);
        assert_eq!(
            app_mouse.right_scrollbar_area,
            page_mouse.right_scrollbar_area
        );
        assert_eq!(
            app_mouse.right_scrollbar_content_len,
            page_mouse.right_scrollbar_content_len
        );
        assert_eq!(
            app_mouse.right_scrollbar_visible_rows,
            page_mouse.right_scrollbar_visible_rows
        );
    }

    /// 搜索展开时 App 必须路由到 SearchPage 渲染，与直接渲染 SearchPage 一致。
    #[test]
    fn search_page_render_matches_app_render() {
        let mut app = App::new(sample_project(), AppConfig::default());
        app.page.nav.input_mode = InputMode::CrateSearch;
        app.page.nav.focus = Focus::Search;
        app.core.search.state.expanded = true;
        app.core.search.state.query = "serde".to_owned();
        app.core.search.state.set_results(vec![CrateSearchResult {
            name: "serde".to_owned(),
            author: None,
            version: "1.0.219".to_owned(),
            description: "A serialization framework for Rust.".to_owned(),
            homepage: None,
            documentation: Some("https://docs.rs/serde".to_owned()),
            repository: Some("https://github.com/serde-rs/serde".to_owned()),
            downloads: Some(100_000_000),
            recent_downloads: Some(10_000_000),
            updated_at: Some("2026-08-01".to_owned()),
        }]);
        app.sync_routes();
        let width = 120;
        let height = 40;

        let mut app_mouse = MouseState::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                app_mouse = render(frame, &app);
            })
            .unwrap();

        let mut page_mouse = MouseState::default();
        let mut page_terminal = Terminal::new(TestBackend::new(width, height - 1)).unwrap();
        page_terminal
            .draw(|frame| {
                let area = frame.area();
                page_mouse = app.search.render(&app.core, frame, area);
            })
            .unwrap();

        let app_buffer = terminal.backend().buffer().clone();
        let page_buffer = page_terminal.backend().buffer().clone();
        for y in 0..height - 1 {
            for x in 0..width {
                assert_eq!(
                    app_buffer[(x, y)],
                    page_buffer[(x, y)],
                    "buffer mismatch at ({x},{y})"
                );
            }
        }

        let app_panels: Vec<_> = app_mouse
            .panel_areas
            .iter()
            .filter(|(focus, _, _)| *focus != Focus::CommandLog)
            .cloned()
            .collect();
        assert_eq!(app_panels, page_mouse.panel_areas);
        assert_eq!(app_mouse.link_areas, page_mouse.link_areas);
        assert_eq!(
            app_mouse.right_scrollbar_area,
            page_mouse.right_scrollbar_area
        );
        assert_eq!(
            app_mouse.right_scrollbar_content_len,
            page_mouse.right_scrollbar_content_len
        );
        assert_eq!(
            app_mouse.right_scrollbar_visible_rows,
            page_mouse.right_scrollbar_visible_rows
        );
    }

    /// 无 Cargo 项目时 App 必须渲染 Init 对话框且不 panic。
    #[test]
    fn init_route_render_draws_confirm_dialog() {
        let mut app = App::new(sample_project(), AppConfig::default());
        app.page.nav.input_mode = InputMode::ProjectNewConfirm;
        app.page.nav.last_status = "limited mode".to_owned();
        app.page.nav.message = "no Cargo project: initialize here? y/n".to_owned();
        app.sync_routes();

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                let _ = render(frame, &app);
            })
            .unwrap();
        assert_eq!(app.route(), Route::Init);
    }

    /// 应答 Init 对话框（y/n）后路由必须回到 Workspace，否则会卡死在 Init。
    #[test]
    fn init_dialog_dismissal_returns_to_workspace_route() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new(sample_project(), AppConfig::default());
        app.page.nav.input_mode = InputMode::ProjectNewConfirm;
        app.sync_routes();
        assert_eq!(app.route(), Route::Init);

        app.init.handle_key(
            &mut app.core,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
        );
        app.sync_routes();
        assert_eq!(app.route(), Route::Workspace);
        assert_eq!(app.page.nav.input_mode, InputMode::Normal);
    }

    /// 打开 docs 后路由必须切到 Docs，返回（q）后回到来源 Workspace 路由。
    #[test]
    fn docs_open_routes_to_docs_and_back() {
        let mut app = App::new(sample_project(), AppConfig::default());
        app.core.docs_open = true;
        app.sync_routes();
        assert_eq!(app.route(), Route::Docs);

        app.core.docs_open = false;
        app.sync_routes();
        assert_eq!(app.route(), Route::Workspace);
    }

    /// DocsPage 渲染必须与 App 装配后的 Docs 路由渲染一致。
    #[test]
    fn docs_page_render_matches_app_render() {
        let mut app = App::new(sample_project(), AppConfig::default());
        app.core.docs_open = true;
        app.core.docs.name = "serde".to_owned();
        app.core.docs.version = "latest".to_owned();
        app.sync_routes();
        let width = 120;
        let height = 40;

        let mut app_mouse = MouseState::default();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                app_mouse = render(frame, &app);
            })
            .unwrap();

        let mut page_mouse = MouseState::default();
        let mut page_terminal = Terminal::new(TestBackend::new(width, height - 1)).unwrap();
        page_terminal
            .draw(|frame| {
                let area = frame.area();
                page_mouse = app.docs.render(&app.core, frame, area);
            })
            .unwrap();

        let app_buffer = terminal.backend().buffer().clone();
        let page_buffer = page_terminal.backend().buffer().clone();
        for y in 0..height - 1 {
            for x in 0..width {
                assert_eq!(
                    app_buffer[(x, y)],
                    page_buffer[(x, y)],
                    "buffer mismatch at ({x},{y})"
                );
            }
        }
    }

    /// 依赖面板选中依赖按 `d`：open_docs + 切 Docs 路由；按 `q` 回到 Workspace。
    #[test]
    fn deps_d_opens_docs_route_and_q_returns() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new(sample_project(), AppConfig::default());
        app.page.nav.focus = Focus::Dependencies;
        app.sync_routes();
        assert_eq!(app.route(), Route::Workspace);

        let key = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
        assert!(app.handle_key(key));
        assert!(app.core.docs_open);
        assert_eq!(app.core.docs.name, "serde");
        app.sync_routes();
        assert_eq!(app.route(), Route::Docs);

        let back = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.handle_key(back));
        assert!(!app.core.docs_open);
        app.sync_routes();
        assert_eq!(app.route(), Route::Workspace);
    }

    /// 搜索页选中 crate 按 `d`：open_docs + 切 Docs 路由；返回后回到 Search。
    #[test]
    fn search_d_opens_docs_route_and_returns_to_search() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut app = App::new(sample_project(), AppConfig::default());
        app.page.nav.input_mode = InputMode::CrateSearch;
        app.page.nav.focus = Focus::Search;
        app.core.search.state.expanded = true;
        app.core.search.state.query = "serde".to_owned();
        app.core.search.state.set_results(vec![CrateSearchResult {
            name: "serde".to_owned(),
            author: None,
            version: "1.0.219".to_owned(),
            description: "A serialization framework for Rust.".to_owned(),
            homepage: None,
            documentation: Some("https://docs.rs/serde".to_owned()),
            repository: Some("https://github.com/serde-rs/serde".to_owned()),
            downloads: Some(100_000_000),
            recent_downloads: Some(10_000_000),
            updated_at: Some("2026-08-01".to_owned()),
        }]);
        app.sync_routes();
        // 结果已就绪后退出输入模式，`d` 才作为动作而非查询字符
        app.search.nav.input_mode = InputMode::Normal;
        assert_eq!(app.route(), Route::Search);

        let key = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
        assert!(app.handle_key(key));
        assert!(app.core.docs_open);
        assert_eq!(app.core.docs.name, "serde");
        app.sync_routes();
        assert_eq!(app.route(), Route::Docs);

        let back = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.handle_key(back));
        assert!(!app.core.docs_open);
        app.sync_routes();
        assert_eq!(app.route(), Route::Search);
    }
}
