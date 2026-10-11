//! per-project memory: markdown you can read and edit, never a black box.
//!
//! everything lives under `<project>/.nana/memory/<class>/<name>.md`, in four
//! classes — user, feedback, project, reference. memory is scoped to the
//! project it belongs to: the agent opening one repository can read and write
//! that repository's memory and nothing else, so a long-lived store never
//! becomes a pile of unrelated notes.

mod calendar;
mod consolidate;
#[cfg(test)]
mod tests;

pub use calendar::{days_from_iso, iso_from_days, today_days};
use calendar::{days_now, today_iso};
pub use consolidate::{
    apply_consolidation, consolidate, consolidation_prompt, parse_consolidation, parse_links,
    Summary,
};
use consolidate::{same_enough, words_of};

use std::collections::BTreeSet;
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

    /// Returns the folder name of the class on disk, the same word the user types in a memory path.
    pub fn id(&self) -> &'static str {
        match self {
            Class::User => "user",
            Class::Feedback => "feedback",
            Class::Project => "project",
            Class::Reference => "reference",
        }
    }

    /// Reads a class from its folder name, ignoring case and spaces, and None for any other word.
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
    /// Returns the word the footer stores for the confidence level.
    pub fn id(&self) -> &'static str {
        match self {
            Confidence::Low => "low",
            Confidence::Medium => "medium",
            Confidence::High => "high",
        }
    }

    /// Reads a confidence level from its word, ignoring case and spaces, and None for any other word.
    pub fn parse(s: &str) -> Option<Confidence> {
        match s.trim().to_lowercase().as_str() {
            "low" => Some(Confidence::Low),
            "medium" => Some(Confidence::Medium),
            "high" => Some(Confidence::High),
            _ => None,
        }
    }
}

/// retention halves after this many days, plus this many days per use, up to
/// the cap. a page nobody reads fades in a month; a page read often lasts.
const HALF_LIFE_DAYS: i64 = 30;
const MAX_HALF_LIFE_DAYS: i64 = 365;
/// below this share of its strength a page is reported as forgotten.
const FORGET_BELOW: f64 = 0.25;

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
    /// how many times the page was read or recalled
    pub uses: u32,
    /// the day of the last use, as yyyy-mm-dd
    pub used: Option<String>,
}

impl Entry {
    /// a page with no date is undated, not stale: we do not claim to know.
    pub fn stale(&self) -> bool {
        self.updated
            .as_deref()
            .and_then(days_from_iso)
            .is_some_and(|d| days_now() - d > STALE_DAYS)
    }

    /// how much of the page is still held, from 1 (just used) towards 0.
    /// it halves every half-life; a page used often has a longer half-life.
    /// a page with no use day falls back to its last write.
    pub fn retention(&self) -> f64 {
        let day = self.used.as_deref().or(self.updated.as_deref());
        let Some(age) = day.and_then(days_from_iso).map(|d| days_now() - d) else {
            return 1.0;
        };
        let half_life = (HALF_LIFE_DAYS * (1 + self.uses as i64)).min(MAX_HALF_LIFE_DAYS);
        0.5f64.powf(age.max(0) as f64 / half_life as f64)
    }

    /// a page whose retention fell under the floor is a candidate for removal;
    /// user pages are never forgotten by age, the user's own words stay.
    pub fn forgotten(&self) -> bool {
        self.class != Class::User && self.retention() < FORGET_BELOW
    }
}

/// the metadata lives at the foot of the page, as comments, so the page still
/// opens with its own heading and reads the same in any markdown viewer.
fn footer(
    updated: &str,
    confidence: Confidence,
    check: Option<&str>,
    uses: u32,
    used: &str,
) -> String {
    let mut out = format!(
        "<!-- nana: updated {updated}, confidence {}, uses {uses}, used {used} -->",
        confidence.id()
    );
    if let Some(c) = check {
        out.push_str(&format!("\n<!-- nana check: {} -->", c.trim()));
    }
    out
}

/// Tells whether a line is one of the nana footer lines, so the body reader can skip them.
fn is_footer(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("<!-- nana:") || t.starts_with("<!-- nana check:")
}

