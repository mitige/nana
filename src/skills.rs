//! skills: packaged workflows the agent picks up when a request matches.
//!
//! a skill is a markdown file with a small frontmatter block. nana looks in
//! `<project>/.nana/skills/`, in `<project>/skills/*/SKILL.md` (the portable
//! layout) and in `~/.config/nana/skills/`. the trigger is plain words: when
//! the request contains them, the skill's body is handed to the model.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// words that make this skill relevant, comma separated in the file
    pub trigger: String,
    pub body: String,
    pub path: PathBuf,
}

impl Skill {
    /// does this skill answer that request?
    pub fn matches(&self, request: &str) -> bool {
        let req = request.to_lowercase();
        self.trigger
            .split(',')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| t.len() > 2)
            .any(|t| req.contains(&t))
    }
}

/// where skills may live, project first.
fn dirs(root: &Path, extra: &[String]) -> Vec<PathBuf> {
    let mut out = vec![root.join(".nana").join("skills"), root.join("skills")];
    for e in extra {
        let p = PathBuf::from(e);
        out.push(if p.is_absolute() { p } else { root.join(p) });
    }
    if let Some(cfg) = crate::config::config_path().parent() {
        out.push(cfg.join("skills"));
    }
    out
}

pub fn discover(root: &Path, extra: &[String]) -> Vec<Skill> {
    let mut out: Vec<Skill> = Vec::new();
    for dir in dirs(root, extra) {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                // the portable layout: skills/<name>/SKILL.md
                let inner = path.join("SKILL.md");
                if inner.exists() {
                    if let Some(s) = parse(&inner) {
                        out.push(s);
                    }
                }
                continue;
            }
            if path.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            if let Some(s) = parse(&path) {
                out.retain(|x| x.name != s.name);
                out.push(s);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// the one-line list handed to the model, so it knows what exists.
pub fn index(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut out = String::from("skills available in this project:\n");
    for s in skills {
        out.push_str(&format!("- {}: {}\n", s.name, s.description));
    }
    out
}

/// the skills a request actually triggers.
pub fn matching<'a>(skills: &'a [Skill], request: &str) -> Vec<&'a Skill> {
    skills.iter().filter(|s| s.matches(request)).collect()
}

fn parse(path: &Path) -> Option<Skill> {
    let text = std::fs::read_to_string(path).ok()?;
    let fallback = path
        .parent()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) != Some("skills"))
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("skill")
        .to_string();
    let mut name = fallback;
    let mut description = String::new();
    let mut trigger = String::new();
    let mut body = text.clone();
    if let Some(rest) = text.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("name:") {
                    name = v.trim().trim_matches('"').to_string();
                } else if let Some(v) = line.strip_prefix("description:") {
                    description = v.trim().trim_matches('"').to_string();
                } else if let Some(v) = line.strip_prefix("trigger:") {
                    trigger = v.trim().trim_matches('"').to_string();
                }
            }
            body = rest[end + 4..].trim_start().to_string();
        }
    }
    if name.is_empty() {
        return None;
    }
    Some(Skill {
        name,
        description,
        trigger,
        body: body.trim_end().to_string(),
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nana-skill-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_skill_file_is_parsed_with_its_trigger() {
        let d = tmp("one");
        std::fs::create_dir_all(d.join(".nana/skills")).unwrap();
        std::fs::write(
            d.join(".nana/skills/release.md"),
            "---\nname: release\ndescription: cut a release\ntrigger: release, tag\n---\n\
             bump the version, tag it, write the changelog.\n",
        )
        .unwrap();
        let found = discover(&d, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "release");
        assert_eq!(found[0].description, "cut a release");
        assert!(found[0].body.contains("bump the version"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_portable_skill_md_layout_is_found_too() {
        let d = tmp("portable");
        std::fs::create_dir_all(d.join("skills/pdf")).unwrap();
        std::fs::write(
            d.join("skills/pdf/SKILL.md"),
            "---\nname: pdf\ndescription: fill pdf forms\ntrigger: pdf\n---\nbody\n",
        )
        .unwrap();
        let found = discover(&d, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "pdf", "the folder name is the fallback");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn triggers_pick_the_right_skill() {
        let s = Skill {
            name: "release".into(),
            description: "cut a release".into(),
            trigger: "release, tag".into(),
            body: "steps".into(),
            path: PathBuf::new(),
        };
        assert!(s.matches("please cut a RELEASE for me"));
        assert!(s.matches("tag this build"));
        assert!(!s.matches("write a poem"));
        let list = [s];
        let picked = matching(&list, "we should tag it");
        assert_eq!(picked.len(), 1);
    }

    #[test]
    fn the_index_lists_names_and_descriptions() {
        let s = Skill {
            name: "pdf".into(),
            description: "fill pdf forms".into(),
            trigger: "pdf".into(),
            body: String::new(),
            path: PathBuf::new(),
        };
        let idx = index(&[s]);
        assert!(idx.contains("- pdf: fill pdf forms"), "{idx}");
    }
}
