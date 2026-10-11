//! robustness tests of the editor screen.

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
