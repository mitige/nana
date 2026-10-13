//! magi: three sages that read the store together and decide what it keeps.
//!
//! a memory store is not one lump: it holds kinds, and each kind ages by its
//! own clock. a conversation is cheap to rewrite, a card of the project is
//! what we know, a page of the world model says where things stand. no single
//! reader sees all of that, so the store is judged by three sages, each with
//! one lens, and the majority carries:
//!
//! * balthasar talks to the episodes: a conversation nobody links to and
//!   nobody has read in months is the first thing to go.
//! * casper talks to the cards: who the user is, what they told us, what is
//!   true of the project. he is the cautious vote, and he reinforces a page
//!   another page leans on.
//! * melchior talks to the world model: the links between pages. a page the
//!   world model still points at is load-bearing, whatever its date.
//!
//! the three communicate through the store they judge: each one writes its
//! reading onto a shared ledger, reads the others before voting, and never
//! forgets a page a brother has kept. that is how the memory kinds protect
//! each other instead of being three separate verdicts.

mod council;
mod judge;
mod kind;
mod sage;

#[cfg(test)]
mod tests;

// the tests read the store directly, so the store's own names are kept in
// reach here rather than repeated in every helper.
pub use crate::memory::{Entry, Memory};
pub use std::path::PathBuf;

pub use council::{after_brothers, format_report, report, run, Decision};
pub use judge::sage_vote;
pub use kind::{kind_map, Kind};
pub use sage::{tally, Sage, Vote};
