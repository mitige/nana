//! one conversation that outlives its turns. the card used to build a fresh
//! agent for every message, so the model met each message with an empty
//! history; the agent now lives here and every turn reuses it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use super::{Agent, Event};
use std::path::Path;

/// a shared handle on the conversation: the card keeps one, each worker
/// thread borrows it for the length of a turn.
#[derive(Clone, Default)]
pub struct Conversation {
    agent: Arc<Mutex<Option<Agent>>>,
    stop: Arc<AtomicBool>,
}

impl Conversation {
    /// a conversation around an agent that is already built (a test client,
    /// a resumed session), so the first turn does not build its own.
    pub fn with_agent(agent: Agent) -> Conversation {
        let conversation = Conversation::default();
        *conversation
            .agent
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(agent);
        conversation
    }

    /// asks the turn in progress to stop at its next step; the history it
    /// built so far is kept for the next turn.
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// drops the conversation when `root` is another project, so the next
    /// turn builds an agent of its own there.
    pub fn follow_project(&self, root: &Path) {
        forget_other_project(
            &mut self.agent.lock().unwrap_or_else(PoisonError::into_inner),
            root,
        );
    }

    /// how many messages the model will see again on the next turn.
    pub fn history_len(&self) -> usize {
        self.agent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map_or(0, |a| a.messages.len())
    }

    /// runs one turn on the shared agent, building it on the first turn or
    /// when the project changed. a turn waits for the one before it, so two
    /// never mix their messages.
    /// hands back the model's answer, or the error that ended the turn.
    pub fn turn<F: FnMut(Event)>(
        &self,
        root: &Path,
        request: &str,
        mut on: F,
    ) -> Result<String, String> {
        let mut slot = self.agent.lock().unwrap_or_else(PoisonError::into_inner);
        forget_other_project(&mut slot, root);
        if slot.is_none() {
            let settings = crate::settings::Settings::load(root);
            match Agent::new(root, settings) {
                Ok(agent) => *slot = Some(agent),
                Err(e) => {
                    on(Event::Error(e.clone()));
                    return Err(e);
                }
            }
        }
        let Some(agent) = slot.as_mut() else {
            return Err("the agent could not start".to_string());
        };
        agent.stop = Arc::clone(&self.stop);
        self.stop.store(false, Ordering::SeqCst);
        agent.run(request, on)
    }
}

/// empties the slot when its agent belongs to another project: the history
/// and the tools of one project never leak into another.
fn forget_other_project(slot: &mut Option<Agent>, root: &Path) {
    if slot.as_ref().is_some_and(|a| a.root != root) {
        *slot = None;
    }
}
