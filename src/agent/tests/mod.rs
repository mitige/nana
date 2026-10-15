//! the agent's tests: the prompt, the tool loop and the transcript; the
//! helpers here are shared with the topic files beside it.

/// read a whole http request: the headers, then exactly as many body bytes
/// as content-length promised. deterministic, no waiting on timeouts.
fn read_request(sock: &mut std::net::TcpStream) -> String {
    use std::io::Read;
    sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut head_len: Option<usize> = None;
    let mut want: usize = 0;
    loop {
        if let Some(h) = head_len {
            if buf.len() >= h + want {
                break;
            }
        }
        match sock.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
        if head_len.is_none() {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                head_len = Some(i + 4);
                let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                want = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
            }
        }
    }
    String::from_utf8_lossy(&buf).to_string()
}
mod conversation;
mod retry;

use super::context::context_chars;
use super::*;
use std::io::Write;
use std::net::TcpListener;

pub(super) fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nana-agent-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// a provider that answers with a scripted list of bodies, one per call.
pub(super) fn scripted(bodies: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for body in bodies {
            // a run that makes fewer model calls than scripted must fail
            // the test, not leave the server waiting forever
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut sock = loop {
                match listener.accept() {
                    Ok((sock, _)) => break sock,
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => return seen,
                }
            };
            sock.set_nonblocking(false).unwrap();
            seen.push(read_request(&mut sock));
            // a body that is already a whole response (an error status) goes as is
            let resp = if body.starts_with("HTTP/") {
                body
            } else {
                format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            };
            let _ = sock.write_all(resp.as_bytes());
        }
        seen
    });
    (format!("http://{addr}"), handle)
}

pub(super) fn agent_at(root: &Path, url: &str) -> Agent {
    let s = Settings {
        sandbox: Some(true),
        ..Settings::default()
    };
    let mut agent = Agent::with_client(root, s, Client::local(url, "mock"));
    // a test does not sit through the real backoff: the retries are instant
    agent.retry_delays = vec![std::time::Duration::ZERO; super::retry::RETRIES];
    agent
}

