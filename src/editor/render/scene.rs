//! the frame as a whole: the scene, the explorer column, the split and the layers.

use super::*;
use crate::editor::*;
use ratatui::layout::Rect;

/// the frame, then a last check on every cell: nothing a terminal would act on
/// reaches the screen. text is cleaned where it enters (safe_text); this is the
/// net under it, so a path that forgets to clean cannot glitch the view.
pub(in crate::editor) fn draw(frame: &mut Frame, ed: &mut Editor) {
    draw_scene(frame, ed);
    scrub(frame.buffer_mut());
}

/// Paints one frame: the hub alone when it is open, otherwise the ide with its
/// agent layer and the windows floating above it. Hands nothing back; it only
/// fills the frame.
pub(in crate::editor) fn draw_scene(frame: &mut Frame, ed: &mut Editor) {
    let area = frame.area();
    // fond unifié sur toute la surface — la signature « Minuit »
    fill(frame, area, Style::default().bg(Ed::bg()));
    // the hub is the whole screen: no top bar, no editor, no status bar behind
    // it. the menu is the only thing the user sees until it is closed.
    if let Some(hub) = &mut ed.hub {
        clear_area(frame, area);
        draw_hub(frame, hub, area);
        draw_toasts(frame, ed, area);
        return;
    }

    // one blank line under the top bar and one above the status bar: a card
    // whose top border touches the bar is a rectangle, not a card. the air
    // around it is what makes the rounded corners read as rounded.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // barre haute
            Constraint::Length(1), // souffle
            Constraint::Min(3),    // corps
            Constraint::Length(1), // souffle
            Constraint::Length(1), // barre basse
        ])
        .split(area);
    fill(frame, chunks[0], Style::default().bg(Ed::bar_bg()));
    fill(frame, chunks[4], Style::default().bg(Ed::bar_bg()));

    draw_topbar(frame, ed, chunks[0]);
    let body_zone = chunks[2];
    let edit_zone = draw_explorer_column(frame, ed, body_zone);
    let (editor_area, term_area) = split_terminal(edit_zone, ed.term.is_some());
    let inner = draw_box(
        frame,
        editor_area,
        border_for(matches!(ed.focus, Focus::Editor | Focus::Search)),
    );
    if let Some(ta) = term_area {
        if let Some(term) = &mut ed.term {
            draw_terminal(frame, term, ta, ed.focus == Focus::Terminal);
        }
    }
    draw_editor_content(frame, ed, inner, editor_area);
    draw_agent_layer(frame, ed, editor_area, body_zone);

    // the search is a window of its own, like the hub: nothing of the ide is
    // drawn around it, so the card floats on a clean screen
    if let Some(fs) = &mut ed.search {
        clear_area(frame, area);
        draw_search(frame, fs, area, ed.focus == Focus::Search);
        draw_toasts(frame, ed, area);
        return;
    }
    draw_toasts(frame, ed, area);
    draw_statusbar(frame, ed, chunks[4]);
}

/// Écran d'accueil : modal flottant centré, sections à puces — le float
/// d'accueil de lazy.nvim, transposé aux gestes de c-nano. Aucune tagline.
pub(in crate::editor) fn draw_welcome_float(frame: &mut Frame, area: ratatui::layout::Rect) {
    let w = area.width.saturating_sub(4).min(46);
    let h = area.height.saturating_sub(2).min(24);
    if w < 20 || h < 10 {
        return;
    }
    let modal = ratatui::layout::Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    let inner = draw_box(frame, modal, border_for(true));
    let bullet = || Span::styled("● ", Style::default().fg(Ed::cyan()));
    let section = |s: &'static str| {
        Line::from(Span::styled(
            format!(" {s}"),
            Style::default().fg(Ed::text()).add_modifier(Modifier::BOLD),
        ))
    };
    let entry = |k: &'static str, label: &'static str| {
        Line::from(vec![
            Span::raw("  "),
            bullet(),
            Span::styled(format!("{k:<5}"), Style::default().fg(Ed::cyan())),
            Span::styled(label, Style::default().fg(Ed::dim())),
        ])
    };
    let lines = vec![
        Line::from(""),
        section("files"),
        entry("^t", "explorer"),
        entry("^o", "find a file"),
        entry("f2", "switch panel"),
        entry("^s", "save"),
        entry("^q", "quit"),
        Line::from(""),
        section("code"),
        entry("^b", "check"),
        entry("f5", "run"),
        entry("f6", "format"),
        entry("f4", "auto header"),
        entry("^f", "search"),
        entry("^r", "replace"),
        entry("^g", "go to line"),
        Line::from(""),
        section("buffer"),
        entry("^k", "cut line"),
        entry("^u", "paste"),
        entry("^z", "cancel"),
        Line::from(""),
        Line::from(Span::styled(
            "  nana <file> to open — ^t explores the project",
            Style::default().fg(Ed::gutter()),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Draws the explorer box on the left when it is open and hands back the zone
/// that stays for the editor column; without the explorer that is the whole body.
pub(in crate::editor) fn draw_explorer_column(
    frame: &mut Frame,
    ed: &mut Editor,
    body_zone: Rect,
) -> Rect {
    if ed.explorer.is_none() {
        return body_zone;
    }
    let sp = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(EXPL_W),
            Constraint::Length(1), // souffle entre les boîtes
            Constraint::Min(10),
        ])
        .split(body_zone);
    // boîte nue : la bordure colorée + le badge suffisent au focus
    let inner = draw_box(frame, sp[0], border_for(ed.focus == Focus::Explorer));
    if let Some(ex) = &mut ed.explorer {
        draw_explorer(frame, ex, inner);
    }
    sp[2]
}

/// Splits the editor column so the terminal takes its foot when it is open.
/// Hands back the editor zone and the terminal zone, which is None when closed.
pub(in crate::editor) fn split_terminal(edit_zone: Rect, term_open: bool) -> (Rect, Option<Rect>) {
    if !term_open {
        return (edit_zone, None);
    }
    // a blank line between the editor and the terminal, so the two cards do
    // not share a border line and read as one rectangle
    let sp = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),
            Constraint::Length(1), // souffle
            Constraint::Length(TERM_H),
        ])
        .split(edit_zone);
    (sp[0], Some(sp[2]))
}

/// Paints what the editor box holds: the welcome card, or the buffer with its
/// diagnostics. `inner` is the box interior and `editor_area` the box itself.
pub(in crate::editor) fn draw_editor_content(
    frame: &mut Frame,
    ed: &Editor,
    inner: Rect,
    editor_area: Rect,
) {
    if ed.is_welcome() {
        draw_welcome_float(frame, inner);
    } else {
        draw_body(frame, ed, inner);
        draw_diagnostics(frame, ed, editor_area);
    }
}

/// Draws the agent's live view over the editor box and its card over the body.
/// The ide stays drawn around and under them; nothing is drawn without a pane.
pub(in crate::editor) fn draw_agent_layer(
    frame: &mut Frame,
    ed: &mut Editor,
    editor_area: Rect,
    body_zone: Rect,
) {
    if let Some(pane) = &mut ed.agent_pane {
        let playing = !ed.live.is_empty();
        draw_agent_view(frame, &ed.agent_view, editor_area, playing);
        draw_agent(frame, pane, body_zone);
    }
}
