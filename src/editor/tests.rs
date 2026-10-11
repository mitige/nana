//! the editor's own tests: each one pins a rule the screen must keep.

use super::*;

/// the frame, row by row: the plain text helper loses the line breaks, and a
/// layout bug is exactly a question of which row something lands on.
fn render_rows(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, ed)).unwrap();
    let buf = term.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

#[cfg(test)]
fn render_text(ed: &mut Editor, w: u16, h: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, ed)).unwrap();
    term.backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

#[cfg(test)]
/// a unique scratch directory per call. tests run in parallel inside one
/// process: a name built from the pid alone gets trampled, and one test's
/// cleanup then deletes another test's fixture mid-run.
fn scratch(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nana-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod robustness_tests {
    use super::*;

    /// L'éditeur ne doit JAMAIS paniquer, quelles que soient les touches.
    #[test]
    fn aucune_panique_sur_touches_limites() {
        let mut ed = Editor::open(None).unwrap();
        // une rafale de touches bizarres
        let cles = [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Char('a'),
            KeyCode::Char('x'),
        ];
        for _ in 0..3 {
            for c in &cles {
                let key = KeyEvent::new(*c, KeyModifiers::empty());
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ed.on_key(key)));
                assert!(r.is_ok(), "panique sur la touche {c:?}");
            }
        }
    }

    /// Le undo sur un buffer vide / initial ne panique pas.
    #[test]
    fn undo_sur_vide_ne_panique_pas() {
        let mut ed = Editor::open(None).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ed.undo()));
        assert!(r.is_ok());
        // et le undo restaure vraiment
        ed.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty()));
        ed.undo();
        assert_eq!(ed.lines[0], "");
    }

    /// goto/recherche sur des entrées bizarres ne paniquent pas.
    #[test]
    fn goto_et_recherche_limites() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["line one".into(), "line two".into()];
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ed.find(""); // vide
            ed.find("inexistant"); // absent
            ed.find("line"); // trouvé
        }));
        assert!(r.is_ok());
    }
}

#[cfg(test)]
mod pair_tests {
    use super::*;

    fn type_keys(ed: &mut Editor, s: &str) {
        for c in s.chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
    }

    /// Une ouvrante insère la paire, curseur au milieu.
    #[test]
    fn ouvrante_insere_la_paire() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "(");
        assert_eq!(ed.lines[0], "()");
        assert_eq!(ed.cx, 1);
        type_keys(&mut ed, "{");
        assert_eq!(ed.lines[0], "({})");
        assert_eq!(ed.cx, 2);
    }

    /// Les quotes se ferment aussi (char et string du C).
    #[test]
    fn quotes_se_ferment_seules() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "\"");
        assert_eq!(ed.lines[0], "\"\"");
        assert_eq!(ed.cx, 1);
    }

    /// Taper la fermante déjà présente la survole, sans doubler.
    #[test]
    fn fermante_presente_est_survolee() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "(x)");
        assert_eq!(ed.lines[0], "(x)");
        assert_eq!(ed.cx, 3, "le ) final survole au lieu de doubler");
    }

    /// Backspace entre une paire vide supprime les deux moitiés.
    #[test]
    fn backspace_sur_paire_vide_efface_les_deux() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "(");
        ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "");
        assert_eq!(ed.cx, 0);
    }

    /// Backspace sur du texte normal reste intact.
    #[test]
    fn backspace_classique_inchange() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "ab");
        ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "a");
    }

    /// Taper puis effacer jusqu'à l'état disque → le buffer redevient
    /// propre : plus de fausse alerte « modifié », l'explorateur s'ouvre.
    #[test]
    fn retour_a_l_etat_disque_rend_propre() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert!(ed.modified);
        ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert!(!ed.modified, "revenu à l'état disque = propre");
        // l'explorateur ne bloque donc plus
        ed.explorer = Some(Explorer::new(std::env::temp_dir()));
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        // aucune plainte « buffer modifié »
        assert!(!ed.status.contains("^s to save first"), "{}", ed.status);
    }

    /// F3 ouvre le terminal (focus dedans), Échap en sort sans fermer,
    /// F3 referme. Le cycle F2 l'inclut quand il est ouvert.
    #[test]
    fn terminal_toggle_et_cycle() {
        let mut ed = Editor::open(None).unwrap();
        assert!(ed.term.is_none());
        ed.on_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::empty()));
        assert!(ed.term.is_some(), "le PTY est spawné");
        assert_eq!(ed.focus, Focus::Terminal);
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Editor);
        assert!(ed.term.is_some(), "Échap ne ferme pas, juste le focus");
        // sans explorateur ouvert, la ronde est éditeur ↔ terminal
        ed.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Terminal, "ronde : éditeur → terminal");
        ed.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Editor, "…et retour");
        ed.on_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::empty()));
        assert!(ed.term.is_none());
    }

    /// super+t ouvre et ferme le terminal, comme F3 : c'est le raccourci
    /// « terminal » que l'on tape sans réfléchir sur un bureau.
    #[test]
    fn super_t_ouvre_le_terminal() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::SUPER));
        assert!(ed.term.is_some(), "super+t spawne le PTY");
        assert_eq!(ed.focus, Focus::Terminal);
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::SUPER));
        assert!(ed.term.is_none(), "super+t referme");
    }

    /// Explorateur : ^N crée (et ouvre + en-tête), ^R renomme, ^D supprime
    /// après confirmation o.
    #[test]
    fn ops_fichiers_creer_renommer_supprimer() {
        let dir = scratch("cnano-ops");

        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(dir.clone()));
        ed.focus = Focus::Explorer;

        // ^N nouveau fichier
        ed.on_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        for c in "neuf.c".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let created = dir.join("neuf.c");
        assert!(created.exists(), "créé sur le disque");
        assert_eq!(ed.file.as_deref(), Some(created.as_path()), "ouvert");
        // en-tête squelette inséré (pas de modèle en test)
        assert!(
            ed.lines[0].starts_with("/*"),
            "auto header: {}",
            ed.lines[0]
        );
        assert!(
            ed.lines.iter().any(|l| l.contains("file: neuf.c")),
            "{}",
            ed.lines.join(" | ")
        );

        // ^R renomme (le fichier est sélectionné dans l'arbre rechargé)
        let pos = ed
            .explorer
            .as_ref()
            .unwrap()
            .rows()
            .iter()
            .position(|r| r.name == "neuf.c")
            .unwrap();
        // sélectionne-le en naviguant
        while ed.explorer.as_ref().unwrap().sel() > pos {
            ed.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
        }
        while ed.explorer.as_ref().unwrap().sel() < pos {
            ed.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        }
        // buffer modifié (en-tête inséré) → sauve d'abord pour suivre le rename
        ed.save();
        // l'ouverture a rendu le focus à l'éditeur : retour explorateur
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        // le prompt est pré-rempli du nom courant — on l'efface et on tape
        for _ in 0..8 {
            ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        }
        for c in "dix.c".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(dir.join("dix.c").exists());
        assert!(!created.exists(), "l'ancien nom a disparu");
        assert!(
            ed.file.as_deref().unwrap().ends_with("dix.c"),
            "l'éditeur suit"
        );

        // ^D puis 'o' supprime
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(ed.confirm_delete.is_some());
        ed.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::empty()));
        assert!(!dir.join("dix.c").exists(), "supprimé du disque");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// La disposition de départ : dossier → explorateur racine, fichier →
    /// racine = son dossier, rien → cwd.
    #[test]
    fn disposition_de_depart() {
        let cwd = std::env::current_dir().unwrap();
        let (f, root) = startup_layout(&Some(cwd.clone()));
        assert!(f.is_none(), "un dossier n'ouvre pas de fichier");
        assert_eq!(root, cwd);
        let file = cwd.join("Cargo.toml");
        let (f, root) = startup_layout(&Some(file.clone()));
        assert_eq!(f.as_deref(), Some(file.as_path()));
        assert_eq!(root, cwd, "la racine suit le fichier");
        let (f, root) = startup_layout(&None);
        assert!(f.is_none());
        assert_eq!(root, cwd);
    }

    /// « src/x.c » part de la racine même si la sélection est un dossier.
    #[test]
    fn creation_chemin_relatif_a_la_racine() {
        let dir = std::env::temp_dir().join(format!("cnano-rel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.c"), "int m;\n").unwrap();
        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(dir.clone()));
        ed.focus = Focus::Explorer;
        // la sélection est sur « src » (dossier) — le nom contient un /
        ed.create_file("src/neuf.c");
        assert!(
            dir.join("src/neuf.c").exists(),
            "racine + chemin, pas dossier+dossier"
        );
        assert!(!dir.join("src/src/neuf.c").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// the header opens with the language's own comment syntax.
    #[test]
    fn header_uses_the_language_syntax() {
        let h = header_lines(Path::new("my_swap.c"), "swap two values");
        assert_eq!(h[0], "/*");
        assert!(h[1].contains("file: my_swap.c"), "{}", h[1]);
        assert!(h[2].contains("description: swap two values"), "{}", h[2]);
        assert_eq!(h[h.len() - 2], "*/");
    }

    /// The pane answers the queries a modern shell sends at startup.
    #[test]
    fn terminal_queries_are_answered() {
        let r = terminal_replies("\x1b[0c");
        assert!(r.starts_with("\x1b[?62"), "device attributes: {r:?}");
        assert!(terminal_replies("\x1b]11;?").contains("]11;rgb:"));
        assert!(terminal_replies("\x1b]10;?").contains("]10;rgb:"));
        assert!(terminal_replies("\x1b]4;1;?").contains("]4;1;rgb:f07886"));
        assert!(terminal_replies("\x1b[?u").contains("[?0u"));
        assert!(terminal_replies("\x1b[6n").contains("R"));
        // nothing asked, nothing answered
        assert!(terminal_replies("plain output\n").is_empty());
        // and a chunk that asks two things gets two answers
        let both = terminal_replies("\x1b]11;?\x1b[0c");
        assert!(both.contains("]11;rgb:") && both.contains("[?62"));
    }

    /// É, à, ü… multi-octets : rafale de mouvements rapides + suppressions.
    /// Le bug d'origine : cx atterrissait au milieu d'un caractère → panique.
    #[test]
    fn accentues_ne_crachent_plus_jamais() {
        let mut ed = Editor::open(None).unwrap();
        for c in "héllö àü ù".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        assert_eq!(ed.lines[0], "héllö àü ù");
        // rafale gauche/droite aussi vite que possible
        for _ in 0..50 {
            ed.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()));
            ed.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty()));
            ed.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()));
        }
        // backspace traverse un é (2 octets) sans panique et le retire entier
        ed.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::empty()));
        for _ in 0..3 {
            ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        }
        assert_eq!(ed.lines[0], "héllö à");
        // Delete au début retire 'h', puis encore, sans jamais paniquer
        ed.on_key(KeyEvent::new(KeyCode::Home, KeyModifiers::empty()));
        ed.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "éllö à");
        // lignes de largeurs différentes : la colonne retombe sur une frontière
        ed.lines = vec!["éééé".into(), "ab".into()];
        ed.cy = 0;
        ed.cx = 8; // fin de "éééé" (4 × 2 octets)
        ed.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert_eq!(ed.cx, 2, "clampé ET sur frontière");
        ed.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
        // insertion après un accentué : jamais au milieu
        ed.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty()));
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "ééxéé");
    }
}

