//! the context budget of one turn: old tool results are cut to their head so a
//! long session stays under the size the model accepts.

use crate::provider::{Msg, Role};

/// the newest tool results stay whole; older ones keep a head so the model
/// still sees what they were, and it can run the tool again to see the rest.
pub(crate) const KEEP_WHOLE_RESULTS: usize = 4;
const OLD_RESULT_HEAD: usize = 300;
/// ceiling on the characters sent in one turn (system prompt included).
pub(crate) const CONTEXT_CAP_CHARS: usize = 200_000;

/// a long session re-sends every message on each turn. an old tool result
/// (a file read, a grep) is the bulk of that, and it is stale by then: cut it
/// to its head, keep the call id so the pairing with the call stays valid.
pub(crate) fn shrink_old_tool_results(msgs: &mut [Msg], keep_whole: usize) {
    let tools: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::Tool)
        .map(|(i, _)| i)
        .collect();
    let old = tools.len().saturating_sub(keep_whole);
    for &i in &tools[..old] {
        let m = &mut msgs[i];
        if m.content.chars().count() <= OLD_RESULT_HEAD {
            continue;
        }
        let head: String = m.content.chars().take(OLD_RESULT_HEAD).collect();
        m.content = format!(
            "{head}\n[… {} characters elided from an older result; run the tool again to see them]",
            m.content.chars().count() - OLD_RESULT_HEAD
        );
    }
}

/// what one turn actually sends: the system prompt and every message. the
/// cap is measured on this, not on the messages alone, or the prompt grows
/// past the budget unseen.
pub(crate) fn context_chars(system: &str, msgs: &[Msg]) -> usize {
    system.chars().count()
        + msgs
            .iter()
            .map(|m| m.content.chars().count())
            .sum::<usize>()
}

/// a hard ceiling on the context of one turn. when the sum is over it, the
/// oldest tool results are cut to their head first, one after the other,
/// until the sum fits. the user messages are never cut, and the newest result
/// is the last one touched.
pub(crate) fn cap_context(system: &str, msgs: &mut [Msg], cap: usize) {
    let mut over = context_chars(system, msgs).saturating_sub(cap);
    if over == 0 {
        return;
    }
    let tools: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::Tool)
        .map(|(i, _)| i)
        .collect();
    for i in tools {
        if over == 0 {
            break;
        }
        let len = msgs[i].content.chars().count();
        if len <= OLD_RESULT_HEAD {
            continue;
        }
        let head: String = msgs[i].content.chars().take(OLD_RESULT_HEAD).collect();
        let cut = len - OLD_RESULT_HEAD;
        msgs[i].content = format!(
            "{head}\n[... {cut} characters elided to keep the context under its cap; run the tool again to see them]"
        );
        over = over.saturating_sub(cut);
    }
}