/// reads the footer back. unknown or missing fields stay None.
fn read_footer(text: &str) -> Footer {
    let mut f = Footer::default();
    for line in text.lines().map(str::trim) {
        if let Some(inner) = line
            .strip_prefix("<!-- nana check:")
            .and_then(|s| s.strip_suffix("-->"))
        {
            f.check = Some(inner.trim().to_string()).filter(|s| !s.is_empty());
        } else if let Some(inner) = line
            .strip_prefix("<!-- nana:")
            .and_then(|s| s.strip_suffix("-->"))
        {
            for part in inner.split(',') {
                let mut kv = part.split_whitespace();
                match (kv.next(), kv.next()) {
                    (Some("updated"), Some(d)) => f.updated = Some(d.to_string()),
                    (Some("confidence"), Some(c)) => f.confidence = Confidence::parse(c),
                    (Some("uses"), Some(n)) => f.uses = n.parse().unwrap_or(0),
                    (Some("used"), Some(d)) => f.used = Some(d.to_string()),
                    _ => {}
                }
            }
        }
    }
    f
}

#[derive(Default)]
struct Footer {
    updated: Option<String>,
    confidence: Option<Confidence>,
    check: Option<String>,
    uses: u32,
    used: Option<String>,
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

    /// Returns the folder the memory lives in, so callers can show or check the location.
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

    /// Writes a page at the default medium confidence, the level used when the caller gives none.
    pub fn write(&self, class: Class, name: &str, body: &str) -> Result<PathBuf, String> {
        self.write_rated(class, name, body, Confidence::Medium)
    }

