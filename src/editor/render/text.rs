//! text helpers for the screen: widths, wrapping, clipping and one rendered line.

use crate::editor::agent_pane::safe_text;
use crate::editor::*;

/// Expand tabs for display only. A tab written into one cell makes the
/// terminal jump: the frame's cursor drifts from the buffer's idea of it and
/// the screen fills with nonsense. The buffer keeps the real tab; this is
/// what the eye gets. Width 4, the tab stop of the editor.
pub(in crate::editor) fn expand_tabs(s: &str) -> String {
    if !s.contains('\t') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    let mut col = 0usize;
    for ch in s.chars() {
        if ch == '\t' {
            let stop = 4 - (col % 4);
            for _ in 0..stop {
                out.push(' ');
            }
            col += stop;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

/// Turns one highlighted line into styled spans with the terminal colours of the syntax, so code keeps its colours without a second parser.
pub(in crate::editor) fn render_line(
    text: &str,
    palette_dim: Color,
    ext: &str,
) -> Vec<Span<'static>> {
    // coloration par langage (extension du fichier) via syntect — thème Minuit
    let hl = highlight::highlight_code(text, ext);
    if let Some(line) = hl.first() {
        line.iter()
            .map(|(style, t)| {
                Span::styled(
                    t.clone(),
                    Style::default().fg(Color::Rgb(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    )),
                )
            })
            .collect()
    } else {
        vec![Span::styled(
            text.to_string(),
            Style::default().fg(palette_dim),
        )]
    }
}

/// how many columns a piece of a line takes once its escapes and tabs are
/// resolved: the cursor and the scroll must agree with what the cells show.
pub(in crate::editor) fn drawn_width(prefix: &str) -> usize {
    safe_text(prefix).chars().count()
}

/// the prompt cut into rows that fit `width` columns: a line break starts a
/// new row, and a long line is cut where it no longer fits. one column stays
/// free for the cursor. the card shows the newest rows, so nothing is cut
/// with dots.
pub(in crate::editor) fn prompt_lines(input: &str, width: usize) -> Vec<String> {
    let width = width.saturating_sub(3).max(1);
    let mut rows = Vec::new();
    for paragraph in input.split('\n') {
        let mut cur = String::new();
        let mut used = 0;
        for ch in paragraph.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + w > width && !cur.is_empty() {
                rows.push(std::mem::take(&mut cur));
                used = 0;
            }
            cur.push(ch);
            used += w;
        }
        rows.push(cur);
    }
    rows
}

/// Cut `text` to `width` columns. A cut row ends in « … » so the reader sees
/// that something was there; text that fits is returned untouched.
pub(in crate::editor) fn clip(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

/// Wrap a paragraph to `width` columns, on words.
pub(in crate::editor) fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let mut cur = String::new();
        for word in paragraph.split_whitespace() {
            let cand = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if UnicodeWidthStr::width(cand.as_str()) > width && !cur.is_empty() {
                out.push(cur);
                cur = word.to_string();
            } else {
                cur = cand;
            }
        }
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}
