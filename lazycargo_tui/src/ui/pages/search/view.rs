use std::cell::Cell;

use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use crate::core::model::{CoreState, OutputSlot};
use crate::ui::components::panel::render_panel;
use crate::ui::components::scrollbar::render_scrollbar;
use crate::ui::components::search_input::render_search_input;
use crate::ui::components::style::{panel_block, semantic_output_line};
use crate::ui::controller::{Focus, InputMode, MouseState};
use crate::ui::pages::search::controller::SearchNav;
use crate::ui::terminal_support::first_url;

pub(crate) fn render_search_page(
    core: &CoreState,
    nav: &SearchNav,
    visible_rows: &Cell<usize>,
    frame: &mut Frame<'_>,
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

    render_search_input(
        frame,
        left[0],
        &core.search.state.query,
        nav.input_mode == InputMode::CrateSearch,
    );
    mouse_state.panel_areas.push((Focus::Search, left[0], 0));

    let offset = render_panel(
        frame,
        left[1],
        &core.search.state.result_items(),
        core.search.state.selected,
        nav.focus == Focus::Search,
        Focus::Search.title(),
        &nav.filter,
    );
    mouse_state.panel_areas.push((Focus::Search, left[1], offset));
    mouse_state.panel_areas.push((
        Focus::Output,
        chunks[1],
        core.output
            .get(&OutputSlot::SearchDetail)
            .map(|ctx| ctx.scroll)
            .unwrap_or(0),
    ));

    let detail_lines = core.search.state.selected_detail();
    let detail_len = detail_lines.len();
    let visible = chunks[1].height.saturating_sub(2) as usize;
    visible_rows.set(visible);
    let detail_offset = core.output
        .get(&OutputSlot::SearchDetail)
        .map(|ctx| ctx.scroll)
        .unwrap_or(0)
        .min(detail_lines.len().saturating_sub(visible));
    for (index, line) in detail_lines.iter().skip(detail_offset).take(visible).enumerate() {
        if let Some(url) = first_url(line) {
            let row = chunks[1].y.saturating_add(1 + index as u16);
            let link_area = Rect {
                x: chunks[1].x.saturating_add(1),
                y: row,
                width: chunks[1].width.saturating_sub(2),
                height: 1,
            };
            mouse_state.link_areas.push((link_area, url.to_owned()));
        }
    }

    let lines = detail_lines
        .into_iter()
        .skip(detail_offset)
        .take(visible)
        .map(|line| semantic_output_line(&line))
        .collect::<Vec<_>>();
    let detail_title = if nav.copy_mode {
        "Search Detail  [copy mode]"
    } else {
        "Search Detail"
    };
    let widget = Paragraph::new(lines)
        .block(panel_block(detail_title, nav.focus == Focus::Output))
        .wrap(Wrap { trim: false });
    frame.render_widget(widget, chunks[1]);

    let scrollbar_area = chunks[1].inner(Margin {
        vertical: 1,
        horizontal: 0,
    });
    mouse_state.right_scrollbar_area = (detail_len > visible).then_some(scrollbar_area);
    mouse_state.right_scrollbar_content_len = detail_len;
    mouse_state.right_scrollbar_visible_rows = visible;
    render_scrollbar(frame, scrollbar_area, detail_offset, detail_len, visible);
}
