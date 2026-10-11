//! hub truncation tests of the editor screen.

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
