//! what nana checks after you save.
//!
//! two layers, both language aware:
//!   1. a small universal style pass (trailing whitespace, tabs, final
//!      newline, line length) — sober, predictable, no surprises;
//!   2. the language's own checker from the registry (gcc, rustc, node
//!      --check, python's ast, luac, bash -n…) parsed by [`crate::diag`].
//!
//! the result feeds the status line and the gutter marks. nothing blocks the
//! editor: the whole thing runs on a worker thread.

use crate::diag;
use crate::langs::{self, Lang};
use std::path::Path;
use std::process::Command;

/// severity, as drawn in the gutter: major = red, minor = amber.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Minor,
    Major,
}

/// one finding, anywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub severity: Severity,
    /// short text shown on hover
    pub message: String,
    /// where it comes from: the tool name, or `style`
    pub rule: String,
}

/// everything the editor needs after a check.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// one-line summary for the status area
    pub note: String,
    /// (line, severity) for the gutter
    pub marks: Vec<(usize, Severity)>,
    /// (line, severity, message) for the hover popup
    pub details: Vec<(usize, Severity, String)>,
    /// the raw findings, for tests and future use
    pub findings: Vec<Finding>,
}

/// style rules that apply to every language.
#[derive(Debug, Clone, Copy)]
pub struct StyleCfg {
    /// 0 disables the rule
    pub max_columns: usize,
    pub trailing_whitespace: bool,
    pub tabs: bool,
    pub final_newline: bool,
}

impl Default for StyleCfg {
    fn default() -> Self {
        Self {
            max_columns: 100,
            trailing_whitespace: true,
            tabs: true,
            final_newline: true,
        }
    }
}

/// the universal pass. text is what is on disk, so the gutter matches it.
pub fn style_findings(text: &str, cfg: &StyleCfg) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut lines: Vec<&str> = text.split('\n').collect();
    // split('\n') leaves a trailing "" when the file ends with a newline
    let ends_with_newline = text.ends_with('\n');
    if ends_with_newline {
        lines.pop();
    }
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        if cfg.trailing_whitespace && line.ends_with([' ', '\t']) {
            out.push(Finding {
                line: n,
                severity: Severity::Minor,
                message: "trailing whitespace".into(),
                rule: "style".into(),
            });
        }
        if cfg.tabs && line.starts_with('\t') {
            out.push(Finding {
                line: n,
                severity: Severity::Minor,
                message: "tab indentation".into(),
                rule: "style".into(),
            });
        }
        if cfg.max_columns > 0 {
            let width = unicode_width::UnicodeWidthStr::width(*line);
            if width > cfg.max_columns {
                out.push(Finding {
                    line: n,
                    severity: Severity::Minor,
                    message: format!("line of {width} columns (max {})", cfg.max_columns),
                    rule: "style".into(),
                });
            }
        }
    }
    if cfg.final_newline && !text.is_empty() && !ends_with_newline {
        out.push(Finding {
            line: lines.len(),
            severity: Severity::Minor,
            message: "the file must end with a newline".into(),
            rule: "style".into(),
        });
    }
    out
}

/// run the language checker and fold its output into findings.
///
/// `Err` carries a human note such as "gcc not found" — never a finding, so a
/// missing toolchain never shouts at you from the gutter.
pub fn run_checker(lang: &Lang, path: &Path) -> Result<Vec<Finding>, String> {
    let Some(cmd) = &lang.check else {
        return Ok(Vec::new());
    };
    let tmp = langs::tmp_dir();
    let (program, args) = langs::expand(cmd, path, &tmp);
    let out = Command::new(&program)
        .args(&args)
        .current_dir(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new(".")),
        )
        .output()
        .map_err(|e| format!("{program} not found ({e})"))?;
    let mut text = String::from_utf8_lossy(&out.stderr).to_string();
    if text.trim().is_empty() {
        // some checkers (gofmt, dart analyze) report on stdout
        text = String::from_utf8_lossy(&out.stdout).to_string();
    }
    let diags = diag::parse(&text, path);
    Ok(diags
        .into_iter()
        .map(|d| Finding {
            line: d.line,
            severity: if d.is_error {
                Severity::Major
            } else {
                Severity::Minor
            },
            message: d.message,
            rule: program.clone(),
        })
        .collect())
}

