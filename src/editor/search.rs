//! recherche dans le fichier et dans le projet.

use super::*;

impl Editor {
    /// ^O : la recherche de fichiers flottante (façon Telescope).
    pub(super) fn toggle_search(&mut self) {
        if self.search.take().is_some() {
            if self.focus == Focus::Search {
                self.focus = Focus::Editor;
            }
            return;
        }
        let root = self
            .explorer
            .as_ref()
            .map(|e| e.root.clone())
            .unwrap_or_else(|| file_dir(self.file.as_deref()));
        self.search = Some(FileSearch::new(root));
        self.focus = Focus::Search;
        self.status = "type to filter · ↑↓ navigate · enter open · esc close".into();
    }

    /// Touches quand le focus est sur la recherche flottante.
    pub(super) fn search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl {
            match key.code {
                KeyCode::Char('q') | KeyCode::Char('x') => self.request_quit(),
                KeyCode::Char('s') => self.save(),
                KeyCode::Char('b') => self.build(),
                KeyCode::Char('t') => self.toggle_explorer(),
                KeyCode::Char('o') => self.toggle_search(),
                _ => {}
            }
            return;
        }
        let Some(fs) = &mut self.search else { return };
        match key.code {
            KeyCode::Up => fs.move_up(),
            KeyCode::Down => fs.move_down(),
            KeyCode::Home => fs.home(),
            KeyCode::End => fs.end(),
            KeyCode::Backspace => fs.pop(),
            KeyCode::Esc => {
                self.search = None;
                self.focus = Focus::Editor;
            }
            KeyCode::Enter => {
                if let Some(path) = fs.sel_path() {
                    self.search = None;
                    self.focus = Focus::Editor;
                    self.open_from_explorer(path);
                }
            }
            KeyCode::Char(c) => fs.push(c),
            _ => {}
        }
    }
}
