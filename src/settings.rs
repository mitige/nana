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
    /// "openai" | "anthropic" | "gemini"; anything else means openai-compatible
    pub kind: String,
    /// the name shown in the hub, when the id is not enough
    pub label: Option<String>,
    pub base_url: String,
    /// environment variable holding the key; an empty string means no key
    pub api_key_env: Option<String>,
    /// model names this endpoint serves, so a model picks it by itself
    pub models: Vec<String>,
}

impl ProviderCfg {
    /// Reads one custom endpoint from a line typed in the hub, in the form
    /// `id url [key env] [kind]`, and hands back its id with its settings.
    ///
    /// A `-` where the key env goes means the endpoint wants no key at all;
    /// `kind` is `openai` (the default), `anthropic` or `gemini`. The error
    /// spells the form out again, because that is the only place the user
    /// learns it.
    pub fn parse(line: &str) -> Result<(String, ProviderCfg), String> {
        let form = "usage: id url [key env] [kind] — e.g. gw https://gw.example/v1 GW_KEY";
        let mut words = line.split_whitespace();
        let id = words.next().ok_or_else(|| form.to_string())?.to_string();
        let base_url = words.next().ok_or_else(|| form.to_string())?.to_string();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(format!("{base_url} is not an http url — {form}"));
        }
        let key = words.next().unwrap_or("NANA_API_KEY");
        let kind = words.next().unwrap_or("openai");
        if words.next().is_some() {
            return Err(form.to_string());
        }
        Ok((
            id,
            ProviderCfg {
                kind: kind.to_string(),
                label: None,
                base_url,
                api_key_env: Some(if key == "-" {
                    String::new()
                } else {
                    key.into()
                }),
                models: Vec::new(),
            },
        ))
    }
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

    /// copies every setting that `other` sets over this one, leaving the rest as they were.
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

    /// off by default: the agent may reach outside its project unless asked to stay in.
    pub fn sandbox_enabled(&self) -> bool {
        self.sandbox.unwrap_or(false)
    }

    /// writes the settings in the user folder and hands back the path, falling back to the current folder.
    pub fn save_user(&self) -> std::io::Result<PathBuf> {
        let path = user_path().unwrap_or_else(|| PathBuf::from("nana-settings.json"));
        write(&path, self)?;
        Ok(path)
    }

    /// writes the settings into the project folder and hands back the path.
    pub fn save_project(&self, root: &Path) -> std::io::Result<PathBuf> {
        let path = project_path(root);
        write(&path, self)?;
        Ok(path)
    }
}

/// reads a settings file; a missing file or a broken one gives the defaults, with a warning for a broken one.
fn read(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t).unwrap_or_else(|e| {
            eprintln!("nana: {} is not valid json: {e}", path.display());
            Settings::default()
        }),
        Err(_) => Settings::default(),
    }
}

/// writes the settings as pretty json, creating the folder first.
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
    fn defaults_are_open_and_silent() {
        let s = Settings::default();
        assert!(!s.sandbox_enabled());
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
        assert!(!s.sandbox_enabled(), "an untouched key keeps its default");
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

    #[test]
    fn a_typed_provider_line_becomes_a_custom_endpoint() {
        let (id, cfg) = ProviderCfg::parse("gw https://gw.example/v1 GW_KEY anthropic").unwrap();
        assert_eq!(id, "gw");
        assert_eq!(cfg.base_url, "https://gw.example/v1");
        assert_eq!(cfg.api_key_env.as_deref(), Some("GW_KEY"));
        assert_eq!(cfg.kind, "anthropic");
    }

    #[test]
    fn a_typed_provider_line_defaults_to_openai_and_the_usual_key_var() {
        let (_, cfg) = ProviderCfg::parse("gw https://gw.example/v1").unwrap();
        assert_eq!(cfg.kind, "openai");
        assert_eq!(cfg.api_key_env.as_deref(), Some("NANA_API_KEY"));
        // a dash where the key goes means this endpoint wants no key at all
        let (_, cfg) = ProviderCfg::parse("gw http://127.0.0.1:8000/v1 -").unwrap();
        assert_eq!(cfg.api_key_env.as_deref(), Some(""));
    }

    #[test]
    fn a_typed_provider_line_that_makes_no_sense_is_refused() {
        // too few words, a non-http url, and too many words
        for bad in ["gw", "gw gw.example/v1", "gw https://x/v1 KEY openai extra"] {
            let err = ProviderCfg::parse(bad).unwrap_err();
            assert!(err.contains("usage"), "{bad} gave {err}");
        }
    }
}
