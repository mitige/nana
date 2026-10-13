//! the agent view: transcript, cursor and badge.

use super::*;
use crate::editor::*;

/// the agent's own view of its file: its lines and its cursor, in its own box,
/// separate from the user's buffer.
pub(in crate::editor) fn draw_agent_view(
    frame: &mut Frame,
    view: &AgentView,
    zone: ratatui::layout::Rect,
    playing: bool,
) {
    let Some(path) = &view.path else {
        return;
    };
    // a new view sweeps in from the left: its box is only as wide as the sweep
    // has got so far, so the handover reads as a movement, not a cut
    let zone = match view.opened {
        Some(at) if at.elapsed() < VIEW_SWEEP => {
            let progress = at.elapsed().as_secs_f32() / VIEW_SWEEP.as_secs_f32();
            let w = ((zone.width as f32) * progress).round().max(4.0) as u16;
            ratatui::layout::Rect {
                width: w.min(zone.width),
                ..zone
            }
        }
        _ => zone,
    };
    let is_action = crate::editor::is_action_view(path);
    // an action is what a person would type in a terminal: no file gutter and
    // no internal window name, only the tool's own words
    clear_area(frame, zone);
    let inner = draw_box(frame, zone, Ed::cyan());
    let h = inner.height as usize;
    let start = view.cy.saturating_sub(h.saturating_sub(1));
    // the number gutter is five columns wide; an action has none
    let gutter: u16 = if is_action { 0 } else { 5 };
    let ext = if is_action {
        ""
    } else {
        crate::langs::for_path(path)
            .exts
            .first()
            .copied()
            .unwrap_or("")
    };
    let lines: Vec<Line> = view
        .lines
        .iter()
        .enumerate()
        .skip(start)
        .take(h)
        .map(|(i, text)| {
            // the current line is marked only while the agent is writing:
            // an idle view shows its file, not a place where nothing happens
            let cur = playing && i == view.cy;
            let mut spans = Vec::new();
            if !is_action {
                spans.push(Span::styled(
                    format!("{:>4} ", i + 1),
                    Style::default().fg(if cur { Ed::text() } else { Ed::gutter() }),
                ));
            }
            // the file's own language colours the code, as the editor does
            spans.extend(render_line(
                &clip(text, inner.width.saturating_sub(gutter) as usize),
                Ed::text(),
                ext,
            ));
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    if playing {
        draw_agent_cursor(frame, view, inner, start, gutter);
    }
}

/// the agent's cursor: a block right after the last letter it typed. between two
/// lines it glides down from the line it left, so the eye follows the typing
/// instead of seeing the cursor jump.
pub(in crate::editor) fn draw_agent_cursor(
    frame: &mut Frame,
    view: &AgentView,
    inner: ratatui::layout::Rect,
    start: usize,
    gutter: u16,
) {
    let line = match view.glide {
        Some((from, since)) if since.elapsed() < CURSOR_GLIDE => {
            let t = since.elapsed().as_secs_f32() / CURSOR_GLIDE.as_secs_f32();
            let (from, to) = (from as f32, view.cy as f32);
            (from + (to - from) * t).round() as usize
        }
        _ => view.cy,
    };
    let Some(row) = line.checked_sub(start) else {
        return;
    };
    if row >= inner.height as usize {
        return;
    }
    let typed = view
        .lines
        .get(view.cy)
        .map(|l| drawn_width(l.get(..view.cx).unwrap_or(l)))
        .unwrap_or(0);
    // the text starts after the gutter, if the view has one
    let x = inner.x + gutter + typed as u16;
    if x >= inner.right() {
        return;
    }
    frame.buffer_mut()[(x, inner.y + row as u16)]
        .set_style(Style::default().add_modifier(Modifier::REVERSED));
}

/// Draws the agent card floating over the editor, sized to the zone, and skips the card when the zone is too small to read.
pub(in crate::editor) fn draw_agent(
    frame: &mut Frame,
    pane: &mut AgentPane,
    zone: ratatui::layout::Rect,
) {
    let w = zone.width.saturating_sub(8).min(84);
    let h = (zone.height.saturating_sub(4)).min(20);
    if w < 30 || h < 8 {
        return;
    }
    let float = ratatui::layout::Rect {
        x: zone.x + (zone.width - w) / 2,
        y: zone.y + (zone.height - h) / 2,
        width: w,
        height: h,
    };
    clear_area(frame, float);
    let inner = draw_box(frame, float, Ed::cyan());

    // the prompt wraps on the card's width, one row per visual line; it may
    // take up to half the card, and its last rows stay in view
    let prompt_rows = prompt_lines(&pane.input, inner.width as usize);
    let prompt_h = prompt_rows
        .len()
        .clamp(1, (inner.height as usize / 2).max(1));
    let shown_prompt = &prompt_rows[prompt_rows.len() - prompt_h..];

    // transcript, wrapped, newest at the bottom
    let body_h = (inner.height as usize).saturating_sub(prompt_h + 1);
    // the rows are wrapped only when a line is new (see TranscriptRows), so a
    // frame costs the same at the first line as at the thousandth
    let total = pane.rendered_rows(inner.width as usize).len();
    // the scroll is clamped to what the transcript holds, so scrolling back
    // down always moves at once instead of first unwinding a runaway count
    pane.scroll = pane.scroll.min(total.saturating_sub(body_h));
    let start = total.saturating_sub(body_h + pane.scroll);
    let end = (start + body_h).min(total);
    let view: Vec<Line> = pane.transcript.rows[start..end].to_vec();
    frame.render_widget(
        Paragraph::new(view),
        ratatui::layout::Rect {
            height: (body_h as u16).min(inner.height),
            ..inner
        },
    );

    // the prompt sits at the bottom of the card, its cursor after the last letter
    let last = shown_prompt.len() - 1;
    let prompt: Vec<Line> = shown_prompt
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let lead = if i == 0 { "> " } else { "  " };
            let mut spans = vec![
                Span::styled(lead, Style::default().fg(Ed::accent())),
                Span::styled(row.clone(), Style::default().fg(Ed::text())),
            ];
            if i == last {
                spans.push(Span::styled(
                    if pane.busy { "..." } else { "_" },
                    Style::default().fg(if pane.busy { Ed::amber() } else { Ed::cyan() }),
                ));
            }
            Line::from(spans)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(prompt),
        ratatui::layout::Rect {
            y: inner.bottom().saturating_sub(prompt_h as u16),
            height: prompt_h as u16,
            ..inner
        },
    );
}

/// the badge while the card is open follows what the agent plays: a shell
/// command is a terminal, a file is named by its language, anything else is
/// the agent itself. the rule lives here so the badge never guesses elsewhere.
pub(in crate::editor) fn agent_badge(ed: &Editor) -> (String, Color) {
    match ed.agent_view.path.as_deref() {
        Some(path) if crate::editor::is_action_view(path) => {
            let name = path.to_string_lossy();
            if name.ends_with("run_shell") {
                ("terminal".to_string(), Ed::amber())
            } else {
                ("agent".to_string(), Ed::cyan())
            }
        }
        Some(path) => {
            let lang = crate::langs::for_path(path);
            (lang.name.to_string(), Color::Indexed(lang.color))
        }
        None => ("agent".to_string(), Ed::cyan()),
    }
}
