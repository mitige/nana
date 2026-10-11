//! memory tests: store round trips, freshness rules, consolidation parsing.

use super::calendar::{civil_from_days, days_from_civil};
use super::*;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nana-mem-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_page_round_trips_as_markdown() {
    let d = tmp("rt");
    let m = Memory::open(&d);
    let path = m.write(Class::Project, "stack", "rust, no async").unwrap();
    assert!(path.ends_with(".nana/memory/project/stack.md"), "{path:?}");
    let text = m.read(Class::Project, "stack").unwrap();
    assert!(text.starts_with("# stack"), "{text}");
    assert!(text.contains("rust, no async"), "{text}");
    assert!(
        std::fs::metadata(&path).unwrap().is_file(),
        "readable on disk"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_body_that_already_has_a_heading_is_kept_as_is() {
    let d = tmp("head");
    let m = Memory::open(&d);
    m.write(Class::User, "tone", "## my heading\n\nblunt")
        .unwrap();
    let t = m.read(Class::User, "tone").unwrap();
    assert!(t.starts_with("## my heading"), "{t}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn names_cannot_escape_the_project() {
    let d = tmp("escape");
    let m = Memory::open(&d);
    for bad in ["../secrets", "a/b", "..", ""] {
        assert!(
            m.write(Class::Project, bad, "x").is_err(),
            "{bad} was allowed"
        );
    }
    // and nothing was written outside the memory root
    assert!(!d.join("secrets.md").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn memory_is_scoped_to_one_project() {
    let a = tmp("a");
    let b = tmp("b");
    let ma = Memory::open(&a);
    let mb = Memory::open(&b);
    ma.write(Class::Project, "only-a", "secret of a").unwrap();
    assert!(mb.read(Class::Project, "only-a").is_err(), "b sees a");
    assert!(mb.list().is_empty(), "b has nothing");
    assert_eq!(ma.list().len(), 1);
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

#[test]
fn search_finds_lines_and_the_index_lists_pages() {
    let d = tmp("search");
    let m = Memory::open(&d);
    m.write(
        Class::Project,
        "stack",
        "we write rust\nand never javascript",
    )
    .unwrap();
    m.write(Class::User, "tone", "blunt, no fluff").unwrap();
    let hits = m.search("rust");
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].line, 3,
        "the heading, then a blank line, then the fact"
    );
    let idx = m.index();
    assert!(idx.contains("project: stack"), "{idx}");
    assert!(idx.contains("user: tone"), "{idx}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn forgetting_removes_only_that_page() {
    let d = tmp("forget");
    let m = Memory::open(&d);
    m.write(Class::Reference, "one", "a").unwrap();
    m.write(Class::Reference, "two", "b").unwrap();
    m.forget(Class::Reference, "one").unwrap();
    assert_eq!(m.list().len(), 1);
    assert_eq!(m.list()[0].name, "two");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_new_page_is_dated_and_rated_medium_by_default() {
    let d = tmp("dated");
    let m = Memory::open(&d);
    m.write(Class::Project, "stack", "rust only").unwrap();
    let text = m.read(Class::Project, "stack").unwrap();
    assert!(
        text.starts_with("# stack"),
        "the heading still comes first: {text}"
    );
    assert!(
        text.contains("<!-- nana: updated ") && text.contains("confidence medium"),
        "{text}"
    );
    let e = &m.list()[0];
    assert_eq!(e.updated.as_deref(), Some(today_iso().as_str()));
    assert_eq!(e.confidence, Some(Confidence::Medium));
    assert!(!e.stale(), "a page written now is not stale");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_writer_can_rate_its_own_confidence() {
    let d = tmp("rated");
    let m = Memory::open(&d);
    m.write_rated(Class::Project, "guess", "maybe async", Confidence::Low)
        .unwrap();
    let e = &m.list()[0];
    assert_eq!(e.confidence, Some(Confidence::Low));
    assert_eq!(Confidence::parse(" High "), Some(Confidence::High));
    assert_eq!(Confidence::parse("sure"), None);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_page_older_than_the_limit_is_stale() {
    let d = tmp("stale");
    let m = Memory::open(&d);
    let dir = d.join(".nana/memory/project");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("old.md"),
        "# old\n\nold fact\n\n<!-- nana: updated 2001-01-01, confidence high -->\n",
    )
    .unwrap();
    let e = m.list().into_iter().find(|e| e.name == "old").unwrap();
    assert!(e.stale(), "a page from 2001 must be stale");
    assert_eq!(e.confidence, Some(Confidence::High));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_page_without_a_date_is_undated_not_stale() {
    let d = tmp("undated");
    let m = Memory::open(&d);
    let dir = d.join(".nana/memory/user");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tone.md"), "# tone\n\nblunt\n").unwrap();
    let e = m.list().into_iter().find(|e| e.name == "tone").unwrap();
    assert_eq!(e.updated, None);
    assert_eq!(e.confidence, None);
    assert!(!e.stale(), "no date means no claim of staleness");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn tidy_removes_stale_pages_and_copies_but_keeps_user_pages() {
    let d = tmp("tidy");
    let m = Memory::open(&d);
    let project = d.join(".nana/memory/project");
    let user = d.join(".nana/memory/user");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    let today = today_iso();
    std::fs::write(
        project.join("old.md"),
        "# old\n\nold fact\n\n<!-- nana: updated 2001-01-01, confidence high -->\n",
    )
    .unwrap();
    std::fs::write(
        user.join("old-tone.md"),
        "# old tone\n\nblunt\n\n<!-- nana: updated 2001-01-01, confidence high -->\n",
    )
    .unwrap();
    let body = "# a\n\nthe same words in the same order\n";
    std::fs::write(
        project.join("a.md"),
        format!("{body}\n<!-- nana: updated {today}, confidence high -->\n"),
    )
    .unwrap();
    std::fs::write(
        project.join("b.md"),
        format!("{body}\n<!-- nana: updated {today}, confidence high -->\n"),
    )
    .unwrap();

    let report = m.tidy();

    assert!(
        !project.join("old.md").exists(),
        "a stale project page goes"
    );
    assert!(
        user.join("old-tone.md").exists(),
        "a user page is never dropped for age"
    );
    let copies = [project.join("a.md").exists(), project.join("b.md").exists()];
    assert_eq!(
        copies.iter().filter(|x| **x).count(),
        1,
        "one of two copies stays"
    );
    assert_eq!(
        report.removed.len(),
        2,
        "the stale page and the copy: {report:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn tidy_merges_pages_that_say_nearly_the_same_thing() {
    let d = tmp("tidy-near");
    let m = Memory::open(&d);
    let project = d.join(".nana/memory/project");
    std::fs::create_dir_all(&project).unwrap();
    let today = today_iso();
    let footer = format!("\n<!-- nana: updated {today}, confidence high -->\n");
    let shared = "the build uses cargo and the tests run in ci on every push to main";
    std::fs::write(
        project.join("build.md"),
        format!("# build\n\n{shared} with the release notes\n{footer}"),
    )
    .unwrap();
    std::fs::write(
        project.join("build-ci.md"),
        format!("# build ci\n\n{shared} plus the release notes\n{footer}"),
    )
    .unwrap();
    std::fs::write(
        project.join("colours.md"),
        format!("# colours\n\nthe hub card is teal and the search is amber\n{footer}"),
    )
    .unwrap();

    let report = m.tidy();

    let left = m.list().len();
    assert_eq!(left, 2, "two near-copies become one page: {report:?}");
    assert!(
        project.join("colours.md").exists(),
        "a page about another thing stays"
    );
    assert_eq!(report.removed.len(), 1, "{report:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn calendar_dates_convert_both_ways() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(civil_from_days(19723), (2024, 1, 1));
    let leap = days_from_iso("2024-02-29").unwrap();
    assert_eq!(civil_from_days(leap), (2024, 2, 29));
    assert_eq!(days_from_iso("2024-13-01"), None);
    assert_eq!(days_from_iso("not a date"), None);
}

#[test]
fn a_page_links_to_others_with_double_brackets() {
    let d = tmp("links");
    let m = Memory::open(&d);
    m.write(
        Class::Project,
        "stack",
        "see [[deploy]] and [[ tone ]], not [[]]",
    )
    .unwrap();
    m.write(Class::Project, "deploy", "uses [[stack]]").unwrap();
    assert_eq!(
        m.links(Class::Project, "stack").unwrap(),
        vec!["deploy", "tone"]
    );
    assert_eq!(m.backlinks("deploy"), vec!["stack"]);
    assert!(m.backlinks("nobody").is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_page_is_recalled_when_the_request_touches_a_file_it_names() {
    let d = tmp("filelink");
    let m = Memory::open(&d);
    m.write(
        Class::Project,
        "token-loop",
        "# token loop\n\nthe loop that reads tokens is in src/lexer.rs, keep it iterative",
    )
    .unwrap();
    m.write(Class::Project, "release", "# release\n\ntag then push")
        .unwrap();
    let hit = m.recall("fix the bug in src/lexer.rs", 4000);
    assert!(
        hit.contains("keep it iterative"),
        "the page naming the touched file is recalled: {hit}"
    );
    let miss = m.recall("fix the bug in src/parser.rs", 4000);
    assert!(
        !miss.contains("keep it iterative"),
        "a page naming another file is not pulled in: {miss}"
    );
}

#[test]
fn recall_brings_the_body_of_the_pages_the_request_touches() {
    let d = tmp("recall");
    let m = Memory::open(&d);
    m.write(
        Class::Project,
        "parser",
        "# parser\n\npratt loop, no recursion past depth 8",
    )
    .unwrap();
    m.write(Class::Project, "release", "# release\n\ntag then push")
        .unwrap();
    m.write(Class::User, "tone", "blunt, no filler").unwrap();
    let r = m.recall("refactor the parser", 4000);
    assert!(
        r.contains("pratt loop"),
        "the touched page is recalled: {r}"
    );
    assert!(
        !r.contains("tag then push"),
        "an unrelated page is not pulled in: {r}"
    );
    assert!(
        r.contains("blunt, no filler"),
        "user preferences are always recalled: {r}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn recall_marks_a_stale_or_unsure_page_so_it_is_checked_before_use() {
    let d = tmp("recall-stale");
    let m = Memory::open(&d);
    let dir = d.join(".nana/memory/project");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("parser.md"),
        "# parser\n\nold parser fact\n\n<!-- nana: updated 2001-01-01, confidence low -->\n",
    )
    .unwrap();
    let r = m.recall("the parser", 4000);
    assert!(r.contains("old parser fact"), "{r}");
    assert!(
        r.contains("stale"),
        "a page from 2001 says it is stale: {r}"
    );
    assert!(r.contains("low"), "an unsure page says so: {r}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn recall_stays_within_its_character_budget() {
    let d = tmp("recall-budget");
    let m = Memory::open(&d);
    m.write(Class::Project, "parser", &"parser ".repeat(2000))
        .unwrap();
    let r = m.recall("the parser", 500);
    assert!(r.chars().count() <= 500, "{} chars", r.chars().count());
    assert!(r.contains("parser"), "the start of the page is kept: {r}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_audit_finds_pages_that_say_the_same_thing_and_stale_ones() {
    let d = tmp("audit");
    let m = Memory::open(&d);
    m.write(Class::Project, "stack", "rust only, no async")
        .unwrap();
    m.write(Class::Reference, "stack-copy", "Rust only,  no async\n")
        .unwrap();
    m.write(Class::User, "tone", "blunt").unwrap();
    let dir = d.join(".nana/memory/project");
    std::fs::write(
        dir.join("ancient.md"),
        "# ancient\n\nold fact\n\n<!-- nana: updated 2001-01-01, confidence high -->\n",
    )
    .unwrap();
    let audit = m.audit();
    assert_eq!(
        audit.duplicates,
        vec![(
            "project/stack".to_string(),
            "reference/stack-copy".to_string()
        )],
        "the two pages with the same words are paired, whatever the case"
    );
    assert_eq!(audit.stale, vec!["project/ancient".to_string()]);
    let _ = std::fs::remove_dir_all(&d);
}

fn days_ago(n: i64) -> String {
    iso_from_days(days_now() - n)
}

#[test]
fn a_page_read_again_counts_its_use_and_restarts_its_age() {
    let d = tmp("use");
    let m = Memory::open(&d);
    let dir = d.join(".nana/memory/project");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("old.md"),
        "# old\n\nfact\n\n<!-- nana: updated 2001-01-01, confidence high -->\n",
    )
    .unwrap();
    m.reinforce(Class::Project, "old").unwrap();
    let e = m.list().into_iter().find(|e| e.name == "old").unwrap();
    assert_eq!(e.uses, 1, "one use is counted");
    assert_eq!(e.used.as_deref(), Some(today_iso().as_str()));
    assert!(!e.forgotten(), "a page just used is kept");
    assert_eq!(
        e.updated.as_deref(),
        Some("2001-01-01"),
        "a use is not an edit"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_unused_page_fades_and_an_often_used_one_fades_slower() {
    let d = tmp("fade");
    let m = Memory::open(&d);
    let dir = d.join(".nana/memory/project");
    std::fs::create_dir_all(&dir).unwrap();
    let day = days_ago(100);
    std::fs::write(
        dir.join("lazy.md"),
        format!(
            "# lazy\n\nx\n\n<!-- nana: updated {day}, confidence high, uses 0, used {day} -->\n"
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("busy.md"),
        format!(
            "# busy\n\nx\n\n<!-- nana: updated {day}, confidence high, uses 10, used {day} -->\n"
        ),
    )
    .unwrap();
    let list = m.list();
    let lazy = list.iter().find(|e| e.name == "lazy").unwrap();
    let busy = list.iter().find(|e| e.name == "busy").unwrap();
    assert!(
        lazy.forgotten(),
        "100 days unused, half-life 30 days: {}",
        lazy.retention()
    );
    assert!(
        !busy.forgotten(),
        "ten uses stretch the half-life: {}",
        busy.retention()
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_audit_lists_faded_pages_but_never_a_user_page() {
    let d = tmp("audit-faded");
    let m = Memory::open(&d);
    let project = d.join(".nana/memory/project");
    let user = d.join(".nana/memory/user");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&user).unwrap();
    let day = days_ago(100);
    let footer = format!("<!-- nana: updated {day}, confidence high, uses 0, used {day} -->");
    std::fs::write(project.join("faded.md"), format!("# f\n\nx\n\n{footer}\n")).unwrap();
    std::fs::write(user.join("tone.md"), format!("# t\n\nx\n\n{footer}\n")).unwrap();
    let audit = m.audit();
    assert_eq!(
        audit.forgotten,
        vec!["project/faded".to_string()],
        "{audit:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn recall_counts_each_page_it_hands_over() {
    let d = tmp("recall-use");
    let m = Memory::open(&d);
    m.write(Class::Project, "parser", "# parser\n\npratt loop")
        .unwrap();
    let _ = m.recall("the parser", 4000);
    let e = m.list().into_iter().find(|e| e.name == "parser").unwrap();
    assert_eq!(e.uses, 1, "the recalled page is used once");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn rewriting_a_page_keeps_its_use_count() {
    let d = tmp("keep-uses");
    let m = Memory::open(&d);
    m.write(Class::Project, "p", "# p\n\nfirst").unwrap();
    m.reinforce(Class::Project, "p").unwrap();
    m.write(Class::Project, "p", "# p\n\nsecond").unwrap();
    let e = m.list().into_iter().find(|e| e.name == "p").unwrap();
    assert_eq!(e.uses, 1, "an edit does not erase the use count");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn every_write_is_kept_in_the_history_with_its_day() {
    let d = tmp("hist");
    let m = Memory::open(&d);
    m.write(Class::User, "tone", "blunt").unwrap();
    m.write(Class::User, "tone", "blunt and short").unwrap();
    m.write(Class::User, "other", "x").unwrap();
    let h = m.history(Class::User, "tone");
    assert_eq!(h.len(), 2, "{h:?}");
    assert_eq!(h[0].0, today_iso(), "each entry is dated");
    assert!(m.history(Class::User, "other").len() == 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_model_answer_is_read_into_summaries() {
    let text = "sure: {\"summaries\": [{\"class\": \"project\", \"name\": \"tooling\", \
                \"body\": \"# tooling\\n\\ncargo and fmt\", \
                \"replaces\": [\"project/a\", \"project/b\"]}]} thanks";
    let plan = parse_consolidation(text).expect("a plan");
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].class, Class::Project);
    assert_eq!(plan[0].name, "tooling");
    assert_eq!(plan[0].replaces, vec!["project/a", "project/b"]);
    assert!(parse_consolidation("no json here").is_err());
}

#[test]
fn consolidation_keeps_the_summary_and_the_history_of_what_it_replaces() {
    let d = tmp("consolidate");
    let m = Memory::open(&d);
    m.write(Class::Project, "a", "# a\n\nfirst fact").unwrap();
    m.write(Class::Project, "b", "# b\n\nsecond fact").unwrap();
    m.write(Class::User, "tone", "blunt").unwrap();
    let plan = vec![Summary {
        class: Class::Project,
        name: "ab".into(),
        body: "# ab\n\nboth facts".into(),
        replaces: vec!["project/a".into(), "project/b".into(), "user/tone".into()],
    }];
    let removed = apply_consolidation(&m, &plan).unwrap();
    assert_eq!(removed, vec!["project/a", "project/b"]);
    let names: Vec<String> = m.list().into_iter().map(|e| e.name).collect();
    assert!(names.contains(&"ab".to_string()));
    assert!(!names.contains(&"a".to_string()));
    assert!(
        names.contains(&"tone".to_string()),
        "a user page is never merged away"
    );
    let h = m.history(Class::Project, "a");
    assert_eq!(h.len(), 2, "the write and the removal are both kept: {h:?}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn consolidation_is_due_on_overlap_and_not_again_on_the_same_memory() {
    let d = tmp("due");
    let m = Memory::open(&d);
    m.write(Class::Project, "a", "# a\n\nthe same words here")
        .unwrap();
    m.write(Class::Project, "b", "# b\n\nthe same words here")
        .unwrap();
    let mut trig = ConsolidationTrigger::default();
    assert!(trig.due(&m), "overlapping pages make consolidation due");
    trig.done(&m);
    assert!(!trig.due(&m), "the same memory is not asked again");
    m.write(Class::Project, "c", "# c\n\nthe same words here")
        .unwrap();
    assert!(trig.due(&m), "a changed memory re-arms the trigger");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_memory_without_overlap_is_not_consolidated() {
    let d = tmp("nodue");
    let m = Memory::open(&d);
    m.write(Class::Project, "a", "# a\n\nalpha beta").unwrap();
    m.write(Class::Project, "b", "# b\n\ngamma delta").unwrap();
    assert!(!ConsolidationTrigger::default().due(&m));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn tidy_keeps_a_page_that_is_still_used_even_if_written_long_ago() {
    let d = tmp("tidy-used");
    let m = Memory::open(&d);
    let dir = d.join(".nana/memory/project");
    std::fs::create_dir_all(&dir).unwrap();
    let written = days_ago(120);
    let used = days_ago(1);
    std::fs::write(
        dir.join("live.md"),
        format!("# live\n\nfact\n\n<!-- nana: updated {written}, confidence high, uses 5, used {used} -->\n"),
    )
    .unwrap();
    let report = m.tidy();
    assert!(
        dir.join("live.md").exists(),
        "a page read yesterday is not removed for its write date: {report:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_use_alone_does_not_rearm_consolidation() {
    let d = tmp("due-use");
    let m = Memory::open(&d);
    m.write(Class::Project, "a", "# a\n\nthe same words here")
        .unwrap();
    m.write(Class::Project, "b", "# b\n\nthe same words here")
        .unwrap();
    let mut trig = ConsolidationTrigger::default();
    trig.done(&m);
    m.reinforce(Class::Project, "a").unwrap();
    assert!(
        !trig.due(&m),
        "reading a page changes its counter, not what it says: no second model call"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_conversation_keeps_its_opening_and_closing_messages() {
    let d = tmp("turn");
    let m = Memory::open(&d);
    let p = m
        .save_turn("1700000000", "build the parser", "parser done, tests green")
        .unwrap();
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(
        text.contains("build the parser"),
        "opening missing:\n{text}"
    );
    assert!(
        text.contains("parser done, tests green"),
        "closing missing:\n{text}"
    );
    assert!(p.ends_with("conversation-1700000000.md"), "{}", p.display());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn consolidate_asks_the_model_then_applies_its_plan() {
    let d = tmp("consolidate-model");
    let m = Memory::open(&d);
    m.write(Class::Reference, "x", "# x\n\none").unwrap();
    m.write(Class::Reference, "y", "# y\n\ntwo").unwrap();
    let mut asked = String::new();
    let removed = consolidate(&m, |prompt| {
        asked = prompt.to_string();
        Ok(
            "{\"summaries\": [{\"class\": \"reference\", \"name\": \"xy\", \
            \"body\": \"# xy\\n\\none and two\", \
            \"replaces\": [\"reference/x\", \"reference/y\"]}]}"
                .to_string(),
        )
    })
    .unwrap();
    assert!(
        asked.contains("reference/x") && asked.contains("two"),
        "the model sees the pages"
    );
    assert_eq!(removed.len(), 2);
    assert!(m.list().iter().any(|e| e.name == "xy"));
    let _ = std::fs::remove_dir_all(&d);
}
