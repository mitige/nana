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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub class: Class,
    pub name: String,
    pub path: PathBuf,
    /// the first heading line, for the index
    pub title: String,
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
        let path = self.safe_path(class, name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let title = name.trim().trim_end_matches(".md");
        let text = if body.trim_start().starts_with('#') {
            body.to_string()
        } else {
            format!("# {title}\n\n{}\n", body.trim_end())
        };
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
                    let title = std::fs::read_to_string(&p)
                        .ok()
                        .and_then(|t| {
                            t.lines()
                                .find(|l| l.trim_start().starts_with('#'))
                                .map(|l| l.trim_start_matches('#').trim().to_string())
                        })
                        .unwrap_or_else(|| name.clone());
                    Entry {
                        class,
                        name,
                        path: p,
                        title,
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
}
