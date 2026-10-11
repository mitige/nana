//! the hub screen: menu, prompt and detail.

use super::*;
use crate::editor::*;
use ratatui::layout::Rect;

/// The agent card: transcript on top, prompt at the bottom, rounded like
/// everything else. it overlays the editor, and the terminal paints its own
/// background underneath.
/// The hub: the boxes side by side. left is the navigation — the sections and
/// the items of the current one — right is the detail of what you selected.
/// two rounded cards with air between them, and like every other card they
/// erase what they cover: the editor must not show through the text.
/// Paints the hub as two cards: the sections and their items on the left, the
/// detail of the selection on the right. Does nothing when the zone is too small
/// to read, so a tiny terminal shows no half-drawn card.
pub(in crate::editor) fn draw_hub(frame: &mut Frame, hub: &mut crate::hub::Hub, zone: Rect) {
    if zone.width < 60 || zone.height < 10 {
        return;
    }
    // a margin all around, so the cards float instead of filling the screen
    let zone = Rect {
        x: zone.x + 2,
        y: zone.y + 1,
        width: zone.width.saturating_sub(4),
        height: zone.height.saturating_sub(2),
    };
    let left_w = (zone.width / 3).clamp(22, 38);
    let sp = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(left_w),
            Constraint::Length(2), // souffle entre les deux cartes
            Constraint::Min(24),
        ])
        .split(zone);
    draw_hub_menu(frame, hub, sp[0]);
    draw_hub_detail(frame, hub, sp[2]);
}

/// Paints the left card of the hub: the section titles, the items of the
/// current section with the highlight on the cursor row, and the prompt line
/// when the card is asking for input or filtering.
pub(in crate::editor) fn draw_hub_menu(frame: &mut Frame, hub: &crate::hub::Hub, zone: Rect) {
    clear_area(frame, zone);
    let inner = draw_box(frame, zone, Ed::cyan());
    let cursor = hub.cursor().min(hub.items().len().saturating_sub(1));
    frame.render_widget(Paragraph::new(hub_menu_lines(hub, inner.width)), inner);
    // the items start right here: everything above is the section list plus its
    // blank line, and the highlight must land on the row it belongs to
    let items_start = hub.section_count() as u16 + 1;
    if !hub.items().is_empty() {
        let y = inner.y + items_start + cursor as u16;
        if y < inner.bottom() {
            round_band(
                frame.buffer_mut(),
                y,
                inner.x,
                inner.right().saturating_sub(1),
                Ed::hover(),
            );
        }
    }
    draw_hub_prompt(frame, hub, zone);
}

/// Builds the rows of the hub menu: one line per section title, a blank line,
/// then one line per item of the current section. `width` is the card's inner
/// width, which decides where a label is cut.
pub(in crate::editor) fn hub_menu_lines(hub: &crate::hub::Hub, width: u16) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();
    for (i, name) in crate::hub::section_titles(hub.current()).iter().enumerate() {
        let current = i == hub.section_index();
        lines.push(Line::from(Span::styled(
            format!(" {name}"),
            Style::default()
                .fg(if current { Ed::text() } else { Ed::gutter() })
                .add_modifier(if current {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        )));
    }
    lines.push(Line::from(""));
    let cursor = hub.cursor().min(hub.items().len().saturating_sub(1));
    for (i, item) in hub.items().iter().enumerate() {
        lines.push(hub_item_line(item, i == cursor, width));
    }
    lines
}

/// Builds one item row of the hub menu: a cursor sign, the label cut to the
/// card, and the note when the row is not selected. The cut gets an ellipsis so
/// it never reads as a bug.
pub(in crate::editor) fn hub_item_line(
    item: &crate::hub::Item,
    selected: bool,
    width: u16,
) -> Line<'static> {
    // the row is cut to the card, with a sign: a name that runs into the
    // border reads as broken, and a cut with no ellipsis reads as a bug
    let room = width.saturating_sub(3) as usize;
    let label = clip(&format!(" {}", item.label), room);
    let mut spans = vec![
        Span::styled(
            if selected { "▎" } else { " " },
            Style::default().fg(Ed::cyan()),
        ),
        Span::styled(
            label.clone(),
            Style::default().fg(if selected { Ed::text() } else { Ed::dim() }),
        ),
    ];
    if !item.note.is_empty() && !selected {
        let used = UnicodeWidthStr::width(label.as_str()) + 1;
        let note = clip(&format!("  {}", item.note), room.saturating_sub(used));
        spans.push(Span::styled(note, Style::default().fg(Ed::gutter())));
    }
    Line::from(spans)
}

/// Paints the hub's prompt one row above the bottom of the menu card: the
/// purpose label and the typed input while it asks, or the filter query.
pub(in crate::editor) fn draw_hub_prompt(frame: &mut Frame, hub: &crate::hub::Hub, zone: Rect) {
    let row = Rect {
        y: zone.bottom().saturating_sub(2),
        height: 1,
        ..zone
    };
    if hub.typing() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", hub.purpose.label()),
                    Style::default().fg(Ed::bg()).bg(Ed::cyan()),
                ),
                Span::styled(format!(" {}▌", hub.input), Style::default().fg(Ed::text())),
            ])),
            row,
        );
    } else if !hub.query().is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ❯ ", Style::default().fg(Ed::accent())),
                Span::styled(hub.query().to_string(), Style::default().fg(Ed::text())),
            ])),
            row,
        );
    }
}

/// Paints the right card of the hub: the detail of the selection, or the sweep
/// frame of the world map while it sweeps. The text is wrapped and cut to the card.
pub(in crate::editor) fn draw_hub_detail(frame: &mut Frame, hub: &crate::hub::Hub, zone: Rect) {
    clear_area(frame, zone);
    let inner = draw_box(frame, zone, Ed::gutter());
    // while the map sweeps, the sweep's own frame is what the card shows
    let detail = match (hub.current(), hub.sweep_progress()) {
        (crate::hub::Section::World, Some(p)) => {
            crate::world::render_sweep(&hub.root, hub.selected().map(|i| &i.action), p)
        }
        _ => hub.detail().to_string(),
    };
    let body: Vec<Line> = wrap(&detail, inner.width.saturating_sub(2) as usize)
        .into_iter()
        .take(inner.height as usize)
        .map(|l| {
            Line::from(Span::styled(
                format!(" {l}"),
                Style::default().fg(Ed::text()),
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(body), inner);
}
