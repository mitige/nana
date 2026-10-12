//! the typescript bootcamp helpers: the pinned setup of an `exXX/` folder and
//! the checks that tell the student what is missing, without writing any code
//! for him.

/// the `tsconfig.json` the subject asks for in each exercise folder.
pub const TSCONFIG_TEMPLATE: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "strict": true,
    "esModuleInterop": true,
    "outDir": "./dist",
    "types": ["node"]
  }
}
"#;

/// the `.gitignore` of an exercise folder: dependencies and build output stay
/// out of the repository.
pub const GITIGNORE_TEMPLATE: &str = "node_modules/\ndist/\n";

/// writes the missing templates into an exercise folder and hands back the
/// names it wrote. A file that already exists is left alone, so work the
/// student did is never overwritten.
pub fn write_templates(dir: &std::path::Path) -> std::io::Result<Vec<String>> {
    let mut written = Vec::new();
    for (name, body) in [
        ("tsconfig.json", TSCONFIG_TEMPLATE),
        (".gitignore", GITIGNORE_TEMPLATE),
    ] {
        let target = dir.join(name);
        if !target.exists() {
            std::fs::write(&target, body)?;
            written.push(name.to_string());
        }
    }
    Ok(written)
}

/// the versions and flags the bootcamp subject pins for an exercise folder.
pub const PINNED_TYPESCRIPT: &str = "6.0.3";
/// the pinned ts-node, the only one that runs with the pinned typescript.
pub const PINNED_TS_NODE: &str = "10.9.2";

/// one thing a `package.json` must or must not have, as the subject states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// a dev dependency is missing or not at the pinned version.
    Version {
        name: &'static str,
        found: Option<String>,
        want: &'static str,
    },
    /// `"type": "module"` is set, which breaks the commonjs setup.
    ModuleType,
    /// the tsconfig does not list `"types": ["node"]`, so node's globals are unknown.
    MissingNodeTypes,
}

/// one departure as a short lowercase sentence for the status line, saying
/// what to change and, for a version, what was found.
pub fn describe(finding: &Finding) -> String {
    match finding {
        Finding::ModuleType => "remove \"type\": \"module\" from package.json".into(),
        Finding::MissingNodeTypes => "tsconfig.json needs \"types\": [\"node\"]".into(),
        Finding::Version { name, found, want } => {
            let found = found.as_deref().unwrap_or("none");
            format!("{name} should be {want} (found {found})")
        }
    }
}

/// reads a `tsconfig.json` and reports a missing `"types": ["node"]`. Unreadable
/// JSON counts as missing, so the student sees the line he must add.
pub fn tsconfig_findings(text: &str) -> Vec<Finding> {
    let json: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    let types = json
        .pointer("/compilerOptions/types")
        .and_then(|t| t.as_array());
    let has_node = types.is_some_and(|list| list.iter().any(|t| t.as_str() == Some("node")));
    if has_node {
        Vec::new()
    } else {
        vec![Finding::MissingNodeTypes]
    }
}

/// reads a `package.json` and lists what departs from the subject: the two
/// pinned dev dependencies and the absence of `"type": "module"`. Unreadable
/// JSON counts as no pins at all, so the student sees both as missing.
pub fn package_findings(text: &str) -> Vec<Finding> {
    let json: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
    let mut out = Vec::new();
    if json.get("type").and_then(|t| t.as_str()) == Some("module") {
        out.push(Finding::ModuleType);
    }
    let dev = json.get("devDependencies");
    for (name, want) in [
        ("typescript", PINNED_TYPESCRIPT),
        ("ts-node", PINNED_TS_NODE),
    ] {
        let found = dev
            .and_then(|d| d.get(name))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if found.as_deref() != Some(want) {
            out.push(Finding::Version { name, found, want });
        }
    }
    out
}

/// the two commands the bootcamp runs on the current file, in order: the
/// compiler without emitting files, then ts-node executing it. Each one is
/// run on its own so the status line can say which of the two failed.
pub fn typescript_steps(file: &str) -> Vec<Step> {
    vec![
        Step {
            program: "tsc",
            args: vec!["--noEmit".into(), file.into()],
        },
        Step {
            program: "ts-node",
            args: vec![file.into()],
        },
    ]
}

