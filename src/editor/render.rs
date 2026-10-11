//! drawing: every frame the editor paints, in one place, as free functions.
//! the layout rules and the glitch guards live here, see the feedback pages.

use super::*;

// ------------------------------------------------------------------ rendu

/// Barre basse, côté droit : la position, rien de plus.
pub(super) fn pos_text(ed: &Editor) -> String {
    format!("ln {}, col {} ", ed.cy + 1, ed.cx + 1)
}

/// Remplit une zone d'une couleur unie (fond d'éditeur, barres).
pub(super) fn fill(frame: &mut Frame, area: ratatui::layout::Rect, style: Style) {
    frame.render_widget(Paragraph::new("").style(style), area);
}

/// Erase a rectangle for a floating card. The cells are blanked and given the
/// terminal's *own* background (`Reset`), never a colour of ours: the blur of
/// the terminal still shows through, but the editor behind does not — two
/// texts drawn on top of each other is not transparency, it is a mess.
pub(super) fn clear_area(frame: &mut Frame, area: ratatui::layout::Rect) {
    let buf = frame.buffer_mut();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
            }
        }
    }
}

/// Expand tabs for display only. A tab written into one cell makes the
/// terminal jump: the frame's cursor drifts from the buffer's idea of it and
/// the screen fills with nonsense. The buffer keeps the real tab; this is
/// what the eye gets. Width 4, the tab stop of the editor.
pub(super) fn expand_tabs(s: &str) -> String {
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

pub(super) fn render_line(text: &str, palette_dim: Color, ext: &str) -> Vec<Span<'static>> {
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

/// Trace une boîte à bords arrondis (le langage visuel de lazy.nvim) :
/// ╭─ titre ──╮ … ╰──╯. Retourne le rectangle intérieur.
/// Bordure d'un panneau : cyan voilé s'il a le focus, gris sinon.
pub(super) fn border_for(focused: bool) -> Color {
    if focused {
        Color::Indexed(6) // cyan du shell — le focus se voit, sans crier
    } else {
        // a quiet blue, not a grey: an idle card keeps a colour of its own
        Color::Indexed(4)
    }
}

/// Fond des floats (modal, recherche, toasts, survol) — un demi-ton au-dessus.
pub(super) fn float_bg() -> Color {
    // the terminal's own background, not a colour of ours: a notification, a
    // search float or a diagnostic is a window, not a slab of dark blue
    Color::Reset
}

/// Peint la bande de fond d'une ligne sur x0..=x1, sans demi-cercle aux bouts
/// (le bord blanc qu'ils laissaient sur certains terminaux).
pub(super) fn round_band(buf: &mut ratatui::buffer::Buffer, y: u16, x0: u16, x1: u16, band: Color) {
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
pub(super) fn put_pill(
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
pub(super) fn put_capsule(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    label: &str,
    fg: Color,
    bg: Color,
    bold: bool,
    edge_bg: Color,
) -> u16 {
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
pub(super) fn put_seg(
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

pub(super) fn draw_box(
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

/// Écran d'accueil : modal flottant centré, sections à puces — le float
/// d'accueil de lazy.nvim, transposé aux gestes de c-nano. Aucune tagline.
pub(super) fn draw_welcome_float(frame: &mut Frame, area: ratatui::layout::Rect) {
    let w = area.width.saturating_sub(4).min(46);
    let h = area.height.saturating_sub(2).min(24);
    if w < 20 || h < 10 {
        return;
    }
    let modal = ratatui::layout::Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    let inner = draw_box(frame, modal, border_for(true));
    let bullet = || Span::styled("● ", Style::default().fg(Ed::cyan()));
    let section = |s: &'static str| {
        Line::from(Span::styled(
            format!(" {s}"),
            Style::default().fg(Ed::text()).add_modifier(Modifier::BOLD),
        ))
    };
    let entry = |k: &'static str, label: &'static str| {
        Line::from(vec![
            Span::raw("  "),
            bullet(),
            Span::styled(format!("{k:<5}"), Style::default().fg(Ed::cyan())),
            Span::styled(label, Style::default().fg(Ed::dim())),
        ])
    };
    let lines = vec![
        Line::from(""),
        section("files"),
        entry("^t", "explorer"),
        entry("^o", "find a file"),
        entry("f2", "switch panel"),
        entry("^s", "save"),
        entry("^q", "quit"),
        Line::from(""),
        section("code"),
        entry("^b", "check"),
        entry("f5", "run"),
        entry("f6", "format"),
        entry("f4", "auto header"),
        entry("^f", "search"),
        entry("^r", "replace"),
        entry("^g", "go to line"),
        Line::from(""),
        section("buffer"),
        entry("^k", "cut line"),
        entry("^u", "paste"),
        entry("^z", "cancel"),
        Line::from(""),
        Line::from(Span::styled(
            "  nana <file> to open — ^t explores the project",
            Style::default().fg(Ed::gutter()),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// the frame, then a last check on every cell: nothing a terminal would act on
/// reaches the screen. text is cleaned where it enters (safe_text); this is the
/// net under it, so a path that forgets to clean cannot glitch the view.
pub(super) fn draw(frame: &mut Frame, ed: &mut Editor) {
    draw_scene(frame, ed);
    scrub(frame.buffer_mut());
}

pub(super) fn scrub(buf: &mut ratatui::buffer::Buffer) {
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

pub(super) fn draw_scene(frame: &mut Frame, ed: &mut Editor) {
    let area = frame.area();
    // fond unifié sur toute la surface — la signature « Minuit »
    fill(frame, area, Style::default().bg(Ed::bg()));
    // the hub is the whole screen: no top bar, no editor, no status bar behind
    // it. the menu is the only thing the user sees until it is closed.
    if let Some(hub) = &mut ed.hub {
        clear_area(frame, area);
        draw_hub(frame, hub, area);
        draw_toasts(frame, ed, area);
        return;
    }

    // one blank line under the top bar and one above the status bar: a card
    // whose top border touches the bar is a rectangle, not a card. the air
    // around it is what makes the rounded corners read as rounded.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // barre haute
            Constraint::Length(1), // souffle
            Constraint::Min(3),    // corps
            Constraint::Length(1), // souffle
            Constraint::Length(1), // barre basse
        ])
        .split(area);
    fill(frame, chunks[0], Style::default().bg(Ed::bar_bg()));
    fill(frame, chunks[4], Style::default().bg(Ed::bar_bg()));

    draw_topbar(frame, ed, chunks[0]);
    let body_zone = chunks[2];

    // ── corps : boîtes arrondies façon lazy.nvim — explorateur à gauche,
    // éditeur à droite, le focus teinte la bordure ──
    let edit_zone = if ed.explorer.is_some() {
        let sp = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(EXPL_W),
                Constraint::Length(1), // souffle entre les boîtes
                Constraint::Min(10),
            ])
            .split(body_zone);
        // boîte nue : la bordure colorée + le badge suffisent au focus
        let inner = draw_box(frame, sp[0], border_for(ed.focus == Focus::Explorer));
        if let Some(ex) = &mut ed.explorer {
            draw_explorer(frame, ex, inner);
        }
        sp[2]
    } else {
        body_zone
    };
    // le terminal prend le bas de la colonne éditeur quand il est ouvert
    let (editor_area, term_area) = if ed.term.is_some() {
        // a blank line between the editor and the terminal, so the two cards
        // do not share a border line and read as one rectangle
        let sp = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),
                Constraint::Length(1), // souffle
                Constraint::Length(TERM_H),
            ])
            .split(edit_zone);
        (sp[0], Some(sp[2]))
    } else {
        (edit_zone, None)
    };
    let inner = draw_box(
        frame,
        editor_area,
        border_for(matches!(ed.focus, Focus::Editor | Focus::Search)),
    );
    if let Some(ta) = term_area {
        if let Some(term) = &mut ed.term {
            draw_terminal(frame, term, ta, ed.focus == Focus::Terminal);
        }
    }
    if ed.is_welcome() {
        draw_welcome_float(frame, inner);
    } else {
        draw_body(frame, ed, inner);
        draw_diagnostics(frame, ed, editor_area);
    }
    // the agent works over the ide: its view and card sit on the body, the
    // explorer, editor and terminal stay drawn around and under them
    if let Some(pane) = &mut ed.agent_pane {
        let playing = !ed.live.is_empty();
        draw_agent_view(frame, &ed.agent_view, editor_area, playing);
        draw_agent(frame, pane, body_zone);
    }
    // floats par-dessus tout : recherche, puis toasts
    // the search and the agent menu are windows of their own, like the hub:
    // nothing of the ide is drawn around them, so the card floats on a clean
    // screen
    if let Some(fs) = &mut ed.search {
        clear_area(frame, area);
        draw_search(frame, fs, area, ed.focus == Focus::Search);
        draw_toasts(frame, ed, area);
        return;
    }
    if let Some(hub) = &mut ed.hub {
        draw_hub(frame, hub, chunks[2]);
    }
    draw_toasts(frame, ed, area);

    draw_statusbar(frame, ed, chunks[4]);
}

/// Barre haute : bloc brand corail + branche git + breadcrumb du fichier —
/// la tabline d'un IDE, version Minuit profond.
pub(super) fn draw_topbar(frame: &mut Frame, ed: &Editor, area: ratatui::layout::Rect) {
    fill(frame, area, Style::default().bg(Ed::bar_bg()));
    let seg_bg = Ed::bar_bg();
    let buf = frame.buffer_mut();
    let mut x = area.x;
    // brand: a rounded pill, so even the wordmark has no square corner
    x = if icons_enabled() {
        put_capsule(
            buf,
            x,
            area.y,
            " nana ",
            Ed::bar_bg(),
            Ed::accent(),
            true,
            Ed::bar_bg(),
        )
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

/// Statusline segmentée façon lualine : badge de focus, fichier, message,
/// diagnostics, norme, position, progression.
pub(super) fn draw_statusbar(frame: &mut Frame, ed: &Editor, area: ratatui::layout::Rect) {
    fill(frame, area, Style::default().bg(Ed::bar_bg()));
    let seg_bg = Ed::bar_bg();
    let buf = frame.buffer_mut();
    let mut x = area.x;

    // right: one check indicator, the position, the progress. the check is
    // the only place the verdict lives — no copy in the top bar, none in the
    // transient line, no toast.
    let mut right: Vec<(String, Color)> = Vec::new();
    let check_age = ed.check_at.map(|t| t.elapsed());
    let check_fresh = check_age.map(|a| a < CHECK_TTL).unwrap_or(true);
    if let Some(note) = &ed.check_note {
        if check_fresh {
            let bad = ed.check_marks.iter().any(|(_, s)| *s == Severity::Major);
            let color = if bad {
                Ed::red()
            } else if ed.check_marks.is_empty() {
                Ed::green()
            } else {
                Ed::amber()
            };
            right.push((note.clone(), color));
        }
    }
    right.push((pos_text(ed).trim().to_string(), Ed::dim()));
    let pct = ((ed.cy + 1) * 100) / ed.lines.len().max(1);
    right.push((format!("{pct}%"), Ed::dim()));
    // a pill costs two columns more than a plain segment (its two caps)
    let pad = if icons_enabled() { 4 } else { 2 };
    let total: u16 = right
        .iter()
        .map(|(s, _)| UnicodeWidthStr::width(s.as_str()) as u16 + pad)
        .sum();
    let mut rx = area.right().saturating_sub(total);
    // a fresh verdict pulses: bold for a moment, then calm
    let pulse = ed
        .check_at
        .map(|t| t.elapsed() < Duration::from_millis(700))
        .unwrap_or(false);
    for (text, color) in right {
        let seg = format!(" {text} ");
        let before = rx;
        rx = if icons_enabled() {
            put_pill(buf, rx, area.y, &seg, color, seg_bg, pulse)
        } else {
            put_seg(buf, rx, area.y, &seg, color, seg_bg, pulse)
        };
        // the verdict dims out at the end of its life, then goes
        let fading = text.starts_with("norm")
            && check_age
                .map(|a| a > CHECK_TTL.saturating_sub(FADE_OUT * 2))
                .unwrap_or(false);
        if fading {
            buf.set_style(
                ratatui::layout::Rect {
                    x: before,
                    y: area.y,
                    width: rx.saturating_sub(before),
                    height: 1,
                },
                Style::default().add_modifier(Modifier::DIM),
            );
        }
    }

    // left: what you are looking at, as a pill — the language while editing,
    // the panel otherwise. the file name is not repeated here: it is in the
    // tree and in the breadcrumb already.
    let lang = ed
        .file
        .as_deref()
        .map(langs::for_path)
        .unwrap_or(&langs::PLAIN);
    // the badge names what has the keyboard: a modal card first, then the
    // panel, and the language while you are editing
    let (label, color) = if ed.agent_pane.is_some() {
        agent_badge(ed)
    } else if ed.hub.is_some() {
        ("hub".to_string(), Color::Indexed(5))
    } else {
        match ed.focus {
            Focus::Editor => (lang.name.to_string(), Color::Indexed(lang.color)),
            Focus::Explorer => ("explorer".to_string(), Ed::green()),
            Focus::Search => ("search".to_string(), Color::Indexed(5)),
            Focus::Terminal => ("terminal".to_string(), Ed::amber()),
        }
    };
    let padded = format!(" {label} ");
    x = if icons_enabled() {
        put_capsule(buf, x, area.y, &padded, seg_bg, color, true, seg_bg)
    } else {
        put_seg(buf, x, area.y, &padded, seg_bg, color, true) + 1
    };
    let _ = x;
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
            seg_bg,
            false,
        );
    }

    // middle: what you are typing, or a message that is fading away. an empty
    // bar is the resting state — nothing is repeated just to fill space.
    let age = ed.status_age();
    let fading = age > STATUS_TTL.saturating_sub(FADE_OUT);
    let mid = if let Some((k, t)) = &ed.prompt {
        let label = match k {
            'f' => "search : ",
            'r' => "replace : ",
            'n' => "new file : ",
            'w' => "rename to : ",
            _ => "line : ",
        };
        format!("{label}{t}▌")
    } else if age < STATUS_TTL {
        ed.status.clone()
    } else {
        String::new()
    };
    if !mid.is_empty() {
        let avail = (rx.saturating_sub(x + 1)) as usize;
        let mid: String = mid.chars().take(avail.saturating_sub(1)).collect();
        let fg = if fading { Ed::gutter() } else { Ed::dim() };
        let start = x;
        let end = put_seg(buf, x, area.y, &format!(" {mid}"), fg, Ed::bar_bg(), false);
        if fading {
            // the terminal's own faint attribute: a real fade, no palette hack
            buf.set_style(
                ratatui::layout::Rect {
                    x: start,
                    y: area.y,
                    width: end.saturating_sub(start),
                    height: 1,
                },
                Style::default().add_modifier(Modifier::DIM),
            );
        }
    }
}

/// Recherche de fichiers flottante — le float Telescope : prompt en haut,
/// séparateur fin, résultats sous le curseur.
/// height of the floating search: the prompt, a separator, the borders and up
/// to nine results — but never so short that a search with a single result has
/// no room to draw. (it used to vanish entirely in that case.)
pub(super) fn search_float_height(results: usize, zone_height: u16) -> u16 {
    let rows = (results.min(9) as u16).max(1);
    (rows + 4).max(6).min(zone_height.saturating_sub(2))
}

/// The agent card: transcript on top, prompt at the bottom, rounded like
/// everything else. it overlays the editor, and the terminal paints its own
/// background underneath.
/// The hub: the boxes side by side. left is the navigation — the sections and
/// the items of the current one — right is the detail of what you selected.
/// two rounded cards with air between them, and like every other card they
/// erase what they cover: the editor must not show through the text.
pub(super) fn draw_hub(frame: &mut Frame, hub: &mut crate::hub::Hub, zone: ratatui::layout::Rect) {
    if zone.width < 60 || zone.height < 10 {
        return;
    }
    // a margin all around, so the cards float instead of filling the screen
    let zone = ratatui::layout::Rect {
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

    // ---- left: the sections, then the items of the current one
    clear_area(frame, sp[0]);
    let inner = draw_box(frame, sp[0], Ed::cyan());
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
    // the items start right here: everything above is the section list plus its
    // blank line, and the highlight must land on the row it belongs to
    let items_start = hub.section_count() as u16 + 1;
    let cursor = hub.cursor().min(hub.items().len().saturating_sub(1));
    for (i, item) in hub.items().iter().enumerate() {
        let selected = i == cursor;
        // the row is cut to the card, with a sign: a name that runs into the
        // border reads as broken, and a cut with no ellipsis reads as a bug
        let room = inner.width.saturating_sub(3) as usize;
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
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), inner);
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
    // what the box is asking for, when it is asking for something
    if hub.typing() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", hub.purpose.label()),
                    Style::default().fg(Ed::bg()).bg(Ed::cyan()),
                ),
                Span::styled(format!(" {}▌", hub.input), Style::default().fg(Ed::text())),
            ])),
            ratatui::layout::Rect {
                y: sp[0].bottom().saturating_sub(2),
                height: 1,
                ..sp[0]
            },
        );
    } else if !hub.query().is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ❯ ", Style::default().fg(Ed::accent())),
                Span::styled(hub.query().to_string(), Style::default().fg(Ed::text())),
            ])),
            ratatui::layout::Rect {
                y: sp[0].bottom().saturating_sub(2),
                height: 1,
                ..sp[0]
            },
        );
    }

    // ---- right: what the selection is
    clear_area(frame, sp[2]);
    let inner = draw_box(frame, sp[2], Ed::gutter());
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

