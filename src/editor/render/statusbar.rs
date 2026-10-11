//! the status bar: position, verdict badge and agent state.

use super::*;
use crate::editor::*;
use ratatui::layout::Rect;

/// Statusline segmentée façon lualine : badge de focus, fichier, message,
/// diagnostics, norme, position, progression.
/// Paints the bottom bar: the verdict and position on the right, the mode badge
/// and busy word on the left, and the typed prompt or fading message between.
/// Hands nothing back; it only fills the bar.
pub(in crate::editor) fn draw_statusbar(frame: &mut Frame, ed: &Editor, area: Rect) {
    fill(frame, area, Style::default().bg(Ed::bar_bg()));
    let buf = frame.buffer_mut();
    let rx = draw_status_right(buf, ed, area);
    let x = draw_status_left(buf, ed, area);
    draw_status_middle(buf, ed, area.y, x, rx);
}

/// Paints the right-hand segments (verdict, position, progress) and returns the
/// column where they start, so the middle text knows how much room is left.
pub(in crate::editor) fn draw_status_right(
    buf: &mut ratatui::buffer::Buffer,
    ed: &Editor,
    area: Rect,
) -> u16 {
    let items = status_right_items(ed);
    // a pill costs two columns more than a plain segment (its two caps)
    let pad = if icons_enabled() { 4 } else { 2 };
    let total: u16 = items
        .iter()
        .map(|(s, _)| UnicodeWidthStr::width(s.as_str()) as u16 + pad)
        .sum();
    let mut rx = area.right().saturating_sub(total);
    // a fresh verdict pulses: bold for a moment, then calm
    let pulse = ed
        .check_at
        .map(|t| t.elapsed() < Duration::from_millis(700))
        .unwrap_or(false);
    for (text, color) in items {
        let seg = format!(" {text} ");
        let before = rx;
        rx = if icons_enabled() {
            put_pill(buf, rx, area.y, &seg, color, Ed::bar_bg(), pulse)
        } else {
            put_seg(buf, rx, area.y, &seg, color, Ed::bar_bg(), pulse)
        };
        if verdict_fading(ed, &text) {
            buf.set_style(
                Rect {
                    x: before,
                    y: area.y,
                    width: rx.saturating_sub(before),
                    height: 1,
                },
                Style::default().add_modifier(Modifier::DIM),
            );
        }
    }
    rx
}

/// Lists the right-hand segments in paint order: the check note when it is
/// fresh, then the position and the progress. The check is the only place the
/// verdict lives, so it is never copied elsewhere.
pub(in crate::editor) fn status_right_items(ed: &Editor) -> Vec<(String, Color)> {
    let mut right: Vec<(String, Color)> = Vec::new();
    if let Some(note) = &ed.check_note {
        if check_is_fresh(ed) {
            right.push((note.clone(), verdict_color(ed)));
        }
    }
    right.push((pos_text(ed).trim().to_string(), Ed::dim()));
    let pct = ((ed.cy + 1) * 100) / ed.lines.len().max(1);
    right.push((format!("{pct}%"), Ed::dim()));
    right
}

/// Picks the colour of the verdict: red for a major mark, green for a clean
/// check, amber for minor marks only.
pub(in crate::editor) fn verdict_color(ed: &Editor) -> Color {
    let bad = ed.check_marks.iter().any(|(_, s)| *s == Severity::Major);
    if bad {
        Ed::red()
    } else if ed.check_marks.is_empty() {
        Ed::green()
    } else {
        Ed::amber()
    }
}

/// Tells whether the last check is still within its time to live. A verdict
/// never checked counts as fresh, so its note is not hidden by default.
pub(in crate::editor) fn check_is_fresh(ed: &Editor) -> bool {
    ed.check_at.map(|t| t.elapsed() < CHECK_TTL).unwrap_or(true)
}

