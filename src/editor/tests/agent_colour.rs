//! agent colour tests of the editor screen.

use super::*;

/// the agent's live view colours code the way the editor does: a keyword
/// of the file's language is not drawn in the same colour as a plain name.
#[test]
fn the_live_view_colours_code_by_its_language() {
    let mut ed = Editor::open(None).unwrap();
    let mut pane = AgentPane::new();
    pane.writes.push(LiveWrite {
        path: PathBuf::from("src/lib.rs"),
        text: vec!["fn main() {".into()],
    });
    ed.agent_pane = Some(pane);
    ed.poll_agent();
    std::thread::sleep(VIEW_SWEEP);
    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, &mut ed)).unwrap();
    let buf = term.backend().buffer().clone();
    let row = (0..40u16)
        .find(|&y| {
            (0..120u16)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains("fn main")
        })
        .expect("the line is drawn");
    let x_of = |word: &str| {
        let text: String = (0..120u16).map(|x| buf[(x, row)].symbol()).collect();
        text.find(word)
            .map(|b| text[..b].chars().count() as u16)
            .unwrap()
    };
    let keyword = buf[(x_of("fn"), row)].fg;
    let name = buf[(x_of("main"), row)].fg;
    assert_ne!(
        keyword, name,
        "a keyword and a name of the code are not the same colour"
    );
}
