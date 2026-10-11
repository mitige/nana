//! the explorer column: header, filter and tree rows.

use super::*;
use crate::editor::*;
use ratatui::layout::Rect;

/// Panneau explorateur : arbre + filtre, séparateur fin, sélection teintée.
/// Paints the explorer inside its box: the root name on top, the tree rows in
/// the middle and the filter line at the bottom. `area` is the box interior.
pub(in crate::editor) fn draw_explorer(frame: &mut Frame, ex: &mut Explorer, area: Rect) {
    draw_explorer_header(frame, ex, area);
    draw_explorer_filter(frame, ex, area);
    // the tree sits between the header and the filter line
    let tree_h = (area.height as usize).saturating_sub(2);
    let rows_area = Rect {
        y: area.y + 1,
        height: tree_h as u16,
        ..area
    };
    draw_explorer_rows(frame, ex, rows_area, tree_h);
}

/// Paints the first line of the explorer: the name of its root with a caret.
pub(in crate::editor) fn draw_explorer_header(frame: &mut Frame, ex: &Explorer, area: Rect) {
    let root_name = ex
        .root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    let header = Line::from(vec![
        Span::styled("▾ ", Style::default().fg(Ed::cyan())),
        Span::styled(
            root_name.to_string(),
            Style::default().fg(Ed::text()).add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(header), Rect { height: 1, ..area });
}

/// Paints the last line of the explorer: the query being typed, or the hint
/// that typing filters the tree when no query is set.
pub(in crate::editor) fn draw_explorer_filter(frame: &mut Frame, ex: &Explorer, area: Rect) {
    let filter_line = if ex.filter().is_empty() {
        Line::from(Span::styled(
            " type to filter",
            Style::default().fg(Ed::dim()),
        ))
    } else {
        Line::from(vec![
            Span::styled("❯ ", Style::default().fg(Ed::cyan())),
            Span::styled(format!("{}▌", ex.filter()), Style::default().fg(Ed::text())),
        ])
    };
    frame.render_widget(
        Paragraph::new(filter_line),
        Rect {
            y: area.bottom().saturating_sub(1),
            height: 1,
            ..area
        },
    );
}

/// Paints the visible tree rows and the selection band behind the selected one.
/// The window is copied out first so the borrow on the explorer ends before the
/// git marks are read while drawing.
pub(in crate::editor) fn draw_explorer_rows(
    frame: &mut Frame,
    ex: &mut Explorer,
    rows_area: Rect,
    tree_h: usize,
) {
    let (start, visible) = {
        let (s, rows) = ex.window(tree_h);
        (s, rows.to_vec())
    };
    let sel = ex.sel();
    let filtering = !ex.filter().is_empty();
    let lines: Vec<Line> = visible
        .iter()
        .enumerate()
        .map(|(i, row)| explorer_row_line(ex, row, start + i == sel, filtering))
        .collect();
    // the band of a row stops where its text stops: measure before drawing,
    // otherwise the closing cap floats at the panel edge and the pill breaks
    frame.render_widget(Paragraph::new(lines), rows_area);
    if sel >= start && sel < start + visible.len() {
        let y = rows_area.y + (sel - start) as u16;
        round_band(
            frame.buffer_mut(),
            y,
            rows_area.x,
            rows_area.right().saturating_sub(1),
            Ed::sel_row(),
        );
    }
}

/// Builds one explorer row: the accent bar when selected, then either the
/// relative path with a muted folder while filtering, or the tree indent with
/// the icon and the name, then the git mark when the file changed.
pub(in crate::editor) fn explorer_row_line(
    ex: &Explorer,
    row: &crate::explorer::Row,
    selected: bool,
    filtering: bool,
) -> Line<'static> {
    let mut spans = Vec::new();
    // barre d'accent corail sur le bord — la sélection se lit d'un coup d'œil
    if selected {
        spans.push(Span::styled("▎", Style::default().fg(Ed::accent())));
    } else {
        spans.push(Span::raw(" "));
    }
    if filtering {
        spans.extend(filter_row_spans(&row.name));
    } else {
        spans.extend(tree_row_spans(row, selected));
    }
    // marqueur git : ● ambre modifié, ● vert nouveau
    if let Some(mark) = ex.git_mark(&row.path) {
        let color = if mark == 'M' {
            Ed::amber()
        } else {
            Ed::green()
        };
        spans.push(Span::styled(" ●", Style::default().fg(color)));
    }
    let mut line = Line::from(spans);
    if selected {
        line = line.style(Style::default().bg(Ed::sel_row()));
    }
    line
}

/// Builds the spans of a row while filtering: the relative path, with its
/// folder in a muted tone and the file name in its language colour.
pub(in crate::editor) fn filter_row_spans(name: &str) -> Vec<Span<'static>> {
    match name.rsplit_once('/') {
        Some((dir, file)) => vec![
            Span::styled(format!(" {dir}/"), Style::default().fg(Ed::gutter())),
            Span::styled(
                file.to_string(),
                Style::default().fg(file_color(file, false)),
            ),
        ],
        None => vec![Span::styled(
            format!(" {name}"),
            Style::default().fg(file_color(name, false)),
        )],
    }
}

/// Builds the spans of a row in the tree: the depth indent, a folder or file
/// icon, and the name, bold when the row is selected.
pub(in crate::editor) fn tree_row_spans(
    row: &crate::explorer::Row,
    selected: bool,
) -> Vec<Span<'static>> {
    let mut spans = vec![Span::raw("  ".repeat(row.depth))];
    if row.is_dir {
        spans.push(Span::styled(
            format!("{} ", dir_icon(row.expanded)),
            Style::default().fg(Ed::cyan()),
        ));
    } else {
        // mini-icône du langage, teintée
        spans.push(Span::styled(
            format!("{} ", file_icon(&row.name, false)),
            Style::default().fg(file_color(&row.name, false)),
        ));
    }
    let mut name_style = Style::default().fg(file_color(&row.name, row.is_dir));
    if selected {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }
    spans.push(Span::styled(row.name.clone(), name_style));
    spans
}
