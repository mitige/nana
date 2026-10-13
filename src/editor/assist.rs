//! assistant IA : contexte, requêtes et réponses de l'éditeur.

use super::*;

impl Editor {
    /// Demande une complétion à l'agent. Contexte : le fichier ENTIER
    /// (fenêtre 1M des modèles) avec le curseur marqué « ▌ ».
    /// Budget de sortie : 128K — la complétion peut écrire plusieurs lignes.
    pub(super) fn ai_complete(&mut self, client: &AiClient) {
        if self.ai_busy {
            return;
        }
        // marqueur de curseur, char-safe (pas de panique sur un octet coupé)
        let line: Vec<char> = self.lines[self.cy].chars().collect();
        let pos = self.cx.min(line.len());
        let marked: String =
            line[..pos].iter().collect::<String>() + "▌" + &line[pos..].iter().collect::<String>();
        // contexte : tout le fichier (fenêtre 1M). Au-delà de 600k chars on
        // garde tête + zone du curseur — jamais vu en piscine, mais sûr.
        let mut ctx_lines: Vec<String> = self.lines.clone();
        ctx_lines[self.cy] = marked;
        let total: usize = ctx_lines.iter().map(|l| l.len() + 1).sum();
        if total > MAX_CTX_CHARS {
            let head: Vec<String> = ctx_lines.iter().take(200).cloned().collect();
            // zone du curseur (±300 lignes), sans chevaucher la tête —
            // si le curseur est dans la tête, la queue commence après
            let from = self.cy.saturating_sub(300).max(200);
            let tail: Vec<String> = ctx_lines.iter().skip(from).take(601).cloned().collect();
            ctx_lines = head;
            ctx_lines.push("/* … (very long file : head + cursor area) … */".into());
            ctx_lines.extend(tail);
        }
        let fname = self
            .file
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "code.c".into());
        let lang_name = langs::for_path(Path::new(&fname)).name;
        let system = system_prompt(self.complete_agent, crate::ai::HelpLevel::Assistance)
            + &format!(
                "\n\nyou are in COMPLETION mode (like github copilot). you are given the \
                 whole {lang_name} file with a cursor marked « ▌ ». answer with the code \
                 that comes at ▌ and nothing else: the logical continuation. you may write \
                 several lines — a whole block, a function body, a loop — up to about 30 \
                 lines if the block calls for it, but stop at the end of the current \
                 logical block: no guessing past it. never: re-emit includes that are \
                 already there, add a main() when the file has one, repeat code from \
                 before ▌, markdown, fences, explanations. just the code that continues. \
                 match the file's existing indentation and style. if the line ends with an \
                 opening brace, start your answer with a newline."
            );
        let user = format!("// file: {fname}\n{}", ctx_lines.join("\n"));
        let (tx, rx) = channel();
        let client = client.clone();
        // budget 128K + retries (réponse vide, 429/5xx, 400) — cf. ai::chat
        crate::guard::spawn_guarded(tx, move || {
            client.chat(&system, &user).map_err(|e| e.to_string())
        });
        self.rx = Some(rx);
        self.ai_busy = true;
        self.status = "in progress… (esc : cancel)".into();
    }

    /// collect what the agent worker produced, each frame. the writes are shown
    /// at once, in the order the agent made them: nothing waits in a queue, so
    /// the view can never trail behind what the agent has done.
    pub(super) fn poll_agent(&mut self) {
        if let Some(pane) = &mut self.agent_pane {
            pane.poll();
        }
        let writes: Vec<LiveWrite> = self
            .agent_pane
            .as_mut()
            .map(|p| p.writes.drain(..).collect())
            .unwrap_or_default();
        for write in writes {
            self.show_write(write);
        }
        if let Some(hub) = &mut self.hub {
            hub.poll();
        }
    }

    /// Shows one agent write whole in the agent's own view, at once, without
    /// touching the user's buffer, file or cursor. Every action has its own
    /// window, as a file does, and the tree follows the file being worked on.
    fn show_write(&mut self, w: LiveWrite) {
        let last = w.text.len().saturating_sub(1);
        // a write that spans lines glides: the cursor comes down from the line
        // it was on, or from the first line of a window just opened, so the eye
        // follows the agent instead of seeing the cursor jump.
        let glide = if last > 0 {
            Some((self.agent_view.cy.min(last), Instant::now()))
        } else {
            None
        };
        self.agent_view = AgentView {
            path: Some(w.path.clone()),
            lines: w.text.clone(),
            cy: last,
            cx: w.text.last().map_or(0, |l| l.chars().count()),
            opened: Some(Instant::now()),
            glide,
        };
        if let Some(ex) = &mut self.explorer {
            if !is_action_view(&w.path) {
                ex.select_path(&w.path);
            }
        }
    }

    /// Takes the result of a finished style check without blocking and keeps its verdict, note and details, stamped with the time so the corner indicator pulses once.
    pub(super) fn poll_check(&mut self) {
        let Some(rx) = &self.check_rx else {
            return;
        };
        if let Ok(report) = rx.try_recv() {
            // no toast and no transient line: the verdict is the indicator in
            // the corner, and it pulses once so you notice it changed
            self.check_at = Some(Instant::now());
            self.check_note = Some(report.note.clone());
            self.check_marks = report.marks.clone();
            self.check_details = report.details.clone();
            self.check_rx = None;
        }
    }

    /// Takes the reply of a finished completion without blocking, strips a stray code fence, and offers the result as a ghost to accept.
    pub(super) fn poll_ai(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        if let Ok(result) = rx.try_recv() {
            self.ai_busy = false;
            self.rx = None;
            match result {
                Ok(text) => {
                    let mut clean = text.trim();
                    // vire une éventuelle fence ```c … ``` autour de la complétion
                    if let Some(rest) = clean.strip_prefix("```") {
                        clean = rest
                            .trim_start_matches(|c: char| c.is_alphanumeric() || c == '+')
                            .trim_start_matches('\n');
                    }
                    if let Some(rest) = clean.strip_suffix("```") {
                        clean = rest.trim_end();
                    }
                    // un \n initial est SÉMANTIQUE (« nouvelle ligne » après « { »)
                    // — on le garde, au plus un ; les \n finaux partent
                    let starts_nl = clean.starts_with('\n');
                    let clean = clean.trim_matches('\n');
                    if !clean.is_empty() {
                        let clean = if starts_nl {
                            format!("\n{clean}")
                        } else {
                            clean.to_string()
                        };
                        self.ghost_lines = clean.lines().map(|l| l.to_string()).collect();
                        self.ghost = Some(clean);
                        let n = self.ghost_lines.len();
                        self.status = format!(
                            "ready ({n} line{}) · tab : accept · esc : decline",
                            if n > 1 { "s" } else { "" }
                        );
                    }
                }
                Err(e) => self.status = format!("error : {e}"),
            }
        }
    }

    /// Inserts the pending ghost completion at the cursor and clears it, so a suggestion is taken once.
    pub(super) fn accept_ghost(&mut self) {
        let Some(g) = self.ghost.take() else {
            self.clear_ghost();
            return;
        };
        self.ghost_lines.clear();
        // le \n initial est sémantique (nouvelle ligne après « { ») :
        // on ne garde que les finaux dehors
        let g = g.trim_end_matches('\n');
        if g.trim().is_empty() {
            return;
        }
        self.snapshot();
        // splice propre : texte avant le curseur + suggestion + texte après,
        // les retours à la ligne de la suggestion deviennent de vraies lignes
        let cur = self.lines[self.cy].clone();
        let (before, after) = (cur[..self.cx].to_string(), cur[self.cx..].to_string());
        let merged = format!("{before}{g}{after}");
        let new_lines: Vec<String> = merged.split('\n').map(|l| l.to_string()).collect();
        let added = new_lines.len().saturating_sub(1);
        self.lines.splice(self.cy..=self.cy, new_lines);
        // curseur à la fin de la suggestion insérée (avant « after »)
        self.cy += added;
        self.cx = self.lines[self.cy].len().saturating_sub(after.len());
        self.sync_modified();
    }

    /// Quitter : confirmation si le buffer est modifié.
    pub(super) fn request_quit(&mut self) {
        if self.modified {
            self.confirm_quit = true;
            self.status = "modified — o quit without saving · s save+quit · other: stay".into();
        } else {
            self.should_quit = true;
        }
    }

    /// Notification toast (coin haut-droit) + le statut garde le dernier état.
    /// the transient line ages: stamped whenever `status` actually changes,
    /// so any action brings it back and it fades on its own afterwards.
    pub(super) fn tick_status(&mut self) {
        if self.status != self.status_seen {
            self.status_seen = self.status.clone();
            self.status_at = Instant::now();
        }
    }

    /// Returns how long the current status line has been shown, so it can fade after a while.
    pub(super) fn status_age(&self) -> Duration {
        self.status_at.elapsed()
    }

    /// is a background job running? then the spinner turns and says what.
    pub(super) fn busy(&self) -> Option<&'static str> {
        if self.ai_busy {
            Some("thinking")
        } else if self.diag_rx.is_some() {
            Some("checking")
        } else if self.check_rx.is_some() {
            Some("reading")
        } else if self.header_rx.is_some() {
            Some("writing")
        } else {
            None
        }
    }

    /// the next spinner frame, from the animation clock.
    pub(super) fn spinner(&self) -> &'static str {
        let i = (self.epoch.elapsed().as_millis() / 80) as usize % SPINNER.len();
        SPINNER[i]
    }

    /// redraw fast while something moves, slowly when the screen is still.
    pub(super) fn animating(&self) -> bool {
        self.busy().is_some()
            || self
                .hub
                .as_ref()
                .map(|h| h.busy.is_some() || h.sweep_progress().is_some())
                .unwrap_or(false)
            || !self.toasts.is_empty()
            || self.status_age() < STATUS_TTL
            || self
                .check_at
                .map(|t| t.elapsed() < CHECK_TTL)
                .unwrap_or(false)
    }

    /// Shows a message in the status line with its level, stripping the icons the card paints itself so none appears twice.
    pub(super) fn notify(&mut self, level: Level, text: impl Into<String>) {
        // the card paints its own icon on the first line: a message that also
        // carries one would show the same emoji twice
        let text: String = text
            .into()
            .replace(['✓', '✗', '⚠', 'ℹ'], "")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        self.status = text.clone();
        // même message déjà affiché ? on le rafraîchit au lieu de l'empiler —
        // sinon la même notif (et son icône) apparaît deux fois.
        if let Some(t) = self.toasts.iter_mut().find(|t| t.text == text) {
            t.level = level;
            t.at = Instant::now();
            return;
        }
        self.toasts.push(Toast {
            level,
            text,
            at: Instant::now(),
        });
        if self.toasts.len() > 8 {
            self.toasts.remove(0);
        }
    }
}
