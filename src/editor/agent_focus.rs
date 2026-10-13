//! le panneau agent : saisie, défilement et envoi.

use super::*;

impl Editor {
    /// Handles one key while the agent card is open: typing, Enter to send, Esc to close.
    pub(super) fn agent_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(pane) = self.agent_pane.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                if pane.busy {
                    pane.cancel();
                } else {
                    self.agent_pane = None;
                }
            }
            // alt or shift + enter starts a new line of the prompt; enter sends it
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
            {
                if !pane.busy {
                    pane.input.push('\n');
                }
            }
            KeyCode::Enter => {
                let root = file_dir(self.file.as_deref());
                pane.send(&root);
            }
            KeyCode::Backspace => {
                pane.input.pop();
            }
            KeyCode::Up => pane.scroll = pane.scroll.saturating_add(1),
            KeyCode::Down => pane.scroll = pane.scroll.saturating_sub(1),
            KeyCode::Char(c) if !ctrl => pane.input.push(c),
            // ctrl+y keeps the conversation in memory now, without waiting for the turn to end
            KeyCode::Char('y') if ctrl => {
                pane.status = if pane.save_turn_now() {
                    "conversation saved to memory".into()
                } else {
                    "nothing to save yet: send a message and get an answer first".into()
                };
            }
            _ => {}
        }
    }
}
