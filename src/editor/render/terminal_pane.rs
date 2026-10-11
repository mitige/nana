//! the terminal pane: colours and the pane itself.

use super::*;
use crate::editor::*;

/// Conversion couleur vt100 → ratatui (défaut = la palette Minuit).
pub(in crate::editor) fn vt_color(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Le panneau terminal : boîte arrondie, cellules vt100 rendues en Minuit.
pub(in crate::editor) fn draw_terminal(
    frame: &mut Frame,
    term: &mut TermPane,
    area: ratatui::layout::Rect,
    focused: bool,
) {
    let inner = draw_box(frame, area, border_for(focused));
    term.resize(inner.height, inner.width);
    let screen = term.parser.screen();
    let mut lines: Vec<Line> = Vec::with_capacity(inner.height as usize);
    for row in 0..inner.height {
        let mut spans: Vec<Span> = Vec::new();
        let mut cur = String::new();
        let mut cur_style: Option<Style> = None;
        for col in 0..inner.width {
            let Some(cell) = screen.cell(row, col) else {
                break;
            };
            let fg = vt_color(cell.fgcolor(), Ed::text());
            let bg = vt_color(cell.bgcolor(), Ed::bg());
            let mut st = Style::default().fg(fg).bg(bg);
            if cell.bold() {
                st = st.add_modifier(Modifier::BOLD);
            }
            let text = cell.contents();
            let text = if text.is_empty() {
                " ".to_string()
            } else {
                text
            };
            if cur_style == Some(st) {
                cur.push_str(&text);
            } else {
                if !cur.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut cur), cur_style.unwrap()));
                }
                cur_style = Some(st);
                cur.push_str(&text);
            }
        }
        if !cur.is_empty() {
            spans.push(Span::styled(cur, cur_style.unwrap_or_default()));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    if focused {
        let (cr, cc) = screen.cursor_position();
        if cr < inner.height && cc < inner.width {
            frame.set_cursor_position((inner.x + cc, inner.y + cr));
        }
    }
}
