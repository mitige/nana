//! drawing: every frame the editor paints, in one place, as free functions.
//! this file only declares the drawing children and re-exports them, so each
//! concept lives in its own file under render/.

mod agent;
mod body;
mod explorer;
mod hub;
mod icons;
mod notices;
mod paint;
mod scene;
mod search_float;
mod statusbar;
mod terminal_pane;
mod text;
mod topbar;

pub(in crate::editor) use agent::*;
pub(in crate::editor) use body::*;
pub(in crate::editor) use explorer::*;
pub(in crate::editor) use hub::*;
pub(in crate::editor) use icons::*;
pub(in crate::editor) use notices::*;
pub(in crate::editor) use paint::*;
pub(in crate::editor) use scene::*;
pub(in crate::editor) use search_float::*;
pub(in crate::editor) use statusbar::*;
pub(in crate::editor) use terminal_pane::*;
pub(in crate::editor) use text::*;
pub(in crate::editor) use topbar::*;