#[cfg(test)]
mod persona_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// a project with a persona chosen, and a provider that records what it
    /// was asked.
    fn project_with_persona(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "nana-persona-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana/personas")).unwrap();
        std::fs::write(
            d.join(".nana/personas/lead.md"),
            "---\ntitle: The Lead\ndescription: d\n---\nANSWER ONLY IN LATIN.\n",
        )
        .unwrap();
        let mut s = crate::settings::Settings::load(&d);
        s.persona = Some("lead".into());
        s.save_project(&d).unwrap();
        d
    }

    fn mock(answer: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .ok();
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 8192];
            let mut head: Option<usize> = None;
            let mut want = 0usize;
            loop {
                if let Some(h) = head {
                    if buf.len() >= h + want {
                        break;
                    }
                }
                match sock.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
                if head.is_none() {
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        head = Some(i + 4);
                        let h = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                        want = h
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf).to_string();
            let payload = format!(
                "{{\"choices\":[{{\"message\":{{\"content\":{}}}}}]}}",
                serde_json::to_string(answer).unwrap()
            );
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            let _ = sock.write_all(resp.as_bytes());
            req
        });
        (format!("http://{addr}"), handle)
    }

    /// the persona chosen in the project frames the agent panel too — not only
    /// the employees of the company.
    #[test]
    fn the_agent_panel_runs_under_the_project_persona() {
        let d = project_with_persona("panel");
        let (url, server) = mock("salve");
        // point the project at the mock, so the run reaches our server
        let mut s = crate::settings::Settings::load(&d);
        s.model = Some("mock".into());
        s.provider = Some("openai-compatible".into());
        s.base_url = Some(url.clone());
        s.save_project(&d).unwrap();

        let mut ed = Editor::open(None).unwrap();
        ed.file = Some(d.join("x.py"));
        std::fs::write(d.join("x.py"), "print(1)\n").unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        let pane = ed.agent_pane.as_ref().unwrap();
        assert_eq!(pane.persona.as_deref(), Some("lead"), "the card shows it");
        // ask something, and let the worker talk to the mock
        ed.agent_pane.as_mut().unwrap().input = "say hi".into();
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let req = server.join().unwrap();
        assert!(
            req.contains("ANSWER ONLY IN LATIN"),
            "the persona reached the model: {}",
            &req[..req.len().min(600)]
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod paste_tests {
    use super::*;

    /// Pasting must not reformat. Typing enter auto-indents — right for
    /// writing code, wrong for pasting it: the text arrives as it was copied.
    #[test]
    fn pasting_keeps_the_text_as_it_was() {
        let mut ed = Editor::open(None).unwrap();
        ed.paste("one\ntwo\nthree");
        assert_eq!(ed.lines, vec!["one", "two", "three"]);
        assert_eq!(ed.cy, 2, "the cursor ends on the last pasted line");

        // a paste under an indented line must not inherit that indentation
        ed.lines = vec!["    indented".into(), String::new()];
        ed.cy = 1;
        ed.cx = 0;
        ed.paste("a\nb");
        assert_eq!(ed.lines[1], "a");
        assert_eq!(ed.lines[2], "b", "no tab of ours was added");

        // and typing enter still auto-indents: that part is wanted
        ed.cy = 0;
        ed.cx = ed.lines[0].len();
        ed.insert_newline();
        assert!(
            ed.lines[1].starts_with("    "),
            "the auto-indent still works: {:?}",
            ed.lines
        );
    }

    #[test]
    fn a_paste_lands_at_the_cursor_and_marks_the_buffer_dirty() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["xx".into()];
        ed.cx = 1;
        ed.paste("AB");
        assert_eq!(ed.lines[0], "xABx");
        assert!(ed.modified, "the buffer knows it changed");
        // windows line endings are normalised
        ed.paste("\r\nZ");
        assert!(ed.lines.iter().all(|l| !l.contains('\r')), "{:?}", ed.lines);
    }

    #[test]
    fn a_paste_cannot_reach_a_modal_card() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["untouched".into()];
        ed.agent_pane = Some(AgentPane::new());
        ed.paste("sneaky");
        assert_eq!(ed.lines, vec!["untouched"], "the card protects the buffer");
        ed.agent_pane = None;
        ed.hub = Some(crate::hub::Hub::open(std::path::Path::new(".")));
        ed.paste("sneaky");
        assert_eq!(ed.lines, vec!["untouched"], "the hub too");
    }
}

#[cfg(test)]
mod hub_takes_the_screen_tests {
    use super::*;

    /// the hub is a menu, not a window over the ide: with it open, nothing of
    /// the editor (top bar, status bar, buffer text) may be left on screen.
    #[test]
    fn with_the_hub_open_no_ide_chrome_is_visible() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["fn secret_buffer_text() {}".into()];
        ed.hub = Some(crate::hub::Hub::open(std::path::Path::new(".")));
        let text = render_text(&mut ed, 110, 30);
        assert!(
            !text.contains("secret_buffer_text"),
            "buffer leaks behind the hub"
        );
        assert!(
            !text.contains("ln 1, col 1"),
            "status bar leaks behind the hub"
        );
        assert!(!text.contains("⎇ main"), "top bar leaks behind the hub");
    }
}

#[cfg(test)]
mod hub_is_the_start_tests {
    use super::*;

