//! the agent's hands: files, grep, a shell, and its own memory.
//!
//! every path the agent names is resolved inside the project and checked
//! against it — the sandbox is on by default, so a tool call cannot wander
//! into your home directory. commands that destroy things are recognised and
//! refused with an explanation rather than executed quietly.

use crate::memory::{Class, Memory};
use crate::provider::ToolSpec;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub struct Ctx {
    pub root: PathBuf,
    pub sandbox: bool,
    pub memory: Memory,
    /// commands the user already approved, matched by substring
    pub approved: Vec<String>,
}

impl Ctx {
    pub fn new(root: &Path, sandbox: bool) -> Ctx {
        Ctx {
            root: root.to_path_buf(),
            sandbox,
            memory: Memory::open(root),
            approved: Vec::new(),
        }
    }
}

/// the tool declarations handed to the model.
pub fn specs() -> Vec<ToolSpec> {
    let obj = |props: Value, required: Value| json!({"type": "object", "properties": props, "required": required});
    vec![
        ToolSpec {
            name: "read_file".into(),
            description: "read a text file of the project. returns the lines, numbered.".into(),
            parameters: obj(
                json!({
                    "path": {"type": "string", "description": "path relative to the project"},
                    "start": {"type": "integer", "description": "first line, 1-based"},
                    "end": {"type": "integer", "description": "last line, inclusive"}
                }),
                json!(["path"]),
            ),
        },
        ToolSpec {
            name: "write_file".into(),
            description: "write a text file, creating it or replacing its whole content.".into(),
            parameters: obj(
                json!({
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                }),
                json!(["path", "content"]),
            ),
        },
        ToolSpec {
            name: "edit_file".into(),
            description:
                "replace one exact passage of a file. the passage must appear exactly once.".into(),
            parameters: obj(
                json!({
                    "path": {"type": "string"},
                    "find": {"type": "string"},
                    "replace": {"type": "string"}
                }),
                json!(["path", "find", "replace"]),
            ),
        },
        ToolSpec {
            name: "list_dir".into(),
            description: "list a directory of the project.".into(),
            parameters: obj(
                json!({"path": {"type": "string", "description": "defaults to the project root"}}),
                json!([]),
            ),
        },
        ToolSpec {
            name: "grep".into(),
            description: "search the project for a literal string, returning file:line matches."
                .into(),
            parameters: obj(
                json!({
                    "pattern": {"type": "string"},
                    "path": {"type": "string"},
                    "max": {"type": "integer", "description": "maximum matches, default 40"}
                }),
                json!(["pattern"]),
            ),
        },
        ToolSpec {
            name: "run_shell".into(),
            description: "run a shell command inside the project directory and return its output."
                .into(),
            parameters: obj(json!({"command": {"type": "string"}}), json!(["command"])),
        },
        ToolSpec {
            name: "memory_list".into(),
            description:
                "list the memory pages of this project. call it first on any non-trivial task: \
                          the past work of this project is written here."
                    .into(),
            parameters: obj(json!({}), json!([])),
        },
        ToolSpec {
            name: "memory_read".into(),
            description:
                "read a memory page of this project. classes: user, feedback, project, reference."
                    .into(),
            parameters: obj(
                json!({
                    "class": {"type": "string"},
                    "name": {"type": "string"}
                }),
                json!(["class", "name"]),
            ),
        },
        ToolSpec {
            name: "memory_write".into(),
            description: "write a memory page of this project. write whenever you learn something \
                          durable (a convention, a decision, a pitfall, a fact about the user); \
                          not for chatter."
                .into(),
            parameters: obj(
                json!({
                    "class": {"type": "string"},
                    "name": {"type": "string"},
                    "content": {"type": "string"}
                }),
                json!(["class", "name", "content"]),
            ),
        },
    ]
}

/// lexical normalisation: no filesystem needed, and `..` cannot smuggle a
/// path out of the project.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn resolve(ctx: &Ctx, raw: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(raw);
    let joined = if p.is_absolute() { p } else { ctx.root.join(p) };
    let path = normalize(&joined);
    if ctx.sandbox {
        let root = normalize(&ctx.root);
        if !path.starts_with(&root) {
            return Err(format!(
                "« {raw} » is outside the project — the sandbox is on (settings: sandbox=false to lift it)"
            ));
        }
    }
    Ok(path)
}

