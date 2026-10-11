//! hub is the start tests of the editor screen.

use super::*;
use crate::editor::session::start_editor;

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
