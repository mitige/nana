//! toasts and diagnostics shown over the buffer.

use super::*;
use crate::editor::*;

/// Notifications toast — cartes flottantes empilées en haut à droite,
/// colorées par niveau, évanouissement après 3,5 s.
pub(in crate::editor) fn draw_toasts(
    frame: &mut Frame,
    ed: &mut Editor,
    area: ratatui::layout::Rect,
) {
    ed.toasts.retain(|t| t.at.elapsed() < TOAST_TTL);
    let shown: Vec<&Toast> = ed.toasts.iter().rev().take(3).collect();
    let mut y = area.y + 1;
    for toast in shown {
        // le texte se wrappe sur 2 lignes — plus jamais tronqué
        let inner_w = 42usize; // colonnes de texte max par ligne
        let words: Vec<&str> = toast.text.split_whitespace().collect();
        let mut lines_t: Vec<String> = Vec::new();
        let mut cur = String::new();
        for w in words {
            let cand = if cur.is_empty() {
                w.to_string()
            } else {
                format!("{cur} {w}")
            };
            if UnicodeWidthStr::width(cand.as_str()) > inner_w && !cur.is_empty() {
                lines_t.push(cur);
                cur = w.to_string();
            } else {
                cur = cand;
            }
        }
        if !cur.is_empty() {
            lines_t.push(cur);
        }
        // jamais coupé : jusqu'à 5 lignes, la dernière porte « … » si besoin
        if lines_t.len() > 5 {
            lines_t.truncate(5);
            let last = lines_t.last_mut().unwrap();
            *last = format!("{}…", last.trim_end());
        }
        let text_w = lines_t
            .iter()
            .map(|l| UnicodeWidthStr::width(l.as_str()))
            .max()
            .unwrap_or(4);
        let w = ((text_w + 6).clamp(14, 48)) as u16;
        let h = lines_t.len() as u16 + 2;
        // the card slides in from the right, then fades out before it goes
        let age = toast.at.elapsed();
        let slide = if age < SLIDE_IN {
            (SLIDE_IN - age).as_millis() as u16 / 6
        } else {
            0
        };
        let x = area.right().saturating_sub(w + 1).saturating_add(slide);
        if y + h + 1 >= area.bottom() || x + w > area.right() {
            y += h;
            continue;
        }
        let rect = ratatui::layout::Rect {
            x,
            y,
            width: w,
            height: h,
        };
        clear_area(frame, rect);
        let (icon, color) = match toast.level {
            Level::Ok => ("✓", Ed::green()),
            Level::Err => ("✗", Ed::red()),
            Level::Warn => ("⚠", Ed::amber()),
            Level::Info => ("ℹ", Ed::cyan()),
        };
        let inner = draw_box(frame, rect, color);
        // l'icône n'apparaît qu'une fois — sur la première ligne ; les
        // lignes de continuation s'alignent dessous, sans la répéter.
        let lines: Vec<Line> = lines_t
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let head = if i == 0 {
                    Span::styled(format!("{icon} "), Style::default().fg(color))
                } else {
                    Span::styled("  ", Style::default().fg(Ed::dim()))
                };
                Line::from(vec![
                    head,
                    Span::styled(l.clone(), Style::default().fg(Ed::text())),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
        if age > TOAST_TTL.saturating_sub(FADE_OUT) {
            frame
                .buffer_mut()
                .set_style(rect, Style::default().add_modifier(Modifier::DIM));
        }
        y += h + 1;
    }
}

/// Survol des diagnostics de la ligne courante — le hover LSP : petit
/// flottant en bas à droite de l'éditeur, bordure rouge ou ambre.
pub(in crate::editor) fn draw_diagnostics(
    frame: &mut Frame,
    ed: &Editor,
    zone: ratatui::layout::Rect,
) {
    let items = ed.line_diagnostics();
    if items.is_empty() {
        return;
    }
    // chaque message peut vivre sur 2 lignes — les longs libellés de norme
    // ne sont plus jamais coupés
    let wrap_w = 50usize;
    let mut wrapped: Vec<(bool, String, bool)> = Vec::new(); // (err, texte, suite?)
    for (is_err, msg) in items.iter().take(4) {
        let words: Vec<&str> = msg.split_whitespace().collect();
        let mut cur = String::new();
        let mut first = true;
        for word in words {
            let cand = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if UnicodeWidthStr::width(cand.as_str()) > wrap_w && !cur.is_empty() {
                wrapped.push((*is_err, std::mem::take(&mut cur), !first));
                first = false;
                cur = word.to_string();
            } else {
                cur = cand;
            }
        }
        if !cur.is_empty() {
            wrapped.push((*is_err, cur, !first));
        }
    }
    wrapped.truncate(8);
    let wmax = wrapped
        .iter()
        .map(|(_, m, _)| UnicodeWidthStr::width(m.as_str()))
        .max()
        .unwrap_or(8);
    let w = ((wmax + 8).clamp(18, 58)) as u16;
    let h = wrapped.len() as u16 + 2;
    let rect = ratatui::layout::Rect {
        x: zone.right().saturating_sub(w + 1),
        y: zone.bottom().saturating_sub(h + 1),
        width: w,
        height: h,
    };
    clear_area(frame, rect);
    let any_err = items.iter().any(|(e, _)| *e);
    let border = if any_err { Ed::red() } else { Ed::amber() };
    let inner = draw_box(frame, rect, border);
    let lines: Vec<Line> = wrapped
        .iter()
        .map(|(is_err, msg, suite)| {
            let (icon, color) = if *is_err {
                ("✗", Ed::red())
            } else {
                ("⚠", Ed::amber())
            };
            let (glyph, gcolor) = if *suite {
                ("  ", Ed::dim())
            } else {
                (icon, color)
            };
            Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(gcolor)),
                Span::styled(msg.clone(), Style::default().fg(Ed::text())),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}
