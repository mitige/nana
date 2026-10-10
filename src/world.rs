//! the world map: memory as a place you can look at.
//!
//! each memory class is a lane (the space: who, how we work, the project, the
//! outside), and time runs left to right, up to today on the right edge. a page
//! sits where it was last written, its glyph says how sure its writer was, and
//! a page gone stale fades. the logic lives here and knows nothing about
//! terminals: the tui paints the rows this module places.

use crate::memory::{days_from_iso, iso_from_days, Class, Confidence, Memory};
use std::path::Path;

const COLUMNS: usize = 44;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub class: Class,
    pub name: String,
    /// days since the unix epoch, None for an undated page
    pub day: Option<i64>,
    pub confidence: Option<Confidence>,
    pub stale: bool,
    pub checked: bool,
}

/// a page placed on the map: which column of its lane, and which node it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub col: usize,
    pub node: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lane {
    pub class: Class,
    pub marks: Vec<Mark>,
}

/// the glyph of a page: its confidence, drawn as fill. an unrated page is a ring.
pub fn glyph(confidence: Option<Confidence>) -> char {
    match confidence {
        Some(Confidence::High) => '◉',
        Some(Confidence::Medium) => '●',
        Some(Confidence::Low) => '◌',
        None => '○',
    }
}

pub fn nodes(root: &Path) -> Vec<Node> {
    let mut out: Vec<Node> = Memory::open(root)
        .list()
        .into_iter()
        .map(|e| Node {
            class: e.class,
            day: e.updated.as_deref().and_then(days_from_iso),
            stale: e.stale(),
            checked: e.check.is_some(),
            confidence: e.confidence,
            name: e.name,
        })
        .collect();
    out.sort_by_key(|n| (n.class.id(), n.name.clone()));
    out
}

/// lays the nodes out on `columns` time columns. the last column is today, the
/// first is the oldest page; an undated page sits in the first column, which is
/// where the map keeps what it cannot place in time.
pub fn place(nodes: &[Node], columns: usize) -> Vec<Lane> {
    let columns = columns.max(2);
    let today = crate::memory::today_days();
    let first = nodes.iter().filter_map(|n| n.day).min().unwrap_or(today);
    let span = (today - first).max(1);
    let col_of = |day: i64| -> usize {
        let t = (day - first).clamp(0, span) as usize;
        t * (columns - 1) / span as usize
    };
    Class::ALL
        .into_iter()
        .map(|class| Lane {
            class,
            marks: nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.class == class)
                .map(|(i, n)| Mark {
                    col: n.day.map(col_of).unwrap_or(0),
                    node: i,
                })
                .collect(),
        })
        .collect()
}

pub fn iso(day: i64) -> String {
    iso_from_days(day)
}

/// the sweep's easing: it leaves fast and settles on today, like a hand
/// that finds its place. `t` is the progress from 0 (start) to 1 (done).
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// the map at a moment of its sweep: the playhead has crossed the columns up
/// to `progress`, and only the pages it has passed are drawn. the lanes stay
/// visible, so the map grows out of its own skeleton instead of popping in.
pub fn render_sweep(root: &Path, selected: Option<&crate::hub::Action>, progress: f32) -> String {
    if progress >= 1.0 {
        return render(root, selected);
    }
    let all = nodes(root);
    if all.is_empty() {
        return render(root, selected);
    }
    let lanes = place(&all, COLUMNS);
    let head = (ease_out(progress) * (COLUMNS - 1) as f32).round() as usize;
    let mut out = String::new();
    out.push_str(&format!("{:<11}{}\n", "", axis()));
    for lane in &lanes {
        let mut row = vec!['┄'; COLUMNS];
        for m in lane.marks.iter().filter(|m| m.col <= head) {
            let n = &all[m.node];
            row[m.col] = if n.stale { '◌' } else { glyph(n.confidence) };
        }
        if head < COLUMNS {
            row[head] = '│';
        }
        out.push_str(&format!(
            "{:<11}{}\n",
            lane.class.id(),
            row.iter().collect::<String>()
        ));
    }
    out
}

/// the map as text, for the detail pane: one row per lane, the axis on top, and
/// the page you selected drawn as a diamond so you can find it on the map.
pub fn render(root: &Path, selected: Option<&crate::hub::Action>) -> String {
    let all = nodes(root);
    if all.is_empty() {
        return "(no memory yet: every page you write lands on this map)".into();
    }
    let lanes = place(&all, COLUMNS);
    let picked = selected.and_then(|a| match a {
        crate::hub::Action::ReadMemory(c, n) => {
            all.iter().position(|x| x.class == *c && x.name == *n)
        }
        _ => None,
    });
    let mut out = String::new();
    out.push_str(&format!("{:<11}{}\n", "", axis()));
    for lane in &lanes {
        let mut row = vec!['┄'; COLUMNS];
        for m in &lane.marks {
            let n = &all[m.node];
            row[m.col] = if Some(m.node) == picked {
                '◆'
            } else if n.stale {
                '◌'
            } else {
                glyph(n.confidence)
            };
        }
        out.push_str(&format!(
            "{:<11}{}\n",
            lane.class.id(),
            row.iter().collect::<String>()
        ));
    }
    out.push('\n');
    if let Some(i) = picked {
        let n = &all[i];
        let when = n.day.map(iso).unwrap_or_else(|| "undated".into());
        let conf = n.confidence.map(|c| c.id()).unwrap_or("unrated");
        out.push_str(&format!("{} · {when} · {conf}\n", n.name));
        if n.stale {
            out.push_str("stale: older than 90 days, check it before you trust it\n");
        }
        if n.checked {
            out.push_str("holds a check: the agent runs it before trusting the page\n");
        }
    } else {
        out.push_str("legend  ◉ high  ● medium  ◌ low or stale  ○ unrated  ◆ selected\n");
    }
    out
}

