//! the typescript bootcamp key: F7 runs tsc then ts-node on the current file in
//! the terminal, so the errors are read where the student already reads them.

use super::*;

impl Editor {
    /// Puts a gutter mark on every line that uses `any` or `var`, the words the
    /// subject forbids. It reads the buffer as it is, so a fix clears the mark
    /// on the next refresh.
    pub(super) fn refresh_bootcamp_marks(&mut self) {
        let is_bootcamp_file = self
            .file
            .as_deref()
            .is_some_and(crate::bootcamp::applies_to);
        if !is_bootcamp_file {
            self.bootcamp_marks.clear();
            return;
        }
        let text = self.lines.join("\n");
        self.bootcamp_marks = crate::bootcamp::forbidden_word_lines(&text)
            .into_iter()
            .map(|(line, _)| line)
            .collect();
    }

    /// F7: runs the bootcamp check (tsc then ts-node) on the current typescript
    /// file in the background, so the verdict lands in the status line and the
    /// terminal stays as the student left it. It saves the file first.
    pub(super) fn bootcamp_check(&mut self) {
        let Some(path) = self.file.clone() else {
            self.status = "bootcamp: open a .ts file first".into();
            return;
        };
        if self.modified {
            self.save();
        }
        let (tx, rx) = channel();
        let file = path.display().to_string();
        crate::guard::spawn_guarded(tx, move || Ok(crate::bootcamp::run_typescript_steps(&file)));
        self.bootcamp_rx = Some(rx);
        self.status = "bootcamp: checking with tsc then ts-node...".into();
    }

    /// Takes the verdict of a finished bootcamp check without blocking and puts
    /// it in the status line: the first error the compiler or ts-node printed,
    /// or that the file passes.
    pub(super) fn poll_bootcamp(&mut self) {
        let Some(rx) = &self.bootcamp_rx else {
            return;
        };
        if let Ok(res) = rx.try_recv() {
            self.bootcamp_rx = None;
            self.status = match res {
                Ok(verdict) => format!("bootcamp: {verdict}"),
                Err(e) => format!("bootcamp: {e}"),
            };
        }
    }

    /// Reads the `package.json` beside the current file and puts the first
    /// departure from the subject in the status line, or says the folder is in
    /// order. Without a package.json the student is told to run npm init.
    pub(super) fn check_bootcamp_package(&mut self) {
        let Some(dir) = self.file.as_deref().and_then(|p| p.parent()) else {
            self.status = "bootcamp: open a file in an exercise folder first".into();
            return;
        };
        let Ok(text) = std::fs::read_to_string(dir.join("package.json")) else {
            self.status = "bootcamp: no package.json here — run npm init -y first".into();
            return;
        };
        let mut findings = crate::bootcamp::package_findings(&text);
        let tsconfig = std::fs::read_to_string(dir.join("tsconfig.json")).unwrap_or_default();
        findings.extend(crate::bootcamp::tsconfig_findings(&tsconfig));
        self.status = match findings.first() {
            None => "bootcamp: package.json and tsconfig.json are in order".into(),
            Some(finding) => format!("bootcamp: {}", crate::bootcamp::describe(finding)),
        };
    }

    /// F9: writes the tsconfig.json and .gitignore templates into the folder of
    /// the current file, keeping any file already there. The status line says
    /// which files were written, or that the folder already had them all.
    pub(super) fn insert_bootcamp_templates(&mut self) {
        let Some(dir) = self
            .file
            .as_deref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
        else {
            self.status = "bootcamp: open a file in an exercise folder first".into();
            return;
        };
        self.status = match crate::bootcamp::write_templates(&dir) {
            Ok(names) if names.is_empty() => "bootcamp: the templates are already here".into(),
            Ok(names) => format!("bootcamp: wrote {}", names.join(", ")),
            Err(e) => format!("bootcamp: could not write the templates: {e}"),
        };
    }
}