/// the whole check for one path: style + the language checker.
pub fn check_path(path: &Path, cfg: &StyleCfg) -> Report {
    let lang = langs::for_path(path);
    let mut findings = Vec::new();
    let mut tool_note = String::new();

    match std::fs::read_to_string(path) {
        Ok(text) => findings.extend(style_findings(&text, cfg)),
        Err(e) => {
            tool_note = format!("unreadable ({e})");
        }
    }
    if lang.check.is_some() {
        match run_checker(lang, path) {
            Ok(f) => findings.extend(f),
            Err(note) => tool_note = note,
        }
    }
    findings.sort_by_key(|f| (f.line, std::cmp::Reverse(f.severity)));

    let major = findings
        .iter()
        .filter(|f| f.severity == Severity::Major)
        .count();
    let minor = findings.len() - major;
    // the language is already the badge on the left: this only has to say
    // whether the norm holds, so it never repeats the name
    let note = if !tool_note.is_empty() && findings.is_empty() {
        tool_note
    } else if findings.is_empty() {
        "norm ✓".to_string()
    } else if major == 0 {
        format!("norm: {minor} note(s)")
    } else {
        format!("norm: {major} error(s), {minor} note(s)")
    };

    Report {
        note,
        marks: findings.iter().map(|f| (f.line, f.severity)).collect(),
        details: findings
            .iter()
            .map(|f| (f.line, f.severity, format!("{} ({})", f.message, f.rule)))
            .collect(),
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn style_catches_the_usual_suspects() {
        let text = "ok\nbad   \n\tindented\nno newline";
        let f = style_findings(
            text,
            &StyleCfg {
                max_columns: 0,
                ..Default::default()
            },
        );
        let msgs: Vec<&str> = f.iter().map(|x| x.message.as_str()).collect();
        assert!(msgs.contains(&"trailing whitespace"), "{msgs:?}");
        assert!(msgs.contains(&"tab indentation"), "{msgs:?}");
        assert!(
            msgs.contains(&"the file must end with a newline"),
            "{msgs:?}"
        );
    }

    #[test]
    fn style_counts_columns_with_unicode_width() {
        let text = format!("{}\n", "é".repeat(60));
        let f = style_findings(
            &text,
            &StyleCfg {
                max_columns: 50,
                ..Default::default()
            },
        );
        assert_eq!(f.len(), 1);
        assert!(f[0].message.contains("60 columns"));
    }

    #[test]
    fn a_clean_file_reports_nothing() {
        let f = style_findings("hello\nworld\n", &StyleCfg::default());
        assert!(f.is_empty(), "{f:?}");
    }

    #[test]
    fn unknown_language_is_never_fatal() {
        let tmp = std::env::temp_dir().join(format!("nana-check-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let p = tmp.join("notes.xyzzy");
        std::fs::write(&p, "just text\n").unwrap();
        let rep = check_path(&p, &StyleCfg::default());
        assert!(rep.findings.is_empty());
        assert!(!rep.note.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_broken_python_file_is_flagged_with_a_line() {
        if Command::new("python3").arg("-V").output().is_err() {
            return; // no python here: the test is a no-op, never a failure
        }
        let tmp = std::env::temp_dir().join(format!("nana-check-py-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let p = tmp.join("broken.py");
        std::fs::write(&p, "def f(:\n    pass\n").unwrap();
        let rep = check_path(&p, &StyleCfg::default());
        assert!(
            rep.findings.iter().any(|f| f.severity == Severity::Major),
            "a syntax error must be major: {:?}",
            rep.findings
        );
        assert!(
            rep.findings.iter().any(|f| f.line == 1),
            "the error sits on line 1: {:?}",
            rep.findings
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn missing_checkers_do_not_become_findings() {
        let mut lang = *langs::by_id("c").unwrap();
        lang.check = Some(langs::Cmd {
            program: "definitely-not-a-real-compiler-xyz",
            args: &["{file}"],
        });
        let tmp = PathBuf::from(std::env::temp_dir());
        let err = run_checker(&lang, &tmp.join("x.c")).unwrap_err();
        assert!(err.contains("not found"), "{err}");
    }
}
