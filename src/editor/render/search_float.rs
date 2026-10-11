//! the search float.

use super::*;
use crate::editor::*;

/// Recherche de fichiers flottante — le float Telescope : prompt en haut,
/// séparateur fin, résultats sous le curseur.
/// height of the floating search: the prompt, a separator, the borders and up
/// to nine results — but never so short that a search with a single result has
/// no room to draw. (it used to vanish entirely in that case.)
pub(in crate::editor) fn search_float_height(results: usize, zone_height: u16) -> u16 {
    let rows = (results.min(9) as u16).max(1);
    (rows + 4).max(6).min(zone_height.saturating_sub(2))
}

/// Draws the file search box floating over the editor, with its results, and skips it when the zone is too small.
pub(in crate::editor) fn draw_search(
    frame: &mut Frame,
    fs: &mut FileSearch,
    zone: ratatui::layout::Rect,
    focused: bool,
) {
    let w = zone.width.saturating_sub(8).min(62);
    let h = search_float_height(fs.len(), zone.height);
    if w < 24 || h < 6 {
        return;
    }
    let float = ratatui::layout::Rect {
        x: zone.x + (zone.width - w) / 2,
        y: zone.y + (zone.height / 8).min(3),
        width: w,
        height: h,
    };
    clear_area(frame, float);
    let inner = draw_box(frame, float, border_for(focused));
    // prompt
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(Ed::cyan())),
            Span::styled(
                format!("{}▌", fs.filter()),
                Style::default().fg(if focused { Ed::text() } else { Ed::dim() }),
            ),
        ])),
        ratatui::layout::Rect { height: 1, ..inner },
    );
    // séparateur fin entre prompt et résultats
    let sep_y = inner.y + 1;
    for cx in inner.x..inner.right() {
        let cell = &mut frame.buffer_mut()[(cx, sep_y)];
        cell.set_symbol("─").set_fg(Ed::ruler()).set_bg(float_bg());
    }
    // résultats
    let list_h = (inner.height as usize).saturating_sub(2);
    let (start, paths) = fs.window(list_h);
    let sel = fs.sel();
    let lines: Vec<Line> = if paths.is_empty() {
        vec![Line::from(Span::styled(
            "  no file",
            Style::default().fg(Ed::gutter()),
        ))]
    } else {
        paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let rel = fs.rel(path);
                let selected = start + i == sel;
                let mut spans = vec![Span::raw(if selected { "▎" } else { " " })];
                let fname = rel.rsplit('/').next().unwrap_or(&rel);
                let icon_color = file_color(fname, false);
                spans.push(Span::styled(
                    format!("{} ", file_icon(fname, false)),
                    Style::default().fg(icon_color),
                ));
                match rel.rsplit_once('/') {
                    Some((dir, name)) => {
                        spans.push(Span::styled(
                            format!("{dir}/"),
                            Style::default().fg(Ed::gutter()),
                        ));
                        let mut st = Style::default().fg(file_color(name, false));
                        if selected {
                            st = st.add_modifier(Modifier::BOLD);
                        }
                        spans.push(Span::styled(name.to_string(), st));
                    }
                    None => spans.push(Span::styled(rel, Style::default().fg(Ed::text()))),
                }
                let mut line = Line::from(spans);
                if selected {
                    line = line.style(Style::default().bg(Ed::sel_row()));
                }
                line
            })
            .collect()
    };
    let list_area = ratatui::layout::Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(2),
        ..inner
    };
    frame.render_widget(Paragraph::new(lines), list_area);
    if sel >= start && sel < start + paths.len() {
        let y = list_area.y + (sel - start) as u16;
        round_band(
            frame.buffer_mut(),
            y,
            list_area.x,
            list_area.right().saturating_sub(1),
            Ed::sel_row(),
        );
    }
}
