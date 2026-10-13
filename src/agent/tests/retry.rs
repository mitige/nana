//! the auto retry: blank turns and passing provider failures are asked again,
//! a lasting failure ends in an error the user reads, never in a silent stop.

use super::*;
use crate::agent::retry::{backoff, is_transient, RETRIES};

const BLANK: &str = r#"{"choices":[{"message":{"content":""}}]}"#;
const DONE: &str = r#"{"choices":[{"message":{"content":"all done"}}]}"#;
const BUSY: &str =
    "HTTP/1.1 503 Service Unavailable\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}";

/// a provider sometimes hands back a turn with neither text nor tool call in
/// the middle of a run. it is not an answer: the run goes on.
#[test]
fn a_blank_reply_mid_run_does_not_end_the_run() {
    let d = tmp("blank");
    let (url, server) = scripted(vec![
        r#"{"choices":[{"message":{"content":"","tool_calls":[
             {"id":"c1","type":"function","function":{"name":"list_dir","arguments":"{}"}}]}}]}"#
            .to_string(),
        BLANK.to_string(),
        DONE.to_string(),
    ]);
    let mut a = agent_at(&d, &url);
    let mut events = Vec::new();
    let answer = a
        .run("go", |e| events.push(e))
        .expect("a blank turn is not a failure");
    assert_eq!(answer, "all done");
    assert_eq!(server.join().unwrap().len(), 3, "the run asked again");
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Retry { attempt: 1, .. })));
    let _ = std::fs::remove_dir_all(&d);
}

/// a model that keeps answering nothing ends the run with an error the user
/// can read, never with a silent "done".
#[test]
fn blank_replies_that_persist_end_in_an_error() {
    let d = tmp("blank-persist");
    let (url, server) = scripted(vec![BLANK.to_string(); RETRIES + 1]);
    let mut a = agent_at(&d, &url);
    let mut events = Vec::new();
    let err = a.run("go", |e| events.push(e)).unwrap_err();
    assert!(err.contains("empty"), "{err}");
    assert!(err.contains("5 retries"), "{err}");
    assert!(events.iter().any(|e| matches!(e, Event::Error(_))));
    assert!(!events.iter().any(|e| matches!(e, Event::Finished { .. })));
    assert_eq!(
        server.join().unwrap().len(),
        RETRIES + 1,
        "one call and five retries"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// a busy provider (http 503) is asked again, and the run carries on.
#[test]
fn a_busy_provider_is_retried_until_it_answers() {
    let d = tmp("busy");
    let (url, server) = scripted(vec![BUSY.to_string(), BUSY.to_string(), DONE.to_string()]);
    let mut a = agent_at(&d, &url);
    let answer = a.run("go", |_| {}).expect("two 503 then an answer");
    assert_eq!(answer, "all done");
    assert_eq!(server.join().unwrap().len(), 3);
    let _ = std::fs::remove_dir_all(&d);
}

/// a refused key fails the same way every time: it is reported at once.
#[test]
fn a_refused_key_is_not_retried() {
    assert!(!is_transient("the provider answered http 401"));
    assert!(!is_transient("the provider answered http 400"));
    assert!(is_transient("the provider answered http 429"));
    assert!(is_transient("the provider answered http 502"));
    assert!(is_transient("network error: connection reset"));
}

/// five retries, each wait longer than the one before.
#[test]
fn the_backoff_has_five_growing_waits() {
    let waits = backoff();
    assert_eq!(waits.len(), 5);
    assert!(waits.windows(2).all(|w| w[0] < w[1]), "{waits:?}");
    assert!(waits[0].as_secs() >= 1, "{waits:?}");
}
