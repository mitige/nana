//! personas: saved system prompts you can write, edit and reuse.
//!
//! a persona is a markdown file with a small frontmatter block, so it is
//! readable in nana itself and editable in any editor. project personas live
//! in `<project>/.nana/personas/`, yours live in `~/.config/nana/personas/`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persona {
    pub id: String,
    pub title: String,
    pub description: String,
    /// the system prompt itself
    pub prompt: String,
    pub path: PathBuf,
}

/// the directories a persona can live in: the project's, then the user's.
/// `user_dir` overrides the config directory (used by the tests so they never
/// touch a real ~/.config).
fn dirs_in(project_root: Option<&Path>, user_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(root) = project_root {
        out.push(root.join(".nana").join("personas"));
    }
    let user = match user_dir {
        Some(d) => Some(d.to_path_buf()),
        None => crate::config::config_path()
            .parent()
            .map(|p| p.to_path_buf()),
    };
    if let Some(d) = user {
        out.push(d.join("personas"));
    }
    out
}

/// every persona visible from this project: the project's own first, then
/// the user's, the later one shadowing an earlier id.
pub fn list(project_root: Option<&Path>) -> Vec<Persona> {
    list_in(project_root, None)
}

pub fn list_in(project_root: Option<&Path>, user_dir: Option<&Path>) -> Vec<Persona> {
    let mut out: Vec<Persona> = Vec::new();
    for dir in dirs_in(project_root, user_dir).into_iter().rev() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            if let Some(p) = parse(&path) {
                out.retain(|x| x.id != p.id);
                out.push(p);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn load(project_root: Option<&Path>, id: &str) -> Option<Persona> {
    let id = id.trim();
    list(project_root).into_iter().find(|p| p.id == id)
}

/// write a persona. `project` decides where it lands.
pub fn save(project_root: Option<&Path>, p: &Persona, project: bool) -> Result<PathBuf, String> {
    save_in(project_root, None, p, project)
}

pub fn save_in(
    project_root: Option<&Path>,
    user_dir: Option<&Path>,
    p: &Persona,
    project: bool,
) -> Result<PathBuf, String> {
    let id = sanitize(&p.id)?;
    let dir = if project {
        project_root
            .ok_or("no project open")?
            .join(".nana")
            .join("personas")
    } else {
        user_dir
            .map(|d| d.to_path_buf())
            .or_else(|| {
                crate::config::config_path()
                    .parent()
                    .map(|p| p.to_path_buf())
            })
            .ok_or("no config directory")?
            .join("personas")
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{id}.md"));
    let text = format!(
        "---\ntitle: {}\ndescription: {}\n---\n{}\n",
        p.title.trim(),
        p.description.trim(),
        p.prompt.trim_end()
    );
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn delete(project_root: Option<&Path>, id: &str) -> Result<(), String> {
    delete_in(project_root, None, id)
}

pub fn delete_in(
    project_root: Option<&Path>,
    user_dir: Option<&Path>,
    id: &str,
) -> Result<(), String> {
    let id = sanitize(id)?;
    for dir in dirs_in(project_root, user_dir) {
        let path = dir.join(format!("{id}.md"));
        if path.exists() {
            return std::fs::remove_file(path).map_err(|e| e.to_string());
        }
    }
    Err(format!("no persona « {id} »"))
}

fn sanitize(id: &str) -> Result<String, String> {
    let clean = id.trim().to_lowercase().replace(' ', "-");
    if clean.is_empty() {
        return Err("a persona needs a name".into());
    }
    if clean.contains('/') || clean.contains('\\') || clean.contains("..") {
        return Err(format!("« {id} » is not a plain name"));
    }
    Ok(clean)
}

/// read a persona file: `---` frontmatter, then the prompt.
fn parse(path: &Path) -> Option<Persona> {
    let text = std::fs::read_to_string(path).ok()?;
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let mut title = id.clone();
    let mut description = String::new();
    let mut prompt = text.clone();
    if let Some(rest) = text.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let head = &rest[..end];
            for line in head.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("title:") {
                    title = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("description:") {
                    description = v.trim().to_string();
                }
            }
            prompt = rest[end + 4..].trim_start().to_string();
        }
    }
    Some(Persona {
        id,
        title,
        description,
        prompt: prompt.trim_end().to_string(),
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nana-per-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn one(id: &str) -> Persona {
        Persona {
            id: id.into(),
            title: "The Reviewer".into(),
            description: "reads code like a colleague".into(),
            prompt: "you review code: precise, direct, no flattery.".into(),
            path: PathBuf::new(),
        }
    }

    #[test]
    fn a_persona_round_trips_through_markdown() {
        let d = tmp("rt");
        let p = one("reviewer");
        let path = save(Some(&d), &p, true).unwrap();
        assert!(path.ends_with(".nana/personas/reviewer.md"), "{path:?}");
        let back = load(Some(&d), "reviewer").unwrap();
        assert_eq!(back.title, "The Reviewer");
        assert_eq!(back.description, "reads code like a colleague");
        assert_eq!(back.prompt, p.prompt);
        // and the file is readable markdown, not a blob
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.starts_with("---\ntitle:"), "{raw}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ids_cannot_escape_and_bad_ones_are_refused() {
        let d = tmp("bad");
        for bad in ["../x", "a/b", ""] {
            assert!(save(Some(&d), &one(bad), true).is_err(), "{bad} allowed");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_project_persona_wins_over_a_user_one_with_the_same_name() {
        let d = tmp("shadow");
        let user = tmp("shadow-user");
        save_in(Some(&d), Some(&user), &one("shared"), true).unwrap();
        let mut mine = one("shared");
        mine.prompt = "the user's version".into();
        mine.title = "User version".into();
        save_in(Some(&d), Some(&user), &mine, false).unwrap();
        let all = list_in(Some(&d), Some(&user));
        let found = all.iter().find(|p| p.id == "shared").unwrap();
        assert_eq!(found.title, "The Reviewer", "the project's one wins");
        assert_eq!(all.iter().filter(|p| p.id == "shared").count(), 1);
        delete_in(Some(&d), Some(&user), "shared").unwrap();
        // the project's one is gone; the user's version is what remains
        let after = list_in(Some(&d), Some(&user));
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].title, "User version");
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&user);
    }

    #[test]
    fn deleting_removes_confines_it_to_the_project() {
        let d = tmp("del");
        save(Some(&d), &one("gone"), true).unwrap();
        assert!(load(Some(&d), "gone").is_some());
        delete(Some(&d), "gone").unwrap();
        assert!(load(Some(&d), "gone").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
