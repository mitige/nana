//! édition du texte : saisie, curseur, sélection, annulation.

use super::*;

impl Editor {
    /// Returns the text of the line under the cursor, the line most edits act on.
    pub(super) fn line(&self) -> &str {
        &self.lines[self.cy]
    }

    /// Écran d'accueil : aucun fichier, buffer vierge et jamais modifié.
    pub(super) fn is_welcome(&self) -> bool {
        self.file.is_none() && !self.modified && self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Recalcule `modified` en comparant au disque — annuler une frappe
    /// en revenant à l'état exact sauvegardé rend le buffer propre.
    pub(super) fn sync_modified(&mut self) {
        self.modified = self.lines != self.saved;
    }

    /// Sauvegarde l'état avant une modification (pour Ctrl+Z).
    pub(super) fn snapshot(&mut self) {
        self.history.push((self.lines.clone(), self.cx, self.cy));
        if self.history.len() > 200 {
            self.history.remove(0);
        }
    }

    /// Restores the last snapshot of text and cursor, reverting one step, and sets the status to cancelled so the user sees the undo happened.
    pub(super) fn undo(&mut self) {
        if let Some((lines, cx, cy)) = self.history.pop() {
            self.lines = lines;
            self.cx = cx;
            self.cy = cy;
            self.sync_modified();
            self.status = "cancelled".into();
        }
    }

    /// Fait disparaître la suggestion (texte fantôme) — état cohérent :
    /// `ghost` (l'acceptation) et `ghost_lines` (le rendu) partent ensemble.
    pub(super) fn clear_ghost(&mut self) {
        self.ghost = None;
        self.ghost_lines.clear();
    }

    /// Inserts one character at the cursor after a snapshot, advancing by its byte length so accented characters are not split.
    pub(super) fn insert_char(&mut self, c: char) {
        self.snapshot();
        let line = &mut self.lines[self.cy];
        line.insert(self.cx, c);
        self.cx += c.len_utf8(); // un é = 2 octets — on avance du caractère entier
        self.sync_modified();
        self.clear_ghost();
    }

    /// Fermante associée à une ouvrante (auto-paires).
    pub(super) fn pair_for(c: char) -> Option<char> {
        Some(match c {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            '"' => '"',
            '\'' => '\'',
            _ => return None,
        })
    }

    /// Frappe d'un caractère : auto-paires à la Xcode —
    /// l'ouvrante insère la paire (curseur au milieu), la fermante déjà
    /// présente est survolée, tout le reste est inséré normalement.
    pub(super) fn type_char(&mut self, c: char) {
        let next = self.lines[self.cy].as_bytes().get(self.cx).copied();
        // survol : la fermante tapée alors qu'elle est déjà sous le curseur
        if matches!(c, ')' | ']' | '}' | '"' | '\'') && next == Some(c as u8) {
            self.cx += 1;
            self.clear_ghost();
            return;
        }
        if let Some(close) = Self::pair_for(c) {
            self.snapshot();
            let line = &mut self.lines[self.cy];
            line.insert(self.cx, close);
            line.insert(self.cx, c);
            self.cx += 1;
            self.sync_modified();
            self.clear_ghost();
            return;
        }
        self.insert_char(c);
    }

    /// Splits the line at the cursor and carries the current indentation to the new line, so Enter keeps the block level.
    pub(super) fn insert_newline(&mut self) {
        self.snapshot();
        let rest = self.lines[self.cy].split_off(self.cx);
        // indentation automatique : reprend l'indentation de la ligne courante
        let indent: String = self.lines[self.cy]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        let indent_len = indent.len();
        self.cy += 1;
        self.lines.insert(self.cy, indent + &rest);
        self.cx = indent_len;
        self.sync_modified();
        self.clear_ghost();
    }

    /// split the line here, with no auto-indent: pasting must not reformat.
    pub(super) fn split_line_here(&mut self) {
        let rest = self.lines[self.cy].split_off(self.cx);
        self.cy += 1;
        self.lines.insert(self.cy, rest);
        self.cx = 0;
    }

    /// Paste a block of text verbatim. Typing enter auto-indents, which is
    /// right for writing code and wrong for pasting it: what you copied is
    /// what you get, newlines included, and nothing grows a tab of its own.
    pub(super) fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // the agent card owns the keyboard: its paste goes into the prompt,
        // line breaks kept, and never into the user's buffer behind the card
        if let Some(pane) = self.agent_pane.as_mut() {
            let cleaned = safe_text(&text.replace("\r\n", "\n").replace('\r', "\n"));
            pane.input.push_str(&cleaned);
            return;
        }
        // a modal card owns the keyboard: a paste goes nowhere while it is open
        if self.hub.is_some() {
            return;
        }
        self.snapshot();
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut lines = 0;
        for (i, part) in normalized.split('\n').enumerate() {
            if i > 0 {
                self.split_line_here();
                lines += 1;
            }
            if part.is_empty() {
                continue;
            }
            let line = &mut self.lines[self.cy];
            line.insert_str(self.cx, part);
            self.cx += part.len();
        }
        self.sync_modified();
        self.clear_ghost();
        self.status = format!("pasted {lines} line(s)");
    }

