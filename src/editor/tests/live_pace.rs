//! the pace of the agent's playback: a step follows the clock, not the loop.

use super::*;

/// the loop wakes on every key and mouse move, not only every tick. a wake-up
/// inside a tick must not play another letter, or the write jumps ahead.
#[test]
fn a_step_waits_for_a_tick_however_often_the_loop_wakes() {
    let mut ed = Editor::open(None).unwrap();
    let t0 = Instant::now();
    assert!(ed.live_step_due(t0), "the first step is due at once");
    assert!(
        !ed.live_step_due(t0 + TICK / 2),
        "a wake-up inside a tick does not step"
    );
    assert!(
        ed.live_step_due(t0 + TICK),
        "one tick later the next step is due"
    );
}
