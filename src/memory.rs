//! per-project memory: markdown you can read and edit, never a black box.
//!
//! everything lives under `<project>/.nana/memory/<class>/<name>.md`, in four
//! classes — user, feedback, project, reference. memory is scoped to the
//! project it belongs to: the agent opening one repository can read and write
//! that repository's memory and nothing else, so a long-lived store never
//! becomes a pile of unrelated notes.

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

pub fn today_days() -> i64 {
    days_now()
}

fn days_now() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    secs.div_euclid(86_400)
}

fn today_iso() -> String {
    iso_from_days(days_now())
}

/// the calendar day as yyyy-mm-dd, for the map and the index alike.
pub fn iso_from_days(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
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

pub fn days_from_iso(s: &str) -> Option<i64> {
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
            let mentioned = req.contains(&name)
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
            let stale = e.class != Class::User && e.stale();
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
    pub fn due(&self, m: &Memory) -> bool {
        let fp = fingerprint(m);
        self.last_done != Some(fp) && !m.audit().duplicates.is_empty()
    }

    pub fn done(&mut self, m: &Memory) {
        self.last_done = Some(fingerprint(m));
    }
}

/// a hash of the page names and bodies: any change to the memory changes it.
fn fingerprint(m: &Memory) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    for e in m.list() {
        e.class.id().hash(&mut h);
        e.name.hash(&mut h);
        std::fs::read_to_string(&e.path)
            .unwrap_or_default()
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

/// two pages are the same thing when their words overlap this much (jaccard).
/// below it they are about different things; exact equality is a special case.
const NEAR_COPY: f64 = 0.8;

fn same_enough(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    if a.is_empty() && b.is_empty() {
        return true;
    }
    let common = a.intersection(b).count() as f64;
    let all = a.union(b).count() as f64;
    common / all >= NEAR_COPY
}

/// the words of a body, lowercased, so a copy that differs only in case or
/// spacing still matches. the heading is left out: two pages with different
/// names but the same content are the duplicate we are looking for.
fn words_of(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !is_footer(l) && !l.trim_start().starts_with('#'))
        .flat_map(|l| l.split_whitespace())
        .map(|w| w.to_lowercase())
        .collect()
}

/// the names inside `[[ ]]`: trimmed, empty ones dropped, each name once.
pub fn parse_links(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else {
            break;
        };
        let name = after[..close].trim();
        if !name.is_empty() && !name.contains('\n') && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
        rest = &after[close + 2..];
    }
    out
}

/// one summary the model proposes: it replaces the pages named in `replaces`.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub class: Class,
    pub name: String,
    pub body: String,
    pub replaces: Vec<String>,
}

/// the prompt that asks the model to fold related pages into summaries.
pub fn consolidation_prompt(root: &Memory) -> String {
    let mut pages = String::new();
    for e in root.list() {
        let Ok(text) = std::fs::read_to_string(&e.path) else {
            continue;
        };
        pages.push_str(&format!(
            "\n--- {}/{} ---\n{}\n",
            e.class.id(),
            e.name,
            text
        ));
    }
    format!(
        "you are consolidating this project's memory: several short pages that \
         overlap can become one summary. keep every fact that still matters, drop \
         what is repeated, and never merge a user page away.\n\n\
         pages:\n{pages}\n\
         answer with json only:\n\
         {{\"summaries\": [{{\"class\": \"project\", \"name\": \"short-lowercase-name\", \
         \"body\": \"# title\\n\\nthe summary, in markdown\", \
         \"replaces\": [\"project/old-name\"]}}]}}"
    )
}

/// the model's answer, read the way the dream reads its own: the first
/// json object in the text, the rest is chatter.
pub fn parse_consolidation(text: &str) -> Result<Vec<Summary>, String> {
    let t = text.trim();
    let start = t.find('{').ok_or("the model answered nothing usable")?;
    let end = t.rfind('}').ok_or("the model answered nothing usable")?;
    let v: serde_json::Value =
        serde_json::from_str(&t[start..=end]).map_err(|e| format!("bad json: {e}"))?;
    let mut out = Vec::new();
    for s in v["summaries"].as_array().into_iter().flatten() {
        let (Some(class), Some(name), Some(body)) = (
            s["class"].as_str().and_then(Class::parse),
            s["name"].as_str(),
            s["body"].as_str(),
        ) else {
            continue;
        };
        let replaces = s["replaces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r.as_str().map(String::from))
            .collect();
        out.push(Summary {
            class,
            name: name.to_string(),
            body: body.to_string(),
            replaces,
        });
    }
    Ok(out)
}

/// writes each summary, then retires the pages it replaces. a replaced page
/// keeps its last text in the history. user pages are never retired, and a
/// page is never retired when it is the summary itself. returns the retired
/// pages as `class/name`.
pub fn apply_consolidation(m: &Memory, plan: &[Summary]) -> Result<Vec<String>, String> {
    let mut retired = Vec::new();
    for s in plan {
        m.write(s.class, &s.name, &s.body)?;
        for r in &s.replaces {
            let Some((class_id, name)) = r.split_once('/') else {
                continue;
            };
            let Some(class) = Class::parse(class_id) else {
                continue;
            };
            if class == Class::User || (class == s.class && name == s.name) {
                continue;
            }
            let Ok(path) = m.safe_path(class, name) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            m.append_history(class, name, &today_iso(), &text);
            m.forget(class, name)?;
            retired.push(format!("{}/{}", class.id(), name));
        }
    }
    Ok(retired)
}

/// asks the model for a plan, then applies it. `ask` is the model call, so the
/// flow is testable without a network.
pub fn consolidate(
    m: &Memory,
    ask: impl FnOnce(&str) -> Result<String, String>,
) -> Result<Vec<String>, String> {
    let prompt = consolidation_prompt(m);
    let answer = ask(&prompt)?;
    let plan = parse_consolidation(&answer)?;
    apply_consolidation(m, &plan)
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
            format!("# lazy\n\nx\n\n<!-- nana: updated {day}, confidence high, uses 0, used {day} -->\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("busy.md"),
            format!("# busy\n\nx\n\n<!-- nana: updated {day}, confidence high, uses 10, used {day} -->\n"),
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
}
