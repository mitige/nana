//! knowledge: the project's own wiki, and the dream that consolidates it.
//!
//! pages are markdown files under `<project>/.nana/knowledge/`, read on demand
//! with grep and read — no embeddings to manage, no index to rebuild. the
//! agent is told what pages exist, never their contents, and opens one when it
//! matters.
//!
//! `dream` reads the last sessions of the project and writes back what they
//! taught: new or updated pages, plus a dated page under `audit/` so the
//! change itself is reviewable with `git diff`.

use crate::agent::Agent;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn dir(root: &Path) -> PathBuf {
    root.join(".nana").join("knowledge")
}

pub fn root_of(root: &Path) -> PathBuf {
    dir(root)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub name: String,
    pub title: String,
    pub path: PathBuf,
    pub lines: usize,
}

fn parse_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string()
}

fn title_of(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.trim_start().starts_with('#'))
                .map(|l| l.trim_start_matches('#').trim().to_string())
        })
        .unwrap_or_else(|| parse_name(path))
}

/// every page, audit pages last (they are history, not knowledge).
pub fn list(root: &Path) -> Vec<Page> {
    let mut out: Vec<Page> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir(root)) else {
        return out;
    };
    for e in rd.flatten() {
        let path = e.path();
        if path.is_dir() {
            continue;
        }
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        out.push(Page {
            name: parse_name(&path),
            title: title_of(&path),
            path,
            lines: text.lines().count(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn audit_list(root: &Path) -> Vec<Page> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir(root).join("audit")) else {
        return out;
    };
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        out.push(Page {
            name: parse_name(&path),
            title: title_of(&path),
            lines: std::fs::read_to_string(&path)
                .map(|t| t.lines().count())
                .unwrap_or(0),
            path,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn read(root: &Path, name: &str) -> Result<String, String> {
    let path = safe_path(&dir(root), name)?;
    std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn write(root: &Path, name: &str, body: &str) -> Result<PathBuf, String> {
    write_in(&dir(root), name, body)
}

fn write_in(base: &Path, name: &str, body: &str) -> Result<PathBuf, String> {
    let path = safe_path(base, name)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, body.trim_end().to_string() + "\n").map_err(|e| e.to_string())?;
    Ok(path)
}

/// a page name is a file name and nothing else.
fn safe_path(base: &Path, name: &str) -> Result<PathBuf, String> {
    let clean = name.trim().trim_end_matches(".md").replace(' ', "-");
    if clean.is_empty() {
        return Err("a page needs a name".into());
    }
    if clean.contains('/') || clean.contains('\\') || clean.contains("..") {
        return Err(format!("« {name} » is not a plain name"));
    }
    Ok(base.join(format!("{clean}.md")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub page: String,
    pub line: usize,
    pub text: String,
}

pub fn search(root: &Path, needle: &str) -> Vec<Hit> {
    let needle = needle.to_lowercase();
    let mut out = Vec::new();
    for page in list(root).into_iter().chain(audit_list(root)) {
        let Ok(text) = std::fs::read_to_string(&page.path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                out.push(Hit {
                    page: page.name.clone(),
                    line: i + 1,
                    text: line.trim().to_string(),
                });
            }
        }
    }
    out
}

/// what the agent is shown: the names, never the contents.
pub fn index(root: &Path) -> String {
    let pages = list(root);
    if pages.is_empty() {
        return String::new();
    }
    let mut out = String::from("knowledge pages of this project:\n");
    for p in pages {
        out.push_str(&format!("- {}: {}\n", p.name, p.title));
    }
    out
}

/// the whole wiki, for the hub and for the shell.
pub fn describe(root: &Path) -> String {
    let pages = list(root);
    let audits = audit_list(root);
    let mut out = format!(
        "the project's wiki: {} page(s), {} audit page(s)\n\
         written by hand, or by /dream from the latest sessions.\n\n",
        pages.len(),
        audits.len()
    );
    if pages.is_empty() {
        out.push_str("no pages yet — write one, or dream over the sessions.");
        return out;
    }
    for p in pages {
        out.push_str(&format!(
            "  {:<20} {:<40} {} lines\n",
            p.name, p.title, p.lines
        ));
    }
    if !audits.is_empty() {
        out.push_str("\nrecent audits:\n");
        for a in audits.iter().rev().take(5) {
            out.push_str(&format!("  {}\n", a.name));
        }
    }
    out
}

// ------------------------------------------------------------------ dreaming

#[derive(Debug, Clone, Default)]
pub struct DreamReport {
    pub pages: Vec<String>,
    pub audit: Option<PathBuf>,
    pub read_sessions: usize,
}

/// the prompt that turns a pile of transcripts into pages.
pub fn dream_prompt(date: &str, sessions: &str, known: &str) -> String {
    format!(
        "you are consolidating what this project's sessions taught, into the \
         project's own wiki. today is {date}.\n\n\
         the pages that already exist:\n{known}\n\n\
         the latest sessions, verbatim:\n{sessions}\n\n\
         write what is worth keeping, and nothing else: durable facts about \
         the project (how it is built, what was decided and why, traps met and \
         how they were escaped). no diary, no summary of what happened, no \
         praise. if a page exists, rewrite it whole; if a fact is new, add a \
         page.\n\n\
         answer with json only:\n\
         {{\"pages\": [{{\"name\": \"short-lowercase-name\", \"body\": \
         \"# title\\n\\nthe page, in markdown\"}}], \"audit\": \"two or three \
         lines for the audit trail: what you read, what you changed, what you \
         left alone\"}}"
    )
}

/// read the last sessions, ask the model what to keep, write it back.
pub fn dream(agent: &mut Agent, how_many: usize, date: &str) -> Result<DreamReport, String> {
    let root = agent.root.clone();
    let sessions = crate::agent::sessions(&root);
    if sessions.is_empty() {
        return Err("no session to dream about yet".into());
    }
    let mut corpus = String::new();
    let mut read = 0;
    for path in sessions.iter().take(how_many) {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        // keep the human turns and the tool names: the whole jsonl is noise
        corpus.push_str(&format!("\n--- {} ---\n", path.display()));
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let role = v.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let tool = v.get("tool").and_then(|t| t.as_str()).unwrap_or("");
            match role {
                "user" => corpus.push_str(&format!("user: {content}\n")),
                "assistant" => corpus.push_str(&format!("agent: {content}\n")),
                "tool" => corpus.push_str(&format!("tool {tool}: {}\n", first_line(content))),
                _ => {}
            }
            if corpus.len() > 60_000 {
                break;
            }
        }
        read += 1;
    }
    let prompt = dream_prompt(date, &corpus, &index(&root));
    let answer = agent.run(&prompt, |_| {})?;
    let plan = parse_dream(&answer)?;

    let mut report = DreamReport {
        read_sessions: read,
        ..Default::default()
    };
    for page in &plan.pages {
        let name = page
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default();
        let body = page
            .get("body")
            .and_then(|b| b.as_str())
            .unwrap_or_default();
        if name.is_empty() || body.trim().is_empty() {
            continue;
        }
        write(&root, name, body)?;
        report.pages.push(name.to_string());
    }
    if let Some(audit) = plan.audit {
        let body = format!("# audit {date}\n\n{audit}\n");
        let path = write_in(&dir(&root).join("audit"), date, &body)?;
        report.audit = Some(path);
    }
    Ok(report)
}

#[derive(Debug, Default)]
pub struct DreamPlan {
    pub pages: Vec<Value>,
    pub audit: Option<String>,
}

/// read the dream's answer: json, fenced or not.
pub fn parse_dream(text: &str) -> Result<DreamPlan, String> {
    let t = text.trim();
    let start = t.find('{').ok_or("the dream answered nothing usable")?;
    let end = t.rfind('}').ok_or("the dream answered nothing usable")?;
    let body = &t[start..=end];
    let v: Value = serde_json::from_str(body)
        .map_err(|e| format!("the dream did not answer with json ({e})"))?;
    let pages = v
        .get("pages")
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    let audit = v.get("audit").and_then(|a| a.as_str()).map(str::to_string);
    if pages.is_empty() && audit.is_none() {
        return Err("the dream kept nothing".into());
    }
    Ok(DreamPlan { pages, audit })
}

fn first_line(s: &str) -> String {
    let l = s.lines().next().unwrap_or_default();
    if l.chars().count() > 100 {
        l.chars().take(100).collect::<String>() + "…"
    } else {
        l.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "nana-kb-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        d
    }

    #[test]
    fn a_page_round_trips() {
        let d = project("rt");
        let path = write(&d, "build", "# how it builds\n\ncargo build --release").unwrap();
        assert!(path.ends_with(".nana/knowledge/build.md"), "{path:?}");
        let text = read(&d, "build").unwrap();
        assert!(text.contains("cargo build"), "{text}");
        assert_eq!(list(&d).len(), 1);
        assert_eq!(list(&d)[0].title, "how it builds");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn names_cannot_escape_the_wiki() {
        let d = project("escape");
        for bad in ["../secrets", "a/b", ""] {
            assert!(write(&d, bad, "x").is_err(), "{bad} was allowed");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn search_finds_lines_and_the_index_lists_names_only() {
        let d = project("search");
        write(
            &d,
            "build",
            "# build\n\ncargo build --release is the only way",
        )
        .unwrap();
        write(&d, "traps", "# traps\n\nnever trust the cache").unwrap();
        let hits = search(&d, "cache");
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].page, "traps");
        let idx = index(&d);
        assert!(idx.contains("- build: build"), "{idx}");
        assert!(
            !idx.contains("--release"),
            "contents are not in the index: {idx}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_description_shows_real_line_breaks_not_escapes() {
        let d = project("describe-breaks");
        let empty = describe(&d);
        assert!(!empty.contains("\\n"), "no literal escapes: {empty}");
        assert!(empty.contains('\n'), "real line breaks: {empty}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn audit_pages_are_separate_from_knowledge() {
        let d = project("audit");
        write(&d, "build", "# build").unwrap();
        write_in(
            &d.join(".nana/knowledge/audit"),
            "2026-10-10",
            "# audit\n\ndid things",
        )
        .unwrap();
        assert_eq!(list(&d).len(), 1, "the wiki has one page");
        assert_eq!(audit_list(&d).len(), 1, "the history has one");
        assert_eq!(audit_list(&d)[0].name, "2026-10-10");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_dream_answer_is_read_even_fenced() {
        let plan = parse_dream(
            "```json\n{\"pages\":[{\"name\":\"build\",\"body\":\"# build\\n\\ncargo\"}],\"audit\":\"read 3 sessions\"}\n```",
        )
        .unwrap();
        assert_eq!(plan.pages.len(), 1);
        assert_eq!(plan.audit.as_deref(), Some("read 3 sessions"));
        assert!(parse_dream("i have nothing to say").is_err());
        assert!(parse_dream("{\"pages\":[],\"audit\":null}").is_err());
    }

    #[test]
    fn the_dream_prompt_carries_the_pages_and_the_sessions() {
        let p = dream_prompt("2026-10-10", "user: hello", "- build: build");
        assert!(p.contains("2026-10-10"));
        assert!(
            p.contains("- build: build"),
            "the existing pages are listed"
        );
        assert!(p.contains("user: hello"), "and the sessions");
        assert!(p.contains("\"pages\""), "and the shape of the answer");
    }

    #[test]
    fn describing_the_wiki_says_what_to_do_when_it_is_empty() {
        let d = project("describe");
        let empty = describe(&d);
        assert!(empty.contains("no pages yet"), "{empty}");
        write(&d, "build", "# build").unwrap();
        assert!(describe(&d).contains("build"), "{}", describe(&d));
        let _ = std::fs::remove_dir_all(&d);
    }
}
