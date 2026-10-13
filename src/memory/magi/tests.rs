//! magi tests: each sage reads the store through its own lens, the majority
//! decides, and a link from one page protects another from being forgotten.

use super::*;
use crate::memory::Class;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nana-magi-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// writes a page whose footer says it was last written on `updated`, so a
/// test can make a page old without waiting for the clock.
fn dated(m: &Memory, class: Class, name: &str, updated: &str, body: &str) {
    let dir = m.root().join(class.id());
    std::fs::create_dir_all(&dir).unwrap();
    let text = format!(
        "# {name}\n\n{body}\n\n<!-- nana: updated {updated}, confidence medium, uses 0, used {updated} -->\n"
    );
    std::fs::write(dir.join(format!("{name}.md")), text).unwrap();
}

fn entry(m: &Memory, name: &str) -> Entry {
    m.list().into_iter().find(|e| e.name == name).unwrap()
}

#[test]
fn an_old_page_nobody_links_to_is_forgotten_by_the_majority() {
    let d = tmp("alone");
    let m = Memory::open(&d);
    dated(
        &m,
        Class::Reference,
        "old-tool",
        "2000-01-01",
        "a tool we dropped",
    );
    run(&m).unwrap();
    assert!(m.read(Class::Reference, "old-tool").is_err());
}

#[test]
fn an_old_page_another_page_links_to_survives() {
    let d = tmp("linked");
    let m = Memory::open(&d);
    dated(
        &m,
        Class::Reference,
        "old-tool",
        "2000-01-01",
        "a tool we dropped",
    );
    m.write(Class::Project, "stack", "the build uses [[old-tool]]")
        .unwrap();
    run(&m).unwrap();
    assert!(m.read(Class::Reference, "old-tool").is_ok());
}

#[test]
fn a_user_page_is_never_forgotten_even_when_old_and_alone() {
    let d = tmp("user");
    let m = Memory::open(&d);
    dated(&m, Class::User, "tone", "2000-01-01", "blunt and short");
    run(&m).unwrap();
    assert!(m.read(Class::User, "tone").is_ok());
}

#[test]
fn balthasar_forgets_an_old_conversation_and_keeps_a_fresh_one() {
    let d = tmp("episodes");
    let m = Memory::open(&d);
    dated(
        &m,
        Class::Project,
        "conversation-1",
        "2000-01-01",
        "an old episode",
    );
    m.write(Class::Project, "conversation-2", "a fresh episode")
        .unwrap();
    let old = entry(&m, "conversation-1");
    let fresh = entry(&m, "conversation-2");
    assert_eq!(sage_vote(Sage::Balthasar, &old, false), Vote::Forget);
    assert_eq!(sage_vote(Sage::Balthasar, &fresh, false), Vote::Keep);
}

#[test]
fn casper_reinforces_a_page_that_is_linked() {
    let d = tmp("graph");
    let m = Memory::open(&d);
    m.write(Class::Project, "stack", "rust, see [[build]]")
        .unwrap();
    let stack = entry(&m, "stack");
    assert_eq!(sage_vote(Sage::Casper, &stack, true), Vote::Reinforce);
}

#[test]
fn a_majority_of_two_decides_the_verdict() {
    let votes = [
        (Sage::Melchior, Vote::Forget),
        (Sage::Casper, Vote::Forget),
        (Sage::Balthasar, Vote::Keep),
    ];
    assert_eq!(tally(&votes), Vote::Forget);
}

#[test]
fn an_empty_memory_has_nothing_to_judge() {
    let d = tmp("empty");
    let m = Memory::open(&d);
    assert!(run(&m).unwrap().is_empty());
}

#[test]
fn a_reinforced_page_has_its_age_restarted_and_survives_the_verdict() {
    let d = tmp("reinforce");
    let m = Memory::open(&d);
    // a page just past the patience of a file, and one page links to it: the
    // world model keeps it, and casper restarts its age instead of burying it.
    dated(
        &m,
        Class::Reference,
        "old-tool",
        "2000-01-01",
        "a tool we dropped",
    );
    m.write(Class::Project, "stack", "the build uses [[old-tool]]")
        .unwrap();
    run(&m).unwrap();
    let used = entry(&m, "old-tool").used.unwrap_or_default();
    assert_ne!(used, "2000-01-01", "a reinforced page kept its old age");
    assert!(m.read(Class::Reference, "old-tool").is_ok());
}

#[test]
fn a_sage_does_not_bury_a_page_a_brother_kept() {
    let prior = [(Sage::Balthasar, Vote::Keep)];
    assert_eq!(after_brothers(Vote::Forget, &prior), Vote::Keep);
}

#[test]
fn a_sage_with_no_brother_yet_keeps_its_own_reading() {
    assert_eq!(after_brothers(Vote::Forget, &[]), Vote::Forget);
}

#[test]
fn a_report_reads_the_store_and_changes_nothing() {
    let d = tmp("report-only");
    let m = Memory::open(&d);
    dated(
        &m,
        Class::Reference,
        "old-tool",
        "2000-01-01",
        "a tool we dropped",
    );
    let decisions = report(&m).unwrap();
    assert_eq!(decisions[0].verdict, Vote::Forget);
    assert!(m.read(Class::Reference, "old-tool").is_ok());
}

#[test]
fn the_kind_of_a_page_is_read_from_its_class_and_its_name() {
    assert_eq!(
        Kind::of(Class::Project, "conversation-1791900490"),
        Kind::Conversation
    );
    assert_eq!(Kind::of(Class::Project, "stack"), Kind::WorldModel);
    assert_eq!(Kind::of(Class::User, "tone"), Kind::Card);
    assert_eq!(Kind::of(Class::Reference, "cargo"), Kind::File);
}

#[test]
fn each_kind_is_kept_by_one_sage_and_every_sage_keeps_something() {
    let map = kind_map();
    assert_eq!(map.len(), 4);
    for sage in Sage::ALL {
        assert!(
            map.iter().any(|(_, k)| *k == sage),
            "{} keeps no kind",
            sage.id()
        );
    }
}

#[test]
fn the_ledger_the_sages_debate_on_is_never_judged_itself() {
    let d = tmp("ledger");
    let m = Memory::open(&d);
    dated(
        &m,
        Class::Reference,
        "old-tool",
        "2000-01-01",
        "a tool we dropped",
    );
    run(&m).unwrap();
    assert!(m.read(Class::Project, "magi-council").is_ok());
}

#[test]
fn the_report_names_each_sage_and_the_verdict() {
    let d = tmp("report");
    let m = Memory::open(&d);
    dated(
        &m,
        Class::Reference,
        "old-tool",
        "2000-01-01",
        "a tool we dropped",
    );
    let decisions = run(&m).unwrap();
    let text = format_report(&decisions);
    for word in [
        "magi",
        "melchior",
        "balthasar",
        "casper",
        "old-tool",
        "forget",
    ] {
        assert!(text.contains(word), "{word} missing from {text}");
    }
}
