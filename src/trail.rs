//! the agent's trail: what it did, placed in time, drawn behind its card.
//!
//! each action of the agent is a step on a lane (the tool it called, the answer
//! it got, what it wrote). time runs left to right, from the first step to the
//! latest one on the right edge, so the trail reads like the memory map. the
//! logic lives here and knows nothing about terminals: the tui paints the rows
//! this module places.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// a tool the agent called
    Tool,
    /// what the tool answered
    Result,
    /// a file the agent wrote or read
    File,
    /// something the agent said
    Say,
}

impl Lane {
    pub const ALL: [Lane; 4] = [Lane::Tool, Lane::Result, Lane::File, Lane::Say];

    pub fn id(self) -> &'static str {
        match self {
            Lane::Tool => "tool",
            Lane::Result => "result",
            Lane::File => "file",
            Lane::Say => "say",
        }
    }
}

/// one action of the agent, placed at `at` seconds since the request started.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub lane: Lane,
    pub label: String,
    pub at: f32,
    /// false for a refusal or an error: the glyph shows it
    pub ok: bool,
}

/// the glyph of a step: a dot for an action that went through, a cross for one
/// that was refused or failed.
pub fn glyph(ok: bool) -> char {
    if ok {
        '●'
    } else {
        '✕'
    }
}

/// the column of a step on `columns` time columns. the first step sits on the
/// left edge, the latest one on the right edge; a single step sits on the left.
pub fn column(at: f32, first: f32, last: f32, columns: usize) -> usize {
    let columns = columns.max(2);
    let span = (last - first).max(f32::EPSILON);
    let t = ((at - first) / span).clamp(0.0, 1.0);
    (t * (columns - 1) as f32).round() as usize
}

/// the trail as rows, one per lane: each row is `columns` glyphs, the empty
/// time is a dotted rule so the lane stays visible even when it is quiet.
pub fn rows(steps: &[Step], columns: usize) -> Vec<(Lane, Vec<char>)> {
    let columns = columns.max(2);
    let first = steps.iter().map(|s| s.at).fold(f32::INFINITY, f32::min);
    let last = steps.iter().map(|s| s.at).fold(f32::NEG_INFINITY, f32::max);
    Lane::ALL
        .into_iter()
        .map(|lane| {
            let mut row = vec!['┄'; columns];
            if steps.is_empty() {
                return (lane, row);
            }
            for s in steps.iter().filter(|s| s.lane == lane) {
                row[column(s.at, first, last, columns)] = glyph(s.ok);
            }
            (lane, row)
        })
        .collect()
}

/// the steps as a list, newest last, one line each: the time, the lane, the label.
pub fn lines(steps: &[Step]) -> Vec<String> {
    steps
        .iter()
        .map(|s| format!("{:>5.1}s {:<7} {}", s.at, s.lane.id(), s.label))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(lane: Lane, at: f32, ok: bool) -> Step {
        Step {
            lane,
            label: format!("{}@{at}", lane.id()),
            at,
            ok,
        }
    }

    #[test]
    fn the_first_step_is_on_the_left_and_the_last_on_the_right() {
        assert_eq!(column(0.0, 0.0, 10.0, 20), 0);
        assert_eq!(column(10.0, 0.0, 10.0, 20), 19);
        assert_eq!(
            column(5.0, 0.0, 10.0, 21),
            10,
            "the middle lands in the middle"
        );
    }

    #[test]
    fn a_single_step_sits_on_the_left_edge_without_dividing_by_zero() {
        assert_eq!(column(3.0, 3.0, 3.0, 20), 0);
    }

    #[test]
    fn a_refused_step_is_drawn_as_a_cross() {
        assert_eq!(glyph(true), '●');
        assert_eq!(glyph(false), '✕');
        let rows = rows(&[step(Lane::Result, 1.0, false)], 10);
        let result = rows.iter().find(|(l, _)| *l == Lane::Result).unwrap();
        assert!(
            result.1.contains(&'✕'),
            "the failure shows on its lane: {:?}",
            result.1
        );
    }

    #[test]
    fn every_lane_is_drawn_even_when_quiet() {
        let rows = rows(&[step(Lane::Tool, 0.0, true)], 12);
        assert_eq!(rows.len(), Lane::ALL.len());
        let say = rows.iter().find(|(l, _)| *l == Lane::Say).unwrap();
        assert!(
            say.1.iter().all(|c| *c == '┄'),
            "a quiet lane is a rule, not a gap"
        );
    }

    #[test]
    fn steps_keep_their_order_and_their_time_in_the_list() {
        let out = lines(&[step(Lane::Tool, 0.5, true), step(Lane::File, 2.0, true)]);
        assert_eq!(out.len(), 2);
        assert!(
            out[0].contains("tool") && out[0].contains("0.5s"),
            "{}",
            out[0]
        );
        assert!(
            out[1].contains("file") && out[1].contains("2.0s"),
            "{}",
            out[1]
        );
    }

    #[test]
    fn an_empty_trail_still_draws_its_lanes() {
        let rows = rows(&[], 8);
        assert_eq!(rows.len(), Lane::ALL.len());
        assert!(rows.iter().all(|(_, r)| r.iter().all(|c| *c == '┄')));
    }
}
