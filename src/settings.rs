//! one settings file, project first.
//!
//! `<project>/.nana/settings.json` overrides `~/.config/nana/settings.json`,
//! which overrides the built-in defaults. every key is optional, and the file
//! is plain json you can read and edit — there is no hidden state.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const PROJECT_DIR: &str = ".nana";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// provider id to use, when the model name is not enough
    pub provider: Option<String>,
    /// model id, e.g. "cheapmodels/claude-opus-5.5", "claude-sonnet-4", "llama3.1"
    pub model: Option<String>,
    /// override the provider's endpoint
    pub base_url: Option<String>,
    /// name of the environment variable holding the key
    pub api_key_env: Option<String>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// keep the agent inside the project directory (default true)
    pub sandbox: Option<bool>,
    /// persona preset used by default
    pub persona: Option<String>,
    /// extra providers, merged with the built-in ones
    pub providers: BTreeMap<String, ProviderCfg>,
    /// directories searched for skills, beyond the defaults
    pub skill_dirs: Vec<String>,
}

/// a provider you added yourself — any openai-compatible endpoint will do.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderCfg {
    /// "openai" | "anthropic" | "gemini" | "ollama"
    pub kind: String,
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub models: Vec<String>,
}

impl Settings {
    /// project settings, over the user's, over the defaults.
    pub fn load(project_root: &Path) -> Settings {
        let mut s = Settings::default();
        if let Some(p) = user_path().filter(|p| p.exists()) {
            s.merge(read(&p));
        }
        let p = project_path(project_root);
        if p.exists() {
            s.merge(read(&p));
        }
        s
    }

    fn merge(&mut self, other: Settings) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f = other.$f; } )* };
        }
        take!(
            provider,
            model,
            base_url,
            api_key_env,
            temperature,
            max_tokens,
            sandbox,
            persona
        );
        if !other.providers.is_empty() {
            for (k, v) in other.providers {
                self.providers.insert(k, v);
            }
        }
        if !other.skill_dirs.is_empty() {
            self.skill_dirs = other.skill_dirs;
        }
    }

    /// sandboxed by default: the agent works in its project, not in your home.
    pub fn sandbox_enabled(&self) -> bool {
        self.sandbox.unwrap_or(true)
    }

    pub fn save_user(&self) -> std::io::Result<PathBuf> {
        let path = user_path().unwrap_or_else(|| PathBuf::from("nana-settings.json"));
        write(&path, self)?;
        Ok(path)
    }

    pub fn save_project(&self, root: &Path) -> std::io::Result<PathBuf> {
        let path = project_path(root);
        write(&path, self)?;
        Ok(path)
    }
}

fn read(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
            eprintln!("nana: {} is not valid json: {e}", path.display());
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

fn write(path: &Path, s: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(s).unwrap_or_else(|_| "{}".into());
    std::fs::write(path, text + "\n")
}

/// `~/.config/nana/settings.json`
pub fn user_path() -> Option<PathBuf> {
    Some(crate::config::config_path().with_file_name("settings.json"))
}

/// `<project>/.nana/settings.json`
pub fn project_path(root: &Path) -> PathBuf {
    root.join(PROJECT_DIR).join("settings.json")
}

/// the project directory nana owns inside a repository.
pub fn project_dir(root: &Path) -> PathBuf {
    root.join(PROJECT_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nana-set-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        d
    }

    #[test]
    fn defaults_are_sandboxed_and_silent() {
        let s = Settings::default();
        assert!(s.sandbox_enabled());
        assert!(s.model.is_none());
    }

    #[test]
    fn the_project_file_wins() {
        let d = tmp("win");
        std::fs::write(
            project_path(&d),
            r#"{"model":"project-model","temperature":0.2}"#,
        )
        .unwrap();
        let s = Settings::load(&d);
        assert_eq!(s.model.as_deref(), Some("project-model"));
        assert_eq!(s.temperature, Some(0.2));
        assert!(s.sandbox_enabled(), "an untouched key keeps its default");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn extra_providers_merge_instead_of_replacing() {
        let d = tmp("prov");
        std::fs::write(
            project_path(&d),
            r#"{"providers":{"mine":{"kind":"openai","base_url":"http://localhost:8080/v1"}}}"#,
        )
        .unwrap();
        let s = Settings::load(&d);
        assert_eq!(s.providers["mine"].base_url, "http://localhost:8080/v1");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_broken_file_is_not_fatal() {
        let d = tmp("broken");
        std::fs::write(project_path(&d), "{not json").unwrap();
        let s = Settings::load(&d);
        assert!(s.model.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
