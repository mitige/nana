//! lancer la build et lire ses erreurs.

use super::*;

impl Editor {
    /// ^b : run the checker the language asks for (gcc, rustc, node --check,
    /// python, luac, bash -n, tsc…) and pour its diagnostics into the gutter.
    pub(super) fn build(&mut self) {
        let Some(path) = self.file.clone() else {
            self.status = "no file — run with: nana <file>".into();
            return;
        };
        if self.modified {
            self.save();
        }
        let lang = langs::for_path(&path);
        let Some(check) = lang.check else {
            self.status = format!("no checker for {} — f5 to run it instead", lang.name);
            return;
        };
        let (tx, rx) = channel();
        crate::guard::spawn_guarded(tx, move || {
            crate::check::run_checker(lang, &path).map(|findings| {
                findings
                    .into_iter()
                    .map(|f| Diag {
                        line: f.line,
                        col: 0,
                        is_error: f.severity == Severity::Major,
                        message: f.message,
                    })
                    .collect::<Vec<_>>()
            })
        });
        self.diag_rx = Some(rx);
        self.status = format!("checking with {}…", check.program);
    }

    /// ^r / f5 : run it. the project's command when the file belongs to a
    /// project nana recognises (cargo run, npm run dev, django…), else the
    /// language's own recipe (python3 x.py, gcc + exec, node x.js…).
    pub(super) fn run_file(&mut self) {
        let Some(path) = self.file.clone() else {
            self.status = "nothing to run — open a file first".into();
            return;
        };
        if self.modified {
            self.save();
        }
        let Some(plan) = crate::run::plan_run(&path) else {
            self.status = format!("nothing to run for {} files", langs::for_path(&path).name);
            return;
        };
        if self.term.is_none() {
            self.toggle_terminal();
        }
        if let Some(term) = self.term.as_mut() {
            let cwd = langs::absolute(&plan.cwd).display().to_string();
            let line = format!("cd {} && {}\n", shell_quote(&cwd), plan.lines.join(" && "));
            term.send(line.as_bytes());
            self.focus = Focus::Terminal;
            self.status = plan.label;
        }
    }

    /// f6 : hand the file to its formatter (the project's, when there is one).
    pub(super) fn run_format(&mut self) {
        let Some(path) = self.file.clone() else {
            self.status = "nothing to format — open a file first".into();
            return;
        };
        let Some(plan) = crate::run::plan_format(&path) else {
            self.status = format!(
                "no formatter known for {} files",
                langs::for_path(&path).name
            );
            return;
        };
        if self.modified {
            self.save();
        }
        if self.term.is_none() {
            self.toggle_terminal();
        }
        if let Some(term) = self.term.as_mut() {
            let cwd = langs::absolute(&plan.cwd).display().to_string();
            let line = format!("cd {} && {}\n", shell_quote(&cwd), plan.lines.join(" && "));
            term.send(line.as_bytes());
            self.focus = Focus::Terminal;
            self.status = format!(
                "{} — ^t reloads the tree, reopen the file to see it",
                plan.label
            );
        }
    }

    /// Collects the diagnostics of a finished background check without blocking, so the editor never waits for the compiler.
    pub(super) fn poll_diag(&mut self) {
        let Some(rx) = &self.diag_rx else {
            return;
        };
        if let Ok(res) = rx.try_recv() {
            self.diag_rx = None;
            match res {
                Ok(diags) => {
                    self.diags = diags;
                    if self.diags.is_empty() {
                        self.diag_idx = None;
                        self.notify(Level::Ok, "clean");
                    } else {
                        let errs = self.diags.iter().filter(|d| d.is_error).count();
                        let warns = self.diags.len() - errs;
                        self.notify(
                            Level::Err,
                            format!("check: {errs} error(s), {warns} note(s) — ^n/^p jumps"),
                        );
                        self.diag_idx = None;
                        self.diag_jump(1); // saute au premier problème
                    }
                }
                Err(e) => self.notify(Level::Err, e),
            }
        }
    }

    /// ^N / ^P : saute au diagnostic suivant / précédent (circulaire).
    pub(super) fn diag_jump(&mut self, dir: i32) {
        if self.diags.is_empty() {
            self.status = "no diagnostic — ^b to build".into();
            return;
        }
        let n = self.diags.len();
        let idx = match self.diag_idx {
            Some(i) => (i as i32 + dir).rem_euclid(n as i32) as usize,
            None => 0,
        };
        self.diag_idx = Some(idx);
        let d = self.diags[idx].clone();
        self.cy = (d.line - 1).min(self.lines.len() - 1);
        let len = self.lines[self.cy].len();
        self.cx = floor_boundary(&self.lines[self.cy], d.col.saturating_sub(1).min(len));
        let kind = if d.is_error { "✗" } else { "⚠" };
        let msg: String = d.message.chars().take(72).collect();
        self.status = format!("{kind} {}/{n} · {}:{} · {msg}", idx + 1, d.line, d.col);
    }
}
