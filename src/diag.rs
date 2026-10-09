//! one diagnostic model for every toolchain.
//!
//! nana runs whatever checker fits the language (gcc, rustc, node --check,
//! python's ast, luac, bash -n, tsc…) and folds all of their dialects into a
//! single shape the gutter can draw. nothing here is language specific: the
//! parser recognises the *shapes* compilers use, and the shapes are stable.

use std::path::Path;

/// a compiler diagnostic, 1-based line and column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub line: usize,
    pub col: usize,
    /// true for errors, false for warnings, notes and hints
    pub is_error: bool,
    pub message: String,
}

fn severity_of(word: &str) -> Option<bool> {
    let w = word.trim().trim_end_matches(':');
    match w {
        "error" | "Error" | "ERROR" | "fatal" | "fatal error" | "panic" | "Panic" => Some(true),
        "warning" | "Warning" | "WARNING" | "warn" | "note" | "help" | "hint" | "info" => {
            Some(false)
        }
        // python and the jvm raise named exceptions: SyntaxError, ValueError,
        // TypeError… and warn with …Warning
        _ if w.ends_with("Error") || w.ends_with("Exception") => Some(true),
        _ if w.ends_with("Warning") => Some(false),
        _ => None,
    }
}

/// does this diagnostic belong to the file we care about?
fn belongs(path: &str, file: &Path) -> bool {
    if path.is_empty() {
        return true;
    }
    let want_name = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let want_full = file.display().to_string();
    let got = Path::new(path);
    let got_name = got.file_name().and_then(|n| n.to_str()).unwrap_or(path);
    if got_name == want_name {
        return true;
    }
    // relative vs absolute paths that end the same way
    want_full.ends_with(path) || path.ends_with(&want_full)
}

