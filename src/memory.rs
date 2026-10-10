//! per-project memory: markdown you can read and edit, never a black box.
//!
//! everything lives under `<project>/.nana/memory/<class>/<name>.md`, in four
//! classes — user, feedback, project, reference. memory is scoped to the
//! project it belongs to: the agent opening one repository can read and write
//! that repository's memory and nothing else, so a long-lived store never
//! becomes a pile of unrelated notes.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// who the user is, how they like to work
    User,
    /// what they told you about your own behaviour
    Feedback,
    /// facts about this project: stack, conventions, decisions
    Project,
    /// pointers to outside material worth keeping
    Reference,
}

impl Class {
    pub const ALL: [Class; 4] = [
        Class::User,
        Class::Feedback,
        Class::Project,
        Class::Reference,
    ];

    pub fn id(&self) -> &'static str {
        match self {
            Class::User => "user",
            Class::Feedback => "feedback",
            Class::Project => "project",
            Class::Reference => "reference",
        }
    }

    pub fn parse(s: &str) -> Option<Class> {
        Class::ALL
            .into_iter()
            .find(|c| c.id() == s.trim().to_lowercase())
    }
}

/// how sure the writer was. a guess written as a fact is the usual way a
/// memory goes wrong, so the level is kept next to the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn id(&self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }

    pub fn parse(s: &str) -> Option<Confidence> {
        match s.trim().to_lowercase().as_str() {
            "low" => Some(Confidence::Low),
            "medium" => Some(Confidence::Medium),
            "high" => Some(Confidence::High),
            _ => None,
        }
    }
}

/// a page older than this is reported as stale: long enough that a stable
/// convention is not flagged every week, short enough that a decision made
/// before a refactor gets a second look.
const STALE_DAYS: i64 = 90;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub class: Class,
    pub name: String,
    pub path: PathBuf,
    /// the first heading line, for the index
    pub title: String,
    /// the day the page was last written, as yyyy-mm-dd
    pub updated: Option<String>,
    pub confidence: Option<Confidence>,
    /// a shell command that holds while the page is still true
    pub check: Option<String>,
}

impl Entry {
    /// a page with no date is undated, not stale: we do not claim to know.
    pub fn stale(&self) -> bool {
        self.updated
            .as_deref()
            .and_then(days_from_iso)
            .is_some_and(|d| days_now() - d > STALE_DAYS)
    }
}

/// the metadata lives at the foot of the page, as comments, so the page still
/// opens with its own heading and reads the same in any markdown viewer.
fn footer(updated: &str, confidence: Confidence, check: Option<&str>) -> String {
    let mut out = format!(
        "<!-- nana: updated {updated}, confidence {} -->",
        confidence.id()
    );
    if let Some(c) = check {
        out.push_str(&format!("\n<!-- nana check: {} -->", c.trim()));
    }
    out
}

fn is_footer(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("<!-- nana:") || t.starts_with("<!-- nana check:")
}

/// reads the footer back. unknown or missing fields stay None.
fn read_footer(text: &str) -> (Option<String>, Option<Confidence>, Option<String>) {
    let mut updated = None;
    let mut confidence = None;
    let mut check = None;
    for line in text.lines().map(str::trim) {
        if let Some(inner) = line
            .strip_prefix("<!-- nana check:")
            .and_then(|s| s.strip_suffix("-->"))
        {
            check = Some(inner.trim().to_string()).filter(|s| !s.is_empty());
        } else if let Some(inner) = line
            .strip_prefix("<!-- nana:")
            .and_then(|s| s.strip_suffix("-->"))
        {
            for part in inner.split(',') {
                let mut kv = part.split_whitespace();
                match (kv.next(), kv.next()) {
                    (Some("updated"), Some(d)) => updated = Some(d.to_string()),
                    (Some("confidence"), Some(c)) => confidence = Confidence::parse(c),
                    _ => {}
                }
            }
        }
    }
    (updated, confidence, check)
}

fn days_now() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    secs.div_euclid(86_400)
}

fn today_iso() -> String {
    let (y, m, d) = civil_from_days(days_now());
    format!("{y:04}-{m:02}-{d:02}")
}

/// howard hinnant's day-count algorithm: no calendar crate for one date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_iso(s: &str) -> Option<i64> {
    let mut parts = s.trim().split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub class: Class,
    pub name: String,
    pub line: usize,
    pub text: String,
}

pub struct Memory {
    root: PathBuf,
}

