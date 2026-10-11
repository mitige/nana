use super::*;

/// today, as a page name: 2026-10-10.
pub(super) fn today() -> String {
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

/// the page's place among the others: what it links to, what links to it,
/// and how many times it was written, with the day of each version.
pub(super) fn page_web(memory: &Memory, class: Class, name: &str) -> String {
    let mut out = String::new();
    let links = memory.links(class, name).unwrap_or_default();
    let back = memory.backlinks(name);
    if !links.is_empty() {
        out.push_str(&format!("\n\nlinks to: {}", links.join(", ")));
    }
    if !back.is_empty() {
        out.push_str(&format!("\n\nlinked from: {}", back.join(", ")));
    }
    let history = memory.history(class, name);
    if !history.is_empty() {
        out.push_str(&format!("\n\nhistory: {} version(s)", history.len()));
        for (day, _) in history.iter().rev() {
            out.push_str(&format!("\n  {day}"));
        }
    }
    out
}
