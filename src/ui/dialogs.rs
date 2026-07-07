use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::text::{display_width_u16, truncate_end};
use super::widgets::{
    action_button_row_rects, centered_popup_rect, panel_contrast_fg, render_action_button,
    render_modal_header, render_modal_shell, render_panel_shell, ActionButtonSpec,
};
use crate::app::text_input::TextInputState;
use crate::app::{state::WorktreeOpenState, AppState, Mode};

const NEW_LINKED_WORKTREE_POPUP_WIDTH: u16 = 68;
const NEW_LINKED_WORKTREE_POPUP_HEIGHT: u16 = 12;
const TEXT_INPUT_LEADING_PADDING: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextInputView {
    pub(crate) rect: Rect,
    pub(crate) text_rect: Rect,
    pub(crate) window_start: usize,
    pub(crate) window_start_col: usize,
    pub(crate) cursor_col: usize,
}

pub(crate) fn rename_button_rects(inner: Rect) -> (Rect, Rect, Rect) {
    let rects = action_button_row_rects(
        inner,
        &[
            ActionButtonSpec {
                hint: Some("↵"),
                label: "save",
            },
            ActionButtonSpec {
                hint: Some("^c"),
                label: "clear",
            },
            ActionButtonSpec {
                hint: Some("esc"),
                label: "cancel",
            },
        ],
        2,
        3,
    );
    (rects[0], rects[1], rects[2])
}

pub(crate) fn rename_input_rect(inner: Rect) -> Rect {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas::<5>(inner);
    Rect::new(rows[2].x, rows[2].y, rows[2].width, 1)
}

pub(crate) fn new_linked_worktree_input_rect(inner: Rect) -> Rect {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas::<8>(inner);
    Rect::new(rows[2].x, rows[2].y, rows[2].width, 1)
}

pub(crate) fn text_input_view(rect: Rect, input: &TextInputState) -> TextInputView {
    let text_width = rect.width.saturating_sub(TEXT_INPUT_LEADING_PADDING) as usize;
    let text_rect = Rect::new(
        rect.x.saturating_add(TEXT_INPUT_LEADING_PADDING),
        rect.y,
        rect.width.saturating_sub(TEXT_INPUT_LEADING_PADDING),
        1,
    );
    let cursor_col = display_width_to(input.text(), input.cursor());
    let cursor_cell_width = input
        .text()
        .get(input.cursor()..)
        .and_then(|suffix| suffix.graphemes(true).next())
        .map_or(1, |grapheme| UnicodeWidthStr::width(grapheme).max(1));
    let cursor_end_col = cursor_col.saturating_add(cursor_cell_width);
    let target_col = cursor_end_col.saturating_sub(text_width);
    let (window_start, window_start_col) = if text_width == 0 || target_col == 0 {
        (0, 0)
    } else {
        first_boundary_at_or_after_col(input.text(), target_col, input.cursor())
    };

    TextInputView {
        rect,
        text_rect,
        window_start,
        window_start_col,
        cursor_col,
    }
}

pub(crate) fn text_input_byte_offset_for_column(
    input: &TextInputState,
    view: TextInputView,
    column: u16,
) -> usize {
    let target_col = view.window_start_col.saturating_add(column as usize);
    nearest_boundary_for_col(input.text(), target_col)
}

fn display_width_to(text: &str, byte_offset: usize) -> usize {
    text.get(..byte_offset)
        .map(UnicodeWidthStr::width)
        .unwrap_or_else(|| UnicodeWidthStr::width(text))
}

fn grapheme_boundaries_with_columns(text: &str) -> Vec<(usize, usize)> {
    let mut boundaries = Vec::new();
    let mut col = 0;
    for (idx, grapheme) in text.grapheme_indices(true) {
        boundaries.push((idx, col));
        col += UnicodeWidthStr::width(grapheme);
    }
    boundaries.push((text.len(), col));
    boundaries
}

fn first_boundary_at_or_after_col(
    text: &str,
    target_col: usize,
    max_byte: usize,
) -> (usize, usize) {
    let mut last = (0, 0);
    for boundary in grapheme_boundaries_with_columns(text)
        .into_iter()
        .take_while(|(idx, _)| *idx <= max_byte)
    {
        if boundary.1 >= target_col {
            return boundary;
        }
        last = boundary;
    }
    last
}

