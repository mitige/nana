//! the hub: every box in one panel.
//!
//! this file only declares the concepts of the hub and re-exports what the
//! rest of the crate uses; the code of each concept lives in src/hub/.

mod action;
mod apply;
mod input;
mod item;
mod items;
mod navigation;
mod outcome;
mod pages;
mod purpose;
mod section;
mod state;
mod sweep;
mod work;

#[cfg(test)]
mod tests;

use crate::company::{dispatch, plan_hiring};
use crate::memory::magi;
use crate::memory::{Class, Memory};
use crate::persona;
use crate::settings::{self, Settings};
use crate::skills;
use crate::world;
use std::path::{Path, PathBuf};

pub use action::Action;
pub use apply::apply;
pub use item::Item;
pub use outcome::Outcome;

pub use purpose::Purpose;
pub use section::{section_titles, Section};
pub use state::Hub;
pub use sweep::{sweep_at, SWEEP};