/// Tells whether a right-hand segment is the verdict at the end of its life,
/// so it should dim out before it goes.
pub(in crate::editor) fn verdict_fading(ed: &Editor, text: &str) -> bool {
    let check_age = ed.check_at.map(|t| t.elapsed());
    text.starts_with("norm")
        && check_age
            .map(|a| a > CHECK_TTL.saturating_sub(FADE_OUT * 2))
            .unwrap_or(false)
}

/// Paints the left-hand segments: the mode badge, the unsaved dot and the busy
/// word. Hands back the column after the last one, where the middle text starts.
pub(in crate::editor) fn draw_status_left(
    buf: &mut ratatui::buffer::Buffer,
    ed: &Editor,
    area: Rect,
) -> u16 {
    let (label, color) = status_badge(ed);
    let padded = format!(" {label} ");
    let mut x = if icons_enabled() {
        put_capsule(buf, area.x, area.y, &padded, Ed::bar_bg(), color, true)
    } else {
        put_seg(buf, area.x, area.y, &padded, color, Ed::bar_bg(), true) + 1
    };
    // unsaved changes: a dot, and nothing around it — no square, no pill,
    // no background of ours behind a single character
    if ed.file.is_some() && ed.modified {
        x = put_seg(buf, x, area.y, " ● ", Ed::accent(), Ed::bar_bg(), false);
    }
    // something is running, and it turns
    if let Some(what) = ed.busy() {
        x = put_seg(
            buf,
            x,
            area.y,
            &format!(" {} {what} ", ed.spinner()),
            Ed::cyan(),
            Ed::bar_bg(),
            false,
        );
    }
    x
}

/// Names what has the keyboard, with its colour: a modal card first, then the
/// hub, then the focused panel, and the language while you are editing.
pub(in crate::editor) fn status_badge(ed: &Editor) -> (String, Color) {
    if ed.agent_pane.is_some() {
        agent_badge(ed)
    } else if ed.hub.is_some() {
        ("hub".to_string(), Color::Indexed(5))
    } else {
        match ed.focus {
            Focus::Editor => {
                let lang = ed
                    .file
                    .as_deref()
                    .map(langs::for_path)
                    .unwrap_or(&langs::PLAIN);
                (lang.name.to_string(), Color::Indexed(lang.color))
            }
            Focus::Explorer => ("explorer".to_string(), Ed::green()),
            Focus::Search => ("search".to_string(), Color::Indexed(5)),
            Focus::Terminal => ("terminal".to_string(), Ed::amber()),
        }
    }
}

/// Paints the middle of the bar between `x` and `rx`: the typed prompt, or a
/// message that fades away. An empty bar is the resting state.
pub(in crate::editor) fn draw_status_middle(
    buf: &mut ratatui::buffer::Buffer,
    ed: &Editor,
    y: u16,
    x: u16,
    rx: u16,
) {
    let mid = status_middle_text(ed);
    if mid.is_empty() {
        return;
    }
    let fading = ed.status_age() > STATUS_TTL.saturating_sub(FADE_OUT);
    let avail = (rx.saturating_sub(x + 1)) as usize;
    let mid: String = mid.chars().take(avail.saturating_sub(1)).collect();
    let fg = if fading { Ed::gutter() } else { Ed::dim() };
    let end = put_seg(buf, x, y, &format!(" {mid}"), fg, Ed::bar_bg(), false);
    if fading {
        // the terminal's own faint attribute: a real fade, no palette hack
        buf.set_style(
            Rect {
                x,
                y,
                width: end.saturating_sub(x),
                height: 1,
            },
            Style::default().add_modifier(Modifier::DIM),
        );
    }
}

/// Returns the text of the middle of the bar: the prompt being typed with its
/// cursor mark, or the status message while it lives, or nothing.
pub(in crate::editor) fn status_middle_text(ed: &Editor) -> String {
    if let Some((k, t)) = &ed.prompt {
        let label = match k {
            'f' => "search : ",
            'r' => "replace : ",
            'n' => "new file : ",
            'w' => "rename to : ",
            _ => "line : ",
        };
        format!("{label}{t}▌")
    } else if ed.status_age() < STATUS_TTL {
        ed.status.clone()
    } else {
        String::new()
    }
}
