//! the auto retry: a model call that fails for a passing reason (the network,
//! a busy provider, an empty reply) is asked again after a wait that grows, so
//! a long run is not lost to one bad second. a lasting failure still ends the
//! run, with an error the user can read.

use std::sync::atomic::Ordering;
use std::time::Duration;

use super::blank_turn::{is_blank, BLANK_REASON};
use super::{Agent, Event};
use crate::provider::{Reply, ToolSpec};

/// how many times a failed call is asked again before the run gives up.
pub(crate) const RETRIES: usize = 5;

/// the waits before each retry: 2, 4, 8, 16 then 32 seconds. a provider that
/// is rate limiting needs more room each time, not the same knock again.
pub(crate) fn backoff() -> Vec<Duration> {
    (0..RETRIES as u32)
        .map(|i| Duration::from_secs(2u64 << i))
        .collect()
}

/// true when an error from the provider may pass on its own: the network, an
/// unreadable answer, a timeout, a rate limit or a server fault. a refused key
/// or a malformed request fails the same way every time, so it is not retried.
pub(crate) fn is_transient(error: &str) -> bool {
    if error.starts_with("network error") || error.starts_with("unreadable answer") {
        return true;
    }
    error
        .strip_prefix("the provider answered http ")
        .and_then(|code| code.trim().parse::<u16>().ok())
        .is_some_and(|code| matches!(code, 408 | 409 | 425 | 429 | 500..=599))
}

impl Agent {
    /// one model turn, asked again on a passing failure after each wait of
    /// `retry_delays`. hands back the first reply that says or does something,
    /// or the error that ended the run; each retry is announced on `on`.
    pub(crate) fn chat_with_retry<F: FnMut(Event)>(
        &mut self,
        system: &str,
        specs: &[ToolSpec],
        on: &mut F,
    ) -> Result<Reply, String> {
        let mut attempt = 0;
        loop {
            let reason = match self.client.chat(system, &self.messages, specs) {
                Ok(reply) if !is_blank(&reply) => return Ok(reply),
                Ok(_) => {
                    self.nudge_after_blank();
                    BLANK_REASON.to_string()
                }
                Err(e) if is_transient(&e) => e,
                Err(e) => return Err(e),
            };
            let Some(wait) = self.retry_delays.get(attempt).copied() else {
                return Err(format!("{reason}; gave up after {attempt} retries"));
            };
            attempt += 1;
            on(Event::Retry {
                attempt,
                of: self.retry_delays.len(),
                wait_secs: wait.as_secs(),
                reason,
            });
            if self.wait_unless_stopped(wait) {
                return Err("cancelled".to_string());
            }
        }
    }

    /// sleeps `wait` in short slices, so a cancel during a long backoff is
    /// heard at once. hands back true when the run was asked to stop.
    fn wait_unless_stopped(&self, wait: Duration) -> bool {
        let slice = Duration::from_millis(100);
        let until = std::time::Instant::now() + wait;
        while std::time::Instant::now() < until {
            if self.stop.load(Ordering::SeqCst) {
                return true;
            }
            std::thread::sleep(slice.min(until - std::time::Instant::now()));
        }
        self.stop.load(Ordering::SeqCst)
    }
}
