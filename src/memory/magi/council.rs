//! the council: the three sages read the store together, debate it, and carry
//! the majority verdict.
//!
//! the sages are autonomous in the sense that matters: none of them holds the
//! whole decision. each one reads the store through its own lens and weighs
//! what its brothers said before it votes, so a page a brother kept is never
//! buried by the next sage. when the council is asked to act, the ledger it
//! debates on is itself a page of the store, and the memory modules really do
//! use each other instead of sharing a hidden state.

use super::judge::{is_linked, sage_vote};
use super::{Kind, Sage, Vote};
use crate::memory::{Class, Confidence, Memory};

/// the one page all three sages write to when they act. it makes the council a
/// debate rather than three separate verdicts, and it is rewritten at the
/// start of every acting pass so it never grows without end.
const LEDGER: &str = "magi-council";

/// one page and what the council decided about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub class: Class,
    /// the kind of the page, as the sages read it.
    pub kind: Kind,
    pub name: String,
    /// one vote per sage, in `Sage::ALL` order.
    pub votes: Vec<(Sage, Vote)>,
    /// the majority verdict.
    pub verdict: Vote,
}

/// how one pass treats the store: a report reads and changes nothing the sages
/// did not write, an acting pass may retire the pages the majority forgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    Report,
    Act,
}

/// true when a page is the council's own ledger: the sages do not judge their
/// own debate, or they would bury the page they talk on.
fn is_ledger(entry: &crate::memory::Entry) -> bool {
    entry.class == Class::Project && entry.name == LEDGER
}

/// the reading one sage leaves for the others, in one lowercase line.
fn ledger_line(sage: Sage, class: Class, name: &str, vote: Vote) -> String {
    format!(
        "- {} ({}) on {} {}: {}\n",
        sage.id(),
        sage.lens(),
        class.id(),
        name,
        vote.word()
    )
}

/// opens a fresh ledger for one acting pass, so the debate of this pass is
/// what the sages read and yesterday's verdicts do not linger.
fn open_ledger(memory: &Memory) {
    let header = "# magi council\n\nthe three sages read the store together.\n";
    let _ = memory.write_rated(Class::Project, LEDGER, header, Confidence::Low);
}

/// appends one sage's reading to the ledger, keeping what the brothers wrote.
/// a failed write must not stop the judgement: the error is dropped and the
/// vote still stands.
fn note_on_ledger(memory: &Memory, line: &str) {
    let mut text = memory.read(Class::Project, LEDGER).unwrap_or_default();
    text.push_str(line);
    let _ = memory.write_rated(Class::Project, LEDGER, &text, Confidence::Low);
}

/// true when a brother already kept the page. a sage will not bury what
/// another sage decided to keep: that is the protection the three owe each
/// other, and it is what makes the council one mind instead of three.
fn a_brother_kept(prior: &[(Sage, Vote)]) -> bool {
    prior.iter().any(|(_, v)| !v.forgets())
}

/// the vote a sage casts once it has heard its brothers: its own reading,
/// unless a brother already kept the page, in which case it will not bury it.
/// kept apart from the council so the rule can be read and tested on its own.
pub fn after_brothers(own: Vote, prior: &[(Sage, Vote)]) -> Vote {
    if own.forgets() && a_brother_kept(prior) {
        Vote::Keep
    } else {
        own
    }
}

/// asks one sage to vote, after it has read what its brothers said. this is
/// the seam a new sage plugs into: one arm here, and nothing else moves.
fn vote_after_debate(
    sage: Sage,
    entry: &crate::memory::Entry,
    linked: bool,
    prior: &[(Sage, Vote)],
    memory: &Memory,
    reach: Reach,
) -> Vote {
    let vote = after_brothers(sage_vote(sage, entry, linked), prior);
    if reach == Reach::Act {
        note_on_ledger(memory, &ledger_line(sage, entry.class, &entry.name, vote));
    }
    vote
}

/// applies a verdict to the store. a page the majority forgets is retired
/// with its history kept; a page a sage reinforced has its use counted, so its
/// age restarts and it will not come up for judgement again next pass.
fn apply_verdict(
    entry: &crate::memory::Entry,
    votes: &[(Sage, Vote)],
    verdict: Vote,
    memory: &Memory,
) -> Result<(), String> {
    if verdict.forgets() {
        return memory.retire(entry.class, &entry.name);
    }
    if votes.iter().any(|(_, v)| *v == Vote::Reinforce) {
        return memory.reinforce(entry.class, &entry.name);
    }
    Ok(())
}

/// judges one page: the three sages vote in turn, each reading the last, and
/// the verdict is their majority.
fn judge_page(
    entry: &crate::memory::Entry,
    linked: bool,
    memory: &Memory,
    reach: Reach,
) -> (Vec<(Sage, Vote)>, Vote) {
    let mut votes: Vec<(Sage, Vote)> = Vec::new();
    for sage in Sage::ALL {
        let vote = vote_after_debate(sage, entry, linked, &votes, memory, reach);
        votes.push((sage, vote));
    }
    let verdict = super::tally(&votes);
    (votes, verdict)
}

/// the whole pass: every page of the store is put to the three sages, and an
/// acting pass retires the pages the majority forgets, their history kept so
/// they can still be read back.
fn deliberate(memory: &Memory, reach: Reach) -> Result<Vec<Decision>, String> {
    if reach == Reach::Act {
        open_ledger(memory);
    }
    let mut out = Vec::new();
    for entry in memory.list() {
        if is_ledger(&entry) {
            continue;
        }
        let linked = is_linked(memory, &entry.name);
        let (votes, verdict) = judge_page(&entry, linked, memory, reach);
        if reach == Reach::Act {
            apply_verdict(&entry, &votes, verdict, memory)?;
        }
        out.push(Decision {
            class: entry.class,
            kind: Kind::of(entry.class, &entry.name),
            name: entry.name,
            votes,
            verdict,
        });
    }
    Ok(out)
}

/// what the council would do: it reads the store and reports, and nothing is
/// retired. the sages debate in memory, so a report leaves no trace.
pub fn report(memory: &Memory) -> Result<Vec<Decision>, String> {
    deliberate(memory, Reach::Report)
}

/// the council acts: what the majority forgets is retired, its history kept.
pub fn run(memory: &Memory) -> Result<Vec<Decision>, String> {
    deliberate(memory, Reach::Act)
}

/// the report of one pass: what each sage said of each page, and the verdict
/// the majority reached.
pub fn format_report(decisions: &[Decision]) -> String {
    let mut out = String::from("magi — three sages read the store\n");
    for d in decisions {
        let said: Vec<String> = d
            .votes
            .iter()
            .map(|(sage, vote)| format!("{}: {}", sage.id(), vote.word()))
            .collect();
        out.push_str(&format!(
            "{} {} ({}) — {} — verdict: {}\n",
            d.class.id(),
            d.name,
            d.kind.label(),
            said.join(", "),
            d.verdict.word()
        ));
    }
    out
}
