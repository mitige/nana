//! drawing primitives: fills, rounded bands, pills, capsules and boxes.

use crate::editor::agent_pane::is_invisible;
use crate::editor::*;

/// Remplit une zone d'une couleur unie (fond d'éditeur, barres).
pub(in crate::editor) fn fill(frame: &mut Frame, area: ratatui::layout::Rect, style: Style) {
    frame.render_widget(Paragraph::new("").style(style), area);
}

/// Erase a rectangle for a floating card. The cells are blanked and given the
/// terminal's *own* background (`Reset`), never a colour of ours: the blur of
/// the terminal still shows through, but the editor behind does not — two
/// texts drawn on top of each other is not transparency, it is a mess.
pub(in crate::editor) fn clear_area(frame: &mut Frame, area: ratatui::layout::Rect) {
    let buf = frame.buffer_mut();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
            }
        }
    }
}

/// Replaces every control or invisible character in the frame with a blank, so no stray glyph can break the layout.
pub(in crate::editor) fn scrub(buf: &mut ratatui::buffer::Buffer) {
    let area = buf.area;
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let cell = &mut buf[(x, y)];
            if cell
                .symbol()
                .chars()
                .any(|c| c.is_control() || is_invisible(c))
            {
                cell.set_symbol(" ");
            }
        }
    }
}

/// Peint la bande de fond d'une ligne sur x0..=x1, sans demi-cercle aux bouts
/// (le bord blanc qu'ils laissaient sur certains terminaux).
pub(in crate::editor) fn round_band(
    buf: &mut ratatui::buffer::Buffer,
    y: u16,
    x0: u16,
    x1: u16,
    band: Color,
) {
    if x1 <= x0 {
        return;
    }
    // paint the whole row first: the widgets only fill the cells they write,
    // so a band built out of them tears open wherever there is no text — before
    // a `:=`, on an empty line, in the gap between the gutter and the code.
    for x in x0..=x1 {
        buf[(x, y)].set_bg(band);
    }
}

/// un segment de barre : le libellé, encadré d'une cellule de la couleur du
/// segment de chaque côté. Pas de demi-cercle : il se voit en blanc sur les
/// terminaux dont la police ne le dessine pas comme le fond.
pub(in crate::editor) fn put_pill(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    label: &str,
    fg: Color,
    bg: Color,
    bold: bool,
) -> u16 {
    buf[(x, y)].set_symbol(" ").set_bg(bg);
    let after = put_seg(buf, x + 1, y, label, fg, bg, bold);
    buf[(after, y)].set_symbol(" ").set_bg(bg);
    after + 1
}

/// la capsule arrondie de la marque et du badge de statut : un demi-cercle
/// à chaque bout, dans la couleur du segment, sur le fond de la barre. Elle
/// reste réservée à ces deux blocs ; les autres segments restent plats.
pub(in crate::editor) fn put_capsule(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    label: &str,
    fg: Color,
    bg: Color,
    bold: bool,
) -> u16 {
    let edge_bg = Ed::bar_bg();
    buf[(x, y)]
        .set_symbol("\u{E0B6}")
        .set_fg(bg)
        .set_bg(edge_bg);
    let after = put_seg(buf, x + 1, y, label, fg, bg, bold);
    buf[(after, y)]
        .set_symbol("\u{E0B4}")
        .set_fg(bg)
        .set_bg(edge_bg);
    after + 1
}

/// Écrit un segment de barre (fond + texte) et retourne la colonne suivante.
pub(in crate::editor) fn put_seg(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    text: &str,
    fg: Color,
    bg: Color,
    bold: bool,
) -> u16 {
    let mut st = Style::default().fg(fg).bg(bg);
    if bold {
        st = st.add_modifier(Modifier::BOLD);
    }
    buf.set_string(x, y, text, st);
    x + UnicodeWidthStr::width(text) as u16
}

/// Draws a rounded box on the given area with a border colour and returns its inner area, or the area unchanged when it is too small to frame.
pub(in crate::editor) fn draw_box(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    border: Color,
) -> ratatui::layout::Rect {
    if area.width < 4 || area.height < 3 {
        return area;
    }
    let bgc = Ed::bg();
    let right = area.right() - 1;
    let bottom = area.bottom() - 1;
    let buf = frame.buffer_mut();
    for y in area.y..=bottom {
        buf[(area.x, y)].set_symbol("│").set_fg(border).set_bg(bgc);
        buf[(right, y)].set_symbol("│").set_fg(border).set_bg(bgc);
    }
    for x in area.x..=right {
        buf[(x, bottom)].set_symbol("─").set_fg(border).set_bg(bgc);
    }
    buf[(area.x, area.y)]
        .set_symbol("╭")
        .set_fg(border)
        .set_bg(bgc);
    buf[(right, area.y)]
        .set_symbol("╮")
        .set_fg(border)
        .set_bg(bgc);
    buf[(area.x, bottom)]
        .set_symbol("╰")
        .set_fg(border)
        .set_bg(bgc);
    buf[(right, bottom)]
        .set_symbol("╯")
        .set_fg(border)
        .set_bg(bgc);
    for x in (area.x + 1)..right {
        buf[(x, area.y)].set_symbol("─").set_fg(border).set_bg(bgc);
    }
    ratatui::layout::Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width - 2,
        height: area.height - 2,
    }
}

/// Trace une boîte à bords arrondis (le langage visuel de lazy.nvim) :
/// ╭─ titre ──╮ … ╰──╯. Retourne le rectangle intérieur.
/// Bordure d'un panneau : cyan voilé s'il a le focus, gris sinon.
pub(in crate::editor) fn border_for(focused: bool) -> Color {
    if focused {
        Color::Indexed(6) // cyan du shell — le focus se voit, sans crier
    } else {
        // a quiet blue, not a grey: an idle card keeps a colour of its own
        Color::Indexed(4)
    }
}

/// Fond des floats (modal, recherche, toasts, survol) — un demi-ton au-dessus.
pub(in crate::editor) fn float_bg() -> Color {
    // the terminal's own background, not a colour of ours: a notification, a
    // search float or a diagnostic is a window, not a slab of dark blue
    Color::Reset
}