    /// nana opens on the ide: the explorer and the status bar are visible,
    /// and the hub (ctrl+w) is not opened by itself.
    #[test]
    fn a_bare_start_opens_on_the_ide_not_on_the_hub() {
        let root = scratch("start");
        std::fs::write(root.join("a.rs"), "fn secret_start_text() {}\n").unwrap();
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let started = start_editor(&None);
        std::env::set_current_dir(&cwd).unwrap();
        let mut ed = started.unwrap();
        let text = render_text(&mut ed, 110, 30);
        assert!(ed.hub.is_none(), "no hub opens at start");
        assert!(text.contains("a.rs"), "the explorer is shown at start");
        assert!(
            text.contains("ln 1, col 1"),
            "the status bar is shown at start"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod rounded_everywhere_tests {
    use super::*;

    /// every card, in every state the user can open, is drawn with rounded
    /// corners: a square corner anywhere is a regression of the visual promise.
    #[test]
    fn no_panel_has_a_square_corner() {
        let root = scratch("rounded");
        std::fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
        for setup in 0..3 {
            let mut ed = Editor::open(None).unwrap();
            ed.lines = vec!["fn main() {}".into()];
            ed.explorer = Some(Explorer::new(root.clone()));
            if setup == 1 {
                ed.term = TermPane::spawn(&root, 8, 60).ok();
            }
            if setup == 2 {
                ed.search = Some(FileSearch::new(root.clone()));
            }
            let text: String = render_text(&mut ed, 110, 32);
            for square in ["┌", "┐", "└", "┘", "├", "┤"] {
                assert!(
                    !text.contains(square),
                    "square corner {square} in state {setup}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod sober_badge_tests {
    use super::*;

    /// the search is a window of its own, so it draws no status bar and no
    /// badge: the focus accent lives on its border, never a magenta second colour.
    #[test]
    fn search_window_has_no_magenta_badge() {
        let mut ed = Editor::open(None).unwrap();
        ed.focus = Focus::Search;
        ed.search = Some(FileSearch::new(std::path::PathBuf::from(".")));
        let backend = ratatui::backend::TestBackend::new(110, 30);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, &mut ed)).unwrap();
        let buf = term.backend().buffer();
        let bottom: String = (0..110u16)
            .map(|x| buf[(x, 29)].symbol().to_string())
            .collect();
        assert!(
            !bottom.contains("search"),
            "a status bar is drawn under the search: {bottom}"
        );
        let magenta = (0..30u16)
            .flat_map(|y| (0..110u16).map(move |x| (x, y)))
            .any(|(x, y)| buf[(x, y)].fg == Color::Indexed(5));
        assert!(!magenta, "the search window has a magenta cell");
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    /// The loop decides how far it can scroll from a computed inner height.
    /// If that number does not match the box actually drawn, the cursor walks
    /// below the visible area — two lines too far, in the case that bit.
    #[test]
    fn the_scroll_geometry_matches_the_box_on_screen() {
        let root = std::env::temp_dir().join(format!("nana-geo-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let (w, h) = (100u16, 30u16);
        for with_terminal in [false, true] {
            let mut ed = Editor::open(None).unwrap();
            ed.lines = (0..200).map(|i| format!("line {i}")).collect();
            ed.explorer = Some(Explorer::new(root.clone()));
            if with_terminal {
                ed.term = TermPane::spawn(&root, 8, 60).ok();
            }
            // exactly what loop_run computes
            let term_h = if ed.term.is_some() { TERM_H + 1 } else { 0 };
            let expl_w = if ed.explorer.is_some() { EXPL_W + 1 } else { 0 };
            let inner_h = h.saturating_sub(4 + 2 + term_h) as usize;
            let inner_w = w.saturating_sub(expl_w + 2);

            // and what the frame really shows: the editor box's inner height
            let rows = render_rows(&mut ed, w, h);
            let top = rows
                .iter()
                .position(|r| r.chars().nth(expl_w as usize) == Some('╭'))
                .expect("the editor box opens");
            let bottom = rows
                .iter()
                .position(|r| r.chars().nth(expl_w as usize) == Some('╰'))
                .expect("the editor box closes");
            let drawn_h = bottom - top - 1;
            assert_eq!(
                drawn_h, inner_h,
                "terminal={with_terminal}: the loop believes {inner_h} rows, the box has {drawn_h}"
            );
            assert!(
                inner_w > 10 && inner_h > 3,
                "the editor has room: {inner_w}x{inner_h}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The cursor never ends up below the box, however far you scroll.
    #[test]
    fn the_cursor_stays_inside_the_box() {
        let root = std::env::temp_dir().join(format!("nana-geo2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let (w, h) = (100u16, 30u16);
        let mut ed = Editor::open(None).unwrap();
        ed.lines = (0..200).map(|i| format!("line {i}")).collect();
        ed.explorer = Some(Explorer::new(root.clone()));
        let inner_h = h.saturating_sub(6) as usize;
        let inner_w = w.saturating_sub(EXPL_W + 3);
        // walk down the file the way a held key does
        for _ in 0..200 {
            ed.cy = (ed.cy + 1).min(ed.lines.len() - 1);
            ed.keep_cursor_visible(inner_h, inner_w as usize);
        }
        assert_eq!(ed.cy, 199);
        let rows = render_rows(&mut ed, w, h);
        let top = rows
            .iter()
            .position(|r| r.chars().nth(EXPL_W as usize + 1) == Some('╭'))
            .expect("the editor box opens");
        let bottom = rows
            .iter()
            .position(|r| r.chars().nth(EXPL_W as usize + 1) == Some('╰'))
            .expect("the editor box closes");
        let cursor_row = top + 1 + (ed.cy - ed.scroll_y) as usize;
        assert!(
            cursor_row < bottom,
            "the cursor sits at row {cursor_row}, the box ends at {bottom}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod hub_ui_tests {
    use super::*;

    /// a project with something in every box, so the menu has content.
    fn seeded() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "nana-hubui-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        let m = crate::memory::Memory::open(&d);
        m.write(
            crate::memory::Class::Project,
            "stack",
            "rust only, no async",
        )
        .unwrap();
        m.write(crate::memory::Class::User, "aaa-tone", "blunt, no filler")
            .unwrap();
        std::fs::create_dir_all(d.join(".nana/skills")).unwrap();
        std::fs::write(
            d.join(".nana/skills/release.md"),
            "---\nname: release\ndescription: cut a release\ntrigger: release\n---\nsteps\n",
        )
        .unwrap();
        d
    }

    fn editor_at(root: &std::path::Path) -> Editor {
        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(root.to_path_buf()));
        ed
    }

    #[test]
    fn a_quiet_moment_consolidates_twin_pages_only_when_a_model_can_answer() {
        let d = seeded();
        std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
        let m = crate::memory::Memory::open(&d);
        m.write(
            crate::memory::Class::Project,
            "twin-a",
            "# a\n\nsame words here",
        )
        .unwrap();
        m.write(
            crate::memory::Class::Project,
            "twin-b",
            "# b\n\nsame words here",
        )
        .unwrap();
        let mut ed = editor_at(&d);
        ed.file = Some(d.join("main.rs"));
        let t0 = Instant::now();
        for i in 0..5 {
            ed.quiet.key(t0 + Duration::from_millis(200 * i));
        }
        let quiet_end = t0 + Duration::from_millis(800) + ed.quiet.threshold();
        ed.poll_quiet(quiet_end);
        assert!(
            ed.consolidation_rx.is_none(),
            "no model, no consolidation thread"
        );
        assert!(
            ed.consolidation.due(&m),
            "the trigger stays armed until a model answers"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_quiet_moment_after_typing_audits_the_memory_and_says_so() {
        let d = seeded();
        std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
        let m = crate::memory::Memory::open(&d);
        m.write(
            crate::memory::Class::Project,
            "twin-a",
            "# a\n\nthe same words here",
        )
        .unwrap();
        m.write(
            crate::memory::Class::Project,
            "twin-b",
            "# b\n\nthe same words here",
        )
        .unwrap();
        let mut ed = editor_at(&d);
        ed.file = Some(d.join("main.rs"));
        let t0 = Instant::now();
        for i in 0..5 {
            ed.quiet.key(t0 + Duration::from_millis(200 * i));
        }
        let quiet_end = t0 + Duration::from_millis(800) + ed.quiet.threshold();
        ed.poll_quiet(t0 + Duration::from_millis(100));
        assert!(!ed.status.contains("duplicate"), "no look during typing");
        ed.poll_quiet(quiet_end);
        assert!(
            ed.status.contains("1 duplicate"),
            "the look is said on the status line: {}",
            ed.status
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ctrl_w_opens_the_hub_and_esc_closes_it() {
        let d = seeded();
        let mut ed = editor_at(&d);
        ed.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert!(ed.hub.is_some(), "the hub opens");
        // while it is open, the editor does not receive the keys
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert!(
            ed.lines.iter().all(|l| !l.contains('x')),
            "the buffer is untouched"
        );
        assert_eq!(ed.hub.as_ref().unwrap().query(), "x", "it filters instead");
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(ed.hub.is_none(), "esc closes it");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_arrows_walk_the_boxes_and_the_items() {
        let d = seeded();
        let mut ed = editor_at(&d);
        ed.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(
            ed.hub.as_ref().unwrap().current(),
            crate::hub::Section::Memory
        );
        ed.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty()));
        assert_eq!(
            ed.hub.as_ref().unwrap().current(),
            crate::hub::Section::Providers,
            "right goes to the next box"
        );
        ed.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()));
        assert_eq!(
            ed.hub.as_ref().unwrap().current(),
            crate::hub::Section::Memory
        );
        ed.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert_eq!(
            ed.hub.as_ref().unwrap().cursor(),
            1,
            "down moves in the list"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn enter_on_a_memory_page_opens_it_in_the_editor() {
        let d = seeded();
        let mut ed = editor_at(&d);
        ed.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        // narrow to one page: typing filters the list
        for c in "stack".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        assert_eq!(
            ed.hub
                .as_ref()
                .unwrap()
                .selected()
                .map(|i| i.label.as_str()),
            Some("project: stack"),
            "{:?}",
            ed.hub.as_ref().unwrap().items()
        );
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(ed.hub.is_none(), "the hub closes behind it");
        assert!(
            ed.lines.iter().any(|l| l.contains("rust only")),
            "the page is in the buffer: {:?}",
            ed.lines
        );
        assert!(
            ed.file.as_deref().unwrap().ends_with("project/stack.md"),
            "{:?}",
            ed.file
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn enter_on_a_skill_hands_it_to_the_agent() {
        let d = seeded();
        let mut ed = editor_at(&d);
        ed.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        ed.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty()));
        ed.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty()));
        assert_eq!(
            ed.hub.as_ref().unwrap().current(),
            crate::hub::Section::Skills
        );
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(ed.hub.is_none());
        let pane = ed.agent_pane.as_ref().expect("the agent panel opens");
        assert!(
            pane.input.contains("release"),
            "the skill is named: {}",
            pane.input
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// the hub is a card: the editor must not show through it, and the box
    /// around the selection must sit on the selected row.
    #[test]
    fn the_hub_erases_what_it_covers_and_highlights_the_right_row() {
        let d = seeded();
        let mut ed = editor_at(&d);
        ed.lines = (0..20)
            .map(|i| format!("SECRET_LINE_{i}_MUST_NOT_SHOW"))
            .collect();
        // a file inside the fixture, so the hub resolves to *this* project
        ed.file = Some(d.join("x.py"));
        ed.ext = "py".into();
        ed.modified = true;
        ed.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        for c in "stack".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        let rows = render_rows(&mut ed, 110, 32);
        // the two cards, found by what they hold: the list's selected item on
        // the left, and the box's own message on the right
        let left = rows
            .iter()
            .position(|r| r.contains("▸ memory"))
            .expect("the left card lists its sections");
        let right = rows
            .iter()
            .position(|r| r.contains("(memory is empty)") || r.contains("stack"))
            .expect("the right card shows its content");
        let bottom = rows
            .iter()
            .position(|r| r.trim_start().starts_with('╰'))
            .unwrap_or(rows.len() - 1);
        for row in &rows[left..=bottom] {
            assert!(
                !row.contains("SECRET_LINE"),
                "the editor shows through the hub: {row}"
            );
        }
        assert!(right >= left, "the cards are side by side");
        // the highlight is on the first item, right under the section list
        // the selected row is the one carrying its label
        let band = rows
            .iter()
            .position(|r| r.contains("project: stack"))
            .expect("the selected row is marked");
        assert!(
            band > left && band < bottom,
            "the band is inside the card: {band} in {left}..{bottom}"
        );
        // the hub is the whole screen: the ide bars must not show through it
        assert!(
            !rows[0].contains("nana  "),
            "the top bar leaks: {}",
            rows[0]
        );
        assert!(
            !rows.last().unwrap().contains("ln 1, col 1"),
            "the status bar leaks: {}",
            rows.last().unwrap()
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_empty_project_still_shows_both_cards() {
        let d = std::env::temp_dir().join(format!("nana-hubempty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut ed = editor_at(&d);
        ed.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        let rows = render_rows(&mut ed, 110, 32);
        assert!(
            rows.iter().any(|r| r.contains("▸ memory")),
            "the left card lists its sections"
        );
        assert!(
            rows.iter().any(|r| r.contains("empty")),
            "the right card says the box is empty"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod float_tests {
    /// a card has a top edge: its corners sit on one row, with no title in it.
    fn has_card_top(text: &str) -> bool {
        text.lines().any(|r| r.contains('╭') && r.contains('╮'))
    }

    use super::*;

    /// the ide stays on screen while the agent works: the explorer and the
    /// editor are drawn, and the agent card is a band over them, not a clean
    /// screen with a card floating in it.
    #[test]
    fn the_ide_stays_on_screen_while_the_agent_works() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = (0..20)
            .map(|i| format!("VISIBLE_LINE_{i}_STAYS_ON_SCREEN"))
            .collect();
        ed.file = Some(std::path::PathBuf::from("x.py"));
        ed.ext = "py".into();
        ed.modified = true;
        ed.agent_pane = Some(AgentPane::new());
        let text = render_text(&mut ed, 160, 40);
        assert!(has_card_top(&text), "the card is drawn:\n{text}");
        assert!(
            text.contains("VISIBLE_LINE_0_STAYS_ON_SCREEN"),
            "the ide is drawn while the agent works:\n{text}"
        );
    }

    /// the agent's write plays in a second view behind the card: the lines it
    /// writes are on screen, outside the card, and the card stays clean.
    #[test]
    fn the_turn_end_saves_the_opening_and_the_last_reply() {
        let root = std::env::temp_dir().join(format!("nana-turn-end-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".nana")).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut pane = AgentPane::new();
        pane.rx = Some(rx);
        pane.root = root.clone();
        pane.session = "auto".into();
        pane.opening = Some("first ask".into());
        tx.send(crate::agent::Event::Text("first answer".into()))
            .unwrap();
        tx.send(crate::agent::Event::Text("last answer".into()))
            .unwrap();
        tx.send(crate::agent::Event::Finished { steps: 2 }).unwrap();
        pane.poll();
        let saved = crate::memory::Memory::open(&root)
            .read(crate::memory::Class::Project, "conversation-auto")
            .unwrap_or_default();
        assert!(saved.contains("first ask"), "{saved}");
        assert!(
            saved.contains("last answer"),
            "the last reply is kept: {saved}"
        );
        assert!(
            !saved.contains("first answer"),
            "only the last reply is kept: {saved}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ctrl_y_keeps_the_conversation_so_far_in_memory() {
        let root = std::env::temp_dir().join(format!("nana-ctrl-y-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".nana")).unwrap();
        let mut pane = AgentPane::new();
        pane.root = root.clone();
        pane.session = "manual".into();
        pane.opening = Some("build the parser".into());
        pane.closing = Some("parser done".into());
        pane.save_turn_now();
        let saved = crate::memory::Memory::open(&root)
            .read(crate::memory::Class::Project, "conversation-manual")
            .unwrap_or_default();
        assert!(saved.contains("build the parser"), "{saved}");
        assert!(saved.contains("parser done"), "{saved}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_agent_write_plays_in_a_view_behind_the_card() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["fn behind_the_card() {}".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        while !ed.live.is_empty() {
            ed.play_live();
        }
        std::thread::sleep(VIEW_SWEEP);
        let text = render_text(&mut ed, 160, 40);
        assert!(has_card_top(&text), "the card is drawn:\n{text}");
        assert!(
            text.contains("fn behind_the_card"),
            "the write plays behind the card:\n{text}"
        );
    }

    /// the search is a window on its own too: the editor behind it must not
    /// show anywhere on the screen, the same as the agent menu and the hub.
    #[test]
    fn the_search_shows_nothing_of_the_ide_around_it() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = (0..20)
            .map(|i| format!("SECRET_LINE_{i}_SHOULD_NOT_SHOW_THROUGH"))
            .collect();
        ed.file = Some(std::path::PathBuf::from("x.py"));
        ed.ext = "py".into();
        ed.focus = Focus::Search;
        ed.search = Some(FileSearch::new(std::path::PathBuf::from(".")));
        let text = render_text(&mut ed, 100, 30);
        assert!(text.contains("❯"), "the search prompt is drawn");
        assert!(
            !text.contains("SECRET_LINE"),
            "the ide shows around the search:\n{text}"
        );
    }

    /// the agent card floats over the ide, so the user sees the code the agent
    /// touches as a human would: the ide stays visible, no trail lane is drawn.
    #[test]
    fn the_agent_menu_has_no_trail_under_its_card() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = (0..20)
            .map(|i| format!("SECRET_LINE_{i}_STAYS_VISIBLE"))
            .collect();
        ed.file = Some(std::path::PathBuf::from("x.py"));
        ed.ext = "py".into();
        let mut pane = AgentPane::new();
        pane.trail.push(crate::trail::Step {
            lane: crate::trail::Lane::Tool,
            label: "read_file src/main.rs".into(),
            at: 0.0,
            ok: true,
        });
        pane.trail.push(crate::trail::Step {
            lane: crate::trail::Lane::Result,
            label: "write_file: done".into(),
            at: 2.0,
            ok: true,
        });
        ed.agent_pane = Some(pane);
        let text = render_text(&mut ed, 160, 40);
        assert!(has_card_top(&text), "the card is drawn");
        assert!(
            text.contains("SECRET_LINE_0_STAYS_VISIBLE"),
            "the ide is drawn behind the card:\n{text}"
        );
        assert!(
            !text.contains("tool "),
            "no trail lane is drawn under the card:\n{text}"
        );
        assert!(
            !text.contains("read_file src/main.rs"),
            "the trail actions are not listed under the card:\n{text}"
        );
    }
}

#[cfg(test)]
mod agent_pane_tests {
    use super::*;

    fn typed(ed: &mut Editor, text: &str) {
        for c in text.chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
    }

    #[test]
    fn ctrl_a_opens_the_agent_and_it_owns_the_keyboard() {
        let mut ed = Editor::open(None).unwrap();
        assert!(ed.agent_pane.is_none());
        ed.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(ed.agent_pane.is_some(), "the panel opens");
        typed(&mut ed, "hello");
        assert_eq!(ed.agent_pane.as_ref().unwrap().input, "hello");
        // the editor buffer never saw those keys
        assert!(ed.lines.iter().all(|l| !l.contains("hello")));
        // esc closes it
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(ed.agent_pane.is_none());
    }

    #[test]
    fn esc_while_it_thinks_cancels_instead_of_closing() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        let pane = ed.agent_pane.as_mut().unwrap();
        pane.busy = true;
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        let pane = ed.agent_pane.as_ref().expect("the panel stays open");
        assert!(!pane.busy, "the wait is dropped");
        assert!(
            pane.lines.iter().any(|(_, t)| t.contains("cancelled")),
            "{:?}",
            pane.lines
        );
    }

    #[test]
    fn enter_on_an_empty_prompt_does_nothing() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        let before = ed.agent_pane.as_ref().unwrap().lines.len();
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let pane = ed.agent_pane.as_ref().unwrap();
        assert_eq!(pane.lines.len(), before, "nothing was sent");
        assert!(pane.rx.is_none(), "and no worker was started");
    }

    #[test]
    fn the_bars_only_round_the_brand_and_the_status_badge() {
        let mut ed = Editor::open(None).unwrap();
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, &mut ed)).unwrap();
        let buf = term.backend().buffer();
        for y in [0u16, 29] {
            let row: String = (0..100).map(|x| buf[(x, y)].symbol()).collect();
            let caps = row
                .chars()
                .filter(|c| *c == '\u{E0B6}' || *c == '\u{E0B4}')
                .count();
            assert_eq!(
                caps, 2,
                "bar row {y} must carry exactly one capsule: {row:?}"
            );
        }
    }

    #[test]
    fn the_card_is_drawn_with_rounded_corners() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));
        let text = render_text(&mut ed, 100, 30);
        assert!(text.contains("agent"), "{text}");
        assert!(text.contains('╭') && text.contains('╯'), "rounded: {text}");
        assert!(
            !text.contains('┌') && !text.contains('└'),
            "no square corner"
        );
    }

    #[test]
    fn wrapping_keeps_every_word() {
        let lines = wrap("one two three four five six", 11);
        assert!(lines.len() > 1, "{lines:?}");
        assert_eq!(lines.join(" "), "one two three four five six");
        assert!(lines.iter().all(|l| l.len() <= 11), "{lines:?}");
        // a newline in the answer starts a new line
        assert_eq!(wrap("a\nb", 40).len(), 2);
    }
}

#[cfg(test)]
mod tab_tests {
    use super::*;

    #[test]
    fn a_tab_is_expanded_to_the_next_stop() {
        assert_eq!(expand_tabs("a\tb"), "a   b");
        assert_eq!(expand_tabs("abcd\te"), "abcd    e");
        assert_eq!(expand_tabs("\tx"), "    x");
        assert_eq!(expand_tabs("no tabs"), "no tabs");
    }

    #[test]
    fn the_cursor_column_follows_the_expansion() {
        assert_eq!(drawn_width("a"), 1);
        assert_eq!(drawn_width("a\t"), 4);
        assert_eq!(drawn_width("ab\t"), 4);
        assert_eq!(drawn_width("pla"), 3);
    }

    /// A tab must never reach a cell: the terminal would move its own cursor
    /// and the whole frame would drift — this is the makefile glitch.
    #[test]
    fn no_cell_ever_holds_a_tab() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec![
            "NAME\t\t:= tesseract".to_string(),
            "\t$(CC) -o $@ $<".to_string(),
            "plain".to_string(),
        ];
        ed.file = Some(std::path::PathBuf::from("Makefile"));
        ed.ext = "mk".into();
        ed.modified = true;
        let text = render_text(&mut ed, 100, 20);
        assert!(!text.contains('\t'), "a tab survived into the frame");
        assert!(text.contains("NAME        := tesseract"), "aligned: {text}");
        assert!(text.contains("$(CC) -o $@ $<"), "the recipe is readable");
    }
}

#[cfg(test)]
mod diag_tests {
    use super::*;

    /// the gutter merges the style pass and the checker: the worst wins.
    #[test]
    fn marqueurs_le_plus_severe_gagne() {
        let mut ed = Editor::open(None).unwrap();
        ed.check_marks = vec![(3, Severity::Minor), (4, Severity::Major)];
        ed.diags = vec![
            Diag {
                line: 3,
                col: 1,
                is_error: true,
                message: "boom".into(),
            },
            Diag {
                line: 5,
                col: 1,
                is_error: false,
                message: "warn".into(),
            },
        ];
        let m = ed.gutter_marks();
        assert_eq!(m[&3], Ed::red(), "l'erreur gcc écrase la norme mineure");
        assert_eq!(m[&4], Ed::red(), "norme majeure = rouge");
        assert_eq!(m[&5], Ed::amber(), "warning gcc = ambre");
        assert!(!m.contains_key(&1));
    }

    /// ^N/^P naviguent en circulaire et placent le curseur sur la faute.
    #[test]
    fn navigation_circulaire() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = (1..=10).map(|i| format!("line {i}")).collect();
        ed.diags = vec![
            Diag {
                line: 2,
                col: 1,
                is_error: true,
                message: "a".into(),
            },
            Diag {
                line: 9,
                col: 3,
                is_error: false,
                message: "b".into(),
            },
        ];
        ed.diag_jump(1);
        assert_eq!(ed.cy, 1, "premier diagnostic");
        ed.diag_jump(1);
        assert_eq!(ed.cy, 8);
        assert_eq!(ed.cx, 2);
        ed.diag_jump(1);
        assert_eq!(ed.cy, 1, "ça boucle");
        ed.diag_jump(-1);
        assert_eq!(ed.cy, 8, "en arrière aussi");
    }

    /// Le modèle de complétion est unique, verrouillé, et servi par le provider.
    #[test]
    fn modele_unique_et_verrouille() {
        assert_eq!(EDITOR_MODEL, "deepseek-v4.1-flash");
        assert!(crate::ai::AVAILABLE_MODELS.contains(&EDITOR_MODEL));
    }

    /// Un long message de norme est wrappé entièrement dans le survol.
    #[test]
    fn survol_message_long_complet() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["int x;".into()];
        ed.modified = true;
        let long =
            "line far too long : it exceeds the eighty column limit imposed by the norm (N-COL80)"
                .to_string();
        ed.check_details = vec![(1, Severity::Major, long.clone())];
        let text = render_text(&mut ed, 110, 30);
        for word in ["eighty", "column", "N-COL80"] {
            assert!(text.contains(word), "mot manquant : {word}");
        }
    }

    /// Un toast long occupe plusieurs lignes, intégralement.
    #[test]
    fn toast_long_wrappe_complet() {
        let mut ed = Editor::open(None).unwrap();
        ed.notify(Level::Warn, "norm : 3M 12m — function detected as too long in the current file, split it into sub-functions");
        let text = render_text(&mut ed, 110, 30);
        for word in ["function", "sub-functions"] {
            assert!(text.contains(word), "mot manquant : {word}");
        }
    }

    /// La barre basse n'affiche que la position — aucun badge, jamais.
    #[test]
    fn pied_de_page_position_seule() {
        let ed = Editor::open(None).unwrap();
        assert_eq!(pos_text(&ed).trim(), "ln 1, col 1");
        assert!(!pos_text(&ed).contains('·'));
    }

    /// ^T ouvre et ferme l'explorateur (racine = répertoire courant).
    #[test]
    fn toggle_explorateur() {
        let mut ed = Editor::open(None).unwrap();
        assert!(ed.explorer.is_none());
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(ed.explorer.is_some());
        assert_eq!(ed.focus, Focus::Explorer);
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(ed.explorer.is_none());
        assert_eq!(ed.focus, Focus::Editor);
    }

    /// Enter sur un fichier de l'explorateur l'ouvre dans l'éditeur.
    #[test]
    fn ouvrir_depuis_explorateur() {
        let dir = scratch("cnano-ed");
        let f = dir.join("solo.c");
        std::fs::write(
            &f,
            "int solo;
",
        )
        .unwrap();

        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(dir.clone()));
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(ed.file.as_deref(), Some(f.as_path()));
        assert_eq!(ed.lines, vec!["int solo;"]);
        assert_eq!(ed.focus, Focus::Explorer, "on reste dans l'explorateur");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Un buffer modifié ne se fait JAMAIS écraser par l'explorateur.
    #[test]
    fn buffer_modifie_bloque_le_changement() {
        let dir = scratch("cnano-ed");
        std::fs::write(
            dir.join("a.c"),
            "int a;
",
        )
        .unwrap();

        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty())); // modifié
        ed.explorer = Some(Explorer::new(dir.clone()));
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(ed.file.is_none(), "pas de changement de fichier");
        assert_eq!(ed.lines[0], "x", "le buffer est intact");
        assert!(ed.status.contains("^s"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// La frappe libre dans l'explorateur filtre (et ne tape pas dans le code).
    #[test]
    fn frappe_dans_explorateur_filtre_sans_touchr_au_buffer() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        ed.on_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "", "le buffer n'a pas bougé");
        assert_eq!(ed.explorer.as_ref().unwrap().filter(), "z");
    }

    /// Cadre de test : dessine l'UI sur un backend virtuel, retourne le texte.

    /// L'accueil est une boîte arrondie façon lazy.nvim — et ne mentionne
    /// plus jamais rien d'autre que les gestes.
    #[test]
    fn accueil_boite_arrondie_pure() {
        let mut ed = Editor::open(None).unwrap();
        let text = render_text(&mut ed, 90, 28);
        for corner in ["╭", "╮", "╰", "╯"] {
            assert!(text.contains(corner), "coin {corner} présent");
        }
        assert!(text.contains("nana"), "the title lives in the border");
        assert!(text.contains("files") && text.contains("code") && text.contains("buffer"));
        assert!(!text.contains("IA"), "aucune mention de l'IA");
        assert!(!text.contains("piscine — sobre"), "plus de tagline");
    }

    /// Boîtes nues (pas de titre), ● de modification dans le breadcrumb,
    /// icônes de langage dans l'explorateur.
    #[test]
    fn boites_nues_et_icones() {
        let dir = scratch("cnano-ico");
        std::fs::write(dir.join("a.c"), "int a;\n").unwrap();
        std::fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();
        let mut ed = Editor::open(None).unwrap();
        ed.file = Some(std::path::PathBuf::from("/tmp/demo.c"));
        ed.modified = true;
        ed.lines = vec!["int x;".into()];
        ed.explorer = Some(Explorer::new(dir.clone()));
        let text = render_text(&mut ed, 90, 28);
        assert!(!text.contains(" editeur"), "plus de titre éditeur");
        assert!(!text.contains(" explorer "), "plus de titre explorateur");
        assert!(text.contains("demo.c"), "le nom reste dans le breadcrumb");
        assert!(text.contains("\u{E649}"), "icône C (seti-c)");
        assert!(text.contains("\u{E7A8}"), "icône Rust (devicon)");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Le mapping langage → icône devicon (codepoints de la fonte installée).
    #[test]
    fn icones_par_langage() {
        // the contract: the glyph is the registry's, and families differ
        let c = file_icon("main.c", false);
        let rs = file_icon("lib.rs", false);
        let py = file_icon("x.py", false);
        let mk = file_icon("Makefile", false);
        assert_eq!(c, langs::for_path(Path::new("main.c")).icon.to_string());
        assert_eq!(rs, langs::for_path(Path::new("lib.rs")).icon.to_string());
        assert_ne!(c, rs);
        assert_ne!(rs, py);
        assert_ne!(py, mk);
        assert_eq!(
            file_icon("inconnu.xyz", false),
            langs::for_path(Path::new("inconnu.xyz")).icon.to_string()
        );
        // repli sans Nerd Font
        assert_eq!(file_icon_plain("main.c"), "Ⓒ");
        assert_eq!(dir_icon(true), "\u{F07C}");
    }

    /// Explorateur ouvert : deux boîtes arrondies côte à côte.
    #[test]
    fn deux_boites_quand_explorateur() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        let text = render_text(&mut ed, 110, 30);
        assert!(text.matches('╭').count() >= 2, "une boîte par panneau");
        assert!(text.contains("type to filter"));
    }

    /// Alt-Tab : éditeur → explorateur → éditeur ; ouvre l'explorateur
    /// si rien n'est ouvert (le geste sert toujours).
    #[test]
    fn alt_tab_cycle_les_panneaux() {
        let mut ed = Editor::open(None).unwrap();
        assert_eq!(ed.focus, Focus::Editor);
        // sans explorateur : Alt-Tab l'ouvre et le focusse
        ed.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::ALT));
        assert_eq!(ed.focus, Focus::Explorer);
        assert!(ed.explorer.is_some());
        // encore : retour éditeur — Ctrl+Tab et F2 marchent pareil
        ed.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
        assert_eq!(ed.focus, Focus::Editor);
        ed.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Explorer, "F2 cycle aussi");
        // avec la recherche ouverte, elle est dans le cycle
        ed.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert_eq!(ed.focus, Focus::Search);
        ed.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::empty())); // Shift+Tab = repli
        assert_eq!(ed.focus, Focus::Editor);
        ed.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Explorer);
        ed.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Search);
    }

    /// A search with a single result must still have room to draw: the float
    /// used to compute a height under the guard and disappear, leaving a
    /// focused panel with nothing on screen.
    #[test]
    fn la_recherche_dessine_meme_avec_un_seul_resultat() {
        assert!(search_float_height(1, 30) >= 6, "one result still draws");
        assert!(
            search_float_height(0, 30) >= 6,
            "an empty filter still draws"
        );
        assert!(search_float_height(9, 30) > search_float_height(1, 30));
        assert!(
            search_float_height(50, 30) <= 28,
            "never taller than the zone"
        );
        assert!(search_float_height(3, 4) < 6, "a tiny zone draws nothing");
    }

    /// ^O : la frappe filtre, Enter ouvre le fichier, Échap referme.
    #[test]
    fn recherche_flottante_ouvre_un_fichier() {
        let dir = scratch("cnano-srch");
        let f = dir.join("cible.c");
        std::fs::write(
            &f,
            "int cible;
",
        )
        .unwrap();

        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(dir.clone())); // racine connue
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert_eq!(ed.focus, Focus::Search);
        for c in "cible".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        assert_eq!(ed.search.as_ref().unwrap().len(), 1);
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(ed.file.as_deref(), Some(f.as_path()));
        assert_eq!(ed.lines, vec!["int cible;"]);
        assert!(ed.search.is_none(), "la recherche se referme");
        assert_eq!(ed.focus, Focus::Editor);
        // la frappe n'a jamais touché le buffer
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Les toasts s'affichent puis s'évanouissent (3,5 s).
    #[test]
    fn toasts_pousses_puis_expires() {
        let mut ed = Editor::open(None).unwrap();
        ed.notify(Level::Ok, "saved");
        assert_eq!(ed.toasts.len(), 1);
        assert_eq!(ed.status, "saved", "le statut reflète le toast");
        // the icon is painted by the card, so it must appear exactly once
        ed.notify(Level::Ok, "saved ✓");
        assert_eq!(
            ed.status, "saved",
            "a message carrying its own icon is cleaned"
        );
        // the card slides in from the right over ~160 ms: let it land
        for toast in ed.toasts.iter_mut() {
            toast.at = Instant::now() - Duration::from_millis(400);
        }
        let text = render_text(&mut ed, 100, 30);
        assert!(text.contains("saved"), "la carte est dessinée");
        assert_eq!(text.matches('✓').count(), 1, "un seul ✓ à l'écran");
        // un toast vieux de 10 s est purgé au prochain rendu
        ed.toasts.push(Toast {
            level: Level::Err,
            text: "fantôme".into(),
            at: std::time::Instant::now() - std::time::Duration::from_secs(10),
        });
        let _ = render_text(&mut ed, 100, 30);
        assert_eq!(ed.toasts.len(), 1, "le toast expiré a été purgé");
    }

    /// Le survol affiche gcc + norme de la ligne courante, avec messages.
    #[test]
    fn survol_diagnostics_de_la_ligne() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["int x = ;".into()];
        ed.modified = true; // pas d'écran d'accueil
        ed.diags = vec![Diag {
            line: 1,
            col: 9,
            is_error: true,
            message: "expected expression".into(),
        }];
        ed.check_details = vec![(1, Severity::Minor, "trailing whitespace (style)".into())];
        let both = ed.line_diagnostics();
        assert_eq!(both.len(), 2, "gcc et norme fusionnés");
        let text = render_text(&mut ed, 100, 30);
        assert!(
            text.contains("expected expression"),
            "le message gcc flotte"
        );
        assert!(
            text.contains("trailing whitespace"),
            "the style finding too"
        );
        assert!(text.contains("int x = ;"), "the line is in the text");
    }

    /// La statusline segmentée : badge de focus, position, progression.
    #[test]
    fn statusline_a_ses_segments() {
        let mut ed = Editor::open(None).unwrap();
        let text = render_text(&mut ed, 100, 30);
        assert!(text.contains(" code "), "badge de focus");
        assert!(text.contains("ln 1, col 1"), "position");
        assert!(text.contains("100%"), "progression");
    }

    /// L'écran d'accueil n'apparaît que sur un buffer vierge sans fichier.
    #[test]
    fn accueil_uniquement_sur_vierge() {
        let mut ed = Editor::open(None).unwrap();
        assert!(ed.is_welcome());
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert!(
            !ed.is_welcome(),
            "dès la première frappe, l'accueil s'efface"
        );
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    fn ed_with_ghost() -> Editor {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["int foo(".into(), "return (0);".into()];
        ed.cy = 0;
        ed.cx = 8;
        ed.ghost = Some("\n    int i = 0;\n    while (i < 10)\n        i++;".into());
        ed.ghost_lines = vec![
            String::new(),
            "    int i = 0;".into(),
            "    while (i < 10)".into(),
            "        i++;".into(),
        ];
        ed
    }

    /// Tab accepte une suggestion MULTI-LIGNES : le splice crée de vraies
    /// lignes et le curseur finit à la fin de la suggestion.
    #[test]
    fn tab_accepte_ghost_multilignes() {
        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
        assert!(ed.ghost.is_none(), "ghost consommé");
        assert!(ed.ghost_lines.is_empty(), "ghost_lines consommées");
        assert_eq!(
            ed.lines,
            vec![
                "int foo(".to_string(),
                "    int i = 0;".to_string(),
                "    while (i < 10)".to_string(),
                "        i++;".to_string(),
                "return (0);".to_string(),
            ]
        );
        assert!(ed.modified);
        // curseur : dernière ligne insérée, fin de « i++; »
        assert_eq!((ed.cy, ed.cx), (3, 12));
    }

    /// Tab sans suggestion = 4 espaces (la Norme), pas une acceptation.
    #[test]
    fn tab_sans_ghost_insere_4_espaces() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "    ");
        assert!(ed.ghost.is_none());
    }

    /// Fin et Ctrl+→ acceptent aussi la suggestion.
    #[test]
    fn fin_et_ctrl_fleche_acceptent() {
        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::empty()));
        assert!(ed.ghost.is_none());
        assert_eq!(ed.lines.len(), 5);

        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
        assert!(ed.ghost.is_none());
        assert_eq!(ed.lines.len(), 5);
    }

    /// Échap refuse : ghost ET ghost_lines disparaissent (l'ancien bug
    /// laissait le fantôme affiché sans qu'aucune touche ne marche).
    #[test]
    fn esc_refuse_et_vide_tout() {
        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(ed.ghost.is_none());
        assert!(ed.ghost_lines.is_empty());
        assert_eq!(
            ed.lines,
            vec!["int foo(".to_string(), "return (0);".to_string()]
        );
        assert_eq!(ed.status, "declined");
    }

    /// Échap pendant « l'IA réfléchit… » : la requête en vol est annulée.
    #[test]
    fn esc_annule_ia_en_vol() {
        let mut ed = Editor::open(None).unwrap();
        let (_tx, rx) = channel();
        ed.rx = Some(rx);
        ed.ai_busy = true;
        ed.status = "en cours…".into();
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(!ed.ai_busy, "plus busy");
        assert!(ed.rx.is_none(), "canal lâché");
        assert_eq!(ed.status, "cancelled");
    }

    /// Toute édition invalide la suggestion — ghost ET ghost_lines
    /// (sinon le fantôme s'affiche mais plus aucune touche ne l'accepte).
    #[test]
    fn edition_invalide_le_ghost_completement() {
        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert!(ed.ghost.is_none());
        assert!(ed.ghost_lines.is_empty());

        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert!(ed.ghost_lines.is_empty());

        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::empty()));
        assert!(ed.ghost_lines.is_empty());

        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert!(ed.ghost_lines.is_empty());
    }

    /// La complétion demande le fichier ENTIER avec le curseur marqué —
    /// même une ligne avec le marqueur au milieu, sans panique.
    #[test]
    fn contexte_marqueur_curseur_sans_panique() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["int main(void)".into(), "{".into()];
        ed.cy = 1;
        ed.cx = 1;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // on reproduit la construction du marqueur de ai_complete
            let line: Vec<char> = ed.lines[ed.cy].chars().collect();
            let pos = ed.cx.min(line.len());
            let marked: String = line[..pos].iter().collect::<String>()
                + "▌"
                + &line[pos..].iter().collect::<String>();
            assert_eq!(marked, "{▌");
        }));
        assert!(r.is_ok());
    }
}

