//! the three sages and their votes. a sage is one lens on the store, and the
//! tally is the one place where the three are heard together.

/// one lens on the store. the three carry the names of the magi of neo
/// genesis evangelion, as the user asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sage {
    /// the episodes: the conversations with the user.
    Balthasar,
    /// the cards: the user, the feedback, the facts of the project.
    Casper,
    /// the world model: the links and the shape of the store.
    Melchior,
}

impl Sage {
    /// every sage, in the order they are asked. the order matters: the first
    /// one to keep a page stops the next one from forgetting it.
    pub const ALL: [Sage; 3] = [Sage::Balthasar, Sage::Casper, Sage::Melchior];

    /// the lowercase name used on the ledger and in the report.
    pub fn id(&self) -> &'static str {
        match self {
            Sage::Balthasar => "balthasar",
            Sage::Casper => "casper",
            Sage::Melchior => "melchior",
        }
    }

    /// what this sage watches, in one lowercase line.
    pub fn lens(&self) -> &'static str {
        match self {
            Sage::Balthasar => "episodes",
            Sage::Casper => "cards",
            Sage::Melchior => "world model",
        }
    }
}

/// what one sage thinks of one page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vote {
    /// the page still earns its place.
    Keep,
    /// the page has faded and may go.
    Forget,
    /// the page has been read lately: its age restarts instead of running on.
    Reinforce,
}

impl Vote {
    /// true when the vote would drop the page.
    pub fn forgets(&self) -> bool {
        *self == Vote::Forget
    }

    /// the lowercase word used in the report.
    pub fn word(&self) -> &'static str {
        match self {
            Vote::Keep => "keep",
            Vote::Forget => "forget",
            Vote::Reinforce => "reinforce",
        }
    }
}

/// the verdict of the three. a majority carries; a tie keeps the page,
/// because forgetting is the one act we cannot undo cheaply.
pub fn tally(votes: &[(Sage, Vote)]) -> Vote {
    let forgets = votes.iter().filter(|(_, v)| v.forgets()).count();
    if forgets * 2 > votes.len() {
        Vote::Forget
    } else {
        Vote::Keep
    }
}
