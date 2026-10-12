//! boucle d'événements : touches, souris, tick et rendu.

use super::*;

impl Editor {
    /// Routes one key press: the agent card owns the keyboard while it is open, otherwise the editor bindings apply.
    pub(super) fn on_key(&mut self, key: KeyEvent) {
        self.quiet.key(Instant::now());
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // the agent panel is modal: while it is open it owns the keyboard
        if self.agent_pane.is_some() {
            return self.agent_key(key);
        }
        if self.hub.is_some() {
            return self.hub_key(key);
        }
        if self.open_pane_key(key, ctrl) {
            return;
        }
        if self.confirm_quit {
            return self.confirm_quit_key(key);
        }
        if self.prompt.is_some() {
            return self.prompt_key(key);
        }
        if self.panel_key(key, ctrl, alt) {
            return;
        }
        self.editing_key(key.code, ctrl, alt);
    }

    /// ^A opens the agent, ^W opens the hub. both are about the project.
    pub(super) fn open_pane_key(&mut self, key: KeyEvent, ctrl: bool) -> bool {
        if ctrl && key.code == KeyCode::Char('a') {
            let root = crate::project::detect(self.file.as_deref().unwrap_or(Path::new(".")))
                .map(|p| p.root)
                .unwrap_or_else(|| file_dir(self.file.as_deref()));
            let mut pane = AgentPane::new();
            pane.persona = crate::settings::Settings::load(&root).persona;
            self.agent_pane = Some(pane);
            self.status = "agent — type a request, enter sends, esc closes".into();
            return true;
        }
        if ctrl && key.code == KeyCode::Char('w') {
            // the boxes are about the project you have open. the repository
            // root wins: memory, personas and the company live at the top of
            // the project, not in whatever folder the open file sits in.
            let root = self
                .file
                .as_deref()
                .and_then(crate::project::detect)
                .map(|p| p.root)
                .or_else(|| self.explorer.as_ref().map(|e| e.root.clone()))
                .unwrap_or_else(|| file_dir(self.file.as_deref()));
            self.hub = Some(crate::hub::Hub::open(&root));
            self.status = "hub — ↑↓ browse, ←→ boxes, enter acts, esc closes".into();
            return true;
        }
        false
    }

