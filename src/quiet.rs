//! When the user stops typing, the editor takes a quiet moment to look at the
//! project memory. The moment is not a fixed pause: it follows the user's own
//! rhythm, so a fast typist and a slow one both get a pause that is theirs.

use std::time::{Duration, Instant};

/// the quiet is this many average gaps between keys
const GAP_FACTOR: f64 = 4.0;
/// a floor, so that a burst of fast keys does not fire between two letters
const MIN_QUIET: Duration = Duration::from_millis(800);
/// a ceiling, so that a slow typist still gets a look after a short break
const MAX_QUIET: Duration = Duration::from_secs(10);

#[derive(Debug, Default)]
pub struct Quiet {
    last_key: Option<Instant>,
    avg_gap_ms: Option<f64>,
    fired: bool,
}

impl Quiet {
    /// a key was pressed at `now`: the gap since the previous key feeds the average.
    pub fn key(&mut self, now: Instant) {
        if let Some(prev) = self.last_key {
            let gap = now.saturating_duration_since(prev).as_secs_f64() * 1000.0;
            self.avg_gap_ms = Some(match self.avg_gap_ms {
                None => gap,
                Some(avg) => avg * 0.8 + gap * 0.2,
            });
        }
        self.last_key = Some(now);
        self.fired = false;
    }

    /// the silence the user has to leave before a look is due.
    pub fn threshold(&self) -> Duration {
        match self.avg_gap_ms {
            None => MAX_QUIET,
            Some(gap) => {
                Duration::from_secs_f64(gap * GAP_FACTOR / 1000.0).clamp(MIN_QUIET, MAX_QUIET)
            }
        }
    }

    /// true once per silence: the first time `now` is past the threshold.
    pub fn due(&mut self, now: Instant) -> bool {
        let Some(last) = self.last_key else {
            return false;
        };
        if self.fired || now.saturating_duration_since(last) < self.threshold() {
            return false;
        }
        self.fired = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fast_typist_gets_a_short_quiet_and_a_slow_one_a_longer_one() {
        let t0 = Instant::now();
        let mut fast = Quiet::default();
        let mut slow = Quiet::default();
        for i in 0..10 {
            fast.key(t0 + Duration::from_millis(100 * i));
            slow.key(t0 + Duration::from_millis(1500 * i));
        }
        assert!(
            fast.threshold() < slow.threshold(),
            "{:?} vs {:?}",
            fast.threshold(),
            slow.threshold()
        );
    }

    #[test]
    fn the_quiet_is_bounded_so_neither_extreme_stalls_or_fires_at_once() {
        let t0 = Instant::now();
        let mut fast = Quiet::default();
        for i in 0..10 {
            fast.key(t0 + Duration::from_millis(5 * i));
        }
        assert_eq!(fast.threshold(), MIN_QUIET);
        let mut slow = Quiet::default();
        slow.key(t0);
        slow.key(t0 + Duration::from_secs(60));
        assert_eq!(slow.threshold(), MAX_QUIET);
    }

    #[test]
    fn a_look_is_due_once_after_the_silence_and_again_only_after_a_new_key() {
        let t0 = Instant::now();
        let mut q = Quiet::default();
        q.key(t0);
        assert!(
            !q.due(t0 + Duration::from_millis(100)),
            "not before the quiet"
        );
        let later = t0 + q.threshold() + Duration::from_millis(1);
        assert!(q.due(later), "due after the quiet");
        assert!(
            !q.due(later + Duration::from_secs(1)),
            "only once per silence"
        );
        q.key(later + Duration::from_secs(2));
        assert!(
            !q.due(later + Duration::from_secs(3)),
            "a key starts a new silence"
        );
        assert!(q.due(later + Duration::from_secs(3) + q.threshold()));
    }

    #[test]
    fn nothing_is_due_before_any_key() {
        let mut q = Quiet::default();
        assert!(!q.due(Instant::now() + Duration::from_secs(60)));
    }
}
