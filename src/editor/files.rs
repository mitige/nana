//! ouverture, sauvegarde et cycle de vie des fichiers.

use super::*;

impl Editor {
    /// Dossier de base pour créer un fichier : la sélection de
    /// l'explorateur si c'est un dossier, son parent si c'est un fichier,
    /// la racine sinon.
    pub(super) fn explorer_base_dir(&self) -> PathBuf {
        if let Some(ex) = &self.explorer {
            if let Some(row) = ex.rows().get(ex.sel()) {
                if row.is_dir {
                    return row.path.clone();
                }
                if let Some(parent) = row.path.parent() {
                    return parent.to_path_buf();
                }
            }
            return ex.root.clone();
        }
        file_dir(self.file.as_deref())
    }

    /// ^N dans l'explorateur : crée le fichier, recharge l'arbre, l'ouvre.
    pub(super) fn create_file(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        // nom avec séparateur → relatif à la racine du projet ;
        // nom nu → dans le dossier sélectionné
        let path = if name.contains('/') || name.contains('\\') {
            match &self.explorer {
                Some(ex) => ex.root.join(name),
                None => self.explorer_base_dir().join(name),
            }
        } else {
            self.explorer_base_dir().join(name)
        };
        if path.exists() {
            self.notify(Level::Warn, format!("{name} already exists"));
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&path, "") {
            Ok(()) => {
                if let Some(ex) = &mut self.explorer {
                    ex.reload();
                }
                let n = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(name)
                    .to_string();
                // ouvre le nouveau fichier puis génère l'en-tête dessus
                let was_modified = self.modified;
                if !was_modified {
                    self.open_from_explorer(path.clone());
                    self.generate_header();
                } else {
                    self.notify(
                        Level::Ok,
                        format!("{n} — created (buffer modified, not opened)"),
                    );
                }
            }
            Err(e) => self.notify(Level::Err, format!("cannot create : {e}")),
        }
    }

    /// ^D dans l'explorateur : supprime le fichier/dossier sélectionné
    /// (confirmé par o/n dans la barre).
    pub(super) fn delete_selected(&mut self) {
        let Some(ex) = &self.explorer else { return };
        let Some(row) = ex.rows().get(ex.sel()) else {
            return;
        };
        self.confirm_delete = Some(row.path.clone());
        self.status = format!("delete {} ? · o confirm · other : cancel", row.name);
    }

    /// Deletes the file waiting for confirmation and reports the outcome, clearing the pending path so it cannot run twice.
    pub(super) fn confirm_delete_now(&mut self) {
        let Some(path) = self.confirm_delete.take() else {
            return;
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        match result {
            Ok(()) => {
                // si le fichier ouvert vient de disparaître : le buffer reste,
                // mais il est orphelin — on le dit
                if self.file.as_ref() == Some(&path) {
                    self.notify(
                        Level::Warn,
                        format!("{name} deleted from disk (buffer kept)"),
                    );
                } else {
                    self.notify(Level::Ok, format!("{name} — deleted"));
                }
                if let Some(ex) = &mut self.explorer {
                    ex.reload();
                }
            }
            Err(e) => self.notify(Level::Err, format!("cannot delete : {e}")),
        }
    }

    /// ^R dans l'explorateur : renomme la sélection (même dossier).
    pub(super) fn rename_selected(&mut self, new_name: &str) {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return;
        }
        let Some(ex) = &self.explorer else { return };
        let Some(row) = ex.rows().get(ex.sel()) else {
            return;
        };
        let old_path = row.path.clone();
        let new_path = old_path.with_file_name(new_name);
        match std::fs::rename(&old_path, &new_path) {
            Ok(()) => {
                // si le fichier renommé est ouvert : suivre le chemin
                if self.file.as_ref() == Some(&old_path) {
                    self.ext = new_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("c")
                        .to_string();
                    self.file = Some(new_path.clone());
                    self.project_label =
                        crate::project::detect(&new_path).map(|p| Self::project_label_of(&p));
                }
                self.notify(Level::Ok, format!("{} → {new_name}", row.name));
                if let Some(ex) = &mut self.explorer {
                    ex.reload();
                }
            }
            Err(e) => self.notify(Level::Err, format!("cannot rename : {e}")),
        }
    }
}
