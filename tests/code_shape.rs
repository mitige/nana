//! Keeps the shape rules of AGENTS.md true: every function outside the tests
//! has a doc comment and stays at or under 100 lines. The scan is textual on
//! purpose, so it needs no dependency and reads the same as the rule.

use std::fs;
use std::path::{Path, PathBuf};

const MAX_FN_LINES: usize = 100;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// the name of the function declared on this line, if the line declares one.
fn fn_name(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let after_vis = match trimmed.strip_prefix("pub") {
        Some(rest) if rest.starts_with('(') => rest.split_once(')')?.1.trim_start(),
        Some(rest) if rest.starts_with(' ') => rest.trim_start(),
        _ => trimmed,
    };
    let after_fn = after_vis.strip_prefix("fn ")?;
    let end = after_fn.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    Some(&after_fn[..end])
}

/// true when the line above the declaration (skipping attributes) is a `///` doc.
fn has_doc(lines: &[&str], index: usize) -> bool {
    lines[..index]
        .iter()
        .rev()
        .map(|l| l.trim())
        .find(|l| !l.starts_with("#["))
        .is_some_and(|l| l.starts_with("///"))
}

/// the line index of the closing brace that sits at the same indent as the fn.
fn closing_brace(lines: &[&str], index: usize) -> Option<usize> {
    if lines[index].trim_end().ends_with(';') {
        return None;
    }
    let indent: String = lines[index].chars().take_while(|c| *c == ' ').collect();
    let close = format!("{indent}}}");
    (index + 1..lines.len()).find(|&k| lines[k] == close)
}

/// true for files that only hold tests: a `tests.rs`, or anything in a `tests/` folder.
fn is_test_code(path: &Path) -> bool {
    path.ends_with("tests.rs") || path.components().any(|c| c.as_os_str() == "tests")
}

/// every problem found in one file, as `path:line name: reason` lines.
fn shape_problems(path: &Path, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut problems = Vec::new();
    let mut in_tests = false;
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]" {
            in_tests = true;
        }
        let Some(name) = fn_name(line) else {
            continue;
        };
        if in_tests {
            continue;
        }
        let place = format!("{}:{} {name}", path.display(), i + 1);
        if !has_doc(&lines, i) {
            problems.push(format!("{place}: no `///` doc comment"));
        }
        if let Some(close) = closing_brace(&lines, i) {
            let len = close - i + 1;
            if len > MAX_FN_LINES {
                problems.push(format!("{place}: {len} lines, over {MAX_FN_LINES}"));
            }
        }
    }
    problems
}

#[test]
fn every_function_outside_tests_is_documented_and_short() {
    let mut files = Vec::new();
    rust_files(Path::new("src"), &mut files);
    assert!(!files.is_empty(), "no source files found under src/");
    let mut problems = Vec::new();
    for path in &files {
        let text = fs::read_to_string(path).unwrap_or_default();
        if is_test_code(path) {
            continue;
        }
        problems.extend(shape_problems(path, &text));
    }
    assert!(
        problems.is_empty(),
        "shape rules broken:\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_scan_sees_a_missing_doc_and_a_long_function() {
    let sample = "/// ok\nfn fine() {\n}\n\nfn bare() {\n}\n";
    let problems = shape_problems(Path::new("x.rs"), sample);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("bare"));

    let mut long = String::from("/// long\nfn long() {\n");
    for _ in 0..120 {
        long.push_str("    let _ = 1;\n");
    }
    long.push_str("}\n");
    let problems = shape_problems(Path::new("y.rs"), &long);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("over 100"));
}

/// files over this many lines must be split by concept, unless they are debt.
const MAX_FILE_LINES: usize = 1200;

/// the files already over the limit, with the size they had when recorded.
/// the size can only shrink: a split lowers it, and growth is refused.
const FILE_DEBT: &[(&str, usize)] = &[];

/// the lines of a file, or none when it cannot be read.
fn line_count(path: &Path) -> usize {
    fs::read_to_string(path).map_or(0, |t| t.lines().count())
}

#[test]
fn no_file_grows_past_the_limit_outside_the_debt_list() {
    let mut files = Vec::new();
    rust_files(Path::new("src"), &mut files);
    let mut problems = Vec::new();
    for path in &files {
        let name = path.to_string_lossy().replace('\\', "/");
        let lines = line_count(path);
        match FILE_DEBT.iter().find(|(d, _)| *d == name) {
            Some((_, recorded)) if lines > *recorded => {
                problems.push(format!(
                    "{name}: {lines} lines, grew past its recorded {recorded}"
                ));
            }
            Some(_) => {}
            None if lines > MAX_FILE_LINES => {
                problems.push(format!(
                    "{name}: {lines} lines, over {MAX_FILE_LINES}; split it"
                ));
            }
            None => {}
        }
    }
    assert!(
        problems.is_empty(),
        "file shape broken:\n{}",
        problems.join("\n")
    );
}

/// the module file that declares a source file: its parent `mod.rs` or the
/// sibling `<dir>.rs`, and `lib.rs` or `main.rs` for the crate root files.
fn declaring_files(path: &Path) -> Vec<PathBuf> {
    let dir = path.parent().unwrap_or(Path::new("src"));
    if dir == Path::new("src") {
        return vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/main.rs")];
    }
    let sibling = dir.with_extension("rs");
    vec![sibling, dir.join("mod.rs")]
}

/// true when some declaring file says `mod <stem>;` for this source file.
fn is_declared(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return true;
    };
    declaring_files(path).iter().any(|decl| {
        fs::read_to_string(decl).is_ok_and(|text| text.lines().any(|l| declares_module(l, stem)))
    })
}

/// true when one line declares `mod <stem>;`, whatever its visibility.
fn declares_module(line: &str, stem: &str) -> bool {
    let rest = line.trim_start();
    let rest = match rest.strip_prefix("pub") {
        Some(after) if after.starts_with('(') => after.split_once(')').map_or("", |(_, r)| r),
        Some(after) => after,
        None => rest,
    };
    rest.trim_start()
        .strip_prefix("mod ")
        .is_some_and(|name| name.trim_end() == format!("{stem};"))
}

#[test]
fn every_source_file_is_declared_by_a_module() {
    let mut files = Vec::new();
    rust_files(Path::new("src"), &mut files);
    let orphans: Vec<String> = files
        .iter()
        .filter(|p| !is_test_code(p))
        .filter(|p| {
            !matches!(
                p.file_name().and_then(|n| n.to_str()),
                Some("mod.rs" | "lib.rs" | "main.rs")
            )
        })
        .filter(|p| !p.starts_with("src/bin"))
        .filter(|p| !is_declared(p))
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        orphans.is_empty(),
        "files not declared by any mod: {orphans:?}"
    );
}