    /// Deletes the character before the cursor, and the closing bracket after it when the cursor sits inside an empty pair, so an auto-closed pair goes in one key.
    pub(super) fn backspace(&mut self) {
        self.snapshot();
        if self.cx > 0 {
            // paire vide sous le curseur : « (|) » → les deux partent
            let pair = {
                let b = self.lines[self.cy].as_bytes();
                self.cx < b.len()
                    && Self::pair_for(b[self.cx - 1] as char) == Some(b[self.cx] as char)
            };
            // on recule à la frontière : l'accentué part en entier
            let prev = floor_boundary(self.line(), self.cx - 1);
            let line = &mut self.lines[self.cy];
            line.drain(prev..self.cx);
            self.cx = prev;
            if pair {
                line.remove(self.cx); // la fermante (ASCII, 1 octet)
            }
            self.sync_modified();
        } else if self.cy > 0 {
            let cur = self.lines.remove(self.cy);
            self.cy -= 1;
            self.cx = self.lines[self.cy].len();
            self.lines[self.cy].push_str(&cur);
            self.sync_modified();
        }
        self.clear_ghost();
    }

    /// Moves the cursor one character left, stepping over multi-byte characters whole.
    pub(super) fn move_left(&mut self) {
        if self.cx > 0 {
            self.cx -= self.line()[..self.cx]
                .chars()
                .next_back()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
        } else if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.lines[self.cy].len();
        }
        self.clear_ghost();
    }

    /// Moves the cursor one character right, stepping over multi-byte characters whole.
    pub(super) fn move_right(&mut self) {
        if self.cx < self.line().len() {
            self.cx += self.line()[self.cx..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
        } else if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            self.cx = 0;
        }
        self.clear_ghost();
    }

    /// Moves the cursor to the line above, keeping the column on a character boundary, and clears the ghost since its place changed.
    pub(super) fn move_up(&mut self) {
        if self.cy > 0 {
            self.cy -= 1;
            let len = self.lines[self.cy].len();
            self.cx = floor_boundary(&self.lines[self.cy], self.cx.min(len));
        }
        self.clear_ghost();
    }

    /// Moves the cursor to the line below, keeping the column on a character boundary, and clears the ghost since its place changed.
    pub(super) fn move_down(&mut self) {
        if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            let len = self.lines[self.cy].len();
            self.cx = floor_boundary(&self.lines[self.cy], self.cx.min(len));
        }
        self.clear_ghost();
    }

    /// Remplace toutes les occurrences de « motif→remplacement » (séparateur →).
    pub(super) fn replace_all(&mut self, spec: &str) {
        let Some((needle, repl)) = spec.split_once('→').or_else(|| spec.split_once("->")) else {
            self.status = "syntax : pattern→replacement".into();
            return;
        };
        if needle.is_empty() {
            return;
        }
        self.snapshot();
        let mut count = 0;
        for line in &mut self.lines {
            if line.contains(needle) {
                let n = line.matches(needle).count();
                *line = line.replace(needle, repl);
                count += n;
            }
        }
        if count > 0 {
            self.sync_modified();
        }
        self.status = format!("{count} replacement(s)");
    }

    /// Moves the cursor to the next line containing the needle, searching from the current line and wrapping, so repeated find keeps going.
    pub(super) fn find(&mut self, needle: &str) {
        if needle.is_empty() {
            return;
        }
        let n = self.lines.len();
        for k in 0..n {
            let row = (self.cy + k) % n;
            if let Some(col) = self.lines[row].find(needle) {
                if row == self.cy && col <= self.cx && k == 0 {
                    continue; // déjà dessus, cherche la suivante
                }
                self.cy = row;
                self.cx = col;
                self.status = format!("« {needle} » — line {}", row + 1);
                return;
            }
        }
        self.status = format!("« {needle} » not found");
    }

    /// Writes the buffer to its file with a final newline and marks it saved, or says in the status line that there is no file name.
    pub(super) fn save(&mut self) {
        let Some(path) = self.file.clone() else {
            self.status = "no file name — run with: nana <file>".into();
            return;
        };
        let mut text = self.lines.join("\n");
        text.push('\n'); // C-A3 : newline final, toujours
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.saved = self.lines.clone();
                self.sync_modified();
                // no file name in the message: it is already in the tree and
                // in the breadcrumb — the bar only says what just happened
                self.notify(Level::Ok, "saved");
                // the check runs off the ui thread after every save: the
                // language's own tool plus the universal style pass
                let p = path.clone();
                let (tx, rx) = channel();
                crate::guard::spawn_guarded(tx, move || {
                    let cfg = crate::config::load().style.to_cfg();
                    crate::check::check_path(&p, &cfg)
                });
                self.check_rx = Some(rx);
            }
            Err(e) => self.status = format!("write error : {e}"),
        }
    }
}