fn nearest_boundary_for_col(text: &str, target_col: usize) -> usize {
    let boundaries = grapheme_boundaries_with_columns(text);
    for pair in boundaries.windows(2) {
        let (start_idx, start_col) = pair[0];
        let (end_idx, end_col) = pair[1];
        if target_col <= end_col {
            return if target_col.saturating_sub(start_col) <= end_col.saturating_sub(target_col) {
                start_idx
            } else {
                end_idx
            };
        }
    }
    text.len()
}

fn render_text_input(app: &AppState, frame: &mut Frame, rect: Rect) {
    let view = text_input_view(rect, &app.name_input);
    let base_style = Style::default()
        .fg(app.palette.text)
        .bg(app.palette.surface0);
    frame.render_widget(Clear, view.rect);
    frame.render_widget(
        Paragraph::new(text_input_line(&app.name_input, view, base_style)).style(base_style),
        view.rect,
    );
}

fn text_input_line(
    input: &TextInputState,
    view: TextInputView,
    base_style: Style,
) -> Line<'static> {
    let cursor_style = base_style.add_modifier(Modifier::REVERSED);
    let text_width = view.text_rect.width as usize;
    let mut spans = Vec::new();
    spans.push(Span::styled(
        " ".repeat(TEXT_INPUT_LEADING_PADDING as usize),
        base_style,
    ));

    if text_width == 0 {
        return Line::from(spans);
    }

    let mut used = 0usize;
    let mut rendered_cursor = false;
    let text = input.text();
    for (idx, grapheme) in text[view.window_start..].grapheme_indices(true) {
        let byte_idx = view.window_start + idx;
        if byte_idx == input.cursor() {
            let width = UnicodeWidthStr::width(grapheme).max(1);
            if used + width > text_width {
                break;
            }
            spans.push(Span::styled(grapheme.to_string(), cursor_style));
            used += width;
            rendered_cursor = true;
            continue;
        }

        let width = UnicodeWidthStr::width(grapheme);
        if used + width > text_width {
            break;
        }
        spans.push(Span::styled(grapheme.to_string(), base_style));
        used += width;
    }

    if input.cursor() == text.len() && !rendered_cursor && used < text_width {
        spans.push(Span::styled(" ", cursor_style));
    }

    Line::from(spans)
}

pub(super) fn render_rename_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    super::dim_background(frame, area);

    let title = match app.mode {
        Mode::RenameWorkspace => "rename workspace",
        Mode::RenameTab if app.creating_new_tab => "new tab",
        Mode::RenameTab => "rename tab",
        Mode::RenamePane => "rename pane",
        _ => return,
    };

    let Some(inner) = render_modal_shell(frame, area, 56, 7, &app.palette) else {
        return;
    };
    if inner.height < 4 {
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas::<5>(inner);

    render_modal_header(frame, rows[0], title, &app.palette);

    render_text_input(app, frame, rename_input_rect(inner));

    let (save_rect, clear_rect, cancel_rect) = rename_button_rects(inner);

    render_action_button(
        frame,
        save_rect,
        Some("↵"),
        "save",
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.palette.accent)
            .add_modifier(Modifier::BOLD),
    );
    render_action_button(
        frame,
        clear_rect,
        Some("^c"),
        "clear",
        Style::default()
            .fg(app.palette.text)
            .bg(app.palette.surface0)
            .add_modifier(Modifier::BOLD),
    );
    render_action_button(
        frame,
        cancel_rect,
        Some("esc"),
        "cancel",
        Style::default()
            .fg(app.palette.text)
            .bg(app.palette.surface0)
            .add_modifier(Modifier::BOLD),
    );
}

pub(crate) fn new_linked_worktree_inner_rect(area: Rect) -> Option<Rect> {
    centered_popup_rect(
        area,
        NEW_LINKED_WORKTREE_POPUP_WIDTH,
        NEW_LINKED_WORKTREE_POPUP_HEIGHT,
    )
    .map(|popup| {
        Rect::new(
            popup.x + 1,
            popup.y + 1,
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        )
    })
}

