//! hub ui tests of the editor screen.

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
