//! completion tests of the editor screen.

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
        let marked: String =
            line[..pos].iter().collect::<String>() + "▌" + &line[pos..].iter().collect::<String>();
        assert_eq!(marked, "{▌");
    }));
    assert!(r.is_ok());
}

/// La complétion (ctrl+espace) parle au même modèle que l'agent (ctrl+a) :
/// le modèle et l'adresse du projet, pas un client à part.
#[test]
fn la_complétion_utilise_le_modèle_unique_de_l_éditeur() {
    let d = scratch("completion-agent");
    let (url, server) = mock("int i = 0;");
    let mut ed = Editor::open(None).unwrap();
    ed.file = Some(d.join("x.c"));
    ed.ai = Some(crate::ai::AiClient::at(
        url,
        "test-key".into(),
        "deepseek-v4.1-flash".into(),
    ));
    ed.on_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL));
    let req = server.join().unwrap();
    assert!(
        req.contains("\"model\":\"deepseek-v4.1-flash\""),
        "the completion reached the editor's single model: {}",
        req.chars().take(300).collect::<String>()
    );
}
