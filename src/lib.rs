//! nana — an adaptive terminal editor.
//!
//! one editor, every language: the language registry (`langs`) decides the
//! comment syntax, the checker, the run recipe and the icon; `project` reads
//! the repo you are in; `check`, `diag`, `run` and `header` do the work.
//! `editor` draws it and never blocks.
//!
//! version 2 adds the agentic half: `provider` speaks to every model vendor,
//! `agent` runs the tool loop, `tools` is what it can do, `memory` keeps
//! what it learns per project, `persona` and `skills` are what you can hand
//! it, and `settings` is the one file that configures all of it.

pub mod agent;
pub mod ai;
pub mod check;
pub mod config;
pub mod diag;
pub mod editor;
pub mod explorer;
pub mod header;
pub mod highlight;
pub mod langs;
pub mod memory;
pub mod persona;
pub mod project;
pub mod provider;
pub mod run;
pub mod settings;
pub mod skills;
pub mod tools;
