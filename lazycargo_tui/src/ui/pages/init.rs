use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::core::command::CommandSpec;
use crate::core::model::{CoreState, OutputSlot};
use crate::ui::components::dialog::centered_rect;
use crate::ui::controller::{InputMode, MouseState, Page};
use crate::keymap::{self, ProjectNewConfirmAction};

/// InitPage 持有的导航状态子集（Task 15 由根 App 在调用前同步）。
pub(crate) struct InitNav {
    pub input_mode: InputMode,
    pub message: String,
    pub last_status: String,
    /// Yes 应答过，App 在路由切回 workspace 时据此把焦点切到 Build 面板。
    pub accepted: bool,
}

impl Default for InitNav {
    fn default() -> Self {
        Self {
            input_mode: InputMode::ProjectNewConfirm,
            message: "no Cargo project: initialize here? y/n".to_owned(),
            last_status: "limited mode".to_owned(),
            accepted: false,
        }
    }
}

pub(crate) struct InitPage {
    pub nav: InitNav,
}

impl InitPage {
    pub fn new() -> Self {
        Self {
            nav: InitNav::default(),
        }
    }
}

impl Page for InitPage {
    fn handle_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool {
        match keymap::project_new_confirm_action(key) {
            ProjectNewConfirmAction::Yes => {
                self.nav.input_mode = InputMode::Normal;
                self.nav.accepted = true;
                self.nav.message = "running: cargo init".to_owned();
                let spec = CommandSpec {
                    program: "cargo".into(),
                    args: vec!["init".into()],
                };
                if core.spawn_command(&spec, OutputSlot::BuildLive).is_err() {
                    self.nav.last_status = "error".to_owned();
                    self.nav.message = format!("failed: {}", spec.display());
                }
            }
            ProjectNewConfirmAction::No => {
                self.nav.input_mode = InputMode::Normal;
                self.nav.message = "project creation skipped".to_owned();
                self.nav.last_status = "limited mode".to_owned();
            }
            ProjectNewConfirmAction::Noop => {}
        }

        true
    }

    fn render(&self, _core: &CoreState, frame: &mut Frame<'_>, area: Rect) -> MouseState {
        let area = centered_rect(54, 28, area);
        let lines = vec![
            Line::from(vec![Span::styled(
                "No Cargo project found",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from("This directory has no Cargo.toml."),
            Line::from("Initialize it as a Cargo project now?"),
            Line::from(""),
            Line::from(vec![
                Span::styled("Y", Style::default().fg(Color::Green)),
                Span::raw(" yes, run cargo init    "),
                Span::styled("N / Esc", Style::default().fg(Color::Red)),
                Span::raw(" no"),
            ]),
        ];
        let widget = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Green)),
            )
            .wrap(Wrap { trim: true });
        frame.render_widget(Clear, area);
        frame.render_widget(widget, area);
        MouseState::default()
    }
}
