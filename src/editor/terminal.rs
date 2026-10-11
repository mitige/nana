//! the integrated terminal pane: a real pty, read by a thread and drawn from
//! a vt100 screen. the shell is the user's own; nana only frames it.

use std::io;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

/// Le terminal intégré : un vrai PTY (portable-pty) dont la sortie est
/// interprétée par vt100 et dessinée en cellules Minuit.
pub(super) struct TermPane {
    pub(super) parser: vt100::Parser,
    writer: Box<dyn std::io::Write + Send>,
    rx: Receiver<Vec<u8>>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    /// taille PTY courante (lignes, colonnes) — pour ne redimensionner
    /// que quand le panneau change de taille
    size: (u16, u16),
}

impl TermPane {
    /// Starts the shell in a pseudo-terminal of the given size, so the terminal pane is a real tty, and returns the pane or the io error.
    pub(super) fn spawn(cwd: &Path, rows: u16, cols: u16) -> io::Result<Self> {
        let pty = portable_pty::native_pty_system();
        let pair = pty
            .openpty(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io::Error::other)?;
        // fish d'abord : c'est le shell de foot — le panneau F3 doit parler
        // le même shell que le dehors. Sinon, celui de l'utilisateur.
        let shell = ["/usr/bin/fish", "/usr/local/bin/fish"]
            .into_iter()
            .find(|p| std::path::Path::new(p).exists())
            .map(str::to_string)
            .unwrap_or_else(|| {
                std::env::var("SHELL").unwrap_or_else(|_| {
                    if cfg!(windows) {
                        "powershell.exe".to_string()
                    } else {
                        "/bin/sh".to_string()
                    }
                })
            });
        let mut cmd = portable_pty::CommandBuilder::new(shell);
        cmd.cwd(cwd);
        let child = pair.slave.spawn_command(cmd).map_err(io::Error::other)?;
        let mut reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pair.master.take_writer().map_err(io::Error::other)?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        // le slave ne sert plus : le shell tourne, le master suffit
        drop(pair.slave);
        Ok(Self {
            parser: vt100::Parser::new(rows, cols, 0),
            writer,
            rx,
            _master: pair.master,
            _child: child,
            size: (rows, cols),
        })
    }

    /// Verse la sortie arrivée depuis le dernier rendu, et répond aux
    /// requêtes du programme qui tourne dedans.
    pub(super) fn poll(&mut self) {
        while let Ok(chunk) = self.rx.try_recv() {
            self.parser.process(&chunk);
            self.answer_queries(&chunk);
        }
    }

    /// A real terminal answers what it is asked. fish, starship and friends
    /// query device attributes, the background colour and the palette at
    /// startup and *wait* for the reply — with no answer the pane looks
    /// frozen. the reply building is pure, so it can be tested.
    pub(super) fn answer_queries(&mut self, chunk: &[u8]) {
        let s = String::from_utf8_lossy(chunk);
        let reply = terminal_replies(&s);
        if !reply.is_empty() {
            self.send(reply.as_bytes());
        }
    }

    /// Resizes the pseudo-terminal and its parser together, ignoring zero and unchanged sizes so a redraw does not reset the shell.
    pub(super) fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        self.parser.set_size(rows, cols);
        let _ = self._master.resize(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// Writes raw bytes to the shell and flushes them, ignoring write errors so a dead shell does not crash the editor.
    pub(super) fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }
}

/// the reply to every terminal query worth answering, or an empty string.
pub(super) fn terminal_replies(s: &str) -> String {
    let mut reply = String::new();
    // primary / secondary device attributes
    if s.contains("\x1b[0c") || s.contains("\x1b[c") {
        reply.push_str("\x1b[?62;1;2;6;9;15;18;21;22c");
    }
    if s.contains("\x1b[>0c") || s.contains("\x1b[>c") {
        reply.push_str("\x1b[>1;4000;0c");
    }
    // the kitty keyboard protocol: no flags are pushed here, say so
    if s.contains("\x1b[?u") {
        reply.push_str("\x1b[?0u");
    }
    // colours: the minuit palette, so the shell's own colours match the editor
    if s.contains("\x1b]11;?") {
        reply.push_str("\x1b]11;rgb:1313/1313/1313\x1b\\");
    }
    if s.contains("\x1b]10;?") {
        reply.push_str("\x1b]10;rgb:e2e2/e2e2/e2e2\x1b\\");
    }
    for (i, rgb) in MINUIT_PALETTE.iter().enumerate() {
        if s.contains(&format!("\x1b]4;{i};?")) {
            reply.push_str(&format!("\x1b]4;{i};rgb:{rgb}\x1b\\"));
        }
    }
    if s.contains("\x1b[6n") {
        reply.push_str("\x1b[1;1R");
    }
    // XTGETTCAP (DCS + q …) is deliberately left unanswered: shells that ask
    // it fall back on their own after a short timeout, and a forged reply
    // arriving before they read raw input gets echoed as typed text — a worse
    // failure than a moment of patience.
    reply
}

/// the sixteen base colours nana hands to the shell running in its pane —
/// the same "minuit" family the editor itself draws with.
pub(super) const MINUIT_PALETTE: [&str; 16] = [
    "24262e", "f07886", "9bdc91", "edca80", "91b9ff", "d9a9ff", "7fd3e0", "e2e2e2", "5b6070",
    "ff8fa3", "b8e6a8", "ffd9a0", "a8c8ff", "e8c4ff", "a5e6ef", "f5f5f5",
];