    /// Answers the quit prompt: o or y quits without saving, s saves then quits, anything else cancels.
    pub(super) fn confirm_quit_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('o') | KeyCode::Char('y') => self.should_quit = true,
            KeyCode::Char('s') => {
                self.save();
                self.should_quit = true;
            }
            _ => self.confirm_quit = false,
        }
    }

    /// the search (^F), replace (^R), goto (^G), new file and rename prompts.
    pub(super) fn prompt_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Enter => self.submit_prompt(),
            KeyCode::Backspace => {
                if let Some((_, t)) = &mut self.prompt {
                    t.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some((_, t)) = &mut self.prompt {
                    t.push(c);
                }
            }
            _ => {}
        }
    }

    /// Runs the bottom prompt's action, dispatching on its kind letter: find, replace, new file or rename.
    pub(super) fn submit_prompt(&mut self) {
        let (kind, text) = self.prompt.take().unwrap();
        match kind {
            'f' => self.find(&text),
            'r' => self.replace_all(&text),
            'n' => self.create_file(&text),
            'w' => self.rename_selected(&text),
            'g' => self.goto_line(&text),
            _ => {}
        }
    }

    /// Moves the cursor to the given 1-based line, ignoring input that is not a line number in range.
    pub(super) fn goto_line(&mut self, text: &str) {
        let Ok(n) = text.trim().parse::<usize>() else {
            return;
        };
        if n >= 1 && n <= self.lines.len() {
            self.cy = n - 1;
            self.cx = 0;
            self.status = format!("line {n}");
        } else {
            self.status = format!("line {n} out of range");
        }
    }

    /// focus and panels. returns true when the key was theirs to take.
    pub(super) fn panel_key(&mut self, key: KeyEvent, ctrl: bool, alt: bool) -> bool {
        // F2 / Alt-Tab / Ctrl-Tab / Shift-Tab : cycle du focus entre panneaux.
        // F2 est le seul que TOUS les terminaux livrent toujours — Alt-Tab et
        // Ctrl-Tab sont avalés par l'OS ou le terminal avant nous.
        if key.code == KeyCode::F(2)
            || key.code == KeyCode::BackTab
            || (key.code == KeyCode::Tab && (alt || ctrl))
        {
            self.cycle_focus();
            return true;
        }
        if ctrl && key.code == KeyCode::Char('o') {
            self.toggle_search();
            return true;
        }
        // super+t : le même terminal que F3, pour qui vient d'un bureau
        if key.code == KeyCode::F(3)
            || (key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::SUPER))
        {
            self.toggle_terminal();
            return true;
        }
        // focus terminal : tout va au shell
        if self.focus == Focus::Terminal && self.term.is_some() {
            self.terminal_key(key);
            return true;
        }
        // focus recherche / explorateur : touches dédiées (les globales passent)
        if self.focus == Focus::Search && self.search.is_some() {
            self.search_key(key);
            return true;
        }
        if self.focus == Focus::Explorer && self.explorer.is_some() {
            self.explorer_key(key);
            return true;
        }
        false
    }

    /// keys of the text itself: the ctrl commands first, then the plain keys.
    pub(super) fn editing_key(&mut self, code: KeyCode, ctrl: bool, alt: bool) {
        if self.ctrl_command(code, ctrl, alt) {
            return;
        }
        match code {
            KeyCode::F(4) => self.generate_header(),
            KeyCode::F(5) => self.run_file(),
            KeyCode::F(6) => self.run_format(),
            KeyCode::F(7) => self.bootcamp_check(),
            KeyCode::F(8) => self.check_bootcamp_package(),
            KeyCode::F(9) => self.insert_bootcamp_templates(),
            KeyCode::Esc => self.cancel_or_decline(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
            KeyCode::Home => self.cx = 0,
            KeyCode::End => {
                if self.ghost.is_some() {
                    self.accept_ghost();
                } else {
                    self.cx = self.line().len();
                }
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Enter => self.insert_newline(),
            // Tab : accepte la suggestion IA si présente (réflexe Copilot),
            // sinon 4 espaces (la Norme)
            KeyCode::Tab => self.accept_or_indent(),
            KeyCode::Char('i') if ctrl => self.accept_or_indent(),
            KeyCode::Char(c) if !ctrl && !alt => self.type_char(c),
            _ => {}
        }
    }

    /// the ctrl shortcuts of the editor. returns false when the key is not one.
    pub(super) fn ctrl_command(&mut self, code: KeyCode, ctrl: bool, alt: bool) -> bool {
        match (code, ctrl, alt) {
            // quitter : ^Q ou ^X (réflexe nano) — PAS ^C, trop de missclicks
            (KeyCode::Char('q'), true, _) | (KeyCode::Char('x'), true, _) => self.request_quit(),
            (KeyCode::Char('t'), true, _) => self.toggle_explorer(),
            (KeyCode::Char('s'), true, _) => self.save(),
            (KeyCode::F(3), _, _) => self.toggle_terminal(),
            (KeyCode::Char('b'), true, _) => self.build(),
            (KeyCode::Char('n'), true, _) => self.diag_jump(1),
            (KeyCode::Char('p'), true, _) => self.diag_jump(-1),
            (KeyCode::Char('z'), true, _) => self.undo(),
            (KeyCode::Char('f'), true, _) => {
                self.prompt = Some(('f', String::new()));
                self.status = "search :".into();
            }
            (KeyCode::Char('r'), true, _) => {
                // chercher-remplacer : « chercher → remplacer » en une saisie
                self.prompt = Some(('r', String::new()));
                self.status = "replace « pattern » with « text » (pattern→text) :".into();
            }
            (KeyCode::Char('g'), true, _) => {
                self.prompt = Some(('g', String::new()));
                self.status = "go to line :".into();
            }
            (KeyCode::Char('a'), true, _) => self.cycle_complete_agent(),
            (KeyCode::Char('k'), true, _) => self.cut_line(),
            (KeyCode::Char('u'), true, _) => self.paste_line(),
            (KeyCode::Char(' '), true, _) => self.ask_completion(),
            (KeyCode::Right, _, true) | (KeyCode::Right, true, _) => {
                if self.ghost.is_some() {
                    self.accept_ghost();
                } else {
                    self.move_right();
                }
            }
            _ => return false,
        }
        true
    }

    /// Switches to the next completion agent and saves the choice, wrapping at the end of the list.
    pub(super) fn cycle_complete_agent(&mut self) {
        let pos = COMPLETE_AGENTS
            .iter()
            .position(|a| *a == self.complete_agent)
            .unwrap_or(0);
        self.complete_agent = COMPLETE_AGENTS[(pos + 1) % COMPLETE_AGENTS.len()];
        save_complete_agent(self.complete_agent);
        self.status = "profile changed".into();
    }

    /// Removes the current line into the clipboard, keeping at least one empty line in the buffer.
    pub(super) fn cut_line(&mut self) {
        self.snapshot();
        self.clipboard = self.lines.remove(self.cy);
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cy = self.cy.min(self.lines.len() - 1);
        self.cx = self.cx.min(self.line().len());
        self.sync_modified();
        self.clear_ghost();
        self.status = "line cut".into();
    }

    /// Inserts the clipboard line above the cursor, doing nothing when the clipboard is empty.
    pub(super) fn paste_line(&mut self) {
        if !self.clipboard.is_empty() {
            self.snapshot();
            let clip = self.clipboard.clone();
            self.lines.insert(self.cy, clip);
            self.sync_modified();
            self.status = "line pasted".into();
        }
    }

    /// Asks the completion model for the cursor context when a client exists, otherwise says in the status line why it cannot.
    pub(super) fn ask_completion(&mut self) {
        if let Some(client) = self.ai.clone() {
            self.ai_complete(&client);
        } else {
            self.status = "unavailable (missing api key)".into();
        }
    }

    /// esc stops a request in flight, or declines the ghost text on screen.
    pub(super) fn cancel_or_decline(&mut self) {
        if self.ai_busy {
            // STOP : annule la requête en vol — on lâche le canal,
            // le thread d'IA finit seul et son résultat est ignoré
            self.rx = None;
            self.ai_busy = false;
            self.clear_ghost();
            self.status = "cancelled".into();
        } else if self.ghost.is_some() || !self.ghost_lines.is_empty() {
            self.clear_ghost();
            self.status = "declined".into();
        }
    }

    /// Deletes the character under the cursor, joining the next line when the cursor is at the end.
    pub(super) fn delete_forward(&mut self) {
        if self.cx < self.line().len() {
            let next = self.cx
                + self.line()[self.cx..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
            self.lines[self.cy].drain(self.cx..next);
            self.sync_modified();
        }
        self.clear_ghost();
    }

    /// Accepts the ghost completion when one is shown, otherwise inserts four spaces as a tab.
    pub(super) fn accept_or_indent(&mut self) {
        if self.ghost.is_some() {
            self.accept_ghost();
        } else {
            for _ in 0..4 {
                self.insert_char(' ');
            }
        }
    }

    /// Scrolls the view so the cursor stays on screen, counting the gutter width in the horizontal scroll.
    pub(super) fn keep_cursor_visible(&mut self, height: usize, width: usize) {
        if self.cy < self.scroll_y {
            self.scroll_y = self.cy;
        } else if self.cy >= self.scroll_y + height {
            self.scroll_y = self.cy - height + 1;
        }
        let gutter = 6;
        if self.cx < self.scroll_x {
            self.scroll_x = self.cx;
        } else if self.cx >= self.scroll_x + width.saturating_sub(gutter) {
            self.scroll_x = self.cx - width.saturating_sub(gutter) + 1;
        }
    }

    /// Diagnostics de la ligne du curseur (gcc + norme, avec messages) —
    /// le survol façon LSP. `true` = erreur, `false` = avertissement.
    pub(super) fn line_diagnostics(&self) -> Vec<(bool, String)> {
        let l = self.cy + 1;
        let mut v: Vec<(bool, String)> = self
            .diags
            .iter()
            .filter(|d| d.line == l)
            .map(|d| (d.is_error, d.message.clone()))
            .collect();
        for (line, sev, msg) in &self.check_details {
            if *line == l {
                v.push((matches!(sev, Severity::Major), msg.clone()));
            }
        }
        // le survol ne répète rien : ni le même message (une règle peut le
        // dire à plusieurs colonnes — deux virgules), ni deux fois la même
        // règle sur la ligne. Une erreur = une ligne, une icône.
        let mut out: Vec<(bool, String)> = Vec::with_capacity(v.len());
        let mut rules: Vec<String> = Vec::new();
        for (err, msg) in v {
            if out.iter().any(|(_, m)| *m == msg) {
                continue;
            }
            match rule_tag(&msg) {
                Some(rule) if rules.contains(&rule) => continue,
                Some(rule) => rules.push(rule),
                None => {}
            }
            out.push((err, msg));
        }
        out
    }

    /// Marqueurs de gouttière par ligne (1-based) — diagnostics gcc et
    /// findings de norme fusionnés : le plus sévère gagne.
    pub(super) fn gutter_marks(&self) -> HashMap<usize, Color> {
        let mut rank: HashMap<usize, (u8, Color)> = HashMap::new();
        let mut put = |line: usize, r: u8, c: Color| {
            rank.entry(line)
                .and_modify(|e| {
                    if r > e.0 {
                        *e = (r, c);
                    }
                })
                .or_insert((r, c));
        };
        for (line, sev) in &self.check_marks {
            match sev {
                Severity::Major => put(*line, 2, Ed::red()),
                Severity::Minor => put(*line, 1, Ed::amber()),
            }
        }
        for line in &self.bootcamp_marks {
            put(*line, 2, Ed::red());
        }
        for d in &self.diags {
            if d.is_error {
                put(d.line, 4, Ed::red());
            } else {
                put(d.line, 3, Ed::amber());
            }
        }
        rank.into_iter().map(|(l, (_, c))| (l, c)).collect()
    }
}
