//! bandeau d'en-tête : chemin, branche, indicateurs.

use super::*;

impl Editor {
    /// F4 : génère l'en-tête du fichier — description écrite par le modèle
    /// (deepseek-v4.1-flash, unique). Sans modèle : squelette à compléter.
    pub(super) fn generate_header(&mut self) {
        if self.file.is_none() {
            self.status = "no file — open or create one first (explorer ^n)".into();
            return;
        }
        let Some(client) = self.ai.clone() else {
            self.insert_header(String::new());
            return;
        };
        if self.header_rx.is_some() {
            return; // déjà en cours
        }
        let name = self
            .file
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let body: String = self
            .lines
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join(
                "
",
            );
        let lang = self
            .file
            .as_deref()
            .map(langs::for_path)
            .unwrap_or(&langs::PLAIN)
            .name;
        let (tx, rx) = channel();
        crate::guard::spawn_guarded(tx, move || {
            let system = format!(
                "you write the `description:` line of a {lang} file header. one line, \
                 imperative, 60 characters at most, no final period, plain english. \
                 answer with that line only."
            );
            let user = format!("file: {name}\n\n{body}");
            client.chat_fast(&system, &user).map_err(|e| e.to_string())
        });
        self.header_rx = Some(rx);
        self.status = "header in progress…".into();
    }

    /// once the user has left a quiet moment, the project memory is audited
    /// (read only) and the verdict goes to the status line. nothing is written.
    pub(super) fn poll_quiet(&mut self, now: Instant) {
        if self.agent_pane.is_some() || !self.quiet.due(now) {
            return;
        }
        let Some(root) =
            crate::project::detect(self.file.as_deref().unwrap_or(Path::new("."))).map(|p| p.root)
        else {
            return;
        };
        let m = crate::memory::Memory::open(&root);
        if self.consolidation_rx.is_none() && self.consolidation.due(&m) {
            if let Some(client) = self.ai.clone() {
                let (tx, rx) = std::sync::mpsc::channel();
                let mem = crate::memory::Memory::open(&root);
                std::thread::spawn(move || {
                    let res = crate::memory::consolidate(&mem, |prompt| {
                        client
                            .chat(
                                "you consolidate a project memory. answer with the json plan only.",
                                prompt,
                            )
                            .map_err(|e| e.to_string())
                    });
                    let _ = tx.send(res.map(|retired| retired.len()));
                });
                self.consolidation_rx = Some(rx);
                self.status = "memory: consolidating in the background...".into();
                return;
            }
        }
        let a = m.audit();
        if a.duplicates.is_empty() && a.stale.is_empty() {
            return;
        }
        self.status = format!(
            "memory: {} duplicate(s), {} stale page(s) — ask the agent to review them",
            a.duplicates.len(),
            a.stale.len()
        );
    }

    /// reads the consolidation thread: when it ends, the trigger is marked done
    /// for the memory as it now stands, so the model is not asked again for it.
    pub(super) fn poll_consolidation(&mut self) {
        let Some(rx) = &self.consolidation_rx else {
            return;
        };
        let Ok(res) = rx.try_recv() else {
            return;
        };
        self.consolidation_rx = None;
        if let Some(root) =
            crate::project::detect(self.file.as_deref().unwrap_or(Path::new("."))).map(|p| p.root)
        {
            let m = crate::memory::Memory::open(&root);
            self.consolidation.done(&m);
            self.status = match res {
                Ok(0) => "memory: nothing to consolidate".into(),
                Ok(n) => format!("memory: consolidated, {n} summary(ies)"),
                Err(e) => format!("memory: consolidation failed: {e}"),
            };
        }
    }

    /// Takes the header description of a finished background job without blocking and stores it for the top bar.
    pub(super) fn poll_header(&mut self) {
        let Some(rx) = &self.header_rx else {
            return;
        };
        if let Ok(res) = rx.try_recv() {
            self.header_rx = None;
            let desc = res
                .ok()
                .and_then(|t| {
                    t.lines()
                        .next()
                        .map(|l| l.trim().trim_matches('"').to_string())
                })
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(80).collect())
                .unwrap_or_default();
            self.insert_header(desc);
        }
    }

    /// Insère (ou remplace) l'en-tête en haut du fichier.
    pub(super) fn insert_header(&mut self, desc: String) {
        let Some(path) = self.file.clone() else {
            return;
        };
        let author = crate::config::load().author;
        let header = crate::header::header_lines(&path, &desc, author.as_deref());
        let lang = langs::for_path(&path);
        let open = lang.block.map(|(o, _)| o).unwrap_or("");
        let close = lang.block.map(|(_, c)| c).unwrap_or("");
        let lc = lang.line_comment;
        self.snapshot();
        // an existing header is replaced: block comment or line comments
        let mut end = 0;
        if !open.is_empty() && self.lines.first().map(|l| l.trim()) == Some(open) {
            if let Some(i) = self.lines.iter().position(|l| l.trim() == close) {
                end = i + 1;
                if self.lines.get(end).map(|l| l.is_empty()) == Some(true) {
                    end += 1;
                }
            }
        } else {
            while self
                .lines
                .get(end)
                .map(|l| l.starts_with(lc))
                .unwrap_or(false)
            {
                end += 1;
            }
            if end > 0 && self.lines.get(end).map(|l| l.is_empty()) == Some(true) {
                end += 1;
            }
        }
        self.lines.splice(0..end, header);
        self.sync_modified();
        self.cy = 0;
        self.cx = 0;
        self.notify(Level::Ok, "header inserted");
    }

    /// re-read the project the current file belongs to (cheap: a few file
    /// probes up the tree) and keep a short label for the topbar.
    pub(super) fn refresh_project(&mut self) {
        self.project_label = self
            .file
            .as_deref()
            .and_then(crate::project::detect)
            .map(|p| Self::project_label_of(&p));
    }

    /// "next · node", "django · python", "cargo"…
    pub(super) fn project_label_of(p: &crate::project::Project) -> String {
        match &p.framework {
            Some(f) => format!("{f} · {}", p.kind.name()),
            None => p.kind.name().to_string(),
        }
    }
}
