//! the editor body: gutter, text lines, ruler and cursor.

use super::*;
use crate::editor::agent_pane::safe_text;
use crate::editor::*;
use ratatui::layout::Rect;

/// Corps de l'éditeur : gouttière à marqueurs + texte coloré.
/// Paints the editor text: the line-number gutter, the visible lines with their
/// current-line band and ruler, and the terminal cursor when the editor has
/// focus. `zone` is the box interior.
pub(in crate::editor) fn draw_body(frame: &mut Frame, ed: &Editor, zone: Rect) {
    let digits = ed.lines.len().to_string().len().max(2);
    let gutter_w = digits + 2; // marqueur + numéro + espace
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(gutter_w as u16), Constraint::Min(10)])
        .split(zone);
    draw_gutter(frame, ed, body[0], digits, gutter_w);
    draw_text(frame, ed, body[1]);
    // the current line is a band with rounded ends, gutter included — and its
    // right end sits at the end of the line, not at the edge of the panel
    let cur_row = body[1].y + (ed.cy - ed.scroll_y) as u16;
    if cur_row >= body[1].y && cur_row < body[1].bottom() {
        round_band(
            frame.buffer_mut(),
            cur_row,
            body[0].x,
            body[1].right().saturating_sub(1),
            Ed::cur_line(),
        );
    }
    draw_ruler(frame, body[1]);
    if ed.focus == Focus::Editor {
        draw_editor_cursor(frame, ed, body[1]);
    }
}

/// Paints the line-number gutter: a diagnostic dot before each number and the
/// current number in the bright text colour, with a band behind the current line.
pub(in crate::editor) fn draw_gutter(
    frame: &mut Frame,
    ed: &Editor,
    area: Rect,
    digits: usize,
    gutter_w: usize,
) {
    let marks = ed.gutter_marks();
    let gutter_lines: Vec<Line> = (0..area.height as usize)
        .map(|i| {
            let n = ed.scroll_y + i + 1; // 1-based
            if n <= ed.lines.len() {
                gutter_line(ed, n, digits, marks.get(&n).copied())
            } else {
                Line::from(" ".repeat(gutter_w))
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(gutter_lines), area);
}

/// Builds one gutter row for the 1-based line `n`: the mark (the most severe
/// diagnostic, or a blank), the number, and the current-line tint when it is
/// the line under the cursor.
pub(in crate::editor) fn gutter_line(
    ed: &Editor,
    n: usize,
    digits: usize,
    mark: Option<Color>,
) -> Line<'static> {
    let is_cur = n == ed.cy + 1;
    let mut spans = Vec::new();
    // marqueur : diagnostic gcc / norme, le plus sévère
    match mark {
        Some(color) => spans.push(Span::styled("●", Style::default().fg(color))),
        None => spans.push(Span::raw(" ")),
    }
    spans.push(Span::styled(
        format!("{:>w$} ", n, w = digits),
        Style::default().fg(if is_cur { Ed::text() } else { Ed::gutter() }),
    ));
    let mut line = Line::from(spans);
    if is_cur {
        line = line.style(Style::default().bg(Ed::cur_line()));
    }
    line
}

/// Paints the text column: one row per visible line, the ghost preview under
/// the cursor, and the current line tinted. Lines are cut by the horizontal
/// scroll and cleaned of control characters before they are measured.
pub(in crate::editor) fn draw_text(frame: &mut Frame, ed: &Editor, area: Rect) {
    let ghost_len = ed.ghost_lines.len();
    let text_lines: Vec<Line> = (0..area.height as usize)
        .map(|i| text_line(ed, ed.scroll_y + i, ghost_len))
        .collect();
    frame.render_widget(Paragraph::new(text_lines), area);
}

/// Builds the row for the buffer line `n`: a ghost line below the cursor for a
/// multi-line insert preview, the highlighted text, or an empty row past the end.
pub(in crate::editor) fn text_line(ed: &Editor, n: usize, ghost_len: usize) -> Line<'static> {
    // lignes fantômes SOUS le curseur : aperçu de l'insertion
    // multi-lignes (les vraies lignes réapparaissent au refus)
    if ghost_len > 0 && n > ed.cy && n - ed.cy < ghost_len {
        let vis: String = ed.ghost_lines[n - ed.cy]
            .chars()
            .skip(ed.scroll_x)
            .collect();
        return Line::from(Span::styled(
            vis,
            Style::default()
                .fg(Ed::ghost())
                .add_modifier(Modifier::ITALIC),
        ));
    }
    if n >= ed.lines.len() {
        return Line::from("");
    }
    let raw = &ed.lines[n];
    // cleaned and tab-expanded before anything is measured or drawn
    let start = drawn_width(&raw.chars().take(ed.scroll_x).collect::<String>());
    let visible: String = safe_text(raw).chars().skip(start).collect();
    let mut spans = render_line(&visible, Ed::dim(), &ed.ext);
    // première ligne du fantôme : à la suite de la ligne du curseur
    if n == ed.cy && ghost_len > 0 {
        spans.push(Span::styled(
            ed.ghost_lines[0].clone(),
            Style::default()
                .fg(Ed::ghost())
                .add_modifier(Modifier::ITALIC),
        ));
    }
    let mut line = Line::from(spans);
    if n == ed.cy {
        // ligne courante teintée — le regard se pose instantanément
        line = line.style(Style::default().bg(Ed::cur_line()));
    }
    line
}

/// Paints the faint ruler at column 80, the style limit. It only marks empty
/// cells, so it never covers code, and it keeps the current-line background.
pub(in crate::editor) fn draw_ruler(frame: &mut Frame, area: Rect) {
    let ruler_x = area.x + 80;
    if ruler_x < area.right() {
        for row in area.top()..area.bottom() {
            let cell = &mut frame.buffer_mut()[(ruler_x, row)];
            if cell.symbol().trim().is_empty() {
                cell.set_symbol("·").set_fg(Ed::ruler());
            }
        }
    }
}

/// Places the terminal cursor on the typed column, offset by the horizontal
/// scroll, and clamps it inside the text area. Only called when the editor has focus.
pub(in crate::editor) fn draw_editor_cursor(frame: &mut Frame, ed: &Editor, area: Rect) {
    let col = ed
        .lines
        .get(ed.cy)
        .map(|l| drawn_width(l.get(..ed.cx).unwrap_or(l)))
        .unwrap_or(ed.cx);
    let origin = ed
        .lines
        .get(ed.cy)
        .map(|l| drawn_width(&l.chars().take(ed.scroll_x).collect::<String>()))
        .unwrap_or(ed.scroll_x);
    let cur_x = area.x + col.saturating_sub(origin) as u16;
    let cur_y = area.y + (ed.cy - ed.scroll_y) as u16;
    frame.set_cursor_position((cur_x.min(area.width.saturating_sub(1) + area.x), cur_y));
}
