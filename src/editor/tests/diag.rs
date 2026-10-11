//! diag tests of the editor screen.

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
