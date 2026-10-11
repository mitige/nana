//! pair tests of the editor screen.

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
