/// how long the map takes to sweep from the oldest page to today.
pub const SWEEP: std::time::Duration = std::time::Duration::from_millis(900);

/// the sweep's progress at `now`, from 0 to 1, once `started` was the start.
pub fn sweep_at(started: std::time::Instant, now: std::time::Instant) -> f32 {
    let t = now.saturating_duration_since(started).as_secs_f32() / SWEEP.as_secs_f32();
    t.clamp(0.0, 1.0)
}
