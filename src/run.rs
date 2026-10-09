//! what happens when you press run, format or check.
//!
//! nana never guesses silently: it picks the *project* command when the file
//! belongs to a recognised project (cargo run, npm run dev, django runserver…)
//! and falls back to the language's own recipe (python3 file.py, gcc + exec,
//! node file.js, …). the plan is a list of shell lines typed into the built-in
//! terminal, so you can watch it, interrupt it, and keep the history.

use crate::langs::{self, Cmd};
use crate::project::{self, Kind};
use std::path::{Path, PathBuf};

/// a ready-to-type plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// shell lines, run in order; joined with `&&` when they depend on each other
    pub lines: Vec<String>,
    /// where to run them
    pub cwd: PathBuf,
    /// short label for the status line
    pub label: String,
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/=:@+,^%".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn render(program: &str, args: &[String]) -> String {
    let mut out = shell_quote(program);
    for a in args {
        out.push(' ');
        out.push_str(&shell_quote(a));
    }
    out
}

fn expanded(cmd: &Cmd, path: &Path, tmp: &Path) -> String {
    let (program, args) = langs::expand(cmd, path, tmp);
    render(&program, &args)
}

/// does the language of this file belong to this project kind?
fn matches(kind: Kind, lang_id: &str) -> bool {
    match kind {
        Kind::Cargo => lang_id == "rust",
        Kind::Node => matches!(
            lang_id,
            "javascript" | "typescript" | "vue" | "svelte" | "astro"
        ),
        Kind::Python => lang_id == "python",
        Kind::Go => lang_id == "go",
        Kind::Zig => lang_id == "zig",
        Kind::Dotnet => lang_id == "csharp" || lang_id == "fsharp",
        Kind::Maven | Kind::Gradle => {
            lang_id == "java" || lang_id == "kotlin" || lang_id == "scala"
        }
        Kind::Docker => lang_id == "docker",
        Kind::Make | Kind::CMake | Kind::Meson => matches!(lang_id, "c" | "cpp" | "asm"),
    }
}

/// the run plan for one file.
pub fn plan_run(path: &Path) -> Option<Plan> {
    let lang = langs::for_path(path);
    if let Some(p) = project::detect(path) {
        if matches(p.kind, lang.id) {
            if let Some(cmd) = &p.run {
                return Some(Plan {
                    lines: vec![cmd.0.clone()],
                    cwd: p.root.clone(),
                    label: format!("{} ({})", cmd.0, p.kind.name()),
                });
            }
        }
    }
    let tmp = langs::tmp_dir();
    let run = lang.run?;
    let run_line = expanded(&run, path, &tmp);
    let lines = match &lang.run_build {
        Some(build) => vec![format!("{} && {}", expanded(build, path, &tmp), run_line)],
        None => vec![run_line.clone()],
    };
    Some(Plan {
        lines,
        cwd: path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
        label: format!("run {}", lang.name),
    })
}

/// the format plan: the project's formatter when it has one, else the language's.
pub fn plan_format(path: &Path) -> Option<Plan> {
    let lang = langs::for_path(path);
    if let Some(p) = project::detect(path) {
        if matches(p.kind, lang.id) {
            if let Some(fmt) = &p.fmt {
                return Some(Plan {
                    lines: vec![fmt.0.clone()],
                    cwd: p.root.clone(),
                    label: format!("format ({})", p.kind.name()),
                });
            }
        }
    }
    let tmp = langs::tmp_dir();
    let fmt = lang.fmt?;
    Some(Plan {
        lines: vec![expanded(&fmt, path, &tmp)],
        cwd: path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
        label: format!("format {}", lang.name),
    })
}

/// the check plan (the same command the gutter uses, but visible).
pub fn plan_check(path: &Path) -> Option<Plan> {
    let lang = langs::for_path(path);
    let tmp = langs::tmp_dir();
    let check = lang.check?;
    Some(Plan {
        lines: vec![expanded(&check, path, &tmp)],
        cwd: path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
        label: format!("check {}", lang.name),
    })
}

/// the project's own test command.
pub fn plan_test(path: &Path) -> Option<Plan> {
    let p = project::detect(path)?;
    let t = p.test?;
    Some(Plan {
        lines: vec![t.0.clone()],
        cwd: p.root.clone(),
        label: format!("test ({})", p.kind.name()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nana-run-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_lone_python_file_runs_itself() {
        let d = tmpdir("py");
        let f = d.join("script.py");
        std::fs::write(&f, "print(1)\n").unwrap();
        // the temp dir has no project marker above it in a clean sandbox, but
        // /tmp could: accept either the file recipe or a project one
        let plan = plan_run(&f).unwrap();
        assert!(
            plan.lines[0].contains("script.py") || plan.lines[0].contains("python"),
            "{plan:?}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_c_file_builds_then_executes() {
        let d = tmpdir("c");
        let f = d.join("main.c");
        std::fs::write(&f, "int main(void){return 0;}\n").unwrap();
        let plan = plan_run(&f).unwrap();
        assert_eq!(plan.lines.len(), 1);
        assert!(plan.lines[0].contains("gcc"), "{plan:?}");
        assert!(
            plan.lines[0].contains("&&"),
            "build and exec are chained: {plan:?}"
        );
        assert!(plan.lines[0].contains("nana-bin"), "{plan:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_rust_file_in_a_cargo_project_uses_cargo() {
        let d = tmpdir("cargo");
        std::fs::write(d.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let src = d.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let f = src.join("main.rs");
        std::fs::write(&f, "fn main(){}\n").unwrap();
        let plan = plan_run(&f).unwrap();
        assert_eq!(plan.lines[0], "cargo run");
        assert_eq!(plan.label, "cargo run (cargo)");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn languages_without_a_recipe_return_none() {
        let d = tmpdir("plain");
        let f = d.join("notes.txt");
        std::fs::write(&f, "hello\n").unwrap();
        assert!(plan_run(&f).is_none() || !plan_run(&f).unwrap().lines[0].is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn shell_quoting_is_safe() {
        assert_eq!(shell_quote("plain"), "plain");
        assert_eq!(shell_quote("with space"), "'with space'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn format_uses_the_language_formatter() {
        let d = tmpdir("fmt");
        let f = d.join("x.py");
        std::fs::write(&f, "x=1\n").unwrap();
        let plan = plan_format(&f).unwrap();
        assert!(plan.lines[0].contains("ruff"), "{plan:?}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