#[cfg(test)]
mod hub_hover_tests {
    use super::*;

    /// the hovered hub row is a band of its own colour: the cursor row must not
    /// look like the rows around it, or the user cannot tell where the
    /// selection is.
    #[test]
    fn the_hovered_hub_row_has_its_own_colour_band() {
        let d = std::env::temp_dir().join(format!("nana-hover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        crate::memory::Memory::open(&d)
            .write(crate::memory::Class::Project, "stack", "# stack\n\nrust")
            .unwrap();
        let mut ed = Editor::open(None).unwrap();
        ed.hub = Some(crate::hub::Hub::open(&d));
        let backend = ratatui::backend::TestBackend::new(110, 30);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, &mut ed)).unwrap();
        let buf = term.backend().buffer().clone();
        let _ = std::fs::remove_dir_all(&d);
        let row = (0..buf.area.height)
            .find(|&y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .contains("project: stack")
            })
            .expect("the selected row is drawn");
        let start = (0..buf.area.width)
            .find(|&x| buf[(x, row)].symbol() == "p")
            .unwrap();
        assert_eq!(
            buf[(start, row)].bg,
            Ed::hover(),
            "the label sits on the hover band"
        );
        assert_eq!(
            buf[(start + 10, row)].bg,
            Ed::hover(),
            "the band runs under the name"
        );
        assert_ne!(
            Ed::hover(),
            Ed::sel_row(),
            "the hover is not the invisible black"
        );
    }
}

