//! explorateur de fichiers : navigation et actions.

use super::*;

impl Editor {
    /// ^T : ouvre/ferme le panneau de fichiers (racine : dossier du fichier,
    /// sinon le répertoire courant).
    pub(super) fn toggle_explorer(&mut self) {
        if self.explorer.take().is_some() {
            if self.focus == Focus::Explorer {
                self.focus = Focus::Editor;
            }
            self.status = "explorer closed".into();
            return;
        }
        let root = self
            .file
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        self.explorer = Some(Explorer::new(root));
        self.focus = Focus::Explorer;
        self.status =
            "↑↓ navigate · enter open · →/← fold · . hidden · type to filter · ^t close".into();
    }

    /// Ouvre le fichier choisi dans l'explorateur (jamais par-dessus un
    /// buffer modifié — on sauvegarde d'abord, la donnée est sacrée).
    pub(super) fn open_from_explorer(&mut self, path: PathBuf) {
        if self.modified {
            self.status = "buffer modified — ^s to save first".into();
            return;
        }
        self.interrupt_live();
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
                if lines.is_empty() {
                    lines.push(String::new());
                }
                self.saved = lines.clone();
                self.lines = lines;
                self.ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("c")
                    .to_string();
                self.cx = 0;
                self.cy = 0;
                self.scroll_x = 0;
                self.scroll_y = 0;
                self.history.clear();
                self.diags.clear();
                self.diag_idx = None;
                self.check_marks.clear();
                self.check_note = None;
                self.clear_ghost();
                // on RESTE dans l'explorateur : on enchaîne les ouvertures,
                // F2 pour aller éditer
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file")
                    .to_string();
                self.notify(Level::Info, format!("{name} — opened · f2 to edit"));
                self.file = Some(path);
                self.refresh_project();
            }
            Err(e) => self.notify(Level::Err, format!("cannot read : {e}")),
        }
    }

    /// Touches quand le focus est sur l'explorateur : les globales (^Q, ^S,
    /// ^B, ^T) passent partout, le reste pilote l'arbre et le filtre.
    pub(super) fn explorer_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // confirmation de suppression en cours : o/y efface, le reste annule
        if self.confirm_delete.is_some() {
            match key.code {
                KeyCode::Char('o') | KeyCode::Char('y') => self.confirm_delete_now(),
                _ => {
                    self.confirm_delete = None;
                    self.status = "deletion cancelled".into();
                }
            }
            return;
        }
        if ctrl {
            match key.code {
                KeyCode::Char('q') | KeyCode::Char('x') => self.request_quit(),
                KeyCode::Char('s') => self.save(),
                KeyCode::Char('b') => self.build(),
                KeyCode::Char('t') => self.toggle_explorer(),
                // opérations fichiers — le ^N/^P des diagnostics reste à l'éditeur
                KeyCode::Char('n') => {
                    self.prompt = Some(('n', String::new()));
                    self.status = "new file :".into();
                }
                KeyCode::Char('d') => self.delete_selected(),
                KeyCode::Char('r') => {
                    let cur = self
                        .explorer
                        .as_ref()
                        .and_then(|ex| ex.rows().get(ex.sel()))
                        .map(|r| r.name.clone())
                        .unwrap_or_default();
                    self.prompt = Some(('w', cur));
                    self.status = "rename to :".into();
                }
                _ => {}
            }
            return;
        }
        let Some(ex) = &mut self.explorer else { return };
        match key.code {
            KeyCode::Up => ex.move_up(),
            KeyCode::Down => ex.move_down(),
            KeyCode::Home => ex.home(),
            KeyCode::End => ex.end(),
            KeyCode::Left => ex.collapse_or_parent(),
            KeyCode::Right => ex.expand(),
            KeyCode::Enter => {
                if let Some(path) = ex.enter() {
                    self.open_from_explorer(path);
                }
            }
            KeyCode::Backspace => ex.pop_filter(),
            KeyCode::Esc => {
                if !ex.clear_filter() {
                    self.focus = Focus::Editor;
                    self.status = "f2 panels · ^b check · f5 run · ^s save".into();
                }
            }
            // « . » bascule les dotfiles ; le reste filtre en flou
            KeyCode::Char('.') => {
                let on = ex.toggle_hidden();
                self.status = if on {
                    "hidden files shown (· to hide)".into()
                } else {
                    "hidden files hidden".into()
                };
            }
            KeyCode::Char(c) => ex.push_filter(c),
            _ => {}
        }
    }
}
