//! user configuration: `~/.config/nana/config.toml`
//!
//! every key is optional; the defaults are the sober ones. the file is created
//! commented-out on first run so nothing is hidden from you.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// written in the auto header (f4) when set
    pub author: Option<String>,
    /// model used for the optional ghost completion
    pub ai_model: Option<String>,
    /// 1 socratic .. 5 full analysis
    pub ai_help_level: Option<u8>,
    /// completion persona, persisted between sessions
    pub nana_agent: Option<String>,
    /// what the save-time style pass reports
    #[serde(flatten)]
    pub style: StyleSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleSection {
    /// 0 disables the long-line note
    pub max_columns: usize,
    pub trailing_whitespace: bool,
    pub tabs: bool,
    pub final_newline: bool,
}

impl Default for StyleSection {
    fn default() -> Self {
        let d = crate::check::StyleCfg::default();
        Self {
            max_columns: d.max_columns,
            trailing_whitespace: d.trailing_whitespace,
            tabs: d.tabs,
            final_newline: d.final_newline,
        }
    }
}

impl StyleSection {
    pub fn to_cfg(&self) -> crate::check::StyleCfg {
        crate::check::StyleCfg {
            max_columns: self.max_columns,
            trailing_whitespace: self.trailing_whitespace,
            tabs: self.tabs,
            final_newline: self.final_newline,
        }
    }
}

/// HOME on unix, USERPROFILE on windows.
pub fn home_dir() -> PathBuf {
    for var in ["HOME", "USERPROFILE"] {
        if let Ok(h) = std::env::var(var) {
            if !h.is_empty() {
                return PathBuf::from(h);
            }
        }
    }
    PathBuf::from(".")
}

fn dirs_config() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"))
}

pub fn config_path() -> PathBuf {
    dirs_config().join("nana").join("config.toml")
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local").join("share"))
        .join("nana")
}

pub fn load() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("nana: {} is not valid toml: {e}", path.display());
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

/// persist the completion persona without touching the rest of the file.
pub fn save_nana_agent(agent: &str) {
    save_key("nana_agent", agent);
}

/// persist the chosen model.
pub fn save_ai_model(model: &str) {
    save_key("ai_model", model);
}

fn save_key(key: &str, value: &str) {
    let path = config_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let new_line = format!("{key} = \"{value}\"");
    let mut done = false;
    for line in &mut lines {
        let t = line.trim_start();
        if t.starts_with(key) || t.starts_with(&format!("# {key}")) {
            *line = new_line.clone();
            done = true;
            break;
        }
    }
    if !done {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(new_line);
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, lines.join("\n") + "\n");
}

/// create the commented config on first run.
pub fn ensure_exists() -> std::io::Result<PathBuf> {
    let path = config_path();
    if path.exists() {
        return Ok(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, DEFAULT_CONFIG)?;
    Ok(path)
}

pub const DEFAULT_CONFIG: &str = r##"# nana configuration — every key is optional.

# written in the auto header (f4) when set:
# author = "your name"

# optional ghost completion (needs an openai-compatible key in the env):
# ai_model = "cheapmodels/claude-opus-5.5"
# ai_help_level = 1        # 1 socratic .. 5 full analysis
# nana_agent = "the elder"

# the style pass that runs after every save:
# max_columns = 100        # 0 disables the long-line note
# trailing_whitespace = true
# tabs = true
# final_newline = true
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_config_parses_and_keeps_the_defaults() {
        let c: Config = toml::from_str(DEFAULT_CONFIG).unwrap();
        assert_eq!(c.style.max_columns, 100);
        assert!(c.style.trailing_whitespace);
        assert!(c.author.is_none());
    }

    #[test]
    fn a_partial_config_keeps_the_other_defaults() {
        let c: Config = toml::from_str("author = \"me\"\n").unwrap();
        assert_eq!(c.author.as_deref(), Some("me"));
        assert_eq!(c.style.max_columns, 100);
    }

    #[test]
    fn the_style_section_feeds_the_checker() {
        let c: Config = toml::from_str("max_columns = 42\n").unwrap();
        let cfg = c.style.to_cfg();
        assert_eq!(cfg.max_columns, 42);
    }

    #[test]
    fn the_config_lives_under_nana() {
        assert!(
            config_path().ends_with("nana/config.toml"),
            "{:?}",
            config_path()
        );
    }
}
