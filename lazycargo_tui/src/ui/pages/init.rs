use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::core::model::CoreState;
use crate::ui::components::dialog::centered_rect;
use crate::ui::controller::{InputMode, MouseState, Page};
use crate::ui::keymap::{self, ProjectNewConfirmAction};

/// InitPage 持有的导航状态子集（Task 15 由根 App 在调用前同步）。
#[allow(dead_code)]
pub(crate) struct InitNav {
    pub input_mode: InputMode,
    pub message: String,
    pub last_status: String,
}

impl Default for InitNav {
    fn default() -> Self {
        Self {
            input_mode: InputMode::ProjectNewConfirm,
            message: "no Cargo project: initialize here? y/n".to_owned(),
            last_status: "limited mode".to_owned(),
        }
    }
}

#[allow(dead_code)]
pub(crate) struct InitPage {
    pub nav: InitNav,
}

impl InitPage {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            nav: InitNav::default(),
        }
    }
}

impl Page for InitPage {
    fn handle_key(&mut self, _core: &mut CoreState, key: KeyEvent) -> bool {
        match keymap::project_new_confirm_action(key) {
            ProjectNewConfirmAction::Yes => {
                self.nav.input_mode = InputMode::Normal;
                // TODO(Task 15): run cargo init (process spawn) owned by root App
                self.nav.message = "initializing project".to_owned();
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

    fn handle_tick(&mut self, _core: &mut CoreState) {}

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

    fn title(&self) -> &'static str {
        "[Init]"
    }
}