#[test]
fn the_prompt_asks_for_one_responsibility_per_function_and_file() {
    let d = tmp("structure");
    let a = Agent::with_client(
        &d,
        Settings::default(),
        Client::local("http://127.0.0.1:1", "m"),
    );
    let p = a.system_prompt("split the editor");
    assert!(
        p.contains("one job per function"),
        "no function-split rule: {p}"
    );
    assert!(
        p.contains("one responsibility per file"),
        "no file-split rule: {p}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_prompt_asks_for_one_unbroken_run() {
    let d = tmp("autonomy");
    let a = Agent::with_client(
        &d,
        Settings::default(),
        Client::local("http://127.0.0.1:1", "m"),
    );
    let p = a.system_prompt("refactor the parser");
    assert!(p.contains("one unbroken run"), "no autonomy nudge: {p}");
    assert!(
        p.contains("do not stop to ask"),
        "the agent must not ask permission between steps: {p}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_prompt_carries_the_memory_the_request_touches_not_only_its_names() {
    let d = tmp("recall-prompt");
    let m = crate::memory::Memory::open(&d);
    m.write(
        crate::memory::Class::Project,
        "parser",
        "# parser\n\npratt loop, no recursion past depth 8",
    )
    .unwrap();
    let a = Agent::with_client(
        &d,
        Settings::default(),
        Client::local("http://127.0.0.1:1", "m"),
    );
    let p = a.system_prompt("refactor the parser");
    assert!(
        p.contains("pratt loop"),
        "the page body is in the prompt: {p}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_checks_of_the_pages_a_request_touches_run_before_the_answer() {
    let d = tmp("auto-check");
    std::fs::write(d.join("Cargo.toml"), "[package]\n").unwrap();
    let m = crate::memory::Memory::open(&d);
    m.write_checked(
        crate::memory::Class::Project,
        "manifest",
        "# manifest\n\na cargo manifest exists",
        crate::memory::Confidence::Medium,
        Some("test -f Cargo.toml"),
    )
    .unwrap();
    m.write_checked(
        crate::memory::Class::Project,
        "parser",
        "# parser\n\nthe parser lives in one file",
        crate::memory::Confidence::Medium,
        Some("test -f missing-parser.rs"),
    )
    .unwrap();
    let a = Agent::with_client(
        &d,
        Settings::default(),
        Client::local("http://127.0.0.1:1", "m"),
    );
    let p = a.system_prompt("refactor the parser and the manifest");
    assert!(p.contains("checks run for you"), "no check report: {p}");
    assert!(
        p.contains("holds: manifest"),
        "a true page is not reported: {p}"
    );
    assert!(
        p.contains("does not hold: parser"),
        "a false page is not reported: {p}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn old_tool_results_are_shrunk_so_the_context_stops_growing() {
    let big = "x".repeat(20_000);
    let mut msgs = vec![Msg::user("q")];
    for i in 0..6 {
        msgs.push(Msg::tool(&format!("c{i}"), big.clone()));
    }
    shrink_old_tool_results(&mut msgs, 2);
    assert_eq!(msgs[0].content, "q", "the question is never shrunk");
    for (i, m) in msgs[1..].iter().enumerate() {
        assert_eq!(
            m.tool_call_id.as_deref(),
            Some(format!("c{i}").as_str()),
            "the call id is kept so the pairing stays valid"
        );
    }
    assert!(msgs[1].content.len() < 1000, "an old result is shrunk");
    assert!(
        msgs[1].content.contains("elided"),
        "the model is told it was cut and can read it again: {}",
        &msgs[1].content
    );
    assert_eq!(msgs[5].content.len(), 20_000, "the last two stay whole");
    assert_eq!(msgs[6].content.len(), 20_000, "the last two stay whole");
}

#[test]
fn the_context_sent_each_turn_stays_under_the_cap() {
    let big = "x".repeat(100_000);
    let mut msgs = vec![Msg::user("q")];
    for i in 0..6 {
        msgs.push(Msg::tool(&format!("c{i}"), big.clone()));
    }
    let system = "s".repeat(1_000);
    assert!(
        context_chars(&system, &msgs) > CONTEXT_CAP_CHARS,
        "the fixture really is over the cap"
    );
    cap_context(&system, &mut msgs, CONTEXT_CAP_CHARS);
    let sent = context_chars(&system, &msgs);
    assert!(
        sent <= CONTEXT_CAP_CHARS,
        "the context sent is {sent} chars, over the cap of {CONTEXT_CAP_CHARS}"
    );
    assert_eq!(msgs[0].content, "q", "the question is never cut");
    assert_eq!(
        msgs[6].content.len(),
        100_000,
        "the newest result is the last to be cut"
    );
}

#[test]
fn an_empty_memory_still_asks_the_agent_to_remember() {
    let d = tmp("memory-empty");
    let a = Agent::with_client(
        &d,
        Settings::default(),
        Client::local("http://127.0.0.1:1", "m"),
    );
    let p = a.system_prompt("refactor the parser");
    assert!(p.contains("memory_write"), "no write nudge: {p}");
    assert!(p.contains("memory_read"), "no read nudge: {p}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn memory_is_a_world_model_the_agent_checks_against_reality() {
    let d = tmp("world");
    let a = Agent::with_client(
        &d,
        Settings::default(),
        Client::local("http://127.0.0.1:1", "m"),
    );
    let p = a.system_prompt("refactor the parser");
    assert!(
        p.contains("world model"),
        "memory is not framed as a model: {p}"
    );
    assert!(p.contains("expect"), "no expectation before acting: {p}");
    assert!(
        p.contains("correct the page"),
        "a wrong belief is never corrected: {p}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn memory_rules_name_each_moment_that_calls_for_a_memory_act() {
    let d = tmp("memory-rules");
    let a = Agent::with_client(
        &d,
        Settings::default(),
        Client::local("http://127.0.0.1:1", "m"),
    );
    let p = a.system_prompt("refactor the parser");
    assert!(p.contains("memory rules"), "the rules have no heading: {p}");
    assert!(
        p.contains("when a task starts"),
        "no start-of-task trigger: {p}"
    );
    assert!(
        p.contains("when a task ends"),
        "no end-of-task trigger: {p}"
    );
    assert!(
        p.contains("when a page is wrong"),
        "no correction trigger: {p}"
    );
    assert!(
        p.contains("when the user states a preference"),
        "no user-preference trigger: {p}"
    );
    assert!(
        p.contains("before your final words"),
        "the agent may end without recording: {p}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// a run that changed files and then answers without a memory page is
/// sent back once: the loop does not let durable work end unrecorded.
#[test]
fn durable_work_without_a_memory_page_is_sent_back_before_the_answer() {
    let d = tmp("gate");
    let write = serde_json::json!({"path": "src/a.rs", "content": "fn a() {}"}).to_string();
    let mem =
        serde_json::json!({"class": "project", "name": "a", "content": "a is new"}).to_string();
    let call = |id: &str, name: &str, args: &str| {
        serde_json::json!({"choices":[{"message":{"content":"","tool_calls":[
            {"id":id,"type":"function","function":{"name":name,"arguments":args}}]}}]})
        .to_string()
    };
    let (url, server) = scripted(vec![
        call("c1", "write_file", &write),
        r#"{"choices":[{"message":{"content":"written"}}]}"#.to_string(),
        call("c2", "memory_write", &mem),
        r#"{"choices":[{"message":{"content":"all recorded"}}]}"#.to_string(),
    ]);
    let mut a = agent_at(&d, &url);
    let answer = a.run("add a", |_| {}).unwrap();
    let seen = server.join().unwrap();
    assert_eq!(answer, "all recorded", "the run went on after the gate");
    assert_eq!(seen.len(), 4, "the gate added one more model call");
    assert!(
        seen[2].contains("changed files without a memory page"),
        "the model was sent back to memory: {}",
        &seen[2][..seen[2].len().min(300)]
    );
    assert!(d.join(".nana/memory/project/a.md").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_read_only_run_is_not_sent_back_to_memory() {
    let d = tmp("gate-read");
    let (url, server) = scripted(vec![
        r#"{"choices":[{"message":{"content":"the answer"}}]}"#.to_string(),
    ]);
    let mut a = agent_at(&d, &url);
    assert_eq!(a.run("what is here?", |_| {}).unwrap(), "the answer");
    assert_eq!(server.join().unwrap().len(), 1, "no extra call for a read");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_prompt_is_ordered_identity_persona_then_the_rest() {
    let d = tmp("prompt");
    std::fs::create_dir_all(d.join(".nana/personas")).unwrap();
    std::fs::write(
        d.join(".nana/personas/lead.md"),
        "---\ntitle: Lead\ndescription: d\n---\nbe terse.\n",
    )
    .unwrap();
    std::fs::write(d.join("AGENTS.md"), "always write tests\n").unwrap();
    std::fs::create_dir_all(d.join(".nana/skills")).unwrap();
    crate::knowledge::write(&d, "build", "# build\n\ncargo build --release").unwrap();
    std::fs::write(
        d.join(".nana/skills/rel.md"),
        "---\nname: rel\ndescription: releases\ntrigger: release\n---\nstep one\n",
    )
    .unwrap();
    let s = Settings {
        persona: Some("lead".into()),
        ..Settings::default()
    };
    let a = Agent::with_client(&d, s, Client::local("http://127.0.0.1:1", "m"));
    let p = a.system_prompt("cut a release");
    let ipos = p.find("you are the agent inside nana").unwrap();
    let ppos = p.find("be terse.").unwrap();
    let apos = p.find("always write tests").unwrap();
    let spos = p.find("- rel: releases").unwrap();
    let tpos = p.find("tools:").unwrap();
    assert!(ipos < ppos, "identity before persona");
    assert!(ppos < apos, "persona before project instructions");
    assert!(apos < spos, "instructions before skills");
    assert!(spos < tpos, "skills before the tool note");
    // a matching trigger brings the skill body in
    assert!(p.contains("step one"), "the triggered skill is included");
    assert!(p.contains("- build: build"), "the wiki is announced: {p}");
    let q = a.system_prompt("write a poem");
    assert!(!q.contains("step one"), "and only when it matches");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_loop_runs_a_tool_then_answers() {
    let d = tmp("loop");
    std::fs::write(d.join("hello.txt"), "the secret word is: rutabaga\n").unwrap();
    let (url, server) = scripted(vec![
        r#"{"choices":[{"message":{"content":"","tool_calls":[
             {"id":"c1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"hello.txt\"}"}}]}}]}"#
            .to_string(),
        r#"{"choices":[{"message":{"content":"the word is rutabaga"}}]}"#.to_string(),
    ]);
    let mut a = agent_at(&d, &url);
    let mut events = Vec::new();
    let answer = a
        .run("what is the secret word?", |e| events.push(e))
        .unwrap();
    assert_eq!(answer, "the word is rutabaga");
    // the tool really ran: the model was shown the file's contents
    let seen = server.join().unwrap();
    assert_eq!(seen.len(), 2, "two model calls");
    assert!(
        seen[1].contains("rutabaga"),
        "the file content went back to the model: {}",
        &seen[1][..seen[1].len().min(400)]
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::ToolCall { name, .. } if name == "read_file")));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Finished { steps: 2 })));
    // and the transcript is on disk, in the project
    let log = std::fs::read_to_string(&a.session).unwrap();
    assert!(log.contains("\"role\":\"user\""), "{log}");
    assert!(log.contains("\"role\":\"tool\""), "{log}");
    assert!(log.contains("rutabaga"), "{log}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_file_write_is_announced_with_its_text_before_it_lands() {
    let d = tmp("live");
    let body = "fn one() {}\nfn two() {}";
    let args = serde_json::json!({"path": "src/new.rs", "content": body}).to_string();
    let reply = serde_json::json!({"choices":[{"message":{"content":"","tool_calls":[
        {"id":"c1","type":"function","function":{"name":"write_file","arguments": args}}]}}]})
    .to_string();
    let (url, server) = scripted(vec![
        reply,
        r#"{"choices":[{"message":{"content":"done"}}]}"#.to_string(),
        // the gate sends the run back once for a memory page
        r#"{"choices":[{"message":{"content":"noted"}}]}"#.to_string(),
    ]);
    let mut a = agent_at(&d, &url);
    let mut events = Vec::new();
    a.run("write it", |e| events.push(e)).unwrap();
    let _ = server.join();
    let file = events
        .iter()
        .find_map(|e| match e {
            Event::File {
                path,
                content,
                write,
            } => Some((path.clone(), content.clone(), *write)),
            _ => None,
        })
        .expect("the write is announced");
    assert_eq!(file, ("src/new.rs".to_string(), body.to_string(), true));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_file_read_is_announced_with_the_text_it_read() {
    let d = tmp("live-read");
    std::fs::write(d.join("notes.txt"), "alpha\nbeta\n").unwrap();
    let args = serde_json::json!({"path": "notes.txt"}).to_string();
    let reply = serde_json::json!({"choices":[{"message":{"content":"","tool_calls":[
        {"id":"c1","type":"function","function":{"name":"read_file","arguments": args}}]}}]})
    .to_string();
    let (url, server) = scripted(vec![
        reply,
        r#"{"choices":[{"message":{"content":"done"}}]}"#.to_string(),
    ]);
    let mut a = agent_at(&d, &url);
    let mut events = Vec::new();
    a.run("read it", |e| events.push(e)).unwrap();
    let _ = server.join();
    let file = events
        .iter()
        .find_map(|e| match e {
            Event::File {
                path,
                content,
                write,
            } => Some((path.clone(), content.clone(), *write)),
            _ => None,
        })
        .expect("the read is announced, like a write");
    assert_eq!(
        file,
        ("notes.txt".to_string(), "alpha\nbeta\n".to_string(), false)
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_refused_tool_is_reported_to_the_model_not_hidden() {
    let d = tmp("refuse");
    let (url, server) = scripted(vec![
        r#"{"choices":[{"message":{"content":"","tool_calls":[
             {"id":"c1","type":"function","function":{"name":"run_shell","arguments":"{\"command\":\"rm -rf .\"}"}}]}}]}"#
            .to_string(),
        r#"{"choices":[{"message":{"content":"understood, i will not"}}]}"#.to_string(),
    ]);
    let mut a = agent_at(&d, &url);
    let answer = a.run("delete everything", |_| {}).unwrap();
    assert_eq!(answer, "understood, i will not");
    let seen = server.join().unwrap();
    assert!(
        seen[1].contains("needs your explicit approval"),
        "the refusal is visible to the model"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_session_can_be_resumed() {
    let d = tmp("resume");
    let (url, _s) = scripted(vec![
        r#"{"choices":[{"message":{"content":"first answer"}}]}"#.to_string(),
    ]);
    let mut a = agent_at(&d, &url);
    a.run("first question", |_| {}).unwrap();
    let mut b = agent_at(&d, "http://127.0.0.1:1");
    let path = b.resume_latest().expect("a previous session");
    assert!(path.starts_with(&d), "{path:?}");
    assert_eq!(b.messages.len(), 2, "question and answer came back");
    assert_eq!(b.messages[0].content, "first question");
    assert_eq!(b.messages[1].content, "first answer");
    let _ = std::fs::remove_dir_all(&d);
}

/// a resumed session holds the model's tool requests, not only their
/// answers: a tool answer without its call is refused by the providers.
#[test]
fn a_resumed_session_keeps_each_tool_call_before_its_answer() {
    let d = tmp("resume-tools");
    let (url, _s) = scripted(vec![
        r#"{"choices":[{"message":{"content":"","tool_calls":[
             {"id":"c1","type":"function","function":{"name":"list_dir","arguments":"{}"}}]}}]}"#
            .to_string(),
        r#"{"choices":[{"message":{"content":"listed"}}]}"#.to_string(),
    ]);
    let mut a = agent_at(&d, &url);
    a.run("list it", |_| {}).unwrap();
    let mut b = agent_at(&d, "http://127.0.0.1:1");
    b.resume_latest().expect("a previous session");
    let roles: Vec<Role> = b.messages.iter().map(|m| m.role).collect();
    assert_eq!(
        roles,
        vec![Role::User, Role::Assistant, Role::Tool, Role::Assistant]
    );
    assert_eq!(b.messages[1].tool_calls.len(), 1, "the call came back");
    assert_eq!(b.messages[1].tool_calls[0].id, "c1");
    assert_eq!(b.messages[1].tool_calls[0].name, "list_dir");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_long_session_keeps_going_until_the_model_answers() {
    let d = tmp("long");
    // far past the old 12-step cap: the agent must not stop on its own
    let calls = 30;
    let mut bodies = Vec::new();
    for i in 0..calls {
        bodies.push(format!(
            r#"{{"choices":[{{"message":{{"content":"","tool_calls":[
                {{"id":"c{i}","type":"function","function":{{"name":"list_dir","arguments":"{{}}"}}}}]}}}}]}}"#
        ));
    }
    bodies.push(r#"{"choices":[{"message":{"content":"all done"}}]}"#.to_string());
    let (url, server) = scripted(bodies);
    let mut a = agent_at(&d, &url);
    let mut events = Vec::new();
    let answer = a
        .run("keep going", |e| events.push(e))
        .expect("a long session must not be cut off");
    assert_eq!(answer, "all done");
    // a passing provider failure is retried, and a retry asks the same step
    // again: a lost or truncated answer costs one extra request, never a step.
    // on a loaded ci runner that happened, so the counts below are floors, not
    // equalities — pinning them exactly made this test flaky, not safer.
    assert!(
        server.join().unwrap().len() > calls,
        "every step was asked at least once"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Finished { steps } if *steps >= calls)),
        "the session ran far past the old 12-step cap and ended on an answer"
    );
    let _ = std::fs::remove_dir_all(&d);
}