/// split `path:line:col:` style heads — but only when the line part is a number.
fn head_colon(line: &str) -> Option<(String, usize, usize, &str)> {
    // scan from the left for `:number:number:` or `:number:`
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let rest = &line[i + 1..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                let after = &rest[digits.len()..];
                if let Some(tail) = after.strip_prefix(':') {
                    if let Ok(n) = digits.parse::<usize>() {
                        // is it `line:col:`?
                        let col_digits: String =
                            tail.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if !col_digits.is_empty() {
                            let after_col = &tail[col_digits.len()..];
                            if after_col.is_empty() {
                                if let Ok(c) = col_digits.parse::<usize>() {
                                    return Some((line[..i].to_string(), n, c, ""));
                                }
                            }
                            if let Some(t2) = after_col.strip_prefix(':') {
                                if let Ok(c) = col_digits.parse::<usize>() {
                                    return Some((line[..i].to_string(), n, c, t2));
                                }
                            }
                        }
                        return Some((line[..i].to_string(), n, 0, tail));
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// parse any checker's output into diagnostics for `file`.
///
/// recognised: `path:line:col: severity: msg` (gcc, clang, go, swift, javac),
/// `path:line: severity: msg`, `error[E0308]: msg` + `--> path:line:col`
/// (rustc), `path(line,col): error TS1234: msg` (tsc), `File "x", line N` +
/// `SyntaxError: msg` (python), `x.js:12` + `SyntaxError:` (node),
/// `luac: x.lua:12: msg`, `x.sh: line 12: msg`, `syntax error at x.pl line 12`,
/// `x.rb:12: msg`, `... in x.php on line 12`.
pub fn parse(text: &str, file: &Path) -> Vec<Diag> {
    let mut out: Vec<Diag> = Vec::new();
    // a "File …, line N" head that the *next* severity line belongs to
    let mut pending: Option<(usize, usize)> = None;
    // a rustc-style severity line waiting for its `--> path:line:col`
    let mut pending_rustc: Option<(bool, String)> = None;

    for raw in text.lines() {
        let line = raw.trim_end();
        let t = line.trim_start();
        if t.is_empty() {
            continue;
        }

        // ---------------------------------------------------------- rustc
        if let Some(rest) = t.strip_prefix("--> ") {
            if let Some((p, l, c, _)) = head_colon(rest.trim()) {
                if let Some((is_error, msg)) = pending_rustc.take() {
                    if belongs(&p, file) {
                        out.push(Diag {
                            line: l,
                            col: c,
                            is_error,
                            message: msg,
                        });
                    }
                }
            }
            continue;
        }
        if pending.is_none() {
            if let Some((is_error, msg)) = rustc_head(t) {
                pending_rustc = Some((is_error, msg));
                continue;
            }
        }

        // ------------------------------------------- python's `File "…", line N`
        if let Some((name, n)) = python_file_head(t) {
            pending = Some((n, 0));
            let _ = name;
            continue;
        }

        // ------------------------------------------------------- tsc: x.ts(3,5)
        if let Some((p, l, c, tail)) = paren_head(t) {
            let sev = tail.split_whitespace().next().and_then(severity_of);
            if let Some(is_error) = sev {
                if belongs(&p, file) {
                    out.push(Diag {
                        line: l,
                        col: c,
                        is_error,
                        message: strip_code(tail),
                    });
                }
                pending = None;
                continue;
            }
        }

        // ------------------------------------------------------- php / perl tails
        if let Some((n, msg)) = php_tail(t) {
            out.push(Diag {
                line: n,
                col: 0,
                is_error: true,
                message: msg,
            });
            pending = None;
            continue;
        }
        if let Some((n, msg)) = perl_tail(t) {
            out.push(Diag {
                line: n,
                col: 0,
                is_error: true,
                message: msg,
            });
            pending = None;
            continue;
        }
        // ------------------------------------------------- bash: `x.sh: line 12: …`
        if let Some((p, n, msg)) = bash_line_head(t) {
            if belongs(&p, file) {
                out.push(Diag {
                    line: n,
                    col: 0,
                    is_error: true,
                    message: msg,
                });
            }
            pending = None;
            continue;
        }

        // ------------------------------------------------------- generic head
        if let Some((p, l, c, tail)) = head_colon(t) {
            let tail_t = tail.trim_start();
            let word = tail_t.split(':').next().unwrap_or("").trim();
            let sev = severity_of(word);
            let message = if sev.is_some() {
                tail_t[word.len()..]
                    .trim_start_matches(':')
                    .trim()
                    .to_string()
            } else {
                tail_t.trim().to_string()
            };
            // tool prefixes such as `luac: x.lua:12: msg` put a word before the path
            let (path, cleaned) = split_tool_prefix(&p);
            let _ = cleaned;
            if belongs(&path, file) {
                out.push(Diag {
                    line: l,
                    col: c,
                    is_error: sev.unwrap_or(true),
                    message,
                });
                pending = None;
                continue;
            }
        }

        // ------------------------------------------- bare severity line
        let word = t.split(':').next().unwrap_or("").trim();
        if let Some(is_error) = severity_of(word) {
            let msg = t[word.len()..].trim_start_matches(':').trim().to_string();
            // node prints `SyntaxError: …` a couple of lines after `x.js:12`
            if let Some((l, c)) = pending.take() {
                out.push(Diag {
                    line: l,
                    col: c,
                    is_error,
                    message: msg,
                });
                continue;
            }
            if let Some(last) = out.last() {
                // a bare severity with no anchor: attach to the last known line
                out.push(Diag {
                    line: last.line,
                    col: 0,
                    is_error,
                    message: msg,
                });
            }
            continue;
        }

        // ------------------------------------------- node's `x.js:12` anchor
        if let Some((p, l, c, tail)) = head_colon(t) {
            if tail.trim().is_empty() && belongs(&p, file) {
                pending = Some((l, c));
            }
        } else if let Some((p, l, c)) = trailing_anchor(t) {
            if belongs(&p, file) {
                pending = Some((l, c));
            }
        }
    }

    // a rustc severity that never got its `-->`: keep it on line 1 rather than
    // losing it — a visible diagnostic beats a silent one
    if let Some((is_error, message)) = pending_rustc {
        out.push(Diag {
            line: 1,
            col: 0,
            is_error,
            message,
        });
    }
    dedup(out)
}

fn dedup(mut v: Vec<Diag>) -> Vec<Diag> {
    v.sort_by(|a, b| (a.line, a.col, &a.message).cmp(&(b.line, b.col, &b.message)));
    v.dedup();
    v
}

/// `error[E0308]: mismatched types` / `warning: unused` / `error: …`
fn rustc_head(t: &str) -> Option<(bool, String)> {
    let (word, rest) = match t.split_once('[') {
        Some((w, r)) => {
            let code = r.split_once(']').map(|(c, _)| c).unwrap_or("");
            (
                w.trim(),
                format!(
                    "[{code}] {}",
                    r.split_once(']').map(|(_, x)| x).unwrap_or("").trim()
                ),
            )
        }
        None => match t.split_once(':') {
            Some((w, r)) => (w.trim(), r.trim().to_string()),
            None => return None,
        },
    };
    if t.starts_with("-->") {
        return None;
    }
    severity_of(word).map(|is_error| (is_error, rest))
}

/// `  File "x.py", line 12` / `File "x.py", line 12, in <module>`
fn python_file_head(t: &str) -> Option<(String, usize)> {
    let rest = t.strip_prefix("File ")?;
    let q = rest.strip_prefix('"')?;
    let end = q.find('"')?;
    let name = q[..end].to_string();
    let after = &q[end..];
    let idx = after.find("line ")?;
    let digits: String = after[idx + 5..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok().map(|n| (name, n))
}

/// `x.ts(3,5): error TS1234: msg`
fn paren_head(t: &str) -> Option<(String, usize, usize, &str)> {
    let open = t.find('(')?;
    let close = t[open..].find(')')? + open;
    let inside = &t[open + 1..close];
    let (l, c) = inside.split_once(',')?;
    let l: usize = l.trim().parse().ok()?;
    let c: usize = c.trim().parse().ok()?;
    let tail = t[close + 1..].strip_prefix(':')?.trim();
    Some((t[..open].to_string(), l, c, tail))
}

/// `PHP Parse error:  syntax error … in /path/x.php on line 12`
fn php_tail(t: &str) -> Option<(usize, String)> {
    if !t.contains("PHP ") {
        return None;
    }
    let idx = t.find(" in ")?;
    let rest = &t[idx + 4..];
    let on = rest.find(" on line ")?;
    let n: usize = rest[on + 9..].trim().parse().ok()?;
    let msg = t.split_once(": ").map(|(_, m)| m).unwrap_or(t);
    let msg = msg.split(" in ").next().unwrap_or(msg).trim().to_string();
    Some((n, msg))
}

/// `syntax error at x.pl line 12, near "…"`
fn perl_tail(t: &str) -> Option<(usize, String)> {
    let idx = t.find(" line ")?;
    if !t.starts_with("syntax error") && !t.contains("error at ") {
        return None;
    }
    let digits: String = t[idx + 6..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let n: usize = digits.parse().ok()?;
    Some((n, t.to_string()))
}

/// `x.sh: line 12: syntax error near …`
fn bash_line_head(t: &str) -> Option<(String, usize, String)> {
    let (p, rest) = t.split_once(": line ")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: usize = digits.parse().ok()?;
    let msg = rest[digits.len()..]
        .trim_start_matches(':')
        .trim()
        .to_string();
    Some((p.to_string(), n, msg))
}

/// `x.js:12` alone on a line (node --check)
fn trailing_anchor(t: &str) -> Option<(String, usize, usize)> {
    let (p, rest) = t.rsplit_once(':')?;
    let n: usize = rest.trim().parse().ok()?;
    Some((p.to_string(), n, 0))
}

/// strip a leading tool name from a path head: `luac: x.lua` → `x.lua`
fn split_tool_prefix(p: &str) -> (String, bool) {
    match p.rsplit_once(": ") {
        Some((_, path)) if !path.contains(':') => (path.trim().to_string(), true),
        _ => (p.trim().to_string(), false),
    }
}

/// drop a leading code such as `error TS2345:` or `error[E0308]:`
fn strip_code(tail: &str) -> String {
    let t = tail.trim();
    for sev in ["error", "warning", "note"] {
        if let Some(rest) = t.strip_prefix(sev) {
            let rest = rest.trim_start();
            if let Some(idx) = rest.find(':') {
                let head = rest[..idx].trim();
                let is_code = head.starts_with("TS")
                    || (head.starts_with('E')
                        && head.len() > 1
                        && head[1..].chars().all(|c| c.is_ascii_digit()))
                    || head.starts_with('[');
                if is_code {
                    return rest[idx + 1..].trim().to_string();
                }
            }
        }
    }
    t.to_string()
}

/// how many errors and warnings.
pub fn counts(diags: &[Diag]) -> (usize, usize) {
    let errs = diags.iter().filter(|d| d.is_error).count();
    (errs, diags.len() - errs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn f(p: &str) -> PathBuf {
        PathBuf::from(p)
    }

    #[test]
    fn gcc_style() {
        let out = "\
main.c:12:5: error: expected ';' before 'return'
main.c:20:1: warning: unused variable 'i' [-Wunused-variable]
main.c:7:2: note: previous definition is here
";
        let d = parse(out, &f("main.c"));
        assert_eq!(d.len(), 3, "{d:?}");
        let err = d.iter().find(|x| x.line == 12).expect("line 12");
        assert_eq!(err.col, 5);
        assert!(err.is_error);
        assert!(err.message.contains("expected"), "{}", err.message);
        let warn = d.iter().find(|x| x.line == 20).expect("line 20");
        assert!(!warn.is_error);
    }

    #[test]
    fn other_files_are_filtered_out() {
        let out = "other.c:1:1: error: boom\nmain.c:2:1: error: real\n";
        let d = parse(out, &f("main.c"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].message, "real");
    }

    #[test]
    fn rustc_style() {
        let out = "\
error[E0308]: mismatched types
  --> src/main.rs:4:9
   |
 4 |     let x: u32 = \"a\";
   |                  ^^^ expected u32

warning: unused variable: `y`
  --> src/main.rs:7:9
";
        let d = parse(out, &f("src/main.rs"));
        assert_eq!(d.len(), 2);
        assert!(d[0].is_error, "the E0308 is an error");
        assert_eq!(d[0].line, 4);
        assert_eq!(d[0].col, 9);
        assert!(d[0].message.contains("E0308"));
        assert_eq!(d[1].line, 7);
        assert!(!d[1].is_error);
    }

    #[test]
    fn python_syntax_error() {
        let out = "\
  File \"/home/me/x.py\", line 3
    def f(:
          ^
SyntaxError: invalid syntax
";
        let d = parse(out, &f("/home/me/x.py"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 3);
        assert!(d[0].is_error);
        assert!(d[0].message.contains("invalid syntax"));
    }

    #[test]
    fn python_traceback() {
        let out = "\
Traceback (most recent call last):
  File \"/home/me/x.py\", line 9, in <module>
    boom()
ValueError: nope
";
        let d = parse(out, &f("/home/me/x.py"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 9);
        assert!(d[0].message.contains("nope"));
    }

    #[test]
    fn node_check() {
        let out = "\
/home/me/x.js:3
const = ;
      ^

SyntaxError: Unexpected token '='
";
        let d = parse(out, &f("/home/me/x.js"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 3);
        assert!(d[0].message.contains("Unexpected token"));
    }

    #[test]
    fn typescript() {
        let out =
            "src/app.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.\n";
        let d = parse(out, &f("src/app.ts"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 12);
        assert_eq!(d[0].col, 5);
        assert!(
            d[0].message.contains("not assignable"),
            "code stripped: {}",
            d[0].message
        );
    }

    #[test]
    fn luac() {
        let out = "luac: x.lua:3: '=' expected near 'x'\n";
        let d = parse(out, &f("x.lua"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 3);
        assert!(d[0].message.contains("expected near"));
    }

    #[test]
    fn bash_syntax() {
        let out = "x.sh: line 4: syntax error near unexpected token `fi'\n";
        let d = parse(out, &f("x.sh"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 4);
    }

    #[test]
    fn php_parse_error() {
        let out = "PHP Parse error:  syntax error, unexpected ';' in /tmp/x.php on line 7\n";
        let d = parse(out, &f("/tmp/x.php"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 7);
    }

    #[test]
    fn perl_syntax() {
        let out = "syntax error at x.pl line 11, near \"my $x\"\n";
        let d = parse(out, &f("x.pl"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].line, 11);
    }

    #[test]
    fn java_and_swift_and_go_share_the_gcc_shape() {
        for (out, file) in [
            ("Main.java:5: error: ';' expected\n", "Main.java"),
            (
                "x.swift:2:7: error: cannot find 'foo' in scope\n",
                "x.swift",
            ),
            ("x.go:9:2: undefined: bar\n", "x.go"),
        ] {
            let d = parse(out, &f(file));
            assert_eq!(d.len(), 1, "{out}");
            assert!(d[0].line > 1 || file.starts_with("x.swift"));
        }
    }

    #[test]
    fn empty_output_is_empty() {
        assert!(parse("", &f("x.py")).is_empty());
        assert!(parse("   \n\n", &f("x.py")).is_empty());
    }

    #[test]
    fn counts_split_errors_and_warnings() {
        let d = parse(
            "a.c:1:1: error: x\na.c:2:1: warning: y\na.c:3:1: note: z\n",
            &f("a.c"),
        );
        assert_eq!(counts(&d), (1, 2));
    }
}