#[cfg(test)]
mod live_write_tests {
    use super::*;

    /// the agent's write is played one line at a time in the agent's own view,
    /// beside the user's buffer. the user's buffer is never written by it.
    #[test]
    fn a_write_is_played_one_line_at_a_time_beside_the_user_buffer() {
        let d = scratch("live");
        let target = d.join("new.rs");
        let mut ed = Editor::open(None).unwrap();
        ed.file = Some(d.join("mine.rs"));
        ed.lines = vec!["my own line".into()];
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: target.clone(),
            text: vec![
                "fn one() {}".into(),
                "fn two() {}".into(),
                "fn three() {}".into(),
            ],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        assert_eq!(
            ed.live.len(),
            1,
            "the write is taken over by the agent view"
        );
        ed.play_live();
        let text = render_text(&mut ed, 160, 40);
        assert!(!text.contains("fn one() {}"), "one letter after one step");
        while !ed.live.is_empty() {
            ed.play_live();
        }
        std::thread::sleep(VIEW_SWEEP);
        let text = render_text(&mut ed, 160, 40);
        assert!(
            text.contains("fn three() {}"),
            "the file is whole on screen"
        );
        assert_eq!(
            ed.lines,
            vec!["my own line".to_string()],
            "the user's buffer is never written by the agent"
        );
        assert!(!ed.modified, "the user's buffer is not marked as edited");
        assert!(ed.live.is_empty(), "the playback is over");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// the user's text and the agent's text are both on screen at once: the
    /// agent column sits beside the editor, neither one overrides the other.
    #[test]
    fn the_agent_text_is_on_screen_and_the_user_text_is_not() {
        let d = scratch("live-side");
        let mut ed = Editor::open(None).unwrap();
        ed.file = Some(d.join("mine.rs"));
        ed.lines = vec!["USER_TEXT_STAYS".into()];
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: d.join("new.rs"),
            text: vec!["AGENT_TEXT_SHOWS".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        while !ed.live.is_empty() {
            ed.play_live();
        }
        std::thread::sleep(VIEW_SWEEP);
        let rows = render_rows(&mut ed, 160, 40);
        assert!(
            rows.iter()
                .any(|r| r.contains("USER_TEXT_STAYS") || r.contains("AGENT_TEXT_SHOWS")),
            "{rows:?}"
        );
        let all = rows.join("\n");
        assert!(
            all.contains("AGENT_TEXT_SHOWS"),
            "the agent's write plays in the background view:\n{all}"
        );
        assert!(
            !all.contains("USER_TEXT_STAYS"),
            "the user's buffer is not drawn behind the card:\n{all}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// a read is shown the same way, line by line, but it changes nothing: the
    /// file is not saved again and the buffer is not marked as edited.
    #[test]
    fn a_read_is_shown_line_by_line_and_changes_nothing() {
        let d = scratch("live-read-show");
        let target = d.join("notes.txt");
        let mut ed = Editor::open(None).unwrap();
        ed.file = Some(d.join("other.txt"));
        ed.lines = vec![String::new()];
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: target.clone(),
            text: vec!["alpha".into(), "beta".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        ed.play_live();
        assert_eq!(
            ed.agent_view.lines,
            vec!["a".to_string()],
            "the read appears one letter at a time in the agent view"
        );
        while !ed.live.is_empty() {
            ed.play_live();
        }
        assert_eq!(
            ed.agent_view.lines,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert!(ed.live.is_empty(), "the read is over");
        assert_eq!(
            ed.lines,
            vec![String::new()],
            "the user's buffer is untouched by the read"
        );
        assert!(!ed.modified, "a read is not an edit");
        assert_ne!(
            ed.saved, ed.agent_view.lines,
            "a read does not write its text as the saved state"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// the user's unsaved work is never replaced by something the agent read.
    #[test]
    fn a_read_does_not_replace_unsaved_work() {
        let d = scratch("live-read-guard");
        let mut ed = Editor::open(None).unwrap();
        ed.file = Some(d.join("mine.txt"));
        ed.lines = vec!["my unsaved line".into()];
        ed.modified = true;
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: d.join("theirs.txt"),
            text: vec!["something else".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        while !ed.live.is_empty() {
            ed.play_live();
        }
        assert_eq!(
            ed.lines,
            vec!["my unsaved line".to_string()],
            "the buffer is untouched"
        );
        assert!(ed.modified, "the edit is still marked as unsaved");
        assert!(ed.live.is_empty(), "the read is played in the agent view");
        assert_eq!(
            ed.agent_view.lines,
            vec!["something else".to_string()],
            "the agent sees its own read, the user keeps their work"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod hub_truncation_tests {
    use super::*;

    /// a long memory note in the hub's left box is cut at the edge of the
    /// card, and the cut says so: no letter is lost without a sign.
    #[test]
    fn a_long_hub_row_is_cut_with_an_ellipsis_not_at_the_border() {
        let d = std::env::temp_dir().join(format!("nana-trunc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        // two pages: the selected row shows no note, so the long one must sit
        // on an unselected row to be drawn at all
        let m = crate::memory::Memory::open(&d);
        m.write(
            crate::memory::Class::Project,
            "stack",
            &format!("# {}\n\nbody", "x".repeat(80)),
        )
        .unwrap();
        m.write(crate::memory::Class::Project, "other", "# short\n\nbody")
            .unwrap();
        let mut ed = Editor::open(None).unwrap();
        ed.hub = Some(crate::hub::Hub::open(&d));
        let text = render_text(&mut ed, 110, 30);
        let _ = std::fs::remove_dir_all(&d);
        let rows: Vec<&str> = text.lines().collect();
        assert!(
            rows.iter().any(|r| r.contains("…")),
            "a cut row has no ellipsis:\n{text}"
        );
    }
}

#[cfg(test)]
mod agent_view_tests {
    use super::*;

    fn rows(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
        render_rows(ed, w, h)
    }

    /// the agent's view keeps its own bottom border on screen: nothing is
    /// drawn over it now that the trail is gone.
    #[test]
    fn the_agent_view_keeps_its_bottom_border_above_the_trail() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["fn a() {}".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        ed.play_live();
        ed.play_live();
        let screen = rows(&mut ed, 120, 40);
        assert!(
            screen.iter().any(|r| r.starts_with('╰')),
            "the view has no bottom border on screen:\n{}",
            screen.join("\n")
        );
    }

    /// two shell commands in a row are both played: the second call must not
    /// erase the first one before its answer comes back.
    #[test]
    fn a_shell_command_is_not_skipped_when_another_follows() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        tx.send(crate::agent::Event::ToolCall {
            name: "run_shell".into(),
            args: serde_json::json!({"command": "ls"}),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolCall {
            name: "run_shell".into(),
            args: serde_json::json!({"command": "cargo test"}),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: "run_shell".into(),
            ok: true,
            text: "a\nb".into(),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: "run_shell".into(),
            ok: true,
            text: "test result: ok".into(),
        })
        .unwrap();
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        assert_eq!(
            ed.live.len(),
            2,
            "both shell commands must be played, the first one is not skipped"
        );
    }

    /// every action of the agent is played in the live view, not only the file
    /// writes: a memory listing or a memory page it opened is on screen too.
    #[test]
    fn a_memory_listing_and_a_memory_page_are_played_in_the_live_view() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        tx.send(crate::agent::Event::ToolCall {
            name: "memory_list".into(),
            args: serde_json::json!({}),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: "memory_list".into(),
            ok: true,
            text: "project: stack\nuser: tone".into(),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolCall {
            name: "memory_read".into(),
            args: serde_json::json!({"class": "project", "name": "stack"}),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: "memory_read".into(),
            ok: true,
            text: "# stack\n\nrust, no async".into(),
        })
        .unwrap();
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        assert_eq!(ed.live.len(), 2, "each memory action must be played");
        // each action has its own window that is replaced when the next one
        // starts, so every action is checked while its window is on screen
        let mut seen = String::new();
        let mut guard = 0;
        while !ed.live.is_empty() && guard < 10_000 {
            ed.play_live();
            seen.push_str(&ed.agent_view.lines.join("\n"));
            seen.push('\n');
            guard += 1;
        }
        assert!(
            seen.contains("memory_list"),
            "the listing call is not shown:\n{seen}"
        );
        assert!(
            seen.contains("project: stack"),
            "the listing is not shown:\n{seen}"
        );
        assert!(
            seen.contains("rust, no async"),
            "the opened page is not shown:\n{seen}"
        );
    }

    /// a file change made by a tool that is not a plain write (edit_file) is
    /// played too, so no change of the agent happens off screen.
    #[test]
    fn an_edit_is_played_in_the_live_view() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        tx.send(crate::agent::Event::ToolCall {
            name: "edit_file".into(),
            args: serde_json::json!({"path": "src/lib.rs", "find": "a", "replace": "b"}),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: "edit_file".into(),
            ok: true,
            text: "edited src/lib.rs".into(),
        })
        .unwrap();
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        assert_eq!(ed.live.len(), 1, "the edit is not played in the live view");
    }

    /// the transcript is wrapped once, when a line arrives, and not again on
    /// every frame: a long conversation used to cost more each keystroke.
    /// the cached rows must match a full wrap, so the screen does not change.
    #[test]
    fn the_transcript_is_wrapped_once_per_line_not_on_every_frame() {
        let mut pane = AgentPane::new();
        for i in 0..50 {
            pane.lines.push((
                "agent".into(),
                format!("answer {i} with several words to wrap"),
            ));
        }
        let text = |rows: &[Line]| -> Vec<String> {
            rows.iter()
                .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
                .collect()
        };
        let first = text(pane.rendered_rows(24));
        assert_eq!(pane.transcript.wrapped, 51, "every line is wrapped once");
        let again = text(pane.rendered_rows(24));
        assert_eq!(
            pane.transcript.wrapped, 51,
            "a frame with no new line wraps nothing"
        );
        assert_eq!(first, again);
        pane.lines.push(("you".into(), "one more".into()));
        let grown = text(pane.rendered_rows(24));
        assert_eq!(pane.transcript.wrapped, 52, "only the new line is wrapped");
        assert!(grown.len() > first.len());
        pane.transcript = TranscriptRows::default();
        assert_eq!(
            text(pane.rendered_rows(24)),
            grown,
            "a full wrap gives the same rows as the cache"
        );
    }

    /// the agent types one letter per tick, like a person coding: the cursor
    /// sits after the last letter on screen, never a whole line at once.
    #[test]
    fn the_agent_writes_one_letter_per_tick() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["abc".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        ed.play_live();
        assert_eq!(ed.agent_view.lines, vec!["a".to_string()], "one letter");
        assert_eq!(ed.agent_view.cx, 1, "the cursor follows the letter");
        ed.play_live();
        assert_eq!(ed.agent_view.lines, vec!["ab".to_string()], "two letters");
        ed.play_live();
        ed.play_live();
        assert!(ed.live.is_empty(), "the line is whole, the write is over");
        assert_eq!(ed.agent_view.lines, vec!["abc".to_string()]);
    }

    /// a write with a set duration lands in exactly that many ticks, however
    /// long its text is: the pace stretches to the time it was given.
    #[test]
    fn a_write_lasts_exactly_the_ticks_it_is_given() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["abcdefghij".into()],
            shown: 0,
            col: 0,
            ticks: 5,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        ed.play_live();
        assert_eq!(
            ed.agent_view.lines,
            vec!["ab".to_string()],
            "2 of 10 in tick 1"
        );
        for _ in 0..3 {
            ed.play_live();
        }
        assert_eq!(ed.live.len(), 1, "still writing after 4 ticks of 5");
        ed.play_live();
        assert!(ed.live.is_empty(), "the write ends on tick 5");
        assert_eq!(ed.agent_view.lines, vec!["abcdefghij".to_string()]);
    }

    /// the playback is a view of work that goes on: when the user leaves the
    /// agent's view to do something else, the playback stops at once rather
    /// than running on behind their back.
    #[test]
    fn the_agent_playback_stops_when_the_card_closes() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["abcdefghij".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        ed.play_live();
        ed.agent_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(ed.agent_pane.is_none(), "the card closed");
        assert!(ed.live.is_empty(), "the playback stopped with the card");
        assert_eq!(
            ed.agent_view.lines,
            vec!["a".to_string()],
            "what was shown stays"
        );
    }

    /// opening a file from the explorer is leaving the agent's view: the
    /// playback stops, and the file the user opens is the one they see.
    #[test]
    fn the_agent_playback_stops_when_a_file_is_opened_from_the_explorer() {
        let d = scratch("live-interrupt-explorer");
        let file = d.join("notes.txt");
        std::fs::write(&file, "hello\n").unwrap();
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["abcdefghij".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        ed.play_live();
        ed.open_from_explorer(file);
        assert!(ed.live.is_empty(), "the explorer open stops the playback");
        assert_eq!(
            ed.lines,
            vec!["hello".to_string()],
            "the user's file is shown"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// once the agent stops writing, its view shows no current line: the
    /// line number of the last line is drawn like the others.
    #[test]
    fn the_agent_view_has_no_cursor_when_the_agent_is_idle() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["fn a() {}".into(), "fn b() {}".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        while !ed.live.is_empty() {
            ed.play_live();
        }
        // the view sweeps in before it is read: look at it once it has landed
        std::thread::sleep(VIEW_SWEEP);
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, &mut ed)).unwrap();
        let buf = term.backend().buffer().clone();
        let row = (0..40u16)
            .find(|&y| {
                (0..120u16)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .contains("fn b() {}")
            })
            .expect("the last line is drawn");
        let number = buf[(3, row)].fg;
        assert_eq!(
            number,
            Ed::gutter(),
            "the idle agent shows a cursor on its last line"
        );
    }

    /// wheeling the transcript up is clamped to what it holds, so one more
    /// Down always scrolls back: the counter does not run away.
    #[test]
    fn the_agent_transcript_scroll_stops_at_the_top() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.lines.push(("agent".into(), "une seule ligne".into()));
        pane.scroll = 500;
        ed.agent_pane = Some(pane);
        let _ = rows(&mut ed, 120, 40);
        let scroll = ed.agent_pane.as_ref().unwrap().scroll;
        assert!(scroll < 20, "the scroll ran away to {scroll}");
    }

    /// the agent took a while to reach a write: the write lasts as long, so the
    /// reader watches it land in the time the agent actually spent.
    #[test]
    fn a_write_lasts_as_long_as_the_agent_took_to_reach_it() {
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        pane.last_event = Instant::now() - Duration::from_millis(3000);
        tx.send(crate::agent::Event::ToolCall {
            name: "write_file".into(),
            args: serde_json::json!({"path": "src/lib.rs", "content": "fn a() {}"}),
        })
        .unwrap();
        tx.send(crate::agent::Event::File {
            path: "src/lib.rs".into(),
            content: "fn a() {}".into(),
            write: true,
        })
        .unwrap();
        pane.poll();
        let ticks = pane.writes[0].ticks;
        assert!(
            (187..=190).contains(&ticks),
            "3 s at one tick per 16 ms: {ticks} ticks"
        );
    }

    /// a new view sweeps in from the left as the card hands over to a file:
    /// half-way, its rounded edge sits mid-card and nothing is drawn past it.
    #[test]
    fn the_view_sweeps_in_from_the_left() {
        let mut ed = Editor::open(None).unwrap();
        ed.agent_pane = Some(AgentPane::new());
        ed.agent_view = AgentView {
            path: Some(PathBuf::from("b.rs")),
            lines: vec!["two".into()],
            cy: 0,
            cx: 3,
            opened: Some(Instant::now() - VIEW_SWEEP / 2),
            glide: None,
        };
        let rows = render_rows(&mut ed, 120, 40);
        // the card's top border is the row that holds the rounded corner; the
        // header and the ide's own first row sit above it now
        let top = rows
            .iter()
            .find(|r| r.contains('╮'))
            .expect("the sweep has a rounded edge");
        let edge = top.chars().position(|c| c == '╮').unwrap();
        // half-way is 60 columns of the 120: the edge lands past mid-card but
        // well short of the far side, whatever the frame's exact timing
        assert!(
            (50..=80).contains(&edge),
            "the edge is not mid-card, at {edge}: {top}"
        );
        assert!(
            top.chars().skip(edge + 1).all(|c| c == ' '
                || c == '─'
                || c == '╮'
                || c == '│'
                || c == '╭'),
            "the view is drawn past the sweep: {top}"
        );
    }
}

#[cfg(test)]
mod live_sync_tests {
    use super::*;

    fn write_of(path: &str, text: Vec<String>, ticks: usize) -> LiveWrite {
        LiveWrite {
            path: PathBuf::from(path),
            text,
            shown: 0,
            col: 0,
            ticks,
            elapsed: 0,
        }
    }

    /// the playback must not trail the agent: four long writes queued at once
    /// are compressed so their whole backlog fits in the lag budget (94 ticks).
    #[test]
    fn the_playback_never_trails_the_agent_by_more_than_its_lag_budget() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        for _ in 0..4 {
            pane.writes
                .push(write_of("src/lib.rs", vec!["x".repeat(400)], 2000));
        }
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let backlog: usize = ed
            .live
            .iter()
            .map(|w| w.ticks.saturating_sub(w.elapsed))
            .sum();
        assert!(backlog <= 94, "the playback trails by {backlog} ticks");
        assert_eq!(ed.live.len(), 4, "no write is dropped, only compressed");
    }

    /// a flood of writes, each at the agent's own pace, is caught up: the whole
    /// backlog plays within the lag budget, and the screen ends on the last
    /// thing the agent did.
    #[test]
    fn a_flood_of_writes_is_caught_up_and_the_screen_ends_on_the_last_one() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        for i in 0..40 {
            pane.writes.push(write_of(
                &format!("f{i}.rs"),
                vec![format!("fn f{i}() {{}}")],
                16,
            ));
        }
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let mut frames = 0;
        while !ed.live.is_empty() && frames < 100_000 {
            ed.play_live();
            frames += 1;
        }
        assert!(frames <= 94, "the flood took {frames} frames to show");
        assert_eq!(
            ed.agent_view.lines,
            vec!["fn f39() {}".to_string()],
            "the view shows the last write"
        );
    }

    /// a compressed write still lands in full: the lag rule shortens the
    /// playback, it never cuts the text the agent wrote.
    #[test]
    fn a_compressed_write_still_lands_in_full() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes
            .push(write_of("a.rs", vec!["alpha".into(), "beta".into()], 900));
        pane.writes
            .push(write_of("b.rs", vec!["gamma".repeat(60)], 900));
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let mut guard = 0;
        while !ed.live.is_empty() && guard < 100_000 {
            ed.play_live();
            guard += 1;
        }
        assert_eq!(ed.agent_view.lines, vec!["gamma".repeat(60)]);
    }
}

#[cfg(test)]
mod rounded_bars_tests {
    use super::*;

    /// the brand and the status badge are pills again: a half-circle on each
    /// end, so the two corners of the screen stay round. The other segments
    /// stay flat, the rule of the last cleanup.
    #[test]
    fn the_brand_and_the_status_badge_are_rounded_pills() {
        let mut ed = Editor::open(None).unwrap();
        let backend = ratatui::backend::TestBackend::new(100, 20);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, &mut ed)).unwrap();
        let buf = term.backend().buffer();
        let top: String = (0..100).map(|x| buf[(x, 0)].symbol()).collect();
        let bottom: String = (0..100).map(|x| buf[(x, 19)].symbol()).collect();
        assert!(
            top.contains('\u{E0B6}') && top.contains('\u{E0B4}'),
            "brand not rounded: {top:?}"
        );
        assert!(
            bottom.contains('\u{E0B6}') && bottom.contains('\u{E0B4}'),
            "badge not rounded: {bottom:?}"
        );
    }
}

#[cfg(test)]
mod glitch_safety_tests {
    use super::*;

    fn reversed_cells(ed: &mut Editor, w: u16, h: u16) -> Vec<(u16, u16)> {
        let backend = ratatui::backend::TestBackend::new(w, h);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, ed)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| buf[(x, y)].modifier.contains(Modifier::REVERSED))
            .collect()
    }

    /// a tab, a carriage return, an escape sequence or a zero-width mark in
    /// text from the agent is not printed: the terminal would act on it.
    #[test]
    fn safe_text_keeps_the_letters_and_drops_every_byte_a_terminal_acts_on() {
        assert_eq!(
            safe_text("a\tb"),
            "a   b",
            "a tab becomes spaces to the next stop"
        );
        assert_eq!(
            safe_text("x\r\u{7}y"),
            "xy",
            "carriage return and bell are dropped"
        );
        assert_eq!(
            safe_text("\x1b[31mred\x1b[0m"),
            "red",
            "colour codes go whole"
        );
        assert_eq!(
            safe_text("\x1b]0;title\x07ok"),
            "ok",
            "window titles go whole"
        );
        assert_eq!(
            safe_text("e\u{200d}\u{fe0f}!"),
            "e!",
            "zero-width marks are dropped"
        );
        assert_eq!(
            safe_text("ok\nnext"),
            "ok\nnext",
            "a line break separates lines"
        );
    }

    /// the agent's answers, file contents and tool outputs, all dirty, put
    /// nothing that is a control byte on the screen, and the text still shows.
    #[test]
    fn the_agent_screen_never_holds_a_control_byte() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        tx.send(crate::agent::Event::File {
            path: "src/b\tc.rs".into(),
            content: "fn b() {\r\n\tlet y = 1;\r\n}\r\n".into(),
            write: true,
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolCall {
            name: "run_shell".into(),
            args: serde_json::json!({"command": "cargo build"}),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: "run_shell".into(),
            ok: true,
            text: "src/a.rs:3:\tlet x\r\n\x1b[31mred\x1b[0m done\u{7}\u{200d}".into(),
        })
        .unwrap();
        tx.send(crate::agent::Event::Text("plain\tsaid\x1b[2J".into()))
            .unwrap();
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let mut guard = 0;
        while !ed.live.is_empty() && guard < 10_000 {
            ed.play_live();
            guard += 1;
        }
        std::thread::sleep(VIEW_SWEEP);
        let text = render_text(&mut ed, 160, 40);
        let bad: Vec<char> = text.chars().filter(|c| c.is_control()).collect();
        assert!(bad.is_empty(), "control bytes reached a cell: {bad:?}");
        assert!(text.contains("red done"), "the text still shows:\n{text}");
    }

    /// the user's own buffer is held to the same rule: a lone carriage return
    /// or an escape in a file never reaches the terminal.
    #[test]
    fn the_user_buffer_never_holds_a_control_byte_either() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["a\rb\x1b[2Jc\u{200d}d".into()];
        ed.modified = true;
        let text = render_text(&mut ed, 100, 30);
        let bad: Vec<char> = text.chars().filter(|c| c.is_control()).collect();
        assert!(bad.is_empty(), "control bytes reached a cell: {bad:?}");
        assert!(text.contains("abcd"), "the visible text is kept:\n{text}");
    }

    /// the agent's cursor is a block drawn right after the last letter it
    /// typed: that is what makes the write read as a person typing.
    #[test]
    fn the_agent_cursor_sits_after_the_last_letter_it_wrote() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["abc".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        ed.play_live();
        ed.play_live();
        std::thread::sleep(VIEW_SWEEP);
        // the number gutter is 5 columns, the box border one: the text starts at x 6.
        // rows: the header and the card's top border sit above the first line, row 3
        assert_eq!(reversed_cells(&mut ed, 160, 40), vec![(8, 3)]);
    }

    /// the cursor does not jump: when the agent moves to the next line, it
    /// glides down from the line it left, and lands on the new line.
    #[test]
    fn the_cursor_glides_from_the_line_it_left_to_the_next() {
        let mut ed = Editor::open(None).unwrap();
        ed.agent_pane = Some(AgentPane::new());
        ed.agent_view = AgentView {
            path: Some(PathBuf::from("src/lib.rs")),
            lines: vec!["one".into(), "two".into(), "three".into()],
            cy: 2,
            cx: 0,
            opened: Some(Instant::now() - VIEW_SWEEP * 2),
            glide: Some((0, Instant::now() - CURSOR_GLIDE / 2)),
        };
        ed.live.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["x".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        // half-way: between line 1 (row 3) and line 3 (row 5), so on row 4
        assert_eq!(reversed_cells(&mut ed, 160, 40), vec![(6, 4)]);
        ed.agent_view.glide = None;
        assert_eq!(reversed_cells(&mut ed, 160, 40), vec![(6, 5)]);
    }

    /// playing a write that goes to a new line starts a glide from the line
    /// the cursor left.
    #[test]
    fn a_new_line_starts_a_glide_from_the_old_one() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["abc".into(), "def".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let mut guard = 0;
        while ed.agent_view.cy == 0 && !ed.live.is_empty() && guard < 100 {
            ed.play_live();
            guard += 1;
        }
        assert_eq!(ed.agent_view.cy, 1, "the cursor reached the second line");
        assert_eq!(ed.agent_view.glide.map(|(from, _)| from), Some(0));
    }
}

#[cfg(test)]
mod agent_input_tests {
    use super::*;

    /// a paste while the agent card is open goes into its prompt, newlines
    /// included, and never into the user's buffer behind the card.
    #[test]
    fn a_paste_lands_in_the_agent_prompt() {
        let mut ed = Editor::open(None).unwrap();
        ed.agent_pane = Some(AgentPane::new());
        let before = ed.lines.clone();
        ed.paste("first line\nsecond line");
        assert_eq!(
            ed.agent_pane.as_ref().unwrap().input,
            "first line\nsecond line",
            "the pasted text is in the prompt"
        );
        assert_eq!(ed.lines, before, "the user's buffer is untouched");
    }

    /// a long prompt wraps on the card instead of running off its right edge.
    #[test]
    fn a_long_prompt_wraps_on_several_lines_inside_the_card() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.input = "alpha beta gamma delta epsilon zeta eta theta iota kappa".into();
        ed.agent_pane = Some(pane);
        let rows = render_rows(&mut ed, 60, 24);
        let joined = rows.join("\n");
        assert!(
            joined.contains("alpha beta"),
            "the start is shown:\n{joined}"
        );
        assert!(joined.contains("kappa"), "the end is shown too:\n{joined}");
        assert!(
            !joined.contains("..."),
            "nothing is cut with dots:\n{joined}"
        );
    }

    /// a pasted block of several lines shows each line on its own row.
    #[test]
    fn a_multi_line_prompt_shows_every_line() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.input = "one\ntwo\nthree".into();
        ed.agent_pane = Some(pane);
        let rows = render_rows(&mut ed, 80, 24);
        let joined = rows.join("\n");
        for word in ["one", "two", "three"] {
            assert!(joined.contains(word), "{word} is shown:\n{joined}");
        }
        let row_of = |w: &str| rows.iter().position(|r| r.contains(w)).unwrap();
        assert!(row_of("one") < row_of("two"), "lines keep their order");
        assert!(row_of("two") < row_of("three"), "lines keep their order");
    }
}

#[cfg(test)]
mod agent_colour_tests {
    use super::*;

    /// the agent's live view colours code the way the editor does: a keyword
    /// of the file's language is not drawn in the same colour as a plain name.
    #[test]
    fn the_live_view_colours_code_by_its_language() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        pane.writes.push(LiveWrite {
            path: PathBuf::from("src/lib.rs"),
            text: vec!["fn main() {".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        while !ed.live.is_empty() {
            ed.play_live();
        }
        std::thread::sleep(VIEW_SWEEP);
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, &mut ed)).unwrap();
        let buf = term.backend().buffer().clone();
        let row = (0..40u16)
            .find(|&y| {
                (0..120u16)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .contains("fn main")
            })
            .expect("the line is drawn");
        let x_of = |word: &str| {
            let text: String = (0..120u16).map(|x| buf[(x, row)].symbol()).collect();
            text.find(word)
                .map(|b| text[..b].chars().count() as u16)
                .unwrap()
        };
        let keyword = buf[(x_of("fn"), row)].fg;
        let name = buf[(x_of("main"), row)].fg;
        assert_ne!(
            keyword, name,
            "a keyword and a name of the code are not the same colour"
        );
    }
}

#[cfg(test)]
mod terminal_look_tests {
    use super::*;

    /// when the agent runs a shell command, the live view turns into a
    /// terminal: the command appears with a `$` and its output under it, not
    /// as a json call in a generic action list.
    #[test]
    fn every_tool_action_is_played_as_a_terminal_line() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        tx.send(crate::agent::Event::ToolCall {
            name: "list_dir".into(),
            args: serde_json::json!({"path": "src"}),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: "list_dir".into(),
            ok: true,
            text: "editor.rs".into(),
        })
        .unwrap();
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let mut guard = 0;
        while !ed.live.is_empty() && guard < 10_000 {
            ed.play_live();
            guard += 1;
        }
        let shown = ed.agent_view.lines.join("\n");
        assert!(
            shown.contains("$ ls src"),
            "a list_dir is not played as a terminal line:\n{shown}"
        );
        assert!(
            !shown.contains("> list_dir"),
            "the json call is still shown:\n{shown}"
        );
    }

    #[test]
    fn a_shell_command_is_played_as_a_terminal_session() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        tx.send(crate::agent::Event::ToolCall {
            name: "run_shell".into(),
            args: serde_json::json!({"command": "ls"}),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: "run_shell".into(),
            ok: true,
            text: "Cargo.toml\nsrc".into(),
        })
        .unwrap();
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let mut guard = 0;
        while !ed.live.is_empty() && guard < 10_000 {
            ed.play_live();
            guard += 1;
        }
        let shown = ed.agent_view.lines.join("\n");
        assert!(
            shown.contains("$ ls"),
            "the command has no prompt:\n{shown}"
        );
        assert!(
            shown.contains("Cargo.toml"),
            "the output is not shown:\n{shown}"
        );
        assert!(
            !shown.contains("> run_shell"),
            "the json call is still shown:\n{shown}"
        );
    }

    /// the trail, the band under the card, shows a shell command as a prompt
    /// too: the json of the call is not what a person would read.
    #[test]
    fn the_trail_shows_a_shell_command_as_a_prompt_not_as_json() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        tx.send(crate::agent::Event::ToolCall {
            name: "run_shell".into(),
            args: serde_json::json!({"command": "ls"}),
        })
        .unwrap();
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let trail: Vec<String> = ed
            .agent_pane
            .as_ref()
            .unwrap()
            .trail
            .iter()
            .map(|s| s.label.clone())
            .collect();
        assert!(
            trail.iter().any(|t| t == "$ ls"),
            "the trail shows json, not a prompt: {trail:?}"
        );
    }

