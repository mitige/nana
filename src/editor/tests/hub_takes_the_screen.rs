//! hub takes the screen tests of the editor screen.

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