fn axis() -> String {
    let left = "oldest";
    let right = "today";
    let gap = COLUMNS.saturating_sub(left.len() + right.len());
    format!("{left}{}{right}", "─".repeat(gap))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(class: Class, name: &str, day: Option<i64>) -> Node {
        Node {
            class,
            name: name.into(),
            day,
            confidence: Some(Confidence::Medium),
            stale: false,
            checked: false,
        }
    }

    #[test]
    fn every_class_is_a_lane_even_when_empty() {
        let lanes = place(&[], 20);
        let classes: Vec<Class> = lanes.iter().map(|l| l.class).collect();
        assert_eq!(classes, Class::ALL.to_vec());
        assert!(lanes.iter().all(|l| l.marks.is_empty()));
    }

    #[test]
    fn a_page_sits_in_the_lane_of_its_class() {
        let today = crate::memory::today_days();
        let nodes = vec![node(Class::User, "tone", Some(today))];
        let lanes = place(&nodes, 30);
        let user = lanes.iter().find(|l| l.class == Class::User).unwrap();
        let project = lanes.iter().find(|l| l.class == Class::Project).unwrap();
        assert_eq!(user.marks.len(), 1);
        assert!(project.marks.is_empty());
    }

    #[test]
    fn time_runs_left_to_right_up_to_today_on_the_right_edge() {
        let today = crate::memory::today_days();
        let nodes = vec![
            node(Class::Project, "old", Some(today - 100)),
            node(Class::Project, "new", Some(today)),
        ];
        let cols = 40;
        let lanes = place(&nodes, cols);
        let project = lanes.iter().find(|l| l.class == Class::Project).unwrap();
        let old = project
            .marks
            .iter()
            .find(|m| nodes[m.node].name == "old")
            .unwrap();
        let new = project
            .marks
            .iter()
            .find(|m| nodes[m.node].name == "new")
            .unwrap();
        assert_eq!(old.col, 0, "the oldest page starts the axis");
        assert_eq!(new.col, cols - 1, "today is the right edge");
    }

    #[test]
    fn an_undated_page_sits_at_the_start_not_nowhere() {
        let today = crate::memory::today_days();
        let nodes = vec![
            node(Class::Reference, "undated", None),
            node(Class::Reference, "dated", Some(today - 10)),
        ];
        let lanes = place(&nodes, 25);
        let lane = lanes.iter().find(|l| l.class == Class::Reference).unwrap();
        let undated = lane
            .marks
            .iter()
            .find(|m| nodes[m.node].name == "undated")
            .unwrap();
        assert_eq!(undated.col, 0);
    }

    #[test]
    fn confidence_is_drawn_as_fill() {
        assert_eq!(glyph(Some(Confidence::High)), '◉');
        assert_eq!(glyph(Some(Confidence::Medium)), '●');
        assert_eq!(glyph(Some(Confidence::Low)), '◌');
        assert_eq!(glyph(None), '○');
    }

    #[test]
    fn the_selected_page_is_drawn_as_a_diamond_with_its_facts() {
        let d = std::env::temp_dir().join(format!("nana-world-sel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        let m = Memory::open(&d);
        m.write_rated(Class::User, "tone", "blunt", Confidence::High)
            .unwrap();
        m.write_rated(Class::Project, "stack", "rust", Confidence::Low)
            .unwrap();
        let picked = crate::hub::Action::ReadMemory(Class::User, "tone".into());
        let text = render(&d, Some(&picked));
        let user_row = text.lines().find(|l| l.starts_with("user")).unwrap();
        assert!(user_row.contains('◆'), "{user_row}");
        assert!(
            !text.contains("legend"),
            "the legend yields to the selection"
        );
        assert!(text.contains("tone ·") && text.contains("high"), "{text}");
        let idle = render(&d, None);
        assert!(idle.contains("legend"), "{idle}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_sweep_eases_from_nothing_to_the_whole_map() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(-3.0), 0.0, "time before the start is clamped");
        let mut last = 0.0;
        for i in 1..=20 {
            let v = ease_out(i as f32 / 20.0);
            assert!(v >= last, "the sweep never runs backwards");
            last = v;
        }
        assert!(ease_out(0.5) > 0.5, "it starts fast and settles on today");
    }

    #[test]
    fn the_playhead_reveals_pages_as_it_passes_them() {
        let d = std::env::temp_dir().join(format!("nana-world-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        let m = Memory::open(&d);
        m.write_rated(Class::Project, "stack", "rust", Confidence::High)
            .unwrap();
        // the page is dated today, so it sits on the right edge of the map
        let start = render_sweep(&d, None, 0.0);
        assert!(
            !start.contains('◉'),
            "nothing is revealed before the sweep moves: {start}"
        );
        let end = render_sweep(&d, None, 1.0);
        assert!(
            end.contains('◉'),
            "the whole map is drawn at the end: {end}"
        );
        assert_eq!(
            end,
            render(&d, None),
            "the finished sweep is the static map"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_map_reads_the_project_memory_with_its_dates_and_checks() {
        let d = std::env::temp_dir().join(format!("nana-world-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        let m = Memory::open(&d);
        m.write_rated(Class::Project, "stack", "rust", Confidence::High)
            .unwrap();
        let found = nodes(&d);
        let stack = found.iter().find(|n| n.name == "stack").unwrap();
        assert_eq!(stack.class, Class::Project);
        assert_eq!(stack.confidence, Some(Confidence::High));
        assert!(stack.day.is_some(), "a new page is dated");
        assert!(!stack.stale);
        let _ = std::fs::remove_dir_all(&d);
    }
}
