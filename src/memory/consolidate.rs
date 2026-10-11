//! consolidation: a model rereads the store and proposes summaries; the
//! rewrite is checked before it lands.

use super::*;

/// two pages are the same thing when their words overlap this much (jaccard).
/// below it they are about different things; exact equality is a special case.
const NEAR_COPY: f64 = 0.8;

/// Tells whether two sets of words are near copies, their overlap reaching the NEAR_COPY share of all words.
pub(super) fn same_enough(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
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
pub(super) fn words_of(text: &str) -> Vec<String> {
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