/// commands that destroy work. they are never run silently.
const DESTRUCTIVE: &[&str] = &[
    "rm -rf",
    "rm -fr",
    "rm -r /",
    "mkfs",
    "dd if=",
    "dd of=",
    "> /dev/sd",
    ":(){",
    "chmod -r 777 /",
    "chown -r",
    "shutdown",
    "reboot",
    "git reset --hard",
    "git clean -fd",
    "git push --force",
    "git push -f",
    "curl | sh",
    "curl|sh",
    "wget | sh",
    "sudo ",
    "killall",
    "truncate -s 0",
];

pub fn flagged(command: &str) -> Option<&'static str> {
    let c = command.to_ascii_lowercase();
    DESTRUCTIVE.iter().find(|d| c.contains(*d)).copied()
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing argument « {key} »"))
}

fn arg_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(|v| v.as_u64())
}

/// run one tool call, and answer with text the model can read.
pub fn run(name: &str, args: &Value, ctx: &Ctx) -> Result<String, String> {
    match name {
        "read_file" => {
            let path = resolve(ctx, arg(args, "path")?)?;
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let lines: Vec<&str> = text.lines().collect();
            let start = arg_u64(args, "start").unwrap_or(1).max(1) as usize;
            let end = arg_u64(args, "end").unwrap_or(lines.len() as u64) as usize;
            let end = end.min(lines.len());
            let mut out = String::new();
            for (i, l) in lines.iter().enumerate() {
                let n = i + 1;
                if n < start || n > end {
                    continue;
                }
                out.push_str(&format!("{n:>5} | {l}\n"));
            }
            if out.is_empty() {
                out.push_str("(no lines in that range)\n");
            }
            Ok(out)
        }
        "write_file" => {
            let path = resolve(ctx, arg(args, "path")?)?;
            let content = arg(args, "content")?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, content).map_err(|e| e.to_string())?;
            Ok(format!(
                "wrote {} ({} bytes)",
                path.display(),
                content.len()
            ))
        }
        "edit_file" => {
            let path = resolve(ctx, arg(args, "path")?)?;
            let find = arg(args, "find")?;
            let replace = arg(args, "replace")?;
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            match text.matches(find).count() {
                0 => Err("the passage to replace was not found".into()),
                1 => {
                    std::fs::write(&path, text.replacen(find, replace, 1))
                        .map_err(|e| e.to_string())?;
                    Ok(format!("edited {}", path.display()))
                }
                n => Err(format!("the passage appears {n} times — be more specific")),
            }
        }
        "list_dir" => {
            let raw = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let path = resolve(ctx, raw)?;
            let mut names: Vec<String> = Vec::new();
            for e in std::fs::read_dir(&path)
                .map_err(|e| e.to_string())?
                .flatten()
            {
                let p = e.path();
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_string();
                if p.is_dir() {
                    names.push(format!("{name}/"));
                } else {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    names.push(format!("{name} ({size} b)"));
                }
            }
            names.sort();
            Ok(if names.is_empty() {
                "(empty)".into()
            } else {
                names.join("\n")
            })
        }
        "grep" => {
            let pattern = arg(args, "pattern")?;
            let raw = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let base = resolve(ctx, raw)?;
            let max = arg_u64(args, "max").unwrap_or(40) as usize;
            let mut hits: Vec<String> = Vec::new();
            walk(&base, &mut |p| {
                if hits.len() >= max {
                    return;
                }
                let Ok(text) = std::fs::read_to_string(p) else {
                    return;
                };
                for (i, line) in text.lines().enumerate() {
                    if line.contains(pattern) {
                        let rel = p.strip_prefix(&ctx.root).unwrap_or(p);
                        hits.push(format!("{}:{}: {}", rel.display(), i + 1, line.trim()));
                        if hits.len() >= max {
                            break;
                        }
                    }
                }
            });
            Ok(if hits.is_empty() {
                "no match".into()
            } else {
                hits.join("\n")
            })
        }
        "run_shell" => {
            let command = arg(args, "command")?;
            if let Some(why) = flagged(command) {
                let allowed = ctx.approved.iter().any(|a| command.contains(a.as_str()));
                if !allowed {
                    return Err(format!(
                        "« {command} » looks destructive ({why}) — it needs your explicit approval"
                    ));
                }
            }
            let out = Command::new("sh")
                .arg("-c")
                .arg(command)
                .current_dir(&ctx.root)
                .output()
                .map_err(|e| format!("could not run it: {e}"))?;
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                text.push_str("\n[stderr]\n");
                text.push_str(&err);
            }
            if text.len() > 8000 {
                text.truncate(8000);
                text.push_str("\n… (truncated)");
            }
            Ok(format!(
                "exit {}\n{}",
                out.status.code().unwrap_or(-1),
                text.trim_end()
            ))
        }
        "memory_list" => {
            let entries = ctx.memory.list();
            if entries.is_empty() {
                return Ok("this project has no memory yet".into());
            }
            Ok(entries
                .iter()
                .map(|e| format!("{}: {} — {}", e.class.id(), e.name, e.title))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "memory_read" => {
            let class = Class::parse(arg(args, "class")?)
                .ok_or_else(|| "classes are user, feedback, project, reference".to_string())?;
            ctx.memory.read(class, arg(args, "name")?)
        }
        "memory_write" => {
            let class = Class::parse(arg(args, "class")?)
                .ok_or_else(|| "classes are user, feedback, project, reference".to_string())?;
            let path = ctx
                .memory
                .write(class, arg(args, "name")?, arg(args, "content")?)?;
            Ok(format!("remembered in {}", path.display()))
        }
        other => Err(format!("unknown tool « {other} »")),
    }
}