impl Memory {
    /// `<project>/.nana/memory`
    pub fn open(project_root: &Path) -> Memory {
        Memory {
            root: project_root.join(".nana").join("memory"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// a name is a file name and nothing else: no slashes, no traversal.
    fn safe_path(&self, class: Class, name: &str) -> Result<PathBuf, String> {
        let clean = name.trim().trim_end_matches(".md");
        if clean.is_empty() {
            return Err("a memory needs a name".into());
        }
        if clean.contains('/') || clean.contains('\\') || clean.contains("..") {
            return Err(format!("« {name} » is not a plain name"));
        }
        Ok(self.root.join(class.id()).join(format!("{clean}.md")))
    }

    pub fn write(&self, class: Class, name: &str, body: &str) -> Result<PathBuf, String> {
        self.write_rated(class, name, body, Confidence::Medium)
    }

    pub fn write_rated(
        &self,
        class: Class,
        name: &str,
        body: &str,
        confidence: Confidence,
    ) -> Result<PathBuf, String> {
        self.write_checked(class, name, body, confidence, None)
    }

    /// `check` is a shell command that holds while the page is still true.
    pub fn write_checked(
        &self,
        class: Class,
        name: &str,
        body: &str,
        confidence: Confidence,
        check: Option<&str>,
    ) -> Result<PathBuf, String> {
        let path = self.safe_path(class, name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let title = name.trim().trim_end_matches(".md");
        // a page read back and written again must not stack two footers
        let kept: Vec<&str> = body.lines().filter(|l| !is_footer(l)).collect();
        let body = kept.join("\n");
        let body = body.trim_end();
        let page = if body.trim_start().starts_with('#') {
            body.to_string()
        } else {
            format!("# {title}\n\n{body}")
        };
        let text = format!(
            "{page}\n\n{}\n",
            footer(&today_iso(), confidence, check)
        );
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn read(&self, class: Class, name: &str) -> Result<String, String> {
        let path = self.safe_path(class, name)?;
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn forget(&self, class: Class, name: &str) -> Result<(), String> {
        let path = self.safe_path(class, name)?;
        std::fs::remove_file(&path).map_err(|e| e.to_string())
    }

    /// every page, class by class.
    pub fn list(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        for class in Class::ALL {
            let dir = self.root.join(class.id());
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut here: Vec<Entry> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md"))
                .map(|p| {
                    let name = p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default()
                        .to_string();
                    let text = std::fs::read_to_string(&p).unwrap_or_default();
                    let title = text
                        .lines()
                        .find(|l| l.trim_start().starts_with('#'))
                        .map(|l| l.trim_start_matches('#').trim().to_string())
                        .unwrap_or_else(|| name.clone());
                    let (updated, confidence, check) = read_footer(&text);
                    Entry {
                        class,
                        name,
                        path: p,
                        title,
                        updated,
                        confidence,
                        check,
                    }
                })
                .collect();
            here.sort_by(|a, b| a.name.cmp(&b.name));
            out.extend(here);
        }
        out
    }

    /// substring search across the pages of this project.
    pub fn search(&self, needle: &str) -> Vec<Hit> {
        let needle_l = needle.to_lowercase();
        let mut out = Vec::new();
        for e in self.list() {
            let Ok(text) = std::fs::read_to_string(&e.path) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&needle_l) {
                    out.push(Hit {
                        class: e.class,
                        name: e.name.clone(),
                        line: i + 1,
                        text: line.trim().to_string(),
                    });
                }
            }
        }
        out
    }

    /// the page the agent gets shown: names and titles, not the whole store.
    pub fn index(&self) -> String {
        let entries = self.list();
        if entries.is_empty() {
            return String::new();
        }
        let mut out = String::from("memory of this project:\n");
        for class in Class::ALL {
            let here: Vec<&Entry> = entries.iter().filter(|e| e.class == class).collect();
            if here.is_empty() {
                continue;
            }
            out.push_str(&format!("- {}: ", class.id()));
            out.push_str(
                &here
                    .iter()
                    .map(|e| e.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
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
        assert!(text.starts_with("# stack"), "the heading still comes first: {text}");
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
    fn calendar_dates_convert_both_ways() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(19723), (2024, 1, 1));
        let leap = days_from_iso("2024-02-29").unwrap();
        assert_eq!(civil_from_days(leap), (2024, 2, 29));
        assert_eq!(days_from_iso("2024-13-01"), None);
        assert_eq!(days_from_iso("not a date"), None);
    }
}