/// the agent's own view of its file: its lines and its cursor, in its own box,
/// separate from the user's buffer.
pub(super) fn draw_agent_view(
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
pub(super) fn draw_agent_cursor(
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

/// how many columns a piece of a line takes once its escapes and tabs are
/// resolved: the cursor and the scroll must agree with what the cells show.
pub(super) fn drawn_width(prefix: &str) -> usize {
    safe_text(prefix).chars().count()
}

pub(super) fn draw_agent(frame: &mut Frame, pane: &mut AgentPane, zone: ratatui::layout::Rect) {
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

/// the prompt cut into rows that fit `width` columns: a line break starts a
/// new row, and a long line is cut where it no longer fits. one column stays
/// free for the cursor. the card shows the newest rows, so nothing is cut
/// with dots.
pub(super) fn prompt_lines(input: &str, width: usize) -> Vec<String> {
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
pub(super) fn clip(text: &str, width: usize) -> String {
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
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
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

pub(super) fn draw_search(
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

/// Notifications toast — cartes flottantes empilées en haut à droite,
/// colorées par niveau, évanouissement après 3,5 s.
pub(super) fn draw_toasts(frame: &mut Frame, ed: &mut Editor, area: ratatui::layout::Rect) {
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
pub(super) fn draw_diagnostics(frame: &mut Frame, ed: &Editor, zone: ratatui::layout::Rect) {
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

/// Icônes activées ? `NANO_ICONS=0` → repli lettres cerclées (terminaux
/// sans Nerd Font). Les glyphes viennent des devicons de JetBrainsMono NF.
pub(super) fn icons_enabled() -> bool {
    static ONCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ONCE.get_or_init(|| {
        std::env::var_os("NANO_ICONS")
            .map(|v| v != "0")
            .unwrap_or(true)
    })
}

/// Icône devicon du fichier (les vrais logos : C , Rust , Python …).
/// the glyph comes from the language registry, so a new language brings its
/// own icon instead of falling into a catch-all branch.
pub(super) fn file_icon_dev(name: &str) -> &'static str {
    static TABLE: std::sync::LazyLock<HashMap<char, &'static str>> =
        std::sync::LazyLock::new(|| {
            let mut m: HashMap<char, &'static str> = HashMap::new();
            for l in langs::all() {
                m.entry(l.icon).or_insert_with(|| {
                    let s: &'static str = Box::leak(l.icon.to_string().into_boxed_str());
                    s
                });
            }
            m
        });
    let icon = langs::for_path(Path::new(name)).icon;
    TABLE.get(&icon).copied().unwrap_or("\u{F15B}")
}

pub(super) fn file_icon_plain(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "c" | "h" => "Ⓒ",
        "rs" => "Ⓡ",
        "py" => "Ⓟ",
        "js" | "ts" | "mjs" => "Ⓙ",
        "sh" | "bash" | "zsh" => "Ⓢ",
        "html" | "htm" => "Ⓗ",
        "css" => "ⓒ",
        "md" | "txt" => "Ⓜ",
        "toml" | "json" | "yaml" | "yml" | "lock" | "cfg" => "Ⓣ",
        _ if lower == "makefile" || ext == "mk" => "Ⓚ",
        _ => "◆",
    }
}

/// Mini-icône du fichier : devicon si possible, lettre cerclée sinon.
pub(super) fn file_icon(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "";
    }
    if icons_enabled() {
        file_icon_dev(name)
    } else {
        file_icon_plain(name)
    }
}

/// Icône de dossier :  ouverte /  fermée (repli : ▾/▸).
pub(super) fn dir_icon(expanded: bool) -> &'static str {
    if icons_enabled() {
        if expanded {
            "\u{F07C}"
        } else {
            "\u{F07B}"
        } // fa-folder-open / fa-folder
    } else if expanded {
        "▾"
    } else {
        "▸"
    }
}

/// Couleur d'un fichier selon son type — chaque famille a sa teinte.
pub(super) fn file_color(name: &str, is_dir: bool) -> Color {
    if is_dir {
        return Ed::cyan();
    }
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        // build artefacts stay quiet
        "o" | "a" | "so" | "bin" | "o_" => Ed::gutter(),
        _ => Color::Indexed(langs::for_path(Path::new(name)).color),
    }
}

/// Panneau explorateur : arbre + filtre, séparateur fin, sélection teintée.
pub(super) fn draw_explorer(frame: &mut Frame, ex: &mut Explorer, area: ratatui::layout::Rect) {
    // `area` est l'intérieur de la boîte (le cadre est déjà tracé)
    let content = area;

    // en-tête : la racine du panneau
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
    frame.render_widget(
        Paragraph::new(header),
        ratatui::layout::Rect {
            height: 1,
            ..content
        },
    );

    // ligne du bas : le filtre en cours, ou l'invitation
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
        ratatui::layout::Rect {
            y: content.bottom().saturating_sub(1),
            height: 1,
            ..content
        },
    );

    // lignes de l'arbre (entre en-tête et ligne de filtre)
    let tree_h = (content.height as usize).saturating_sub(2);
    let rows_area = ratatui::layout::Rect {
        y: content.y + 1,
        height: tree_h as u16,
        ..content
    };
    // on clone la fenêtre (quelques dizaines de lignes) pour libérer
    // l'emprunt : git_mark/sel restent accessibles pendant le rendu
    let (start, visible) = {
        let (s, rows) = ex.window(tree_h);
        (s, rows.to_vec())
    };
    let sel = ex.sel();
    let filtering = !ex.filter().is_empty();
    let lines: Vec<Line> = visible
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let selected = start + i == sel;
            let mut spans = Vec::new();
            // barre d'accent corail sur le bord — la sélection se lit d'un coup d'œil
            if selected {
                spans.push(Span::styled("▎", Style::default().fg(Ed::accent())));
            } else {
                spans.push(Span::raw(" "));
            }
            if filtering {
                // mode filtre : chemin relatif, dossier en sourdine
                match row.name.rsplit_once('/') {
                    Some((dir, name)) => {
                        spans.push(Span::styled(
                            format!(" {dir}/"),
                            Style::default().fg(Ed::gutter()),
                        ));
                        spans.push(Span::styled(
                            name.to_string(),
                            Style::default().fg(file_color(name, false)),
                        ));
                    }
                    None => spans.push(Span::styled(
                        format!(" {}", row.name),
                        Style::default().fg(file_color(&row.name, false)),
                    )),
                }
            } else {
                spans.push(Span::raw("  ".repeat(row.depth)));
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
        })
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

/// Conversion couleur vt100 → ratatui (défaut = la palette Minuit).
pub(super) fn vt_color(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Le panneau terminal : boîte arrondie, cellules vt100 rendues en Minuit.
pub(super) fn draw_terminal(
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

/// Corps de l'éditeur : gouttière à marqueurs + texte coloré.
pub(super) fn draw_body(frame: &mut Frame, ed: &Editor, zone: ratatui::layout::Rect) {
    let digits = ed.lines.len().to_string().len().max(2);
    let gutter_w = digits + 2; // marqueur + numéro + espace
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(gutter_w as u16), Constraint::Min(10)])
        .split(zone);

    let marks = ed.gutter_marks();
    let inner_h = zone.height as usize;
    let gutter_lines: Vec<Line> = (0..inner_h)
        .map(|i| {
            let n = ed.scroll_y + i + 1; // 1-based
            if n <= ed.lines.len() {
                let is_cur = n == ed.cy + 1;
                let mut spans = Vec::new();
                // marqueur : diagnostic gcc / norme, le plus sévère
                match marks.get(&n) {
                    Some(color) => spans.push(Span::styled("●", Style::default().fg(*color))),
                    None => spans.push(Span::raw(" ")),
                }
                spans.push(Span::styled(
                    format!("{:>w$} ", n, w = digits),
                    Style::default().fg(if is_cur { Ed::text() } else { Ed::gutter() }),
                ));
                let mut line = Line::from(spans);
                if is_cur {
                    line = line.style(Style::default().bg(Ed::cur_line()));
                }
                line
            } else {
                Line::from(" ".repeat(gutter_w))
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(gutter_lines), body[0]);

    let ghost_len = ed.ghost_lines.len();
    let text_lines: Vec<Line> = (0..inner_h)
        .map(|i| {
            let n = ed.scroll_y + i;
            // lignes fantômes SOUS le curseur : aperçu de l'insertion
            // multi-lignes (les vraies lignes réapparaissent au refus)
            if ghost_len > 0 && n > ed.cy && n - ed.cy < ghost_len {
                let vis: String = ed.ghost_lines[n - ed.cy]
                    .chars()
                    .skip(ed.scroll_x)
                    .collect();
                return Line::from(Span::styled(
                    vis,
                    Style::default()
                        .fg(Ed::ghost())
                        .add_modifier(Modifier::ITALIC),
                ));
            }
            if n < ed.lines.len() {
                let raw = &ed.lines[n];
                // cleaned and tab-expanded before anything is measured or drawn
                let start = drawn_width(&raw.chars().take(ed.scroll_x).collect::<String>());
                let visible: String = safe_text(raw).chars().skip(start).collect();
                let mut spans = render_line(&visible, Ed::dim(), &ed.ext);
                // première ligne du fantôme : à la suite de la ligne du curseur
                if n == ed.cy && ghost_len > 0 {
                    spans.push(Span::styled(
                        ed.ghost_lines[0].clone(),
                        Style::default()
                            .fg(Ed::ghost())
                            .add_modifier(Modifier::ITALIC),
                    ));
                }
                let mut line = Line::from(spans);
                if n == ed.cy {
                    // ligne courante teintée — le regard se pose instantanément
                    line = line.style(Style::default().bg(Ed::cur_line()));
                }
                line
            } else {
                Line::from("")
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(text_lines), body[1]);
    // the current line is a band with rounded ends, gutter included — and its
    // right end sits at the end of the line, not at the edge of the panel
    let cur_row = body[1].y + (ed.cy - ed.scroll_y) as u16;
    if cur_row >= body[1].y && cur_row < body[1].bottom() {
        round_band(
            frame.buffer_mut(),
            cur_row,
            body[0].x,
            body[1].right().saturating_sub(1),
            Ed::cur_line(),
        );
    }

    // repère subtil à la colonne 80 (la limite de la norme) — un filet discret
    let ruler_x = body[1].x + 80;
    if ruler_x < body[1].right() {
        for row in body[1].top()..body[1].bottom() {
            let cell = &mut frame.buffer_mut()[(ruler_x, row)];
            // n'écrase pas le code : uniquement sur une case vide,
            // et préserve le fond (ligne courante comprise)
            if cell.symbol().trim().is_empty() {
                cell.set_symbol("·").set_fg(Ed::ruler());
            }
        }
    }

    // curseur : seulement quand l'éditeur a le focus (dans l'explorateur,
    // c'est la ligne sélectionnée qui porte le regard)
    if ed.focus == Focus::Editor {
        let col = ed
            .lines
            .get(ed.cy)
            .map(|l| drawn_width(l.get(..ed.cx).unwrap_or(l)))
            .unwrap_or(ed.cx);
        let origin = ed
            .lines
            .get(ed.cy)
            .map(|l| drawn_width(&l.chars().take(ed.scroll_x).collect::<String>()))
            .unwrap_or(ed.scroll_x);
        let cur_x = body[1].x + col.saturating_sub(origin) as u16;
        let cur_y = body[1].y + (ed.cy - ed.scroll_y) as u16;
        frame.set_cursor_position((
            cur_x.min(body[1].width.saturating_sub(1) + body[1].x),
            cur_y,
        ));
    }
}

/// the editor as the user first sees it: the hub, on the project at the start.
/// the ide (explorer, buffer, terminal) waits behind it, one esc away.
pub(super) fn start_editor(path: &Option<PathBuf>) -> io::Result<Editor> {
    let (file, root) = startup_layout(path);
    let mut ed = Editor::open(file.as_deref())?;
    ed.explorer = Some(Explorer::new(root.clone()));
    // un seul modèle, toujours le même — pas de réglage, pas de dérive
    let ai = AiClient::from_env(&crate::config::load())
        .ok()
        .map(|mut c| {
            c.set_model(EDITOR_MODEL);
            c
        });
    ed.ai = ai;
    Ok(ed)
}

/// Lance l'éditeur. `path` : fichier à ouvrir/créer.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    // NO_COLOR est une convention pensée pour les logs, pas pour un éditeur :
    // posée dans l'environnement, elle fait disparaître TOUTE la coloration
    // (crossterm émet alors des séquences vides). Un éditeur de code sans
    // couleurs est un bug, pas une préférence — c-nano la retire de SON
    // processus (l'environnement du shell reste intact).
    std::env::remove_var("NO_COLOR");
    // IDE complet dès l'ouverture : explorateur + terminal toujours visibles,
    // le focus reste à l'éditeur (ou à l'accueil)
    let mut ed = start_editor(&path)?;

    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    // bracketed paste: the terminal wraps what you paste, so the editor can
    // take it as a block instead of mistaking it for typing
    let _ = crossterm::execute!(io::stdout(), crossterm::event::EnableBracketedPaste);
    // Protocole clavier « kitty » : foot l'implémente. Sans lui, foot envoie
    // Ctrl+Tab sous une forme que crossterm ne décode pas — la touche
    // disparaît avant nous. Inoffensif pour les terminaux qui l'ignorent.
    let kitty = kitty_keyboard();
    if kitty {
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::event::PushKeyboardEnhancementFlags(
                crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        );
    }
    let mut terminal =
        ratatui::DefaultTerminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    let result = loop_run(&mut terminal, &mut ed);
    if kitty {
        let _ = crossterm::execute!(io::stdout(), crossterm::event::PopKeyboardEnhancementFlags);
    }
    let _ = crossterm::execute!(io::stdout(), crossterm::event::DisableBracketedPaste);
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    result
}

/// Le terminal parle-t-il le protocole clavier « kitty » ? foot oui — c'est
/// ce qui rend Ctrl+Tab (et Alt+Tab) réellement distincts de Tab.
pub(super) fn kitty_keyboard() -> bool {
    matches!(
        std::env::var("TERM").as_deref(),
        Ok(t) if t.contains("foot")
            || t.contains("kitty")
            || t.contains("wezterm")
            || t.contains("ghostty")
    )
}

pub(super) fn loop_run(terminal: &mut ratatui::DefaultTerminal, ed: &mut Editor) -> io::Result<()> {
    while !ed.should_quit {
        ed.tick_status();
        ed.poll_agent();
        ed.play_live();
        ed.poll_ai();
        ed.poll_check();
        ed.poll_diag();
        ed.poll_header();
        ed.poll_consolidation();
        ed.poll_quiet(Instant::now());
        if let Some(term) = &mut ed.term {
            term.poll();
        }
        // le scroll se calcule sur la zone INTÉRIEURE de la boîte éditeur :
        // barres (2) + bordures (2) + terminal (TERM_H si ouvert) +
        // explorateur (EXPL_W + souffle) — sinon le curseur traverse les
        // bordures en bas de fichier
        // geometry of the editor box, exactly as draw() lays it out:
        //   top bar + blank + body + blank + status bar, then the box borders,
        //   minus the explorer column and its blank, minus the terminal card
        //   and its blank row.
        let size = terminal.size()?;
        let term_h = if ed.term.is_some() { TERM_H + 1 } else { 0 };
        let expl_w = if ed.explorer.is_some() { EXPL_W + 1 } else { 0 };
        let inner_h = size.height.saturating_sub(4 + 2 + term_h);
        let inner_w = size.width.saturating_sub(expl_w + 2);
        ed.keep_cursor_visible(inner_h as usize, inner_w as usize);
        terminal.draw(|frame| draw(frame, ed))?;
        // the frame rate follows the screen: ~60 fps while something moves,
        // a lazy 8 fps when everything is still. the loop must also wake up
        // to collect the ai/check/build answers arriving on worker threads.
        let timeout = if ed.animating() {
            std::time::Duration::from_millis(16)
        } else {
            std::time::Duration::from_millis(120)
        };
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        ed.on_key(key);
                    }
                }
                Event::Paste(text) => ed.paste(&text),
                _ => {}
            }
        }
    }
    Ok(())
}

/// the badge while the card is open follows what the agent plays: a shell
/// command is a terminal, a file is named by its language, anything else is
/// the agent itself. the rule lives here so the badge never guesses elsewhere.
fn agent_badge(ed: &Editor) -> (String, Color) {
    match ed.live.first() {
        Some(w) if crate::editor::is_action_view(&w.path) => {
            let name = w.path.to_string_lossy();
            if name.ends_with("run_shell") {
                ("terminal".to_string(), Ed::amber())
            } else {
                ("agent".to_string(), Ed::cyan())
            }
        }
        Some(w) => {
            let lang = crate::langs::for_path(&w.path);
            (lang.name.to_string(), Color::Indexed(lang.color))
        }
        None => ("agent".to_string(), Ed::cyan()),
    }
}
