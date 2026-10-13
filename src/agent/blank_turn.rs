//! a blank turn: a reply with neither text nor tool call. some providers send
//! one mid-run (a dropped stream, a filtered answer); it is not the end of the
//! task, and taking it for one stopped the agent with no word of why.

use super::Agent;
use crate::provider::{Msg, Reply};

/// what the retry reports when the model sent nothing.
pub(crate) const BLANK_REASON: &str = "the model sent an empty reply";

/// the note that sends the model back to work after a blank turn.
const NUDGE: &str = "your last reply was empty. carry on with the task where you left \
                     it, or say in words that it is done.";

/// true when the reply carries nothing at all: no words, no tool call.
pub(crate) fn is_blank(reply: &Reply) -> bool {
    reply.text.trim().is_empty() && reply.tool_calls.is_empty()
}

impl Agent {
    /// tells the model its last turn was empty, so the next call resumes the
    /// task instead of sending the same nothing again.
    pub(crate) fn nudge_after_blank(&mut self) {
        self.messages.push(Msg::user(NUDGE));
        self.log("user", NUDGE, None, None);
    }
}
