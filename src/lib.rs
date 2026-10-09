//! nana — an adaptive terminal editor.
//!
//! one editor, every language: the language registry (`langs`) decides the
//! comment syntax, the checker, the run recipe and the icon; `project` reads
//! the repo you are in; `check`, `diag`, `run` and `header` do the work.
//! `editor` draws it and never blocks.

pub mod ai;
pub mod check;
pub mod config;
pub mod diag;
pub mod editor;
pub mod explorer;
pub mod header;
pub mod highlight;
pub mod langs;
pub mod project;
pub mod run;
