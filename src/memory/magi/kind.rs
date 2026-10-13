//! the kinds of page the store holds, and which sage keeps each of them.
//! a kind decides how long a page may sit unread before a sage calls it
//! faded: an episode is cheap to rewrite, a card of the user is what we know.

use super::Sage;
use crate::memory::Class;

/// one kind of memory page. the user asked for the kinds to be told apart,
/// and this is where the store's classes and names are read as kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// a conversation with the user, one project page per episode.
    Conversation,
    /// what is true of the user, of the project, or of the agent: a fact.
    Card,
    /// a page about a file of the project.
    File,
    /// a page of the world model: where things stand, and how they link.
    WorldModel,
}

impl Kind {
    /// the kind a page belongs to, read from its class and its name. a
    /// conversation is recognised by the name the agent gives it, so the same
    /// class can hold both an episode and a card.
    pub fn of(class: Class, name: &str) -> Kind {
        match class {
            Class::User | Class::Feedback => Kind::Card,
            Class::Project if name.starts_with("conversation-") => Kind::Conversation,
            Class::Project => Kind::WorldModel,
            Class::Reference => Kind::File,
        }
    }

    /// how many days a page of this kind may sit unread before a sage calls it
    /// faded. an episode is cheap to rewrite; a card is what we know.
    pub fn patience_days(&self) -> i64 {
        match self {
            Kind::Conversation => 30,
            Kind::File => 60,
            Kind::WorldModel => 90,
            Kind::Card => 3650,
        }
    }

    /// the sage whose lens this kind is: he reads it first and knows it best.
    pub fn keeper(&self) -> Sage {
        match self {
            Kind::Conversation => Sage::Balthasar,
            Kind::Card => Sage::Casper,
            Kind::File | Kind::WorldModel => Sage::Melchior,
        }
    }

    /// the lowercase name of the kind, for the report and for the ledger.
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Conversation => "conversation",
            Kind::Card => "card",
            Kind::File => "file",
            Kind::WorldModel => "world model",
        }
    }
}

/// every kind, with the sage that keeps it. the map the user asked for,
/// readable without opening the code.
pub fn kind_map() -> Vec<(Kind, Sage)> {
    [Kind::Conversation, Kind::Card, Kind::File, Kind::WorldModel]
        .into_iter()
        .map(|k| (k, k.keeper()))
        .collect()
}