/// runs the bootcamp check on one file: each step in turn, and the first step
/// that fails stops the run, so ts-node never runs on a file tsc rejected. It
/// hands back the verdict for the status line: the first line the failing step
/// printed, or that the file passes.
pub fn run_typescript_steps(file: &str) -> String {
    run_steps(&typescript_steps(file))
}

/// runs the given steps in order and hands back the verdict; the first failing
/// step stops the run.
pub fn run_steps(steps: &[Step]) -> String {
    for step in steps {
        let output = std::process::Command::new(step.program)
            .args(&step.args)
            .output();
        match output {
            Ok(out) if out.status.success() => {}
            Ok(out) => return first_line_of_failure(&out),
            Err(_) => return format!("{} is not installed", step.program),
        }
    }
    "the file passes tsc and ts-node".into()
}

/// the first non-empty line a failing step printed, on stderr or stdout, so the
/// status line shows the real error and not a blank.
fn first_line_of_failure(out: &std::process::Output) -> String {
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("the check failed")
        .to_string()
}

/// one command of the bootcamp check, with its arguments already resolved for
/// the current file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// the program to run, found on the PATH.
    pub program: &'static str,
    /// the arguments, in order.
    pub args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pinned_package_json_has_no_finding() {
        let text = r#"{"devDependencies":{"typescript":"6.0.3","ts-node":"10.9.2"}}"#;
        assert!(package_findings(text).is_empty());
    }

    #[test]
    fn a_wrong_typescript_version_is_reported() {
        let text = r#"{"devDependencies":{"typescript":"7.0.0","ts-node":"10.9.2"}}"#;
        let found = package_findings(text);
        assert_eq!(
            found,
            vec![Finding::Version {
                name: "typescript",
                found: Some("7.0.0".into()),
                want: "6.0.3",
            }]
        );
    }

    #[test]
    fn a_missing_ts_node_is_reported() {
        let text = r#"{"devDependencies":{"typescript":"6.0.3"}}"#;
        let found = package_findings(text);
        assert_eq!(
            found,
            vec![Finding::Version {
                name: "ts-node",
                found: None,
                want: "10.9.2"
            }]
        );
    }

    #[test]
    fn type_module_is_reported() {
        let text =
            r#"{"type":"module","devDependencies":{"typescript":"6.0.3","ts-node":"10.9.2"}}"#;
        assert_eq!(package_findings(text), vec![Finding::ModuleType]);
    }

    #[test]
    fn broken_json_reports_the_two_pins_as_missing() {
        let found = package_findings("{ not json");
        assert_eq!(found.len(), 2);
    }
}

/// true when the file belongs to the bootcamp: its language in the registry is
/// typescript. The language table decides, so no extension is named here.
pub fn applies_to(path: &std::path::Path) -> bool {
    crate::langs::for_path(path).id == "typescript"
}

/// the lines where the subject forbids a word: `any` as a type and `var` as a
/// keyword. Comments and string literals are skipped, so a word written in a
/// sentence does not raise a false alarm. Lines are 1-based.
pub fn forbidden_word_lines(text: &str) -> Vec<(usize, &'static str)> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let code = strip_comment_and_strings(raw);
        for word in ["any", "var"] {
            if contains_word(&code, word) {
                out.push((i + 1, word));
            }
        }
    }
    out
}