pub(crate) fn new_linked_worktree_button_rects(inner: Rect) -> (Rect, Rect) {
    let rects = action_button_row_rects(
        inner,
        &[
            ActionButtonSpec {
                hint: Some("↵"),
                label: "create and open",
            },
            ActionButtonSpec {
                hint: Some("esc"),
                label: "cancel",
            },
        ],
        2,
        inner.height.saturating_sub(1),
    );
    (rects[0], rects[1])
}

pub(crate) fn remove_worktree_popup_rect(area: Rect) -> Option<Rect> {
    centered_popup_rect(area, 72, 10)
}

pub(crate) fn remove_worktree_button_rects(inner: Rect, force_confirmation: bool) -> (Rect, Rect) {
    let primary_label = if force_confirmation {
        "delete anyway"
    } else {
        "remove"
    };
    let rects = action_button_row_rects(
        inner,
        &[
            ActionButtonSpec {
                hint: Some("↵"),
                label: primary_label,
            },
            ActionButtonSpec {
                hint: Some("esc"),
                label: "cancel",
            },
        ],
        2,
        inner.height.saturating_sub(1),
    );
    (rects[0], rects[1])
}

pub(crate) fn open_existing_worktree_inner_rect(area: Rect, entry_count: usize) -> Option<Rect> {
    let height = (entry_count as u16)
        .saturating_mul(2)
        .saturating_add(7)
        .clamp(12, 26);
    centered_popup_rect(area, 96, height).map(|popup| {
        Rect::new(
            popup.x + 1,
            popup.y + 1,
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        )
    })
}

pub(crate) fn open_existing_worktree_max_visible_rows(inner: Rect) -> usize {
    usize::from(inner.height.saturating_sub(5) / 2)
}

pub(crate) fn open_existing_worktree_visible_start(
    open: &WorktreeOpenState,
    max_rows: usize,
) -> usize {
    let filtered = open.filtered_indices();
    let selected = open.selected_entry_index().unwrap_or(open.selected);
    let selected_pos = filtered
        .iter()
        .position(|idx| *idx == selected)
        .unwrap_or(0);
    selected_pos.saturating_sub(max_rows.saturating_sub(1))
}

pub(crate) fn open_existing_worktree_button_rects(inner: Rect) -> (Rect, Rect) {
    let rects = action_button_row_rects(
        inner,
        &[
            ActionButtonSpec {
                hint: Some("↵"),
                label: "open",
            },
            ActionButtonSpec {
                hint: Some("esc"),
                label: "cancel",
            },
        ],
        2,
        inner.height.saturating_sub(1),
    );
    (rects[0], rects[1])
}

pub(super) fn render_new_linked_worktree_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    let Some(create) = app.worktree_create.as_ref() else {
        return;
    };

    super::dim_background(frame, area);
    let Some(inner) = render_modal_shell(
        frame,
        area,
        NEW_LINKED_WORKTREE_POPUP_WIDTH,
        NEW_LINKED_WORKTREE_POPUP_HEIGHT,
        &app.palette,
    ) else {
        return;
    };
    if inner.height < 9 {
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas::<8>(inner);

    render_modal_header(frame, rows[0], "new worktree", &app.palette);

    frame.render_widget(
        Paragraph::new(" branch").style(Style::default().fg(app.palette.overlay0)),
        rows[1],
    );
    render_text_input(app, frame, new_linked_worktree_input_rect(inner));

    let checkout = create.checkout_path.display().to_string();
    frame.render_widget(
        Paragraph::new(" checkout").style(Style::default().fg(app.palette.overlay0)),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(format!(" {checkout}")).style(Style::default().fg(app.palette.subtext0)),
        rows[4],
    );

    if create.creating {
        frame.render_widget(
            Paragraph::new(" creating…").style(Style::default().fg(app.palette.overlay0)),
            rows[5],
        );
    } else if let Some(error) = &create.error {
        frame.render_widget(
            Paragraph::new(format!(" {error}"))
                .style(Style::default().fg(app.palette.red))
                .wrap(Wrap { trim: false }),
            rows[5],
        );
    }

    let (create_rect, cancel_rect) = new_linked_worktree_button_rects(inner);
    render_action_button(
        frame,
        create_rect,
        Some("↵"),
        "create and open",
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.palette.accent)
            .add_modifier(Modifier::BOLD),
    );
    render_action_button(
        frame,
        cancel_rect,
        Some("esc"),
        "cancel",
        Style::default()
            .fg(app.palette.text)
            .bg(app.palette.surface0)
            .add_modifier(Modifier::BOLD),
    );
}

