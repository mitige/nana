//! the conversation outlives its turns: the second message is sent with the
//! first one and its answer, so the model keeps its context.

use super::*;
use crate::agent::Conversation;

/// a message sent after a first turn carries that turn: the request the
/// provider receives holds the first question and the first answer.
#[test]
fn the_second_turn_sends_the_first_one_to_the_model() {
    let d = tmp("conversation");
    let (url, server) = scripted(vec![
        r#"{"choices":[{"message":{"content":"the word is rutabaga"}}]}"#.to_string(),
        r#"{"choices":[{"message":{"content":"you asked for the word"}}]}"#.to_string(),
    ]);
    let chat = Conversation::with_agent(agent_at(&d, &url));
    chat.turn(&d, "remember rutabaga", |_| {}).unwrap();
    chat.turn(&d, "what did i ask?", |_| {}).unwrap();
    let seen = server.join().unwrap();
    assert_eq!(seen.len(), 2);
    assert!(seen[1].contains("remember rutabaga"), "first question lost");
    assert!(
        seen[1].contains("the word is rutabaga"),
        "first answer lost"
    );
    assert_eq!(chat.history_len(), 4, "two questions and two answers");
    let _ = std::fs::remove_dir_all(&d);
}

/// a cancelled turn stops the run at its next step and keeps the history,
/// and the next turn starts clean instead of being cancelled too.
#[test]
fn a_cancel_stops_one_turn_not_the_next() {
    let d = tmp("cancel");
    let (url, server) = scripted(vec![
        r#"{"choices":[{"message":{"content":"back again"}}]}"#.to_string(),
    ]);
    let mut agent = agent_at(&d, &url);
    agent.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let err = agent.run("first", |_| {}).unwrap_err();
    assert_eq!(err, "cancelled");
    let chat = Conversation::with_agent(agent);
    chat.cancel();
    let answer = chat.turn(&d, "second", |_| {}).unwrap();
    assert_eq!(answer, "back again");
    assert_eq!(server.join().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

/// a conversation that outgrows the cap loses its oldest turns, whole, so
/// what is sent fits and every tool answer still follows its call.
#[test]
fn a_long_conversation_drops_its_oldest_turns_to_fit() {
    use crate::agent::context::cap_context;
    use crate::provider::{Msg, Role, ToolCall};
    let big = "x".repeat(1_000);
    let mut msgs = Vec::new();
    for i in 0..20 {
        msgs.push(Msg::user(format!("question {i} {big}")));
        msgs.push(Msg {
            role: Role::Assistant,
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: format!("c{i}"),
                name: "list_dir".into(),
                args: serde_json::json!({}),
            }],
            tool_call_id: None,
        });
        msgs.push(Msg::tool(&format!("c{i}"), "ok"));
        msgs.push(Msg {
            role: Role::Assistant,
            content: format!("answer {i} {big}"),
            tool_calls: Vec::new(),
            tool_call_id: None,
        });
    }
    cap_context("system", &mut msgs, 10_000);
    assert!(context_chars("system", &msgs) <= 10_000);
    assert_eq!(msgs[0].role, Role::User, "the history starts on a question");
    assert!(
        msgs.last().unwrap().content.starts_with("answer 19"),
        "newest kept"
    );
    for (i, m) in msgs.iter().enumerate() {
        if m.role == Role::Tool {
            assert!(
                !msgs[i - 1].tool_calls.is_empty(),
                "orphan tool answer at {i}"
            );
        }
    }
}

/// a turn sent from another project starts a fresh conversation: the history
/// and the tools of one project never leak into another.
#[test]
fn another_project_starts_a_fresh_conversation() {
    let here = tmp("conv-here");
    let there = tmp("conv-there");
    let mut agent = agent_at(&here, "http://127.0.0.1:1");
    agent.messages.push(crate::provider::Msg::user("old"));
    let chat = Conversation::with_agent(agent);
    // no turn is run: a turn would build a real client, and a test never
    // calls a real provider
    chat.follow_project(&here);
    assert_eq!(chat.history_len(), 1, "the same project keeps its history");
    chat.follow_project(&there);
    assert_eq!(chat.history_len(), 0, "the old project's history leaked");
    let _ = std::fs::remove_dir_all(&here);
    let _ = std::fs::remove_dir_all(&there);
}