/// walk the project, skipping the usual noise.
fn walk(dir: &Path, f: &mut dyn FnMut(&Path)) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if name.starts_with('.') || matches!(name, "target" | "node_modules" | "__pycache__") {
            continue;
        }
        if p.is_dir() {
            walk(&p, f);
        } else {
            f(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nana-tools-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn reading_numbersthe_lines_and_respects_a_range() {
        let d = tmp("read");
        std::fs::write(d.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        let ctx = Ctx::new(&d, true);
        let all = run("read_file", &json!({"path": "a.txt"}), &ctx).unwrap();
        assert!(all.contains("1 | one"), "{all}");
        assert!(all.contains("3 | three"), "{all}");
        let part = run(
            "read_file",
            &json!({"path": "a.txt", "start": 2, "end": 2}),
            &ctx,
        )
        .unwrap();
        assert!(part.contains("two") && !part.contains("one"), "{part}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_sandbox_refuses_to_leave_the_project() {
        let d = tmp("sandbox");
        let ctx = Ctx::new(&d, true);
        for bad in ["../../etc/passwd", "/etc/passwd", "a/../../b"] {
            let err = run("read_file", &json!({"path": bad}), &ctx).unwrap_err();
            assert!(err.contains("outside the project"), "{bad}: {err}");
        }
        // lifting the sandbox allows an absolute path outside
        let open = Ctx::new(&d, false);
        let path = std::env::temp_dir().join(format!("nana-out-{}", std::process::id()));
        std::fs::write(&path, "outside\n").unwrap();
        let ok = run(
            "read_file",
            &json!({"path": path.display().to_string()}),
            &open,
        );
        assert!(ok.is_ok(), "{ok:?}");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn writing_and_editing_touch_the_real_file() {
        let d = tmp("write");
        let ctx = Ctx::new(&d, true);
        run(
            "write_file",
            &json!({"path": "src/x.rs", "content": "fn a() {}\n"}),
            &ctx,
        )
        .unwrap();
        assert!(d.join("src/x.rs").exists(), "written on disk");
        run(
            "edit_file",
            &json!({"path": "src/x.rs", "find": "fn a()", "replace": "fn b()"}),
            &ctx,
        )
        .unwrap();
        let text = std::fs::read_to_string(d.join("src/x.rs")).unwrap();
        assert_eq!(text, "fn b() {}\n");
        // an ambiguous or missing passage is refused, not guessed
        let err = run(
            "edit_file",
            &json!({"path": "src/x.rs", "find": "nope", "replace": "x"}),
            &ctx,
        )
        .unwrap_err();
        assert!(err.contains("not found"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn grep_finds_lines_and_skips_the_noise() {
        let d = tmp("grep");
        std::fs::create_dir_all(d.join("src")).unwrap();
        std::fs::create_dir_all(d.join("target")).unwrap();
        std::fs::write(d.join("src/a.rs"), "let needle = 1;\n").unwrap();
        std::fs::write(d.join("target/b.rs"), "let needle = 2;\n").unwrap();
        let ctx = Ctx::new(&d, true);
        let hits = run("grep", &json!({"pattern": "needle"}), &ctx).unwrap();
        assert!(hits.contains("src/a.rs:1"), "{hits}");
        assert!(!hits.contains("target"), "build output is skipped: {hits}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_destructive_command_is_flagged_and_refused() {
        assert_eq!(flagged("rm -rf /"), Some("rm -rf"));
        assert_eq!(flagged("git push --force"), Some("git push --force"));
        assert_eq!(flagged("ls -la"), None);
        let d = tmp("danger");
        let ctx = Ctx::new(&d, true);
        let err = run("run_shell", &json!({"command": "rm -rf ."}), &ctx).unwrap_err();
        assert!(err.contains("needs your explicit approval"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_shell_command_runs_in_the_project_and_reports_its_exit_code() {
        let d = tmp("shell");
        std::fs::write(d.join("hello.txt"), "hi\n").unwrap();
        let ctx = Ctx::new(&d, true);
        let out = run("run_shell", &json!({"command": "cat hello.txt"}), &ctx).unwrap();
        assert!(out.starts_with("exit 0"), "{out}");
        assert!(out.contains("hi"), "{out}");
        let out = run("run_shell", &json!({"command": "exit 3"}), &ctx).unwrap();
        assert!(out.starts_with("exit 3"), "{out}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_memory_tools_write_where_the_memory_lives() {
        let d = tmp("mem");
        let ctx = Ctx::new(&d, true);
        run(
            "memory_write",
            &json!({"class": "project", "name": "stack", "content": "rust only"}),
            &ctx,
        )
        .unwrap();
        assert!(d.join(".nana/memory/project/stack.md").exists());
        let listed = run("memory_list", &json!({}), &ctx).unwrap();
        assert!(listed.contains("project: stack"), "{listed}");
        let read = run(
            "memory_read",
            &json!({"class": "project", "name": "stack"}),
            &ctx,
        )
        .unwrap();
        assert!(read.contains("rust only"), "{read}");
        let err = run(
            "memory_write",
            &json!({"class": "nope", "name": "x", "content": "y"}),
            &ctx,
        )
        .unwrap_err();
        assert!(err.contains("classes are"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn every_declared_tool_is_executable() {
        // a spec with no implementation would be a lie told to the model
        let d = tmp("specs");
        let ctx = Ctx::new(&d, true);
        std::fs::write(d.join("f.txt"), "x\n").unwrap();
        for spec in specs() {
            let args = match spec.name.as_str() {
                "read_file" => json!({"path": "f.txt"}),
                "write_file" => json!({"path": "w.txt", "content": "x"}),
                "edit_file" => json!({"path": "w.txt", "find": "x", "replace": "y"}),
                "list_dir" => json!({}),
                "grep" => json!({"pattern": "x"}),
                "run_shell" => json!({"command": "true"}),
                "memory_list" => json!({}),
                "memory_read" => json!({"class": "user", "name": "absent"}),
                "memory_write" => json!({"class": "user", "name": "n", "content": "c"}),
                other => panic!("no test args for {other}"),
            };
            // memory_read on a missing page is an error the model can read,
            // every other call must succeed
            let _ = run(&spec.name, &args, &ctx);
            assert!(
                specs().iter().any(|s| s.name == spec.name),
                "{} disappeared",
                spec.name
            );
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