pub(super) fn render_remove_worktree_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    let Some(remove) = app.worktree_remove.as_ref() else {
        return;
    };

    super::dim_background(frame, area);
    let Some(popup) = remove_worktree_popup_rect(area) else {
        return;
    };
    let Some(inner) = render_panel_shell(frame, popup, app.palette.red, app.palette.panel_bg)
    else {
        return;
    };

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas::<8>(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            " delete worktree checkout?",
            Style::default()
                .fg(app.palette.red)
                .add_modifier(Modifier::BOLD),
        )])),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(" This removes the checkout folder:")
            .style(Style::default().fg(app.palette.overlay0)),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(format!(" {}", remove.path.display()))
            .style(Style::default().fg(app.palette.text)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(" The branch is not deleted. The Herdr workspace will close.")
            .style(Style::default().fg(app.palette.overlay0)),
        rows[3],
    );
    if remove.force_confirmation {
        frame.render_widget(
            Paragraph::new(" Dirty or untracked files will be permanently deleted.")
                .style(Style::default().fg(app.palette.red)),
            rows[4],
        );
    }
    if remove.removing {
        frame.render_widget(
            Paragraph::new(" removing…").style(Style::default().fg(app.palette.overlay0)),
            rows[5],
        );
    } else if let Some(error) = &remove.error {
        frame.render_widget(
            Paragraph::new(format!(" {error}")).style(Style::default().fg(app.palette.red)),
            rows[5],
        );
    }

    let (remove_rect, cancel_rect) = remove_worktree_button_rects(inner, remove.force_confirmation);
    let remove_label = if remove.force_confirmation {
        "delete anyway"
    } else {
        "remove"
    };
    render_action_button(
        frame,
        remove_rect,
        Some("↵"),
        remove_label,
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.palette.red)
            .add_modifier(Modifier::BOLD),
    );
    render_action_button(
        frame,
        cancel_rect,
        Some("esc"),
        "cancel",
        Style::default()
            .fg(app.palette.text)
            .bg(app.palette.surface0)
            .add_modifier(Modifier::BOLD),
    );
}

pub(super) fn render_open_existing_worktree_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    let Some(open) = app.worktree_open.as_ref() else {
        return;
    };

    super::dim_background(frame, area);
    let height = (open.entries.len() as u16)
        .saturating_mul(2)
        .saturating_add(7)
        .clamp(12, 26);
    let Some(inner) = render_modal_shell(frame, area, 96, height, &app.palette) else {
        return;
    };
    if inner.height < 8 {
        return;
    }

    render_modal_header(
        frame,
        Rect::new(inner.x, inner.y, inner.width, 1),
        "open worktree",
        &app.palette,
    );
    render_open_worktree_search(
        app,
        frame,
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
        open,
    );
    frame.render_widget(
        Paragraph::new("─".repeat(inner.width as usize))
            .style(Style::default().fg(app.palette.surface1)),
        Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 1),
    );

    let filtered = open.filtered_indices();
    let max_rows = open_existing_worktree_max_visible_rows(inner);
    let start = open_existing_worktree_visible_start(open, max_rows);
    for (visible_idx, entry_idx) in filtered.iter().skip(start).take(max_rows).enumerate() {
        let Some(entry) = open.entries.get(*entry_idx) else {
            continue;
        };
        let selected = Some(*entry_idx) == open.selected_entry_index();
        let y = inner.y.saturating_add(3 + (visible_idx as u16 * 2));
        let marker = if selected { "›" } else { " " };
        let row_style = if selected {
            Style::default()
                .fg(app.palette.text)
                .bg(app.palette.surface0)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(app.palette.subtext0)
        };
        let path_style = if selected {
            Style::default()
                .fg(app.palette.subtext0)
                .bg(app.palette.surface0)
        } else {
            Style::default().fg(app.palette.overlay0)
        };
        let status = entry.status_label();
        let title_width = inner
            .width
            .saturating_sub(display_width_u16(status))
            .saturating_sub(4) as usize;
        let mut title = format!(
            "{marker} {}",
            truncate_end(&entry.display_name(), title_width)
        );
        if !status.is_empty() {
            let pad = inner
                .width
                .saturating_sub(display_width_u16(&title))
                .saturating_sub(display_width_u16(status))
                .max(1);
            title.push_str(&" ".repeat(pad as usize));
            title.push_str(status);
        }
        frame.render_widget(
            Paragraph::new(truncate_end(&title, inner.width as usize)).style(row_style),
            Rect::new(inner.x, y, inner.width, 1),
        );
        frame.render_widget(
            Paragraph::new(truncate_end(
                &format!("  {}", entry.path.display()),
                inner.width as usize,
            ))
            .style(path_style),
            Rect::new(inner.x, y.saturating_add(1), inner.width, 1),
        );
    }

    if filtered.is_empty() {
        frame.render_widget(
            Paragraph::new(" no matching worktrees")
                .style(Style::default().fg(app.palette.overlay0)),
            Rect::new(inner.x, inner.y.saturating_add(3), inner.width, 1),
        );
    }

    if let Some(error) = &open.error {
        frame.render_widget(
            Paragraph::new(format!(" {error}")).style(Style::default().fg(app.palette.red)),
            Rect::new(
                inner.x,
                inner.y + inner.height.saturating_sub(2),
                inner.width,
                1,
            ),
        );
    }

    let (open_rect, cancel_rect) = open_existing_worktree_button_rects(inner);
    render_action_button(
        frame,
        open_rect,
        Some("↵"),
        "open",
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.palette.accent)
            .add_modifier(Modifier::BOLD),
    );
    render_action_button(
        frame,
        cancel_rect,
        Some("esc"),
        "cancel",
        Style::default()
            .fg(app.palette.text)
            .bg(app.palette.surface0)
            .add_modifier(Modifier::BOLD),
    );
}