    /// Writes a page with the given confidence and no check, the common path for a page the agent learned.
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
        let today = today_iso();
        let uses = std::fs::read_to_string(&path)
            .map(|t| read_footer(&t).uses)
            .unwrap_or(0);
        let text = format!(
            "{page}\n\n{}\n",
            footer(&today, confidence, check, uses, &today)
        );
        std::fs::write(&path, &text).map_err(|e| e.to_string())?;
        self.append_history(class, title, &today_iso(), &text);
        Ok(path)
    }

    /// keeps how one conversation began and ended, as a project page.
    /// confidence is low so that the usual degradation and consolidation apply to it.
    pub fn save_turn(
        &self,
        session: &str,
        opening: &str,
        closing: &str,
    ) -> Result<PathBuf, String> {
        let body = format!(
            "## opened by the user\n\n{}\n\n## closed by the agent\n\n{}",
            opening.trim(),
            closing.trim()
        );
        self.write_rated(
            Class::Project,
            &format!("conversation-{session}"),
            &body,
            Confidence::Low,
        )
    }

    /// the file that keeps every version of one page, in the project memory.
    fn history_path(&self, class: Class, name: &str) -> PathBuf {
        self.root.join(".history").join(format!(
            "{}--{}.log",
            class.id(),
            name.trim().trim_end_matches(".md")
        ))
    }

    /// each write adds an entry, so the page can be read back as it was.
    fn append_history(&self, class: Class, name: &str, day: &str, text: &str) {
        let path = self.history_path(class, name);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let entry = format!("<!-- nana entry: {day} -->\n{text}\n");
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = f.write_all(entry.as_bytes());
        }
    }

    /// every version written of one page, oldest first, each with its day.
    pub fn history(&self, class: Class, name: &str) -> Vec<(String, String)> {
        let text = std::fs::read_to_string(self.history_path(class, name)).unwrap_or_default();
        let mut out: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            if let Some(day) = line
                .strip_prefix("<!-- nana entry: ")
                .and_then(|s| s.strip_suffix(" -->"))
            {
                out.push((day.to_string(), String::new()));
            } else if let Some(last) = out.last_mut() {
                last.1.push_str(line);
                last.1.push('\n');
            }
        }
        for (_, body) in &mut out {
            *body = body.trim_end().to_string();
        }
        out
    }

    /// the `[[name]]` links a page makes, in order, without repeats.
    pub fn links(&self, class: Class, name: &str) -> Option<Vec<String>> {
        let text = self.read(class, name).ok()?;
        Some(parse_links(&text))
    }

    /// the pages that link to `name`, in any class.
    pub fn backlinks(&self, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        for e in self.list() {
            let Ok(text) = std::fs::read_to_string(&e.path) else {
                continue;
            };
            if e.name != name && parse_links(&text).iter().any(|l| l == name) {
                out.push(e.name.clone());
            }
        }
        out
    }

    /// Reads a page from its class and name, with the file path in the error, and the footer lines are dropped from the text returned.
    pub fn read(&self, class: Class, name: &str) -> Result<String, String> {
        let path = self.safe_path(class, name)?;
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// a use of a page: counted in its footer, and its age restarts. the body
    /// is not touched, so a use is never mistaken for an edit.
    pub fn reinforce(&self, class: Class, name: &str) -> Result<(), String> {
        let path = self.safe_path(class, name)?;
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let f = read_footer(&text);
        let body: Vec<&str> = text.lines().filter(|l| !is_footer(l)).collect();
        let body = body.join("\n");
        let body = body.trim_end();
        let updated = f.updated.unwrap_or_else(today_iso);
        let confidence = f.confidence.unwrap_or(Confidence::Medium);
        let today = today_iso();
        let out = format!(
            "{body}\n\n{}\n",
            footer(&updated, confidence, f.check.as_deref(), f.uses + 1, &today)
        );
        std::fs::write(&path, out).map_err(|e| e.to_string())
    }

    /// Deletes a page file, returning the io error text when the page cannot be removed.
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
                    let f = read_footer(&text);
                    Entry {
                        class,
                        name,
                        path: p,
                        title,
                        updated: f.updated,
                        confidence: f.confidence,
                        check: f.check,
                        uses: f.uses,
                        used: f.used,
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

    /// the bodies the request needs, inside a character budget: every user
    /// preference, then the pages whose name or title the request mentions.
    /// a page that is stale or unsure says so, so the agent checks it first.
    pub fn recall(&self, request: &str, budget: usize) -> String {
        let req = request.to_lowercase();
        let entries = self.list();
        let mut picked: Vec<&Entry> = entries.iter().filter(|e| e.class == Class::User).collect();
        for e in entries.iter().filter(|e| e.class != Class::User) {
            let name = e.name.to_lowercase();
            let title = e.title.to_lowercase();
            // a page is tied to the files it names: a path in the request that
            // a page writes down brings that page back, whatever its name.
            let names_a_touched_file = std::fs::read_to_string(&e.path)
                .map(|text| {
                    text.split_whitespace()
                        .map(|w| {
                            w.trim_matches(|c: char| {
                                !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-'
                            })
                        })
                        .filter(|w| w.contains('/') && w.len() > 3)
                        .any(|w| req.contains(&w.to_lowercase()))
                })
                .unwrap_or(false);
            let mentioned = names_a_touched_file
                || req.contains(&name)
                || (!title.is_empty() && req.contains(&title))
                || req
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w.len() > 3 && name.split('-').any(|p| p == w));
            if mentioned {
                picked.push(e);
            }
        }
        let mut out = String::new();
        for e in picked {
            let Ok(text) = std::fs::read_to_string(&e.path) else {
                continue;
            };
            let body: String = text
                .lines()
                .filter(|l| !is_footer(l))
                .collect::<Vec<_>>()
                .join("\n");
            let mut flags = Vec::new();
            if e.stale() {
                flags.push("stale: check it before you rely on it".to_string());
            }
            if e.confidence == Some(Confidence::Low) {
                flags.push("low confidence: check it before you rely on it".to_string());
            }
            let flag = if flags.is_empty() {
                String::new()
            } else {
                format!(" ({})", flags.join("; "))
            };
            let _ = self.reinforce(e.class, &e.name);
            let block = format!("--- {}/{}{flag}\n{}\n", e.class.id(), e.name, body.trim());
            if out.chars().count() + block.chars().count() > budget {
                let room = budget.saturating_sub(out.chars().count());
                out.extend(block.chars().take(room));
                break;
            }
            out.push_str(&block);
        }
        out
    }

    /// removes the stale pages and the near-copies of another page, without asking.
    /// a user page is never dropped for its age: the user's own words are not
    /// the agent's to expire. every removed page is first written to the
    /// history, so it can still be read back.
    pub fn tidy(&self) -> Tidy {
        let mut entries = self.list();
        entries.sort_by(|a, b| (a.class.id(), &a.name).cmp(&(b.class.id(), &b.name)));
        let day = today_iso();
        let mut seen: Vec<(String, BTreeSet<String>)> = Vec::new();
        let mut removed = Vec::new();
        for e in &entries {
            let Ok(text) = std::fs::read_to_string(&e.path) else {
                continue;
            };
            let key = format!("{}/{}", e.class.id(), e.name);
            let words: BTreeSet<String> = words_of(&text).into_iter().collect();
            // age alone is not enough: a page written long ago but read lately is still in use
            let stale = e.class != Class::User && e.stale() && e.forgotten();
            let copy = seen.iter().any(|(_, w)| same_enough(w, &words));
            if stale || copy {
                self.append_history(e.class, &e.name, &day, &text);
                if std::fs::remove_file(&e.path).is_ok() {
                    removed.push(key);
                }
            } else {
                seen.push((key, words));
            }
        }
        Tidy { removed }
    }

    /// reads every page and reports what needs a human decision. read only.
    pub fn audit(&self) -> Audit {
        let entries = self.list();
        let mut bodies: Vec<(String, Vec<String>)> = Vec::new();
        let mut stale = Vec::new();
        let mut forgotten = Vec::new();
        for e in &entries {
            let key = format!("{}/{}", e.class.id(), e.name);
            if e.stale() {
                stale.push(key.clone());
            }
            if e.forgotten() {
                forgotten.push(key.clone());
            }
            if let Ok(text) = std::fs::read_to_string(&e.path) {
                bodies.push((key, words_of(&text)));
            }
        }
        let mut duplicates = Vec::new();
        for (i, (a, wa)) in bodies.iter().enumerate() {
            for (b, wb) in bodies.iter().skip(i + 1) {
                if !wa.is_empty() && wa == wb {
                    duplicates.push((a.clone(), b.clone()));
                }
            }
        }
        Audit {
            duplicates,
            stale,
            forgotten,
        }
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

/// decides, without asking, when the memory is worth a consolidation: when it
/// holds pages that say the same thing, and only once per state of the memory.
/// the fingerprint is what the trigger remembers, so an unchanged memory never
/// costs a second model call, and any write re-arms it.
#[derive(Debug, Default)]
pub struct ConsolidationTrigger {
    last_done: Option<u64>,
}

impl ConsolidationTrigger {
    /// Tells whether the memory needs consolidating: its content changed since the last run and it holds duplicate pages.
    pub fn due(&self, m: &Memory) -> bool {
        let fp = fingerprint(m);
        self.last_done != Some(fp) && !m.audit().duplicates.is_empty()
    }

    /// Records the current content as consolidated, so the same content does not trigger a second run.
    pub fn done(&mut self, m: &Memory) {
        self.last_done = Some(fingerprint(m));
    }
}

/// a hash of the page names and bodies: any change to what the memory says
/// changes it. the footer is left out, because a read only moves its counter,
/// and a counter must not make the same memory look new.
fn fingerprint(m: &Memory) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    for e in m.list() {
        e.class.id().hash(&mut h);
        e.name.hash(&mut h);
        let text = std::fs::read_to_string(&e.path).unwrap_or_default();
        text.lines()
            .filter(|l| !is_footer(l))
            .collect::<Vec<_>>()
            .hash(&mut h);
    }
    h.finish()
}

/// what `tidy` removed, as `class/name`. the copies stay readable in the history.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tidy {
    pub removed: Vec<String>,
}

/// what the audit found: pages that say the same words, and pages past the
/// staleness limit. it only reports; deleting or merging a page is a change the
/// user approves, never a side effect of looking.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Audit {
    /// pairs of `class/name` whose bodies hold the same words in the same order
    pub duplicates: Vec<(String, String)>,
    /// `class/name` of every page past the staleness limit
    pub stale: Vec<String>,
    /// `class/name` of every page whose retention fell under the floor
    pub forgotten: Vec<String>,
}