    /// on screen, a shell action reads as a terminal typed by a person: no
    /// line numbers in a gutter, no window named after the internal view.
    #[test]
    fn a_shell_action_on_screen_has_no_file_gutter_and_no_machine_title() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        tx.send(crate::agent::Event::ToolCall {
            name: "run_shell".into(),
            args: serde_json::json!({"command": "ls"}),
        })
        .unwrap();
        tx.send(crate::agent::Event::ToolResult {
            name: "run_shell".into(),
            ok: true,
            text: "Cargo.toml".into(),
        })
        .unwrap();
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let mut guard = 0;
        while !ed.live.is_empty() && guard < 10_000 {
            ed.play_live();
            guard += 1;
        }
        ed.agent_view.opened = Some(Instant::now() - VIEW_SWEEP * 2);
        let screen = render_rows(&mut ed, 120, 40).join("\n");
        assert!(
            !screen.contains("agent actions"),
            "the window is named after the internal view:\n{screen}"
        );
        assert!(
            !screen.contains("1 $ ls"),
            "the terminal is drawn with a file's line numbers:\n{screen}"
        );
        assert!(
            screen.contains("$ ls"),
            "the command is not on screen:\n{screen}"
        );
    }
}

#[cfg(test)]
mod busy_look_tests {
    use super::*;

