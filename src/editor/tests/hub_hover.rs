//! hub hover tests of the editor screen.

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
