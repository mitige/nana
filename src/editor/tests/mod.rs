//! the editor's own tests: each one pins a rule the screen must keep, one file per topic.

use super::*;

/// the frame, row by row: the plain text helper loses the line breaks, and a
/// layout bug is exactly a question of which row something lands on.
fn render_rows(ed: &mut Editor, w: u16, h: u16) -> Vec<String> {
    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, ed)).unwrap();
    let buf = term.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

fn render_text(ed: &mut Editor, w: u16, h: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, ed)).unwrap();
    term.backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect()
}

/// a unique scratch directory per call. tests run in parallel inside one
/// process: a name built from the pid alone gets trampled, and one test's
/// cleanup then deletes another test's fixture mid-run.
fn scratch(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "nana-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

mod agent_colour;
mod agent_input;
mod agent_pane;
mod agent_view;
mod bootcamp;
mod busy_look;
mod completion;
mod diag;
mod float;
mod glitch_safety;
mod hub_hover;
mod hub_is_the_start;
mod hub_takes_the_screen;
mod hub_truncation;
mod hub_ui;
mod human_inference;
mod layout;
mod live_pace;
mod live_sync;
mod live_write;
mod pair;
mod paste;
mod persona;
mod robustness;
mod rounded_bars;
mod rounded_everywhere;
mod sober_badge;
mod status_badge_follows_the_agent;
mod statusbar_background;
mod tab;
mod terminal_look;