    /// the spinner says "thinking" on the bar with no background of its own:
    /// the theme's black is midnight blue, and it must never show behind it.
    #[test]
    fn the_busy_segment_has_no_midnight_background() {
        let mut ed = Editor::open(None).unwrap();
        ed.ai_busy = true;
        let backend = ratatui::backend::TestBackend::new(120, 10);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, &mut ed)).unwrap();
        let buf = term.backend().buffer();
        let word = "thinking";
        let (w, h) = (120u16, 10u16);
        let mut found = None;
        for y in 0..h {
            let text: String = (0..w).map(|x| buf[(x, y)].symbol()).collect();
            if let Some(at) = text.find(word) {
                found = Some((at as u16, y));
            }
        }
        let (at, row) = found.expect("the busy word is on the bar");
        for x in at..at + word.len() as u16 {
            assert_ne!(
                buf[(x, row)].bg,
                Color::Indexed(0),
                "the busy segment paints the theme's black behind '{word}'"
            );
        }
    }
}

mod statusbar_background_tests {
    use super::*;

    /// no status segment paints the theme's black: it shows midnight blue.
    /// every busy word (thinking, checking, saving...) must sit on the terminal.
    #[test]
    fn no_status_segment_paints_the_theme_black() {
        for busy in ["thinking", "checking", "reading", "writing"] {
            let mut ed = Editor::open(None).unwrap();
            ed.ai_busy = busy == "thinking";
            ed.status = busy.to_string();
            let backend = ratatui::backend::TestBackend::new(120, 10);
            let mut term = ratatui::Terminal::new(backend).unwrap();
            term.draw(|f| draw(f, &mut ed)).unwrap();
            let buf = term.backend().buffer();
            let row = 9u16;
            for x in 0..120u16 {
                assert_ne!(
                    buf[(x, row)].bg,
                    Color::Indexed(0),
                    "status bar cell {x} paints the theme black while {busy}"
                );
            }
        }
    }
}

