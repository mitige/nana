//! agent pane tests of the editor screen.

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
