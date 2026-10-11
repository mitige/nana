//! the top bar of the ide.

use super::*;
use crate::editor::*;

/// Barre basse, côté droit : la position, rien de plus.
pub(in crate::editor) fn pos_text(ed: &Editor) -> String {
    format!("ln {}, col {} ", ed.cy + 1, ed.cx + 1)
}

/// Barre haute : bloc brand corail + branche git + breadcrumb du fichier —
/// la tabline d'un IDE, version Minuit profond.
pub(in crate::editor) fn draw_topbar(frame: &mut Frame, ed: &Editor, area: ratatui::layout::Rect) {
    fill(frame, area, Style::default().bg(Ed::bar_bg()));
    let seg_bg = Ed::bar_bg();
    let buf = frame.buffer_mut();
    let mut x = area.x;
    // brand: a rounded pill, so even the wordmark has no square corner
    x = if icons_enabled() {
        put_capsule(buf, x, area.y, " nana ", Ed::bar_bg(), Ed::accent(), true)
    } else {
        put_seg(buf, x, area.y, " nana ", Ed::bar_bg(), Ed::accent(), true) + 1
    };
    // branche git
    if let Some(br) = &ed.git_branch {
        let label = format!(" ⎇ {br} ");
        if icons_enabled() {
            put_pill(buf, x, area.y, &label, Ed::text(), seg_bg, false);
        } else {
            put_seg(buf, x, area.y, &label, Ed::text(), seg_bg, false);
        }
    }
    // right: the project you are in. the check verdict lives in the status
    // bar only — one fact, one place.
    // no project label here: it read "make" while you were in a .c file, and
    // a corner that names the wrong thing is worse than an empty corner. the
    // kind still drives f5 and f6 — it just does not need a badge.
    // breadcrumb centré : dossier › fichier ●
    if let Some(p) = &ed.file {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let dir = p
            .parent()
            .and_then(|d| d.file_name())
            .and_then(|n| n.to_str());
        let crumb = match dir {
            Some(d) if !d.is_empty() => format!("{d} › {name}"),
            _ => name.to_string(),
        };
        let dirty_w = if ed.modified { 2 } else { 0 };
        let icon = file_icon(name, false);
        // the breadcrumb sits on the bar itself — no box, no chip, no caps:
        // just the path, in the middle, where the eye expects a title
        let total = UnicodeWidthStr::width(crumb.as_str()) as u16 + dirty_w + 2;
        let cx = area.x + area.width.saturating_sub(total) / 2;
        let ix = put_seg(
            buf,
            cx,
            area.y,
            icon,
            file_color(name, false),
            Ed::bar_bg(),
            false,
        );
        let nx = put_seg(
            buf,
            ix,
            area.y,
            &format!(" {crumb}"),
            Ed::text(),
            Ed::bar_bg(),
            false,
        );
        if ed.modified {
            put_seg(buf, nx, area.y, " ●", Ed::accent(), Ed::bar_bg(), false);
        }
    }
}