fn render_open_worktree_search(
    app: &AppState,
    frame: &mut Frame,
    area: Rect,
    open: &WorktreeOpenState,
) {
    let focus_style = if open.search_focused {
        Style::default()
            .fg(app.palette.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(app.palette.overlay0)
    };
    let filtered_count = open.filtered_indices().len();
    let count = if open.query.trim().is_empty() {
        format!("{} checkouts", open.entries.len())
    } else {
        format!("{filtered_count}/{} checkouts", open.entries.len())
    };
    let mut spans = vec![Span::styled(" / ", focus_style)];
    if open.query.trim().is_empty() {
        spans.push(Span::styled(
            "filter worktrees",
            Style::default().fg(app.palette.overlay0),
        ));
    } else {
        spans.push(Span::styled(
            open.query.clone(),
            Style::default().fg(app.palette.text),
        ));
    }
    spans.push(Span::styled(
        format!(
            "{count:>width$}",
            width = area.width.saturating_sub(18) as usize
        ),
        Style::default().fg(app.palette.overlay0),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn confirm_close_overlay_text(app: &AppState) -> (String, String) {
    let ws_name = app
        .workspaces
        .get(app.selected)
        .map(|ws| ws.display_name())
        .unwrap_or_else(|| "?".to_string());
    let selected_space = app
        .workspaces
        .get(app.selected)
        .and_then(|ws| ws.worktree_space());
    let group_member_indices = selected_space
        .filter(|space| !space.is_linked_worktree)
        .map(|space| {
            app.workspaces
                .iter()
                .enumerate()
                .filter_map(|(idx, ws)| {
                    ws.worktree_space()
                        .is_some_and(|member| member.key == space.key)
                        .then_some(idx)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let closes_group = group_member_indices.len() > 1;
    let pane_count = if closes_group {
        group_member_indices
            .iter()
            .filter_map(|idx| app.workspaces.get(*idx))
            .map(|ws| ws.layout.pane_count())
            .sum()
    } else {
        app.workspaces
            .get(app.selected)
            .map(|ws| ws.layout.pane_count())
            .unwrap_or(0)
    };

    let pane_text = if pane_count == 1 {
        "1 pane".to_string()
    } else {
        format!("{pane_count} panes")
    };
    let workspace_text = if closes_group {
        let count = group_member_indices.len();
        if count == 1 {
            "1 workspace, ".to_string()
        } else {
            format!("{count} workspaces, ")
        }
    } else {
        String::new()
    };

    let title = if closes_group {
        "Close worktree group?"
    } else {
        "Close workspace?"
    };
    let detail = format!("{ws_name} — {workspace_text}{pane_text}");
    (title.to_string(), detail)
}

pub(super) fn render_confirm_close_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    let (title, detail) = confirm_close_overlay_text(app);

    super::dim_background(frame, area);

    let Some(popup) = confirm_close_popup_rect(area) else {
        return;
    };

    let warn = Style::default()
        .fg(app.palette.red)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(app.palette.overlay0);

    let title_line = Line::from(vec![Span::styled(format!(" {title}"), warn)]);

    let detail_line = Line::from(vec![
        Span::styled(
            format!(" {}", detail.split(" — ").next().unwrap_or(&detail)),
            Style::default()
                .fg(app.palette.text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            detail
                .split_once(" — ")
                .map(|(_, rest)| format!(" — {rest}"))
                .unwrap_or_default(),
            dim,
        ),
    ]);

    let Some(inner) = render_panel_shell(frame, popup, app.palette.red, app.palette.panel_bg)
    else {
        return;
    };

    if inner.height >= 3 {
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas::<4>(inner);

        frame.render_widget(Paragraph::new(title_line), rows[0]);
        frame.render_widget(Paragraph::new(detail_line), rows[1]);

        let (confirm_rect, cancel_rect) = confirm_close_button_rects(inner);
        render_action_button(
            frame,
            confirm_rect,
            Some("↵"),
            "confirm",
            Style::default()
                .fg(panel_contrast_fg(&app.palette))
                .bg(app.palette.red)
                .add_modifier(Modifier::BOLD),
        );
        render_action_button(
            frame,
            cancel_rect,
            Some("esc"),
            "cancel",
            Style::default()
                .fg(app.palette.text)
                .bg(app.palette.surface0)
                .add_modifier(Modifier::BOLD),
        );
    }
}

pub(crate) fn confirm_close_popup_rect(area: Rect) -> Option<Rect> {
    centered_popup_rect(area, 64, 6)
}

pub(crate) fn confirm_close_button_rects(inner: Rect) -> (Rect, Rect) {
    let rects = action_button_row_rects(
        inner,
        &[
            ActionButtonSpec {
                hint: Some("↵"),
                label: "confirm",
            },
            ActionButtonSpec {
                hint: Some("esc"),
                label: "cancel",
            },
        ],
        2,
        3,
    );
    (rects[0], rects[1])
}

#[cfg(test)]
mod tests {
    use crate::{
        app::text_input::TextInputState,
        app::{state::WorktreeCreateState, AppState},
        workspace::Workspace,
    };
    use ratatui::{backend::TestBackend, layout::Rect, style::Modifier, Terminal};

    use super::{confirm_close_overlay_text, render_new_linked_worktree_overlay};

    #[test]
    fn confirm_close_text_reports_parent_group_scope() {
        let mut app = AppState::test_new();
        let mut parent = Workspace::test_new("main");
        parent.worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: "repo-key".into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: "/repo/herdr".into(),
            is_linked_worktree: false,
        });
        let mut child = Workspace::test_new("issue");
        child.worktree_space = Some(crate::workspace::WorktreeSpaceMembership {
            key: "repo-key".into(),
            label: "herdr".into(),
            repo_root: "/repo/herdr".into(),
            checkout_path: "/repo/herdr-issue".into(),
            is_linked_worktree: true,
        });
        app.workspaces = vec![parent, child];
        app.selected = 0;

        let (title, detail) = confirm_close_overlay_text(&app);

        assert_eq!(title, "Close worktree group?");
        assert_eq!(detail, "main — 2 workspaces, 2 panes");
    }

    #[test]
    fn new_worktree_error_renders_fatal_stderr_line() {
        let mut app = AppState::test_new();
        app.name_input = "foo".into();
        app.worktree_create = Some(WorktreeCreateState {
            source_workspace_id: "source".into(),
            source_checkout_path: "/repo/herdr".into(),
            source_existing_membership: None,
            source_repo_root: "/repo/herdr".into(),
            repo_key: "repo-key".into(),
            repo_name: "herdr".into(),
            branch: "foo".into(),
            checkout_path: "/repo/.worktrees/herdr/foo".into(),
            error: Some(
                "Preparing worktree (new branch 'foo')\nfatal: a branch named 'foo' already exists"
                    .into(),
            ),
            creating: false,
        });

        let mut terminal =
            Terminal::new(TestBackend::new(100, 30)).expect("test terminal should initialize");
        terminal
            .draw(|frame| render_new_linked_worktree_overlay(&app, frame, Rect::new(0, 0, 100, 30)))
            .expect("new worktree overlay should render");
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(rendered.contains("fatal: a branch named 'foo' already exists"));
    }

    fn modal_inner(area: Rect, width: u16, height: u16) -> Rect {
        let popup = super::centered_popup_rect(area, width, height).unwrap();
        Rect::new(
            popup.x + 1,
            popup.y + 1,
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        )
    }

    #[test]
    fn rename_input_renders_reverse_video_cursor_without_literal_block() {
        let area = Rect::new(0, 0, 80, 20);
        let mut app = AppState::test_new();
        app.mode = crate::app::Mode::RenameTab;
        app.name_input = "abcdef".into();
        app.name_input.set_cursor("abc".len());

        let mut terminal =
            Terminal::new(TestBackend::new(area.width, area.height)).expect("terminal");
        terminal
            .draw(|frame| super::render_rename_overlay(&app, frame, area))
            .expect("rename overlay should render");

        let inner = modal_inner(area, 56, 7);
        let input_rect = super::rename_input_rect(inner);
        let cursor = terminal.backend().buffer()[(input_rect.x + 1 + 3, input_rect.y)].clone();
        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert_eq!(cursor.symbol(), "d");
        assert!(cursor.style().add_modifier.contains(Modifier::REVERSED));
        assert!(!rendered.contains('█'));
    }

    #[test]
    fn text_input_view_windows_to_keep_cursor_visible() {
        let area = Rect::new(0, 0, 80, 20);
        let inner = modal_inner(area, 56, 7);
        let input_rect = super::rename_input_rect(inner);
        let mut app = AppState::test_new();
        app.mode = crate::app::Mode::RenameTab;
        app.name_input = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ".into();
        let view = super::text_input_view(input_rect, &app.name_input);

        let mut terminal =
            Terminal::new(TestBackend::new(area.width, area.height)).expect("terminal");
        terminal
            .draw(|frame| super::render_rename_overlay(&app, frame, area))
            .expect("rename overlay should render");
        let row = (view.text_rect.x..view.text_rect.x + view.text_rect.width)
            .map(|x| terminal.backend().buffer()[(x, view.text_rect.y)].symbol())
            .collect::<String>();
        let cursor_x = view.text_rect.x + (view.cursor_col - view.window_start_col) as u16;
        let cursor = terminal.backend().buffer()[(cursor_x, view.text_rect.y)].clone();

        assert!(view.window_start > 0);
        assert!(!row.contains("abcde"));
        assert!(cursor.style().add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn text_input_column_mapping_snaps_to_grapheme_boundaries() {
        let input = TextInputState::with_text("a界b");
        let view = super::text_input_view(Rect::new(0, 0, 10, 1), &input);

        assert_eq!(super::text_input_byte_offset_for_column(&input, view, 0), 0);
        assert_eq!(
            super::text_input_byte_offset_for_column(&input, view, 1),
            "a".len()
        );
        assert_eq!(
            super::text_input_byte_offset_for_column(&input, view, 3),
            "a界".len()
        );
    }

    #[test]
    fn new_worktree_hit_test_geometry_matches_modal_size() {
        let area = Rect::new(0, 0, 100, 30);
        let inner = super::new_linked_worktree_inner_rect(area).unwrap();
        let (create, cancel) = super::new_linked_worktree_button_rects(inner);

        assert_eq!(inner.width, super::NEW_LINKED_WORKTREE_POPUP_WIDTH - 2);
        assert_eq!(inner.height, super::NEW_LINKED_WORKTREE_POPUP_HEIGHT - 2);
        assert_eq!(create.y, inner.y + inner.height - 1);
        assert_eq!(cancel.y, inner.y + inner.height - 1);
    }
}
