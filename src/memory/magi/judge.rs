//! the judgement of one page: what each sage says, and the one entry point the
//! council calls so that a new sage is one arm here and nothing else changes.

use super::{Kind, Sage, Vote};
use crate::memory::{days_from_iso, today_days, Class, Entry, Memory};

/// how many days ago a page was last used, or `None` when it is undated: an
/// undated page is not old, we simply do not know.
fn days_since_use(entry: &Entry) -> Option<i64> {
    let day = entry
        .used
        .as_deref()
        .or(entry.updated.as_deref())
        .and_then(days_from_iso)?;
    Some(today_days() - day)
}

/// true when a page has sat past the patience of its kind.
fn is_faded(entry: &Entry) -> bool {
    match days_since_use(entry) {
        Some(days) => days > Kind::of(entry.class, &entry.name).patience_days(),
        None => false,
    }
}

/// what balthasar says of a page: the episodes are the first to go, and a
/// page nobody has read in months is weight. he keeps who the user is, and he
/// keeps anything the world model leans on.
fn balthasar(entry: &Entry, linked: bool) -> Vote {
    if entry.class == Class::User || linked {
        return Vote::Keep;
    }
    if is_faded(entry) {
        Vote::Forget
    } else {
        Vote::Keep
    }
}

/// what casper says of a page: he keeps the cards, he will not bury a page the
/// world model still points at, and he only forgets what is twice past its
/// patience. he is the cautious vote.
fn casper(entry: &Entry, linked: bool) -> Vote {
    if entry.class == Class::User || entry.class == Class::Feedback {
        return Vote::Keep;
    }
    if linked {
        return Vote::Reinforce;
    }
    let kind = Kind::of(entry.class, &entry.name);
    match days_since_use(entry) {
        Some(days) if days > kind.patience_days() * 2 => Vote::Forget,
        _ => Vote::Keep,
    }
}

/// what melchior says of a page: he reads the shape of the store, not the
/// dates. a page the world model still points at is load-bearing whatever its
/// date; a page nothing points at is weight.
fn melchior(entry: &Entry, linked: bool) -> Vote {
    if linked {
        return Vote::Keep;
    }
    if is_faded(entry) {
        Vote::Forget
    } else {
        Vote::Keep
    }
}

/// asks one sage for its vote on one page. `linked` is true when another page
/// links to this one, which is what the world model reads; it is passed in so
/// the three verdicts can be compared without touching the disk again.
pub fn sage_vote(sage: Sage, entry: &Entry, linked: bool) -> Vote {
    match sage {
        Sage::Balthasar => balthasar(entry, linked),
        Sage::Casper => casper(entry, linked),
        Sage::Melchior => melchior(entry, linked),
    }
}

/// true when some page links to `name`: the store's own answer, kept here so
/// the council asks it once per page.
pub fn is_linked(memory: &Memory, name: &str) -> bool {
    !memory.backlinks(name).is_empty()
}
