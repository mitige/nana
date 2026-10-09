//! the auto header (f4), written in the language's own comment syntax.
//!
//! sober by design: the file name, a one-line description, the date. no
//! school branding, no banner art — and for languages whose comment syntax
//! nana knows, the header is valid code the moment it is inserted.

use crate::langs;
use std::path::Path;

/// the year, for the header.
pub fn current_year() -> i32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (1970 + secs / 31_557_600) as i32
}

/// `2026-10-09`
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (mut y, mut d) = (1970i64, days as i64);
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if d < len {
            break;
        }
        d -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    while m < 12 && d >= months[m] {
        d -= months[m];
        m += 1;
    }
    format!("{y}-{:02}-{:02}", m + 1, d + 1)
}

/// the header block for a path, as lines to insert at the top.
/// an empty description becomes a `todo` line rather than an empty comment.
pub fn header_lines(path: &Path, description: &str, author: Option<&str>) -> Vec<String> {
    let lang = langs::for_path(path);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled");
    let desc = if description.trim().is_empty() {
        "todo: describe this file"
    } else {
        description.trim()
    };
    let stamp = today();
    let mut body = vec![format!("file: {name}"), format!("description: {desc}")];
    if let Some(a) = author.map(str::trim).filter(|a| !a.is_empty()) {
        body.push(format!("author: {a}"));
    }
    body.push(format!("created: {stamp}"));

    if let Some((open, close)) = lang.block {
        let mut out = vec![open.to_string()];
        let wear = !open.starts_with("<!--")
            && !open.starts_with("{-")
            && !open.starts_with("(*")
            && !open.starts_with("--[[")
            && !open.starts_with("%{")
            && !open.starts_with("(;");
        for l in body {
            // `/*`-style blocks get the classic ` * ` gutter, others stay plain
            out.push(if wear {
                format!(" * {l}")
            } else {
                format!(" {l}")
            });
        }
        out.push(close.to_string());
        out.push(String::new());
        return out;
    }

    let lc = lang.line_comment;
    let mut out: Vec<String> = body.iter().map(|l| format!("{lc} {l}")).collect();
    out.push(String::new());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn h(p: &str, d: &str) -> Vec<String> {
        header_lines(&PathBuf::from(p), d, None)
    }

    #[test]
    fn c_gets_a_block_comment() {
        let out = h("main.c", "entry point");
        assert_eq!(out[0], "/*");
        assert!(out.iter().any(|l| l.contains("file: main.c")));
        assert!(out.iter().any(|l| l.contains("description: entry point")));
        assert_eq!(out[out.len() - 2], "*/");
        assert!(out.last().unwrap().is_empty());
    }

    #[test]
    fn python_gets_hash_comments() {
        let out = h("x.py", "a script");
        assert!(out[0].starts_with("# file: x.py"), "{out:?}");
        assert!(out.iter().all(|l| l.is_empty() || l.starts_with("# ")));
    }

    #[test]
    fn html_gets_an_html_comment() {
        let out = h("index.html", "the page");
        assert_eq!(out[0], "<!--");
        assert!(out.iter().any(|l| l.contains("file: index.html")));
        assert!(out.contains(&"-->".to_string()));
    }

    #[test]
    fn an_author_is_added_only_when_configured() {
        let without = header_lines(&PathBuf::from("x.c"), "d", None);
        assert!(
            !without.iter().any(|l| l.contains("author:")),
            "{without:?}"
        );
        let with = header_lines(&PathBuf::from("x.c"), "d", Some("me"));
        assert!(with.iter().any(|l| l.contains("author: me")), "{with:?}");
        let blank = header_lines(&PathBuf::from("x.c"), "d", Some("   "));
        assert!(!blank.iter().any(|l| l.contains("author:")), "{blank:?}");
    }

    #[test]
    fn an_empty_description_still_says_something() {
        let out = h("x.rs", "");
        assert!(out.iter().any(|l| l.contains("todo")), "{out:?}");
    }

    #[test]
    fn every_language_header_is_at_least_commented() {
        for l in langs::all() {
            let ext = l.exts.first().copied().unwrap_or("txt");
            let path = PathBuf::from(format!("sample.{ext}"));
            // shared extensions (.m, .pl) resolve to the first language listed,
            // so assert against the language the path actually maps to
            let real = langs::for_path(&path);
            let out = header_lines(&path, "desc", None);
            assert!(!out.is_empty(), "{} produced nothing", l.id);
            let first = &out[0];
            let commented = matches!(real.block, Some((open, _)) if first == open)
                || first.starts_with(real.line_comment);
            assert!(
                commented,
                "{} ({}) produced an uncommented header: {first}",
                l.id, real.id
            );
        }
    }

    #[test]
    fn the_date_is_a_plain_iso_day() {
        let d = today();
        assert_eq!(d.len(), 10, "{d}");
        assert!(d.starts_with("20"), "{d}");
        assert_eq!(d.matches('-').count(), 2, "{d}");
    }
}