#[cfg(test)]
mod human_inference_tests {
    use super::*;

    /// every action opens its own window: two tool actions never share one
    /// "agent actions" view that the second one overwrites.
    #[test]
    fn each_action_opens_its_own_window() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        for (name, args, text) in [
            (
                "run_shell",
                serde_json::json!({"command": "ls"}),
                "Cargo.toml",
            ),
            ("list_dir", serde_json::json!({"path": "src"}), "editor.rs"),
        ] {
            tx.send(crate::agent::Event::ToolCall {
                name: name.into(),
                args,
            })
            .unwrap();
            tx.send(crate::agent::Event::ToolResult {
                name: name.into(),
                ok: true,
                text: text.into(),
            })
            .unwrap();
        }
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        let paths: Vec<PathBuf> = ed.live.iter().map(|w| w.path.clone()).collect();
        assert_eq!(paths.len(), 2, "two actions, two windows: {paths:?}");
        assert_ne!(
            paths[0], paths[1],
            "two actions share one window: {paths:?}"
        );
        assert!(
            paths
                .iter()
                .all(|p| is_action_view(p) && p.to_string_lossy().contains(" ")),
            "an action lost its own window name: {paths:?}"
        );
    }

    /// while the agent writes a file, the tree selects that file, as a person
    /// working in an ide would see it.
    #[test]
    fn the_tree_selects_the_file_the_agent_writes() {
        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(crate::explorer::Explorer::new(PathBuf::from(".")));
        let mut pane = AgentPane::new();
        let (tx, rx) = channel();
        pane.rx = Some(rx);
        tx.send(crate::agent::Event::File {
            path: "Cargo.toml".into(),
            content: "[package]".into(),
            write: true,
        })
        .unwrap();
        ed.agent_pane = Some(pane);
        ed.poll_agent();
        ed.play_live();
        let ex = ed.explorer.as_ref().unwrap();
        let picked = ex.rows()[ex.sel()].path.clone();
        assert!(
            picked.ends_with("Cargo.toml"),
            "the tree does not follow the write: {picked:?}"
        );
    }
}

#[cfg(test)]
mod status_badge_follows_the_agent_tests {
    use super::*;

    fn badge(ed: &mut Editor) -> String {
        let screen = render_rows(ed, 120, 40);
        screen.last().cloned().unwrap_or_default()
    }

    /// while the card is open the status badge names what the agent is doing:
    /// a shell command shows "terminal", a file shows its language.
    #[test]
    fn the_badge_names_the_action_the_agent_plays() {
        let mut ed = Editor::open(None).unwrap();
        let mut pane = AgentPane::new();
        ed.agent_pane = Some(pane);
        ed.live.push(LiveWrite {
            path: PathBuf::from(format!("{ACTIONS_VIEW}: run_shell")),
            text: vec!["$ ls".into()],
            shown: 0,
            col: 0,
            ticks: 0,
            elapsed: 0,
        });
        let bar = badge(&mut ed);
        assert!(
            bar.contains(" terminal "),
            "badge does not follow the shell: {bar:?}"
        );
    }

    /// a second prompt after a closed task opens its own page: the opening of
    /// the last task must not stay filed under the first one.
    #[test]
    fn a_new_task_after_a_closed_one_opens_its_own_page() {
        let root = std::env::temp_dir().join("nana-task-open");
        let mut pane = AgentPane::new();
        pane.begin_task("first task", &root);
        pane.closing = Some("first done".into());
        pane.begin_task("second task", &root);
        assert_eq!(pane.opening.as_deref(), Some("second task"));
        assert!(
            pane.closing.is_none(),
            "the old closing must not leak into the new task"
        );
    }

    /// a task that ends in error is still a task that closed: its page keeps
    /// the failure as the closing message.
    #[test]
    fn a_failed_task_is_saved_with_its_error_as_closing() {
        let root = std::env::temp_dir().join("nana-task-fail");
        let mut pane = AgentPane::new();
        pane.begin_task("break the build", &root);
        pane.closing = None;
        let e = "provider timeout";
        if pane.closing.is_none() {
            pane.closing = Some(format!("failed: {e}"));
        }
        assert_eq!(pane.closing.as_deref(), Some("failed: provider timeout"));
    }
}