/// the part of one line that is code: everything after `//` and the quoted
/// strings removed, so the words inside them are not counted.
fn strip_comment_and_strings(line: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' || c == '`' => quote = Some(c),
            None if c == '/' && chars.peek() == Some(&'/') => break,
            None => out.push(c),
        }
    }
    out
}

/// the byte ranges of the forbidden words in one line of code, so the editor
/// can paint them. A word in a comment or a string is not a range.
pub fn forbidden_word_spans(line: &str) -> Vec<std::ops::Range<usize>> {
    let mut spans = Vec::new();
    for word in ["any", "var"] {
        let code = strip_comment_and_strings(line);
        for (at, _) in code.match_indices(word) {
            let before = code[..at].chars().next_back();
            let after = code[at + word.len()..].chars().next();
            if !before.is_some_and(is_ident) && !after.is_some_and(is_ident) {
                spans.push(at..at + word.len());
            }
        }
    }
    spans
}

/// true when `word` appears in `code` as a whole identifier, not inside a
/// longer name such as `anyway` or `variable`.
fn contains_word(code: &str, word: &str) -> bool {
    code.match_indices(word).any(|(at, _)| {
        let before = code[..at].chars().next_back();
        let after = code[at + word.len()..].chars().next();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

/// the characters that make up a TypeScript identifier.
fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

#[cfg(test)]
mod check_tests {
    use super::*;

    #[test]
    fn the_typescript_check_runs_tsc_then_ts_node() {
        let steps = typescript_steps("ex00/hello.ts");
        assert_eq!(steps[0].program, "tsc");
        assert_eq!(steps[1].program, "ts-node");
        assert!(steps[1].args.iter().any(|a| a == "ex00/hello.ts"));
    }

    #[test]
    fn a_tsconfig_without_node_types_is_reported() {
        let text = r#"{"compilerOptions":{"target":"ES2020","strict":true}}"#;
        assert_eq!(tsconfig_findings(text), vec![Finding::MissingNodeTypes]);
    }

    #[test]
    fn a_tsconfig_with_node_types_has_no_finding() {
        let text = r#"{"compilerOptions":{"types":["node"]}}"#;
        assert!(tsconfig_findings(text).is_empty());
    }

    #[test]
    fn a_forbidden_word_span_is_found_in_code_only() {
        assert_eq!(forbidden_word_spans("let x: any = 1;"), vec![7..10]);
        assert!(forbidden_word_spans("// any here").is_empty());
        assert!(forbidden_word_spans("const anyway = 1;").is_empty());
    }

    #[test]
    fn a_missing_tool_is_named_not_panicked() {
        let steps = vec![Step {
            program: "nana-no-such-tool-xyz",
            args: Vec::new(),
        }];
        assert_eq!(run_steps(&steps), "nana-no-such-tool-xyz is not installed");
    }
}

#[cfg(test)]
mod forbidden_tests {
    use super::*;

    #[test]
    fn any_and_var_are_reported_with_their_line() {
        let found = forbidden_word_lines("let x: any = 1;\nvar y = 2;\n");
        assert_eq!(found, vec![(1, "any"), (2, "var")]);
    }

    #[test]
    fn words_inside_comments_and_strings_are_ignored() {
        let found = forbidden_word_lines("// any var here\nconsole.log(\"var any\");\n");
        assert!(found.is_empty());
    }

    #[test]
    fn longer_identifiers_are_not_reported() {
        let found = forbidden_word_lines("const anyway = variable + vary;\n");
        assert!(found.is_empty());
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    #[test]
    fn only_typescript_files_are_in_scope() {
        assert!(applies_to(std::path::Path::new("ex00/hello.ts")));
        assert!(!applies_to(std::path::Path::new("main.rs")));
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn the_tsconfig_template_is_the_subject_one() {
        let json: serde_json::Value = serde_json::from_str(TSCONFIG_TEMPLATE).unwrap();
        let opts = &json["compilerOptions"];
        assert_eq!(opts["module"], "commonjs");
        assert_eq!(opts["strict"], true);
        assert_eq!(opts["types"][0], "node");
    }

    #[test]
    fn the_gitignore_template_keeps_node_modules_and_dist_out() {
        assert!(GITIGNORE_TEMPLATE
            .lines()
            .any(|l| l.trim() == "node_modules/"));
        assert!(GITIGNORE_TEMPLATE.lines().any(|l| l.trim() == "dist/"));
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;

    #[test]
    fn a_missing_template_is_written_and_named() {
        let dir = std::env::temp_dir().join("nana-bootcamp-write-unit");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let written = write_templates(&dir).unwrap();
        assert_eq!(written.len(), 2);
        assert!(dir.join("tsconfig.json").exists());
        assert!(dir.join(".gitignore").exists());
    }
}

#[cfg(test)]
mod describe_tests {
    use super::*;

    #[test]
    fn a_module_type_is_described_in_lowercase() {
        assert_eq!(
            describe(&Finding::ModuleType),
            "remove \"type\": \"module\" from package.json"
        );
    }

    #[test]
    fn a_missing_pin_is_described_with_the_wanted_version() {
        let f = Finding::Version {
            name: "ts-node",
            found: None,
            want: "10.9.2",
        };
        assert_eq!(describe(&f), "ts-node should be 10.9.2 (found none)");
    }
}
