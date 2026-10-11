//! background workers that cannot write over the screen.
//!
//! A panic runs the process-wide panic hook before it unwinds, and the default
//! hook prints on stderr. In the alternate screen that text lands on the
//! editor's cells. A worker's panic is therefore silenced at the hook and sent
//! back on the worker's own channel as an error, so the user reads a message.

use std::any::Any;
use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::Sender;
use std::sync::Once;

thread_local! {
    static IN_WORKER: Cell<bool> = const { Cell::new(false) };
}

/// is this thread a guarded worker? true only inside `spawn_guarded`.
pub fn in_worker() -> bool {
    IN_WORKER.with(Cell::get)
}

/// what a worker sends back: its own result, or the text of its panic.
pub trait FromPanic {
    fn from_panic(message: String) -> Self;
}

impl FromPanic for crate::check::Report {
    fn from_panic(message: String) -> Self {
        crate::check::Report {
            note: message,
            ..Default::default()
        }
    }
}

impl<T> FromPanic for Result<T, String> {
    fn from_panic(message: String) -> Self {
        Err(message)
    }
}

impl FromPanic for crate::agent::Event {
    fn from_panic(message: String) -> Self {
        crate::agent::Event::Error(message)
    }
}

/// runs `work` on its own thread and sends its value on `tx`. a panic inside
/// it is sent in place of the value, never printed on the terminal.
pub fn spawn_guarded<T, F>(tx: Sender<T>, work: F)
where
    T: FromPanic + Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    install_hook();
    std::thread::spawn(move || {
        IN_WORKER.with(|flag| flag.set(true));
        let value = catch_unwind(AssertUnwindSafe(work))
            .unwrap_or_else(|payload| T::from_panic(panic_message(payload.as_ref())));
        let _ = tx.send(value);
    });
}

/// runs `work` on its own thread, which sends as many events as it likes on
/// `tx`. a panic is sent as one last `Event::Error`.
pub fn spawn_streaming<F>(tx: Sender<crate::agent::Event>, work: F)
where
    F: FnOnce(Sender<crate::agent::Event>) + Send + 'static,
{
    install_hook();
    std::thread::spawn(move || {
        IN_WORKER.with(|flag| flag.set(true));
        let on_panic = tx.clone();
        if let Err(payload) = catch_unwind(AssertUnwindSafe(move || work(tx))) {
            let message = panic_message(payload.as_ref());
            let _ = on_panic.send(crate::agent::Event::Error(message));
        }
    });
}

/// the default hook prints every panic on stderr. a worker's panic is already
/// reported on its channel, so the hook stays quiet for it and keeps the rest.
fn install_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !in_worker() {
                previous(info);
            }
        }));
    });
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    let text = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown".to_string());
    format!("internal error : {text}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Event;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    /// the panic text reaches the user as an error event, nothing is printed.
    #[test]
    fn a_worker_panic_becomes_an_error_event() {
        let (tx, rx) = channel::<Event>();
        spawn_guarded(tx, || panic!("boom"));
        match rx.recv_timeout(Duration::from_secs(5)).expect("an event") {
            Event::Error(msg) => assert!(msg.contains("boom"), "{msg}"),
            _ => panic!("expected an error event"),
        }
    }

    /// a worker that returns normally sends its own value, not an error.
    #[test]
    fn a_normal_result_is_sent_unchanged() {
        let (tx, rx) = channel::<Result<u32, String>>();
        spawn_guarded(tx, || Ok(7));
        let got = rx.recv_timeout(Duration::from_secs(5)).expect("a value");
        assert_eq!(got, Ok(7));
    }

    /// a panic in a completion worker comes back as that worker's error.
    #[test]
    fn a_panic_in_a_result_worker_is_an_err() {
        let (tx, rx) = channel::<Result<u32, String>>();
        spawn_guarded(tx, || panic!("kaboom"));
        let got = rx.recv_timeout(Duration::from_secs(5)).expect("a value");
        assert!(got.unwrap_err().contains("kaboom"));
    }
}
