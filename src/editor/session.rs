//! the session: the entry point, the terminal modes and the draw loop.

use crate::editor::*;

/// the editor as the user first sees it: the hub, on the project at the start.
/// the ide (explorer, buffer, terminal) waits behind it, one esc away.
pub(crate) fn start_editor(path: &Option<PathBuf>) -> io::Result<Editor> {
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
pub(in crate::editor) fn kitty_keyboard() -> bool {
    matches!(
        std::env::var("TERM").as_deref(),
        Ok(t) if t.contains("foot")
            || t.contains("kitty")
            || t.contains("wezterm")
            || t.contains("ghostty")
    )
}

/// Runs the draw loop until the editor quits: each frame advances the background pollers, then draws and reads input.
pub(in crate::editor) fn loop_run(
    terminal: &mut ratatui::DefaultTerminal,
    ed: &mut Editor,
) -> io::Result<()> {
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
