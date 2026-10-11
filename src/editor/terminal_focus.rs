//! pilotage du terminal intégré depuis l'éditeur.

use super::*;

impl Editor {
    /// F3 : ouvre/ferme le terminal intégré (panneau bas).
    pub(super) fn toggle_terminal(&mut self) {
        if self.term.take().is_some() {
            if self.focus == Focus::Terminal {
                self.focus = Focus::Editor;
            }
            self.status = "terminal closed".into();
            return;
        }
        let cwd = file_dir(self.file.as_deref());
        // taille approximative au spawn ; affinée au premier rendu
        match TermPane::spawn(&cwd, TERM_H.saturating_sub(2), 80) {
            Ok(pane) => {
                self.term = Some(pane);
                self.focus = Focus::Terminal;
                self.status =
                    "terminal — all keys go to the shell · esc/f2 : back to editor · f3 : close"
                        .into();
            }
            Err(e) => self.notify(Level::Err, format!("terminal unavailable : {e}")),
        }
    }

    /// Touches quand le terminal a le focus : tout part au shell, sauf
    /// Échap (retour éditeur) et F3 (fermer) — F2 reste global.
    pub(super) fn terminal_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => {
                self.focus = Focus::Editor;
                self.status = "f2 panels · ^o search · ^b check · f5 run · ^s save".into();
            }
            _ => {
                let bytes: Vec<u8> = match (key.code, ctrl, alt) {
                    (KeyCode::Char(c), true, _) if c.is_ascii_lowercase() => {
                        vec![(c as u8) & 0x1f]
                    }
                    (KeyCode::Char(c), false, true) => {
                        let mut v = vec![0x1b];
                        v.extend(c.to_string().as_bytes());
                        v
                    }
                    (KeyCode::Char(c), _, _) => c.to_string().into_bytes(),
                    (KeyCode::Enter, _, _) => b"\r".to_vec(),
                    (KeyCode::Backspace, _, _) => b"".to_vec(),
                    (KeyCode::Tab, _, _) => b"	".to_vec(),
                    (KeyCode::Up, _, _) => b"[A".to_vec(),
                    (KeyCode::Down, _, _) => b"[B".to_vec(),
                    (KeyCode::Right, _, _) => b"[C".to_vec(),
                    (KeyCode::Left, _, _) => b"[D".to_vec(),
                    (KeyCode::Home, _, _) => b"[H".to_vec(),
                    (KeyCode::End, _, _) => b"[F".to_vec(),
                    (KeyCode::Delete, _, _) => b"[3~".to_vec(),
                    (KeyCode::PageUp, _, _) => b"[5~".to_vec(),
                    (KeyCode::PageDown, _, _) => b"[6~".to_vec(),
                    _ => Vec::new(),
                };
                if !bytes.is_empty() {
                    if let Some(term) = &mut self.term {
                        term.send(&bytes);
                    }
                }
            }
        }
    }
}
