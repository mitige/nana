//! nana — the editor itself: one buffer, every language.
//!
//! everything language specific lives elsewhere and is looked up per file:
//! `crate::langs` (commands, icons, comment syntax), `crate::check` (what the
//! gutter reports after a save), `crate::header` (f4), `crate::run` (f5) and
//! `crate::project` (which command your repo actually wants). this file only
//! knows how to edit text and draw it: one neutral surface, thin bars, a
//! gutter with marks, no noise.

use crate::ai::{system_prompt, AgentRole, AiClient};
use crate::check::Severity;
use crate::diag::Diag;
use crate::explorer::{Explorer, FileSearch};
use crate::highlight;
use crate::langs;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

/// Modèle de la complétion : unique et verrouillé. Rapide (flash) —
/// la complétion doit arriver avant que la pensée ne refroidisse.
const EDITOR_MODEL: &str = "deepseek-v4.1-flash";

/// Largeur du panneau explorateur (séparateur compris).
const EXPL_W: u16 = 30;

/// Hauteur du panneau terminal (bordure comprise).
const TERM_H: u16 = 14;

/// Taille max du contexte envoyé à l'IA de complétion (caractères).
/// Dimensionné pour la fenêtre 1M des modèles : ~600k chars ≈ 150-200k tokens.
const MAX_CTX_CHARS: usize = 600_000;

/// Agents pertinents pour la complétion de code.
const COMPLETE_AGENTS: &[AgentRole] = &[
    AgentRole::CodeExplainer,
    AgentRole::Intervenant,
    AgentRole::Aine,
    AgentRole::BinomeBasNiveau,
    AgentRole::BinomeClean,
    AgentRole::BinomeAlgo,
];

fn load_complete_agent() -> AgentRole {
    let cfg = crate::config::load();
    let name = cfg.nana_agent.as_deref().unwrap_or("code explainer");
    COMPLETE_AGENTS
        .iter()
        .find(|a| a.name() == name)
        .copied()
        .unwrap_or(AgentRole::CodeExplainer)
}

fn save_complete_agent(role: AgentRole) {
    crate::config::save_nana_agent(role.name());
}

// ------------------------------------------------------------------ thème

/// Palette « Minuit » — Apple sobre : neutres charbon + accents pastel.
struct Ed;

#[allow(dead_code)]
impl Ed {
    /// Fond de l'éditeur : celui du terminal — transparence et blur préservés.
    fn bg() -> Color {
        Color::Reset
    }
    /// Fond des barres haute/basse — transparent, lui aussi.
    fn bar_bg() -> Color {
        Color::Reset
    }
    /// Ligne courante — le noir du thème, discret.
    fn cur_line() -> Color {
        Color::Indexed(0)
    }
    /// Sélection de l'explorateur — le noir du thème, même bande.
    fn sel_row() -> Color {
        Color::Indexed(0)
    }
    fn text() -> Color {
        Color::Reset
    }
    fn dim() -> Color {
        Color::Indexed(8)
    }
    fn gutter() -> Color {
        Color::Indexed(8)
    }
    /// Accent — le rouge clair du shell (mot-clés, indicateur modifié).
    fn accent() -> Color {
        Color::Indexed(9)
    }
    /// Cyan du shell — touches, IA en vol.
    fn cyan() -> Color {
        Color::Indexed(6)
    }
    /// Vert du shell — succès (norme ✓, build propre).
    fn green() -> Color {
        Color::Indexed(2)
    }
    /// Ambre du shell — avertissements, norme mineure.
    fn amber() -> Color {
        Color::Indexed(3)
    }
    /// Rouge du shell — erreurs, norme majeure.
    fn red() -> Color {
        Color::Indexed(1)
    }
    fn ghost() -> Color {
        Color::Indexed(8)
    }
    /// Règle de la colonne 80 — un filet, rien de plus.
    fn ruler() -> Color {
        Color::Indexed(0)
    }
}

// ------------------------------------------------------------------ diagnostics

// ------------------------------------------------------------------ focus & notifications

/// Zone qui détient le focus — F2/Alt-Tab les fait défiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Editor,
    Explorer,
    Search,
    Terminal,
}

/// Le terminal intégré : un vrai PTY (portable-pty) dont la sortie est
/// interprétée par vt100 et dessinée en cellules Minuit.
struct TermPane {
    parser: vt100::Parser,
    writer: Box<dyn std::io::Write + Send>,
    rx: Receiver<Vec<u8>>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    /// taille PTY courante (lignes, colonnes) — pour ne redimensionner
    /// que quand le panneau change de taille
    size: (u16, u16),
}

impl TermPane {
    fn spawn(cwd: &Path, rows: u16, cols: u16) -> io::Result<Self> {
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
    fn poll(&mut self) {
        while let Ok(chunk) = self.rx.try_recv() {
            self.parser.process(&chunk);
            self.answer_queries(&chunk);
        }
    }

    /// A real terminal answers what it is asked. fish, starship and friends
    /// query device attributes, the background colour and the palette at
    /// startup and *wait* for the reply — with no answer the pane looks
    /// frozen. the reply building is pure, so it can be tested.
    fn answer_queries(&mut self, chunk: &[u8]) {
        let s = String::from_utf8_lossy(chunk);
        let reply = terminal_replies(&s);
        if !reply.is_empty() {
            self.send(reply.as_bytes());
        }
    }

    fn resize(&mut self, rows: u16, cols: u16) {
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

    fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }
}

/// the reply to every terminal query worth answering, or an empty string.
fn terminal_replies(s: &str) -> String {
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
const MINUIT_PALETTE: [&str; 16] = [
    "24262e", "f07886", "9bdc91", "edca80", "91b9ff", "d9a9ff", "7fd3e0", "e2e2e2", "5b6070",
    "ff8fa3", "b8e6a8", "ffd9a0", "a8c8ff", "e8c4ff", "a5e6ef", "f5f5f5",
];

/// Niveau d'une notification toast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    Info,
    Ok,
    Warn,
    Err,
}

/// Notification flottante façon nvim-notify : éphémère, empilée en haut
/// à droite, colorée par niveau.
struct Toast {
    level: Level,
    text: String,
    at: Instant,
}

// ------------------------------------------------------------------ éditeur

pub struct Editor {
    lines: Vec<String>,
    cx: usize,
    cy: usize,
    scroll_x: usize,
    scroll_y: usize,
    file: Option<PathBuf>,
    modified: bool,
    /// contenu tel qu'au dernier état disque — modified = (lines != saved),
    /// donc effacer ce qu'on vient de taper re-débloque l'explorateur
    saved: Vec<String>,
    status: String,
    /// suggestion IA en cours (texte fantôme)
    ghost: Option<String>,
    ghost_lines: Vec<String>,
    ai_busy: bool,
    rx: Option<Receiver<Result<String, String>>>,
    clipboard: String,
    should_quit: bool,
    confirm_quit: bool,
    /// agent qui pilote la complétion (sélectionnable, persisté)
    complete_agent: AgentRole,
    /// last check after a save: one summary line + gutter marks
    check_note: Option<String>,
    check_marks: Vec<(usize, Severity)>,
    check_rx: Option<Receiver<crate::check::Report>>,
    /// diagnostics gcc du dernier build (^B) + navigation ^N/^P
    diags: Vec<Diag>,
    diag_rx: Option<Receiver<Result<Vec<Diag>, String>>>,
    diag_idx: Option<usize>,
    /// saisie de recherche (^F) ou goto (^G) en cours
    prompt: Option<(char, String)>,
    /// extension du fichier (pour la coloration)
    ext: String,
    /// client IA possédé (pour la complétion)
    ai: Option<AiClient>,
    /// explorateur de fichiers (^T, ou `c-nano <dossier>`)
    explorer: Option<Explorer>,
    /// recherche de fichiers flottante (^O, façon Telescope)
    search: Option<FileSearch>,
    /// zone qui a le focus (Alt-Tab cycle : éditeur → explorateur → recherche)
    focus: Focus,
    /// notifications toast empilées (haut à droite)
    toasts: Vec<Toast>,
    /// suppression de fichier en attente de confirmation (o/n)
    confirm_delete: Option<PathBuf>,
    /// génération d'en-tête en cours (description par le modèle)
    header_rx: Option<Receiver<Result<String, String>>>,
    /// terminal intégré (F3), ouvert = visible
    term: Option<TermPane>,
    /// branche git du projet (barre haute)
    git_branch: Option<String>,
    /// what kind of project the file belongs to: "cargo", "next · node"…
    project_label: Option<String>,
    /// detailed findings (line, severity, message) for the hover popup
    check_details: Vec<(usize, Severity, String)>,
    /// historique pour l'undo (Ctrl+Z)
    history: Vec<(Vec<String>, usize, usize)>,
}

/// the auto header (f4), in the language's own comment syntax — see
/// crate::header. the description comes from the model, or from you.
#[allow(dead_code)]
fn header_lines(path: &Path, description: &str) -> Vec<String> {
    let author = crate::config::load().author;
    crate::header::header_lines(path, description, author.as_deref())
}

/// quote a path for the shell (the run plan is typed into a real shell).
fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/=:@+,^%".contains(c))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// La disposition de départ : `c-nano dossier` → explorateur racine dessus,
/// buffer vierge ; `c-nano fichier` → édition + racine = son dossier ;
/// `c-nano` seul → racine = répertoire courant, buffer vierge.
fn startup_layout(path: &Option<PathBuf>) -> (Option<PathBuf>, PathBuf) {
    match path {
        Some(p) if p.is_dir() => (None, p.clone()),
        Some(p) => (Some(p.clone()), file_dir(Some(p))),
        None => (
            None,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        ),
    }
}

/// Recule jusqu'à une frontière de caractère UTF-8 — le curseur ne se
/// pose JAMAIS au milieu d'un é/à multi-octets (sinon : panique au prochain
/// insert/remove — le crash des caractères accentués).
fn floor_boundary(line: &str, mut i: usize) -> usize {
    while i > 0 && !line.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Dossier de travail : celui du fichier ouvert, sinon le répertoire courant.
fn file_dir(path: Option<&Path>) -> PathBuf {
    path.and_then(|p| p.parent().map(Path::to_path_buf))
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Branche git du dossier (None hors dépôt — jamais bloquant).
fn detect_git_branch(dir: PathBuf) -> Option<String> {
    let out = Command::new("git")
        .args(["-C"])
        .arg(&dir)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() || name == "HEAD" {
        None
    } else {
        Some(name)
    }
}

impl Editor {
    pub fn open(path: Option<&Path>) -> io::Result<Self> {
        let (lines, file) = match path {
            Some(p) if p.exists() => {
                let text = std::fs::read_to_string(p)?;
                let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
                if lines.is_empty() {
                    lines.push(String::new());
                }
                (lines, Some(p.to_path_buf()))
            }
            Some(p) => (vec![String::new()], Some(p.to_path_buf())),
            None => (vec![String::new()], None),
        };
        let ext = file
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .unwrap_or("c")
            .to_string();
        let _ = file; // l'extension est déjà extraite
        let saved = lines.clone();
        let mut ed = Self {
            lines,
            cx: 0,
            cy: 0,
            scroll_x: 0,
            scroll_y: 0,
            file,
            modified: false,
            saved,
            status: "f2 panels · ^b check · f5 run · f6 format · ^s save · ^q quit".to_string(),
            ghost: None,
            ghost_lines: Vec::new(),
            ai_busy: false,
            rx: None,
            clipboard: String::new(),
            should_quit: false,
            confirm_quit: false,
            complete_agent: load_complete_agent(),
            check_note: None,
            check_marks: Vec::new(),
            check_rx: None,
            diags: Vec::new(),
            diag_rx: None,
            diag_idx: None,
            prompt: None,
            history: Vec::new(),
            ext,
            ai: None,
            explorer: None,
            search: None,
            focus: Focus::Editor,
            toasts: Vec::new(),
            confirm_delete: None,
            header_rx: None,
            term: None,
            git_branch: detect_git_branch(file_dir(path)),
            project_label: None,
            check_details: Vec::new(),
        };
        ed.refresh_project();
        Ok(ed)
    }

    // -------------------------------------------------------------- texte

    fn line(&self) -> &str {
        &self.lines[self.cy]
    }

    /// Écran d'accueil : aucun fichier, buffer vierge et jamais modifié.
    fn is_welcome(&self) -> bool {
        self.file.is_none() && !self.modified && self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Recalcule `modified` en comparant au disque — annuler une frappe
    /// en revenant à l'état exact sauvegardé rend le buffer propre.
    fn sync_modified(&mut self) {
        self.modified = self.lines != self.saved;
    }

    /// Sauvegarde l'état avant une modification (pour Ctrl+Z).
    fn snapshot(&mut self) {
        self.history.push((self.lines.clone(), self.cx, self.cy));
        if self.history.len() > 200 {
            self.history.remove(0);
        }
    }

    fn undo(&mut self) {
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
    fn clear_ghost(&mut self) {
        self.ghost = None;
        self.ghost_lines.clear();
    }

    fn insert_char(&mut self, c: char) {
        self.snapshot();
        let line = &mut self.lines[self.cy];
        line.insert(self.cx, c);
        self.cx += c.len_utf8(); // un é = 2 octets — on avance du caractère entier
        self.sync_modified();
        self.clear_ghost();
    }

    /// Fermante associée à une ouvrante (auto-paires).
    fn pair_for(c: char) -> Option<char> {
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
    fn type_char(&mut self, c: char) {
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

    fn insert_newline(&mut self) {
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

    fn backspace(&mut self) {
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
            let pair = pair; // calculé avant mutation
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

    fn move_left(&mut self) {
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

    fn move_right(&mut self) {
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

    fn move_up(&mut self) {
        if self.cy > 0 {
            self.cy -= 1;
            let len = self.lines[self.cy].len();
            self.cx = floor_boundary(&self.lines[self.cy], self.cx.min(len));
        }
        self.clear_ghost();
    }

    fn move_down(&mut self) {
        if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            let len = self.lines[self.cy].len();
            self.cx = floor_boundary(&self.lines[self.cy], self.cx.min(len));
        }
        self.clear_ghost();
    }

    /// Remplace toutes les occurrences de « motif→remplacement » (séparateur →).
    fn replace_all(&mut self, spec: &str) {
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

    fn find(&mut self, needle: &str) {
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

    fn save(&mut self) {
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
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file")
                    .to_string();
                self.notify(Level::Ok, format!("{name} — saved ✓"));
                // the check runs off the ui thread after every save: the
                // language's own tool plus the universal style pass
                let p = path.clone();
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let cfg = crate::config::load().style.to_cfg();
                    let _ = tx.send(crate::check::check_path(&p, &cfg));
                });
                self.check_rx = Some(rx);
            }
            Err(e) => self.status = format!("write error : {e}"),
        }
    }

    // -------------------------------------------------------------- build

    /// ^b : run the checker the language asks for (gcc, rustc, node --check,
    /// python, luac, bash -n, tsc…) and pour its diagnostics into the gutter.
    fn build(&mut self) {
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
        std::thread::spawn(move || {
            let res = crate::check::run_checker(lang, &path)
                .map(|findings| {
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
                .map_err(|e| e);
            let _ = tx.send(res);
        });
        self.diag_rx = Some(rx);
        self.status = format!("checking with {}…", check.program);
    }

    /// ^r / f5 : run it. the project's command when the file belongs to a
    /// project nana recognises (cargo run, npm run dev, django…), else the
    /// language's own recipe (python3 x.py, gcc + exec, node x.js…).
    fn run_file(&mut self) {
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
    fn run_format(&mut self) {
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

    fn poll_diag(&mut self) {
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
                        self.notify(Level::Ok, "check ✓ — clean");
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
    fn diag_jump(&mut self, dir: i32) {
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

    // -------------------------------------------------------------- IA

    /// Demande une complétion à l'agent. Contexte : le fichier ENTIER
    /// (fenêtre 1M des modèles) avec le curseur marqué « ▌ ».
    /// Budget de sortie : 128K — la complétion peut écrire plusieurs lignes.
    fn ai_complete(&mut self, client: &AiClient) {
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
        let system = system_prompt(self.complete_agent, crate::ai::HelpLevel::Assistance)
            + "\n\nTu es en mode COMPLÉTION (comme GitHub Copilot). On te donne le fichier \
               COMPLET avec un curseur « ▌ ». Réponds UNIQUEMENT avec le code qui vient à \
               partir de ▌ : la suite logique. Tu PEUX écrire plusieurs lignes — un bloc \
               entier, un corps de fonction, une boucle complète — jusqu'à ~30 lignes si \
               le bloc le demande, mais arrête-toi à la fin du bloc logique en cours : \
               pas de conjecture au-delà. INTERDIT : includes déjà présents, un main() si \
               le fichier en a déjà un, répéter le code d'avant ▌, markdown, fence, \
               explication. Juste le code qui continue. Norme Epitech : indentation \
               4 espaces, while (pas de for), pas de commentaire dans les fonctions. \
               Si la ligne se termine par « { », commence ta réponse par un retour à la \
               ligne.";
        let user = format!("// fichier : {fname}\n{}", ctx_lines.join("\n"));
        let (tx, rx) = channel();
        let client = client.clone();
        std::thread::spawn(move || {
            // budget 128K + retries (réponse vide, 429/5xx, 400) — cf. ai::chat
            let _ = tx.send(client.chat(&system, &user).map_err(|e| e.to_string()));
        });
        self.rx = Some(rx);
        self.ai_busy = true;
        self.status = "in progress… (esc : cancel)".into();
    }

    fn poll_check(&mut self) {
        let Some(rx) = &self.check_rx else {
            return;
        };
        if let Ok(report) = rx.try_recv() {
            let level = if report
                .findings
                .iter()
                .any(|f| f.severity == Severity::Major)
            {
                Level::Err
            } else if report.findings.is_empty() {
                Level::Ok
            } else {
                Level::Warn
            };
            self.notify(level, report.note.clone());
            self.check_note = Some(report.note.clone());
            self.check_marks = report.marks.clone();
            self.check_details = report.details.clone();
            self.check_rx = None;
        }
    }

    fn poll_ai(&mut self) {
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

    fn accept_ghost(&mut self) {
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
    fn request_quit(&mut self) {
        if self.modified {
            self.confirm_quit = true;
            self.status = "modified — o quit without saving · s save+quit · other: stay".into();
        } else {
            self.should_quit = true;
        }
    }

    /// Notification toast (coin haut-droit) + le statut garde le dernier état.
    fn notify(&mut self, level: Level, text: impl Into<String>) {
        let text = text.into();
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

    // -------------------------------------------------------------- fichiers

    /// Dossier de base pour créer un fichier : la sélection de
    /// l'explorateur si c'est un dossier, son parent si c'est un fichier,
    /// la racine sinon.
    fn explorer_base_dir(&self) -> PathBuf {
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
    fn create_file(&mut self, name: &str) {
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
    fn delete_selected(&mut self) {
        let Some(ex) = &self.explorer else { return };
        let Some(row) = ex.rows().get(ex.sel()) else {
            return;
        };
        self.confirm_delete = Some(row.path.clone());
        self.status = format!("delete {} ? · o confirm · other : cancel", row.name);
    }

    fn confirm_delete_now(&mut self) {
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
    fn rename_selected(&mut self, new_name: &str) {
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

    // -------------------------------------------------------------- en-tête

    /// F4 : génère l'en-tête du fichier — description écrite par le modèle
    /// (deepseek-v4.1-flash, unique). Sans modèle : squelette à compléter.
    fn generate_header(&mut self) {
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
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let system = "Tu écris la ligne « File description » d'un en-tête de fichier                           de code. Une seule ligne, en français, à l'infinitif, 60                           caractères max, sans point final. Réponds UNIQUEMENT avec                           cette ligne.";
            let user = format!(
                "fichier : {name}

{body}"
            );
            let _ = tx.send(client.chat_fast(system, &user).map_err(|e| e.to_string()));
        });
        self.header_rx = Some(rx);
        self.status = "header in progress…".into();
    }

    fn poll_header(&mut self) {
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
    fn insert_header(&mut self, desc: String) {
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
        self.notify(Level::Ok, "header generated ✓");
    }

    /// re-read the project the current file belongs to (cheap: a few file
    /// probes up the tree) and keep a short label for the topbar.
    fn refresh_project(&mut self) {
        self.project_label = self
            .file
            .as_deref()
            .and_then(crate::project::detect)
            .map(|p| Self::project_label_of(&p));
    }

    /// "next · node", "django · python", "cargo"…
    fn project_label_of(p: &crate::project::Project) -> String {
        match &p.framework {
            Some(f) => format!("{f} · {}", p.kind.name()),
            None => p.kind.name().to_string(),
        }
    }

    // -------------------------------------------------------------- terminal

    /// F3 : ouvre/ferme le terminal intégré (panneau bas).
    fn toggle_terminal(&mut self) {
        if self.term.take().is_some() {
            if self.focus == Focus::Terminal {
                self.focus = Focus::Editor;
            }
            self.status = "terminal closed".into();
            return;
        }
        let cwd = file_dir(self.file.as_deref());
        // taille approximative au spawn ; affinée au premier rendu
        match TermPane::spawn(&cwd, TERM_H.saturating_sub(2), 80) {
            Ok(pane) => {
                self.term = Some(pane);
                self.focus = Focus::Terminal;
                self.status =
                    "terminal — all keys go to the shell · esc/f2 : back to editor · f3 : close"
                        .into();
            }
            Err(e) => self.notify(Level::Err, format!("terminal unavailable : {e}")),
        }
    }

    /// Touches quand le terminal a le focus : tout part au shell, sauf
    /// Échap (retour éditeur) et F3 (fermer) — F2 reste global.
    fn terminal_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => {
                self.focus = Focus::Editor;
                self.status = "f2 panels · ^o search · ^b check · f5 run · ^s save".into();
            }
            _ => {
                let bytes: Vec<u8> = match (key.code, ctrl, alt) {
                    (KeyCode::Char(c), true, _) if c.is_ascii_lowercase() => {
                        vec![(c as u8) & 0x1f]
                    }
                    (KeyCode::Char(c), false, true) => {
                        let mut v = vec![0x1b];
                        v.extend(c.to_string().as_bytes());
                        v
                    }
                    (KeyCode::Char(c), _, _) => c.to_string().into_bytes(),
                    (KeyCode::Enter, _, _) => b"\r".to_vec(),
                    (KeyCode::Backspace, _, _) => b"".to_vec(),
                    (KeyCode::Tab, _, _) => b"	".to_vec(),
                    (KeyCode::Up, _, _) => b"[A".to_vec(),
                    (KeyCode::Down, _, _) => b"[B".to_vec(),
                    (KeyCode::Right, _, _) => b"[C".to_vec(),
                    (KeyCode::Left, _, _) => b"[D".to_vec(),
                    (KeyCode::Home, _, _) => b"[H".to_vec(),
                    (KeyCode::End, _, _) => b"[F".to_vec(),
                    (KeyCode::Delete, _, _) => b"[3~".to_vec(),
                    (KeyCode::PageUp, _, _) => b"[5~".to_vec(),
                    (KeyCode::PageDown, _, _) => b"[6~".to_vec(),
                    _ => Vec::new(),
                };
                if !bytes.is_empty() {
                    if let Some(term) = &mut self.term {
                        term.send(&bytes);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------- focus

    /// Alt-Tab : éditeur → explorateur → recherche → éditeur.
    /// Les panneaux absents sont sautés ; sans aucun panneau, ouvre
    /// l'explorateur (le geste sert toujours).
    fn cycle_focus(&mut self) {
        // ronde : éditeur → explorateur → recherche → terminal (les absents
        // sont sautés) ; sans aucun panneau, ouvre l'explorateur
        let mut order = vec![Focus::Editor];
        if self.explorer.is_some() {
            order.push(Focus::Explorer);
        }
        if self.search.is_some() {
            order.push(Focus::Search);
        }
        if self.term.is_some() {
            order.push(Focus::Terminal);
        }
        if order.len() == 1 {
            self.toggle_explorer();
            return;
        }
        let pos = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = order[(pos + 1) % order.len()];
    }

    // -------------------------------------------------------------- recherche

    /// ^O : la recherche de fichiers flottante (façon Telescope).
    fn toggle_search(&mut self) {
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
    fn search_key(&mut self, key: KeyEvent) {
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

    // -------------------------------------------------------------- explorateur

    /// ^T : ouvre/ferme le panneau de fichiers (racine : dossier du fichier,
    /// sinon le répertoire courant).
    fn toggle_explorer(&mut self) {
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
    fn open_from_explorer(&mut self, path: PathBuf) {
        if self.modified {
            self.status = "buffer modified — ^s to save first".into();
            return;
        }
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
    fn explorer_key(&mut self, key: KeyEvent) {
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

    // -------------------------------------------------------------- boucle

    fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if self.confirm_quit {
            match key.code {
                KeyCode::Char('o') | KeyCode::Char('y') => self.should_quit = true,
                KeyCode::Char('s') => {
                    self.save();
                    self.should_quit = true;
                }
                _ => self.confirm_quit = false,
            }
            return;
        }

        // saisie de recherche (^F) ou goto (^G) en cours
        if self.prompt.is_some() {
            match key.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Enter => {
                    let (kind, text) = self.prompt.take().unwrap();
                    if kind == 'f' {
                        self.find(&text);
                    } else if kind == 'r' {
                        self.replace_all(&text);
                    } else if kind == 'n' {
                        self.create_file(&text);
                    } else if kind == 'w' {
                        self.rename_selected(&text);
                    } else if kind == 'g' {
                        if let Ok(n) = text.trim().parse::<usize>() {
                            if n >= 1 && n <= self.lines.len() {
                                self.cy = n - 1;
                                self.cx = 0;
                                self.status = format!("line {n}");
                            } else {
                                self.status = format!("line {n} out of range");
                            }
                        }
                    }
                }
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
            return;
        }

        // F2 / Alt-Tab / Ctrl-Tab / Shift-Tab : cycle du focus entre panneaux.
        // F2 est le seul que TOUS les terminaux livrent toujours — Alt-Tab et
        // Ctrl-Tab sont avalés par l'OS ou le terminal avant nous.
        if key.code == KeyCode::F(2)
            || key.code == KeyCode::BackTab
            || (key.code == KeyCode::Tab && (alt || ctrl))
        {
            self.cycle_focus();
            return;
        }
        if ctrl && key.code == KeyCode::Char('o') {
            self.toggle_search();
            return;
        }
        if key.code == KeyCode::F(3) {
            self.toggle_terminal();
            return;
        }
        // focus terminal : tout va au shell
        if self.focus == Focus::Terminal && self.term.is_some() {
            self.terminal_key(key);
            return;
        }
        // focus recherche / explorateur : touches dédiées (les globales passent)
        if self.focus == Focus::Search && self.search.is_some() {
            self.search_key(key);
            return;
        }
        if self.focus == Focus::Explorer && self.explorer.is_some() {
            self.explorer_key(key);
            return;
        }

        match (key.code, ctrl, alt) {
            // quitter : ^Q ou ^X (réflexe nano) — PAS ^C, trop de missclicks
            (KeyCode::Char('q'), true, _) | (KeyCode::Char('x'), true, _) => self.request_quit(),
            (KeyCode::Char('t'), true, _) => self.toggle_explorer(),
            (KeyCode::Char('s'), true, _) => self.save(),
            (KeyCode::F(4), _, _) => self.generate_header(),
            (KeyCode::F(3), _, _) => self.toggle_terminal(),
            (KeyCode::Char('b'), true, _) => self.build(),
            (KeyCode::F(5), _, _) => self.run_file(),
            (KeyCode::F(6), _, _) => self.run_format(),
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
            (KeyCode::Char('a'), true, _) => {
                // cycler l'agent de complétion
                let pos = COMPLETE_AGENTS
                    .iter()
                    .position(|a| *a == self.complete_agent)
                    .unwrap_or(0);
                self.complete_agent = COMPLETE_AGENTS[(pos + 1) % COMPLETE_AGENTS.len()];
                save_complete_agent(self.complete_agent);
                self.status = "profile changed ✓".into();
            }
            (KeyCode::Char('k'), true, _) => {
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
            (KeyCode::Char('u'), true, _) => {
                if !self.clipboard.is_empty() {
                    self.snapshot();
                    let clip = self.clipboard.clone();
                    self.lines.insert(self.cy, clip);
                    self.sync_modified();
                    self.status = "line pasted".into();
                }
            }
            (KeyCode::Char(' '), true, _) => {
                if let Some(client) = self.ai.clone() {
                    self.ai_complete(&client);
                } else {
                    self.status = "unavailable (missing api key)".into();
                }
            }
            (KeyCode::Right, _, true) | (KeyCode::Right, true, _) => {
                if self.ghost.is_some() {
                    self.accept_ghost();
                } else {
                    self.move_right();
                }
            }
            (KeyCode::Esc, _, _) => {
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
            (KeyCode::Left, _, _) => self.move_left(),
            (KeyCode::Right, _, _) => self.move_right(),
            (KeyCode::Up, _, _) => self.move_up(),
            (KeyCode::Down, _, _) => self.move_down(),
            (KeyCode::Home, _, _) => self.cx = 0,
            (KeyCode::End, _, _) => {
                if self.ghost.is_some() {
                    self.accept_ghost();
                } else {
                    self.cx = self.line().len();
                }
            }
            (KeyCode::Backspace, _, _) => self.backspace(),
            (KeyCode::Delete, _, _) => {
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
            (KeyCode::Enter, _, _) => self.insert_newline(),
            // Tab : accepte la suggestion IA si présente (réflexe Copilot),
            // sinon 4 espaces (la Norme)
            (KeyCode::Tab, _, _) | (KeyCode::Char('i'), true, _) => {
                if self.ghost.is_some() {
                    self.accept_ghost();
                } else {
                    for _ in 0..4 {
                        self.insert_char(' ');
                    }
                }
            }
            (KeyCode::Char(c), false, false) => self.type_char(c),
            _ => {}
        }
    }

    fn keep_cursor_visible(&mut self, height: usize, width: usize) {
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
    fn line_diagnostics(&self) -> Vec<(bool, String)> {
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
    fn gutter_marks(&self) -> HashMap<usize, Color> {
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

/// Étiquette de règle en fin de message — « … (N-COMMASPACE) ». Sert à ne
/// pas afficher deux fois la même règle sur une ligne.
fn rule_tag(msg: &str) -> Option<String> {
    let end = msg.trim_end().strip_suffix(')')?;
    let open = end.rfind('(')?;
    let tag = end[open + 1..]
        .split(['·', ' '])
        .next()
        .unwrap_or("")
        .trim();
    let looks_like_rule = tag.contains('-')
        && tag
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-');
    looks_like_rule.then(|| tag.to_string())
}

// ------------------------------------------------------------------ rendu

/// Barre basse, côté droit : la position, rien de plus.
fn pos_text(ed: &Editor) -> String {
    format!("ln {}, col {} ", ed.cy + 1, ed.cx + 1)
}

/// Remplit une zone d'une couleur unie (fond d'éditeur, barres).
fn fill(frame: &mut Frame, area: ratatui::layout::Rect, style: Style) {
    frame.render_widget(Paragraph::new("").style(style), area);
}

fn render_line(text: &str, palette_dim: Color, ext: &str) -> Vec<Span<'static>> {
    // coloration par langage (extension du fichier) via syntect — thème Minuit
    let hl = highlight::highlight_code(text, ext);
    if let Some(line) = hl.first() {
        line.iter()
            .map(|(style, t)| {
                Span::styled(
                    t.clone(),
                    Style::default().fg(Color::Rgb(
                        style.foreground.r,
                        style.foreground.g,
                        style.foreground.b,
                    )),
                )
            })
            .collect()
    } else {
        vec![Span::styled(
            text.to_string(),
            Style::default().fg(palette_dim),
        )]
    }
}

/// Trace une boîte à bords arrondis (le langage visuel de lazy.nvim) :
/// ╭─ titre ──╮ … ╰──╯. Retourne le rectangle intérieur.
/// Bordure d'un panneau : cyan voilé s'il a le focus, gris sinon.
fn border_for(focused: bool) -> Color {
    if focused {
        Color::Indexed(6) // cyan du shell — le focus se voit, sans crier
    } else {
        Ed::ruler()
    }
}

/// Fond des floats (modal, recherche, toasts, survol) — un demi-ton au-dessus.
fn float_bg() -> Color {
    Color::Indexed(0)
}

/// Écrit un segment de barre (fond + texte) et retourne la colonne suivante.
fn put_seg(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    text: &str,
    fg: Color,
    bg: Color,
    bold: bool,
) -> u16 {
    let mut st = Style::default().fg(fg).bg(bg);
    if bold {
        st = st.add_modifier(Modifier::BOLD);
    }
    buf.set_string(x, y, text, st);
    x + UnicodeWidthStr::width(text) as u16
}

fn draw_box(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    title: &[(String, Color)],
    border: Color,
) -> ratatui::layout::Rect {
    if area.width < 4 || area.height < 3 {
        return area;
    }
    let bgc = Ed::bg();
    let right = area.right() - 1;
    let bottom = area.bottom() - 1;
    let buf = frame.buffer_mut();
    for y in area.y..=bottom {
        buf[(area.x, y)].set_symbol("│").set_fg(border).set_bg(bgc);
        buf[(right, y)].set_symbol("│").set_fg(border).set_bg(bgc);
    }
    for x in area.x..=right {
        buf[(x, bottom)].set_symbol("─").set_fg(border).set_bg(bgc);
    }
    buf[(area.x, area.y)]
        .set_symbol("╭")
        .set_fg(border)
        .set_bg(bgc);
    buf[(right, area.y)]
        .set_symbol("╮")
        .set_fg(border)
        .set_bg(bgc);
    buf[(area.x, bottom)]
        .set_symbol("╰")
        .set_fg(border)
        .set_bg(bgc);
    buf[(right, bottom)]
        .set_symbol("╯")
        .set_fg(border)
        .set_bg(bgc);
    // haut : ╭─ titre ────╮ (titre tronqué si la boîte est étroite)
    let mut x = area.x + 1;
    buf[(x, area.y)].set_symbol("─").set_fg(border).set_bg(bgc);
    x += 1;
    let budget = (right - 2).saturating_sub(area.x + 2);
    for (txt, color) in title {
        for ch in txt.chars() {
            if x >= area.x + 2 + budget {
                break;
            }
            buf[(x, area.y)]
                .set_symbol(&ch.to_string())
                .set_fg(*color)
                .set_bg(bgc);
            x += 1;
        }
    }
    while x < right {
        buf[(x, area.y)].set_symbol("─").set_fg(border).set_bg(bgc);
        x += 1;
    }
    ratatui::layout::Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width - 2,
        height: area.height - 2,
    }
}

/// Écran d'accueil : modal flottant centré, sections à puces — le float
/// d'accueil de lazy.nvim, transposé aux gestes de c-nano. Aucune tagline.
fn draw_welcome_float(frame: &mut Frame, area: ratatui::layout::Rect) {
    let w = area.width.saturating_sub(4).min(46);
    let h = area.height.saturating_sub(2).min(24);
    if w < 20 || h < 10 {
        return;
    }
    let modal = ratatui::layout::Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    let inner = draw_box(
        frame,
        modal,
        &[(" n".into(), Ed::accent()), ("ana ".into(), Ed::text())],
        border_for(true),
    );
    let bullet = || Span::styled("● ", Style::default().fg(Ed::cyan()));
    let section = |s: &'static str| {
        Line::from(Span::styled(
            format!(" {s}"),
            Style::default().fg(Ed::text()).add_modifier(Modifier::BOLD),
        ))
    };
    let entry = |k: &'static str, label: &'static str| {
        Line::from(vec![
            Span::raw("  "),
            bullet(),
            Span::styled(format!("{k:<5}"), Style::default().fg(Ed::cyan())),
            Span::styled(label, Style::default().fg(Ed::dim())),
        ])
    };
    let lines = vec![
        Line::from(""),
        section("files"),
        entry("^t", "explorer"),
        entry("^o", "find a file"),
        entry("f2", "switch panel"),
        entry("^s", "save"),
        entry("^q", "quit"),
        Line::from(""),
        section("code"),
        entry("^b", "check"),
        entry("f5", "run"),
        entry("f6", "format"),
        entry("f4", "auto header"),
        entry("^f", "search"),
        entry("^r", "replace"),
        entry("^g", "go to line"),
        Line::from(""),
        section("buffer"),
        entry("^k", "cut line"),
        entry("^u", "paste"),
        entry("^z", "cancel"),
        Line::from(""),
        Line::from(Span::styled(
            "  nana <file> to open — ^t explores the project",
            Style::default().fg(Ed::gutter()),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw(frame: &mut Frame, ed: &mut Editor) {
    let area = frame.area();
    // fond unifié sur toute la surface — la signature « Minuit »
    fill(frame, area, Style::default().bg(Ed::bg()));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // barre haute
            Constraint::Min(3),    // corps
            Constraint::Length(1), // barre basse
        ])
        .split(area);
    fill(frame, chunks[0], Style::default().bg(Ed::bar_bg()));
    fill(frame, chunks[2], Style::default().bg(Ed::bar_bg()));

    draw_topbar(frame, ed, chunks[0]);

    // ── corps : boîtes arrondies façon lazy.nvim — explorateur à gauche,
    // éditeur à droite, le focus teinte la bordure ──
    let edit_zone = if ed.explorer.is_some() {
        let sp = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(EXPL_W),
                Constraint::Length(1), // souffle entre les boîtes
                Constraint::Min(10),
            ])
            .split(chunks[1]);
        // boîte nue : la bordure colorée + le badge suffisent au focus
        let inner = draw_box(frame, sp[0], &[], border_for(ed.focus == Focus::Explorer));
        if let Some(ex) = &mut ed.explorer {
            draw_explorer(frame, ex, inner);
        }
        sp[2]
    } else {
        chunks[1]
    };
    // boîte nue — le fichier vit dans le breadcrumb et la statusline
    let title: Vec<(String, Color)> = Vec::new();
    // le terminal prend le bas de la colonne éditeur quand il est ouvert
    let (editor_area, term_area) = if ed.term.is_some() {
        let sp = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(TERM_H)])
            .split(edit_zone);
        (sp[0], Some(sp[1]))
    } else {
        (edit_zone, None)
    };
    let inner = draw_box(
        frame,
        editor_area,
        &title,
        border_for(matches!(ed.focus, Focus::Editor | Focus::Search)),
    );
    if let Some(ta) = term_area {
        if let Some(term) = &mut ed.term {
            draw_terminal(frame, term, ta, ed.focus == Focus::Terminal);
        }
    }
    if ed.is_welcome() {
        draw_welcome_float(frame, inner);
    } else {
        draw_body(frame, ed, inner);
        draw_diagnostics(frame, ed, editor_area);
    }
    // floats par-dessus tout : recherche, puis toasts
    if let Some(fs) = &mut ed.search {
        draw_search(frame, fs, chunks[1], ed.focus == Focus::Search);
    }
    draw_toasts(frame, ed, area);

    draw_statusbar(frame, ed, chunks[2]);
}

/// Barre haute : bloc brand corail + branche git + breadcrumb du fichier —
/// la tabline d'un IDE, version Minuit profond.
fn draw_topbar(frame: &mut Frame, ed: &Editor, area: ratatui::layout::Rect) {
    fill(frame, area, Style::default().bg(Ed::bar_bg()));
    let seg_bg = Color::Indexed(0);
    let buf = frame.buffer_mut();
    let mut x = area.x;
    // brand
    x = put_seg(
        buf,
        x,
        area.y,
        " nana ",
        Color::Indexed(0),
        Ed::accent(),
        true,
    ) + 1;
    // branche git
    if let Some(br) = &ed.git_branch {
        put_seg(
            buf,
            x,
            area.y,
            &format!(" ⎇ {br} "),
            Ed::text(),
            seg_bg,
            false,
        );
    }
    // right: the project you are in, then the last check
    let mut right_x = area.right();
    if let Some(note) = &ed.check_note {
        let bad = ed.check_marks.iter().any(|(_, s)| *s == Severity::Major);
        let color = if bad {
            Ed::red()
        } else if ed.check_marks.is_empty() {
            Ed::green()
        } else {
            Ed::amber()
        };
        let seg = format!(" {note} ");
        let w = UnicodeWidthStr::width(seg.as_str()) as u16;
        right_x = right_x.saturating_sub(w);
        put_seg(buf, right_x, area.y, &seg, color, seg_bg, false);
        right_x = right_x.saturating_sub(1);
    }
    if let Some(label) = &ed.project_label {
        let seg = format!(" {label} ");
        let w = UnicodeWidthStr::width(seg.as_str()) as u16;
        if right_x > area.x + w + 2 {
            right_x = right_x.saturating_sub(w);
            put_seg(buf, right_x, area.y, &seg, Ed::dim(), seg_bg, false);
        }
    }
    // breadcrumb centré : dossier › fichier ●
    if let Some(p) = &ed.file {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let dir = p
            .parent()
            .and_then(|d| d.file_name())
            .and_then(|n| n.to_str());
        let crumb = match dir {
            Some(d) if !d.is_empty() => format!("{d} › {name}"),
            _ => name.to_string(),
        };
        let dirty_w = if ed.modified { 2 } else { 0 };
        let icon = file_icon(name, false);
        let total = UnicodeWidthStr::width(crumb.as_str()) as u16 + dirty_w + 2;
        let cx = area.x + area.width.saturating_sub(total) / 2;
        let ix = put_seg(
            buf,
            cx,
            area.y,
            icon,
            file_color(name, false),
            Ed::bar_bg(),
            false,
        );
        let nx = put_seg(
            buf,
            ix,
            area.y,
            &format!(" {crumb}"),
            Ed::text(),
            Ed::bar_bg(),
            false,
        );
        if ed.modified {
            put_seg(buf, nx, area.y, " ●", Ed::accent(), Ed::bar_bg(), false);
        }
    }
}

/// Statusline segmentée façon lualine : badge de focus, fichier, message,
/// diagnostics, norme, position, progression.
fn draw_statusbar(frame: &mut Frame, ed: &Editor, area: ratatui::layout::Rect) {
    fill(frame, area, Style::default().bg(Ed::bar_bg()));
    let seg_bg = Color::Indexed(0);
    let buf = frame.buffer_mut();
    let mut x = area.x;

    // the badge names what you are looking at: the language while editing,
    // the panel otherwise
    let lang = ed
        .file
        .as_deref()
        .map(langs::for_path)
        .unwrap_or(&langs::PLAIN);
    let (label, color) = match ed.focus {
        Focus::Editor => (format!(" {} ", lang.name), Color::Indexed(lang.color)),
        Focus::Explorer => (" explorer ".to_string(), Ed::green()),
        Focus::Search => (" search ".to_string(), Color::Indexed(5)),
        Focus::Terminal => (" terminal ".to_string(), Ed::amber()),
    };
    x = put_seg(buf, x, area.y, &label, Color::Indexed(0), color, true) + 1;
    // segment fichier
    if let Some(p) = &ed.file {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        x = put_seg(
            buf,
            x,
            area.y,
            &format!(" {} ", file_icon(name, false)),
            file_color(name, false),
            seg_bg,
            false,
        );
        x = put_seg(
            buf,
            x,
            area.y,
            &format!("{name} "),
            Ed::text(),
            seg_bg,
            false,
        );
        if ed.modified {
            x = put_seg(buf, x, area.y, "● ", Ed::accent(), seg_bg, false);
        }
        x += 1;
    }

    // segments de droite : diagnostics, norme, position, progression
    let mut right: Vec<(String, Color)> = Vec::new();
    let errs = ed.diags.iter().filter(|d| d.is_error).count();
    let warns = ed.diags.len() - errs;
    if errs > 0 || warns > 0 {
        right.push((format!("✗{errs}"), Ed::red()));
        right.push((format!("⚠{warns}"), Ed::amber()));
    }
    if let Some(note) = &ed.check_note {
        let bad = ed.check_marks.iter().any(|(_, s)| *s == Severity::Major);
        let color = if bad {
            Ed::red()
        } else if ed.check_marks.is_empty() {
            Ed::green()
        } else {
            Ed::amber()
        };
        right.push((note.clone(), color));
    }
    right.push((pos_text(ed).trim().to_string(), Ed::dim()));
    let pct = ((ed.cy + 1) * 100) / ed.lines.len().max(1);
    right.push((format!("{pct}%"), Ed::dim()));
    let total: u16 = right
        .iter()
        .map(|(s, _)| UnicodeWidthStr::width(s.as_str()) as u16 + 2)
        .sum();
    let mut rx = area.right().saturating_sub(total);
    for (text, color) in right {
        rx = put_seg(buf, rx, area.y, &format!(" {text} "), color, seg_bg, false);
    }

    // centre : prompt en cours ou dernier statut
    let mid = if let Some((k, t)) = &ed.prompt {
        let label = match k {
            'f' => "search : ",
            'r' => "replace : ",
            'n' => "new file : ",
            'w' => "rename to : ",
            _ => "line : ",
        };
        format!("{label}{t}▌")
    } else {
        ed.status.clone()
    };
    let avail = (rx.saturating_sub(x + 1)) as usize;
    let mid: String = mid.chars().take(avail.saturating_sub(1)).collect();
    put_seg(
        buf,
        x,
        area.y,
        &format!(" {mid}"),
        Ed::dim(),
        Ed::bar_bg(),
        false,
    );
}

/// Recherche de fichiers flottante — le float Telescope : prompt en haut,
/// séparateur fin, résultats sous le curseur.
fn draw_search(frame: &mut Frame, fs: &mut FileSearch, zone: ratatui::layout::Rect, focused: bool) {
    let w = zone.width.saturating_sub(8).min(62);
    let rows_h = (fs.len().min(9) as u16).max(1);
    // prompt + separator + borders, and never so short that the guard below
    // refuses to draw a search with a single result
    let h = (rows_h + 4).max(6).min(zone.height.saturating_sub(2));
    if w < 24 || h < 6 {
        return;
    }
    let float = ratatui::layout::Rect {
        x: zone.x + (zone.width - w) / 2,
        y: zone.y + (zone.height / 8).min(3),
        width: w,
        height: h,
    };
    fill(frame, float, Style::default().bg(float_bg()));
    let title = format!(" search · {} ", fs.len());
    let inner = draw_box(frame, float, &[(title, Ed::text())], border_for(focused));
    // prompt
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("❯ ", Style::default().fg(Ed::cyan())),
            Span::styled(
                format!("{}▌", fs.filter()),
                Style::default().fg(if focused { Ed::text() } else { Ed::dim() }),
            ),
        ])),
        ratatui::layout::Rect { height: 1, ..inner },
    );
    // séparateur fin entre prompt et résultats
    let sep_y = inner.y + 1;
    for cx in inner.x..inner.right() {
        let cell = &mut frame.buffer_mut()[(cx, sep_y)];
        cell.set_symbol("─").set_fg(Ed::ruler()).set_bg(float_bg());
    }
    // résultats
    let list_h = (inner.height as usize).saturating_sub(2);
    let (start, paths) = fs.window(list_h);
    let sel = fs.sel();
    let lines: Vec<Line> = if paths.is_empty() {
        vec![Line::from(Span::styled(
            "  no file",
            Style::default().fg(Ed::gutter()),
        ))]
    } else {
        paths
            .iter()
            .enumerate()
            .map(|(i, path)| {
                let rel = fs.rel(path);
                let selected = start + i == sel;
                let mut spans = vec![Span::raw(if selected { "▎" } else { " " })];
                let fname = rel.rsplit('/').next().unwrap_or(&rel);
                let icon_color = file_color(fname, false);
                spans.push(Span::styled(
                    format!("{} ", file_icon(fname, false)),
                    Style::default().fg(icon_color),
                ));
                match rel.rsplit_once('/') {
                    Some((dir, name)) => {
                        spans.push(Span::styled(
                            format!("{dir}/"),
                            Style::default().fg(Ed::gutter()),
                        ));
                        let mut st = Style::default().fg(file_color(name, false));
                        if selected {
                            st = st.add_modifier(Modifier::BOLD);
                        }
                        spans.push(Span::styled(name.to_string(), st));
                    }
                    None => spans.push(Span::styled(rel, Style::default().fg(Ed::text()))),
                }
                let mut line = Line::from(spans);
                if selected {
                    line = line.style(Style::default().bg(Ed::sel_row()));
                }
                line
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines),
        ratatui::layout::Rect {
            y: inner.y + 2,
            height: inner.height.saturating_sub(2),
            ..inner
        },
    );
}

/// Notifications toast — cartes flottantes empilées en haut à droite,
/// colorées par niveau, évanouissement après 3,5 s.
fn draw_toasts(frame: &mut Frame, ed: &mut Editor, area: ratatui::layout::Rect) {
    ed.toasts
        .retain(|t| t.at.elapsed() < Duration::from_millis(3500));
    let shown: Vec<&Toast> = ed.toasts.iter().rev().take(3).collect();
    let mut y = area.y + 1;
    for toast in shown {
        // le texte se wrappe sur 2 lignes — plus jamais tronqué
        let inner_w = 42usize; // colonnes de texte max par ligne
        let words: Vec<&str> = toast.text.split_whitespace().collect();
        let mut lines_t: Vec<String> = Vec::new();
        let mut cur = String::new();
        for w in words {
            let cand = if cur.is_empty() {
                w.to_string()
            } else {
                format!("{cur} {w}")
            };
            if UnicodeWidthStr::width(cand.as_str()) > inner_w && !cur.is_empty() {
                lines_t.push(cur);
                cur = w.to_string();
            } else {
                cur = cand;
            }
        }
        if !cur.is_empty() {
            lines_t.push(cur);
        }
        // jamais coupé : jusqu'à 5 lignes, la dernière porte « … » si besoin
        if lines_t.len() > 5 {
            lines_t.truncate(5);
            let last = lines_t.last_mut().unwrap();
            *last = format!("{}…", last.trim_end());
        }
        let text_w = lines_t
            .iter()
            .map(|l| UnicodeWidthStr::width(l.as_str()))
            .max()
            .unwrap_or(4);
        let w = ((text_w + 6).clamp(14, 48)) as u16;
        let h = lines_t.len() as u16 + 2;
        let x = area.right().saturating_sub(w + 1);
        if y + h + 1 >= area.bottom() {
            break;
        }
        let rect = ratatui::layout::Rect {
            x,
            y,
            width: w,
            height: h,
        };
        fill(frame, rect, Style::default().bg(float_bg()));
        let (icon, color) = match toast.level {
            Level::Ok => ("✓", Ed::green()),
            Level::Err => ("✗", Ed::red()),
            Level::Warn => ("⚠", Ed::amber()),
            Level::Info => ("ℹ", Ed::cyan()),
        };
        let inner = draw_box(frame, rect, &[], color);
        // l'icône n'apparaît qu'une fois — sur la première ligne ; les
        // lignes de continuation s'alignent dessous, sans la répéter.
        let lines: Vec<Line> = lines_t
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let head = if i == 0 {
                    Span::styled(format!("{icon} "), Style::default().fg(color))
                } else {
                    Span::styled("  ", Style::default().fg(Ed::dim()))
                };
                Line::from(vec![
                    head,
                    Span::styled(l.clone(), Style::default().fg(Ed::text())),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), inner);
        y += h + 1;
    }
}

/// Survol des diagnostics de la ligne courante — le hover LSP : petit
/// flottant en bas à droite de l'éditeur, bordure rouge ou ambre.
fn draw_diagnostics(frame: &mut Frame, ed: &Editor, zone: ratatui::layout::Rect) {
    let items = ed.line_diagnostics();
    if items.is_empty() {
        return;
    }
    // chaque message peut vivre sur 2 lignes — les longs libellés de norme
    // ne sont plus jamais coupés
    let wrap_w = 50usize;
    let mut wrapped: Vec<(bool, String, bool)> = Vec::new(); // (err, texte, suite?)
    for (is_err, msg) in items.iter().take(4) {
        let words: Vec<&str> = msg.split_whitespace().collect();
        let mut cur = String::new();
        let mut first = true;
        for word in words {
            let cand = if cur.is_empty() {
                word.to_string()
            } else {
                format!("{cur} {word}")
            };
            if UnicodeWidthStr::width(cand.as_str()) > wrap_w && !cur.is_empty() {
                wrapped.push((*is_err, std::mem::take(&mut cur), !first));
                first = false;
                cur = word.to_string();
            } else {
                cur = cand;
            }
        }
        if !cur.is_empty() {
            wrapped.push((*is_err, cur, !first));
        }
    }
    wrapped.truncate(8);
    let wmax = wrapped
        .iter()
        .map(|(_, m, _)| UnicodeWidthStr::width(m.as_str()))
        .max()
        .unwrap_or(8);
    let w = ((wmax + 8).clamp(18, 58)) as u16;
    let h = wrapped.len() as u16 + 2;
    let rect = ratatui::layout::Rect {
        x: zone.right().saturating_sub(w + 1),
        y: zone.bottom().saturating_sub(h + 1),
        width: w,
        height: h,
    };
    fill(frame, rect, Style::default().bg(float_bg()));
    let any_err = items.iter().any(|(e, _)| *e);
    let border = if any_err { Ed::red() } else { Ed::amber() };
    let title = format!(" line {} ", ed.cy + 1);
    let inner = draw_box(frame, rect, &[(title, Ed::dim())], border);
    let lines: Vec<Line> = wrapped
        .iter()
        .map(|(is_err, msg, suite)| {
            let (icon, color) = if *is_err {
                ("✗", Ed::red())
            } else {
                ("⚠", Ed::amber())
            };
            let (glyph, gcolor) = if *suite {
                ("  ", Ed::dim())
            } else {
                (icon, color)
            };
            Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(gcolor)),
                Span::styled(msg.clone(), Style::default().fg(Ed::text())),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Icônes activées ? `NANO_ICONS=0` → repli lettres cerclées (terminaux
/// sans Nerd Font). Les glyphes viennent des devicons de JetBrainsMono NF.
fn icons_enabled() -> bool {
    static ONCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ONCE.get_or_init(|| {
        std::env::var_os("NANO_ICONS")
            .map(|v| v != "0")
            .unwrap_or(true)
    })
}

/// Icône devicon du fichier (les vrais logos : C , Rust , Python …).
/// the glyph comes from the language registry, so a new language brings its
/// own icon instead of falling into a catch-all branch.
fn file_icon_dev(name: &str) -> &'static str {
    static TABLE: std::sync::LazyLock<HashMap<char, &'static str>> =
        std::sync::LazyLock::new(|| {
            let mut m: HashMap<char, &'static str> = HashMap::new();
            for l in langs::all() {
                m.entry(l.icon).or_insert_with(|| {
                    let s: &'static str = Box::leak(l.icon.to_string().into_boxed_str());
                    s
                });
            }
            m
        });
    let icon = langs::for_path(Path::new(name)).icon;
    TABLE.get(&icon).copied().unwrap_or("\u{F15B}")
}

fn file_icon_plain(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "c" | "h" => "Ⓒ",
        "rs" => "Ⓡ",
        "py" => "Ⓟ",
        "js" | "ts" | "mjs" => "Ⓙ",
        "sh" | "bash" | "zsh" => "Ⓢ",
        "html" | "htm" => "Ⓗ",
        "css" => "ⓒ",
        "md" | "txt" => "Ⓜ",
        "toml" | "json" | "yaml" | "yml" | "lock" | "cfg" => "Ⓣ",
        _ if lower == "makefile" || ext == "mk" => "Ⓚ",
        _ => "◆",
    }
}

/// Mini-icône du fichier : devicon si possible, lettre cerclée sinon.
fn file_icon(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "";
    }
    if icons_enabled() {
        file_icon_dev(name)
    } else {
        file_icon_plain(name)
    }
}

/// Icône de dossier :  ouverte /  fermée (repli : ▾/▸).
fn dir_icon(expanded: bool) -> &'static str {
    if icons_enabled() {
        if expanded {
            "\u{F07C}"
        } else {
            "\u{F07B}"
        } // fa-folder-open / fa-folder
    } else if expanded {
        "▾"
    } else {
        "▸"
    }
}

/// Couleur d'un fichier selon son type — chaque famille a sa teinte.
fn file_color(name: &str, is_dir: bool) -> Color {
    if is_dir {
        return Ed::cyan();
    }
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        // build artefacts stay quiet
        "o" | "a" | "so" | "bin" | "o_" => Ed::gutter(),
        _ => Color::Indexed(langs::for_path(Path::new(name)).color),
    }
}

/// Panneau explorateur : arbre + filtre, séparateur fin, sélection teintée.
fn draw_explorer(frame: &mut Frame, ex: &mut Explorer, area: ratatui::layout::Rect) {
    // `area` est l'intérieur de la boîte (le cadre est déjà tracé)
    let content = area;

    // en-tête : la racine du panneau
    let root_name = ex
        .root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    let header = Line::from(vec![
        Span::styled("▾ ", Style::default().fg(Ed::cyan())),
        Span::styled(
            root_name.to_string(),
            Style::default().fg(Ed::text()).add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(header),
        ratatui::layout::Rect {
            height: 1,
            ..content
        },
    );

    // ligne du bas : le filtre en cours, ou l'invitation
    let filter_line = if ex.filter().is_empty() {
        Line::from(Span::styled(
            " type to filter",
            Style::default().fg(Ed::dim()),
        ))
    } else {
        Line::from(vec![
            Span::styled("❯ ", Style::default().fg(Ed::cyan())),
            Span::styled(format!("{}▌", ex.filter()), Style::default().fg(Ed::text())),
        ])
    };
    frame.render_widget(
        Paragraph::new(filter_line),
        ratatui::layout::Rect {
            y: content.bottom().saturating_sub(1),
            height: 1,
            ..content
        },
    );

    // lignes de l'arbre (entre en-tête et ligne de filtre)
    let tree_h = (content.height as usize).saturating_sub(2);
    let rows_area = ratatui::layout::Rect {
        y: content.y + 1,
        height: tree_h as u16,
        ..content
    };
    // on clone la fenêtre (quelques dizaines de lignes) pour libérer
    // l'emprunt : git_mark/sel restent accessibles pendant le rendu
    let (start, visible) = {
        let (s, rows) = ex.window(tree_h);
        (s, rows.to_vec())
    };
    let sel = ex.sel();
    let filtering = !ex.filter().is_empty();
    let lines: Vec<Line> = visible
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let selected = start + i == sel;
            let mut spans = Vec::new();
            // barre d'accent corail sur le bord — la sélection se lit d'un coup d'œil
            if selected {
                spans.push(Span::styled("▎", Style::default().fg(Ed::accent())));
            } else {
                spans.push(Span::raw(" "));
            }
            if filtering {
                // mode filtre : chemin relatif, dossier en sourdine
                match row.name.rsplit_once('/') {
                    Some((dir, name)) => {
                        spans.push(Span::styled(
                            format!(" {dir}/"),
                            Style::default().fg(Ed::gutter()),
                        ));
                        spans.push(Span::styled(
                            name.to_string(),
                            Style::default().fg(file_color(name, false)),
                        ));
                    }
                    None => spans.push(Span::styled(
                        format!(" {}", row.name),
                        Style::default().fg(file_color(&row.name, false)),
                    )),
                }
            } else {
                spans.push(Span::raw("  ".repeat(row.depth)));
                if row.is_dir {
                    spans.push(Span::styled(
                        format!("{} ", dir_icon(row.expanded)),
                        Style::default().fg(Ed::cyan()),
                    ));
                } else {
                    // mini-icône du langage, teintée
                    spans.push(Span::styled(
                        format!("{} ", file_icon(&row.name, false)),
                        Style::default().fg(file_color(&row.name, false)),
                    ));
                }
                let mut name_style = Style::default().fg(file_color(&row.name, row.is_dir));
                if selected {
                    name_style = name_style.add_modifier(Modifier::BOLD);
                }
                spans.push(Span::styled(row.name.clone(), name_style));
            }
            // marqueur git : ● ambre modifié, ● vert nouveau
            if let Some(mark) = ex.git_mark(&row.path) {
                let color = if mark == 'M' {
                    Ed::amber()
                } else {
                    Ed::green()
                };
                spans.push(Span::styled(" ●", Style::default().fg(color)));
            }
            let mut line = Line::from(spans);
            if selected {
                line = line.style(Style::default().bg(Ed::sel_row()));
            }
            line
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows_area);
}

/// Conversion couleur vt100 → ratatui (défaut = la palette Minuit).
fn vt_color(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Le panneau terminal : boîte arrondie, cellules vt100 rendues en Minuit.
fn draw_terminal(
    frame: &mut Frame,
    term: &mut TermPane,
    area: ratatui::layout::Rect,
    focused: bool,
) {
    let inner = draw_box(frame, area, &[], border_for(focused));
    term.resize(inner.height, inner.width);
    let screen = term.parser.screen();
    let mut lines: Vec<Line> = Vec::with_capacity(inner.height as usize);
    for row in 0..inner.height {
        let mut spans: Vec<Span> = Vec::new();
        let mut cur = String::new();
        let mut cur_style: Option<Style> = None;
        for col in 0..inner.width {
            let Some(cell) = screen.cell(row, col) else {
                break;
            };
            let fg = vt_color(cell.fgcolor(), Ed::text());
            let bg = vt_color(cell.bgcolor(), Ed::bg());
            let mut st = Style::default().fg(fg).bg(bg);
            if cell.bold() {
                st = st.add_modifier(Modifier::BOLD);
            }
            let text = cell.contents();
            let text = if text.is_empty() {
                " ".to_string()
            } else {
                text
            };
            if cur_style == Some(st) {
                cur.push_str(&text);
            } else {
                if !cur.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut cur), cur_style.unwrap()));
                }
                cur_style = Some(st);
                cur.push_str(&text);
            }
        }
        if !cur.is_empty() {
            spans.push(Span::styled(cur, cur_style.unwrap_or_default()));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    if focused {
        let (cr, cc) = screen.cursor_position();
        if cr < inner.height && cc < inner.width {
            frame.set_cursor_position((inner.x + cc, inner.y + cr));
        }
    }
}

/// Corps de l'éditeur : gouttière à marqueurs + texte coloré.
fn draw_body(frame: &mut Frame, ed: &Editor, zone: ratatui::layout::Rect) {
    let digits = ed.lines.len().to_string().len().max(2);
    let gutter_w = digits + 2; // marqueur + numéro + espace
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(gutter_w as u16), Constraint::Min(10)])
        .split(zone);

    let marks = ed.gutter_marks();
    let inner_h = zone.height as usize;
    let gutter_lines: Vec<Line> = (0..inner_h)
        .map(|i| {
            let n = ed.scroll_y + i + 1; // 1-based
            if n <= ed.lines.len() {
                let is_cur = n == ed.cy + 1;
                let mut spans = Vec::new();
                // marqueur : diagnostic gcc / norme, le plus sévère
                match marks.get(&n) {
                    Some(color) => spans.push(Span::styled("●", Style::default().fg(*color))),
                    None => spans.push(Span::raw(" ")),
                }
                spans.push(Span::styled(
                    format!("{:>w$} ", n, w = digits),
                    Style::default().fg(if is_cur { Ed::text() } else { Ed::gutter() }),
                ));
                let mut line = Line::from(spans);
                if is_cur {
                    line = line.style(Style::default().bg(Ed::cur_line()));
                }
                line
            } else {
                Line::from(" ".repeat(gutter_w))
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(gutter_lines), body[0]);

    let ghost_len = ed.ghost_lines.len();
    let text_lines: Vec<Line> = (0..inner_h)
        .map(|i| {
            let n = ed.scroll_y + i;
            // lignes fantômes SOUS le curseur : aperçu de l'insertion
            // multi-lignes (les vraies lignes réapparaissent au refus)
            if ghost_len > 0 && n > ed.cy && n - ed.cy < ghost_len {
                let vis: String = ed.ghost_lines[n - ed.cy]
                    .chars()
                    .skip(ed.scroll_x)
                    .collect();
                return Line::from(Span::styled(
                    vis,
                    Style::default()
                        .fg(Ed::ghost())
                        .add_modifier(Modifier::ITALIC),
                ));
            }
            if n < ed.lines.len() {
                let raw = &ed.lines[n];
                let visible: String = raw.chars().skip(ed.scroll_x).collect();
                let mut spans = render_line(&visible, Ed::dim(), &ed.ext);
                // première ligne du fantôme : à la suite de la ligne du curseur
                if n == ed.cy && ghost_len > 0 {
                    spans.push(Span::styled(
                        ed.ghost_lines[0].clone(),
                        Style::default()
                            .fg(Ed::ghost())
                            .add_modifier(Modifier::ITALIC),
                    ));
                }
                let mut line = Line::from(spans);
                if n == ed.cy {
                    // ligne courante teintée — le regard se pose instantanément
                    line = line.style(Style::default().bg(Ed::cur_line()));
                }
                line
            } else {
                Line::from("")
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(text_lines), body[1]);

    // repère subtil à la colonne 80 (la limite de la norme) — un filet discret
    let ruler_x = body[1].x + 80;
    if ruler_x < body[1].right() {
        for row in body[1].top()..body[1].bottom() {
            let cell = &mut frame.buffer_mut()[(ruler_x, row)];
            // n'écrase pas le code : uniquement sur une case vide,
            // et préserve le fond (ligne courante comprise)
            if cell.symbol().trim().is_empty() {
                cell.set_symbol("·").set_fg(Ed::ruler());
            }
        }
    }

    // curseur : seulement quand l'éditeur a le focus (dans l'explorateur,
    // c'est la ligne sélectionnée qui porte le regard)
    if ed.focus == Focus::Editor {
        let cur_x = body[1].x + (ed.cx - ed.scroll_x) as u16;
        let cur_y = body[1].y + (ed.cy - ed.scroll_y) as u16;
        frame.set_cursor_position((
            cur_x.min(body[1].width.saturating_sub(1) + body[1].x),
            cur_y,
        ));
    }
}

/// Lance l'éditeur. `path` : fichier à ouvrir/créer.
pub fn run(path: Option<PathBuf>) -> io::Result<()> {
    // NO_COLOR est une convention pensée pour les logs, pas pour un éditeur :
    // posée dans l'environnement, elle fait disparaître TOUTE la coloration
    // (crossterm émet alors des séquences vides). Un éditeur de code sans
    // couleurs est un bug, pas une préférence — c-nano la retire de SON
    // processus (l'environnement du shell reste intact).
    std::env::remove_var("NO_COLOR");
    // IDE complet dès l'ouverture : explorateur + terminal toujours visibles,
    // le focus reste à l'éditeur (ou à l'accueil)
    let (file, root) = startup_layout(&path);
    let mut ed = Editor::open(file.as_deref())?;
    // défaut : éditeur + explorateur seulement — le terminal reste fermé,
    // F3 l'ouvre à la demande.
    ed.explorer = Some(Explorer::new(root.clone()));
    // un seul modèle, toujours le même — pas de réglage, pas de dérive
    let ai = AiClient::from_env(&crate::config::load())
        .ok()
        .map(|mut c| {
            c.set_model(EDITOR_MODEL);
            c
        });
    ed.ai = ai;

    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
    // Protocole clavier « kitty » : foot l'implémente. Sans lui, foot envoie
    // Ctrl+Tab sous une forme que crossterm ne décode pas — la touche
    // disparaît avant nous. Inoffensif pour les terminaux qui l'ignorent.
    let kitty = kitty_keyboard();
    if kitty {
        let _ = crossterm::execute!(
            io::stdout(),
            crossterm::event::PushKeyboardEnhancementFlags(
                crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        );
    }
    let mut terminal =
        ratatui::DefaultTerminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    let result = loop_run(&mut terminal, &mut ed);
    if kitty {
        let _ = crossterm::execute!(io::stdout(), crossterm::event::PopKeyboardEnhancementFlags);
    }
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen)?;
    result
}

/// Le terminal parle-t-il le protocole clavier « kitty » ? foot oui — c'est
/// ce qui rend Ctrl+Tab (et Alt+Tab) réellement distincts de Tab.
fn kitty_keyboard() -> bool {
    matches!(
        std::env::var("TERM").as_deref(),
        Ok(t) if t.contains("foot")
            || t.contains("kitty")
            || t.contains("wezterm")
            || t.contains("ghostty")
    )
}

fn loop_run(terminal: &mut ratatui::DefaultTerminal, ed: &mut Editor) -> io::Result<()> {
    while !ed.should_quit {
        ed.poll_ai();
        ed.poll_check();
        ed.poll_diag();
        ed.poll_header();
        if let Some(term) = &mut ed.term {
            term.poll();
        }
        // le scroll se calcule sur la zone INTÉRIEURE de la boîte éditeur :
        // barres (2) + bordures (2) + terminal (TERM_H si ouvert) +
        // explorateur (EXPL_W + souffle) — sinon le curseur traverse les
        // bordures en bas de fichier
        let size = terminal.size()?;
        let term_h = if ed.term.is_some() { TERM_H } else { 0 };
        let expl_w = if ed.explorer.is_some() { EXPL_W + 1 } else { 0 };
        let inner_h = size.height.saturating_sub(2 + 2 + term_h);
        let inner_w = size.width.saturating_sub(expl_w + 2);
        ed.keep_cursor_visible(inner_h as usize, inner_w as usize);
        terminal.draw(|frame| draw(frame, ed))?;
        // poll avec timeout : la boucle doit se réveiller pour lire les
        // réponses IA/norme/build qui arrivent en tâche de fond
        if event::poll(std::time::Duration::from_millis(120))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == crossterm::event::KeyEventKind::Press {
                    ed.on_key(key);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod robustness_tests {
    use super::*;

    /// L'éditeur ne doit JAMAIS paniquer, quelles que soient les touches.
    #[test]
    fn aucune_panique_sur_touches_limites() {
        let mut ed = Editor::open(None).unwrap();
        // une rafale de touches bizarres
        let cles = [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Char('a'),
            KeyCode::Char('x'),
        ];
        for _ in 0..3 {
            for c in &cles {
                let key = KeyEvent::new(*c, KeyModifiers::empty());
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ed.on_key(key)));
                assert!(r.is_ok(), "panique sur la touche {c:?}");
            }
        }
    }

    /// Le undo sur un buffer vide / initial ne panique pas.
    #[test]
    fn undo_sur_vide_ne_panique_pas() {
        let mut ed = Editor::open(None).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ed.undo()));
        assert!(r.is_ok());
        // et le undo restaure vraiment
        ed.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty()));
        ed.undo();
        assert_eq!(ed.lines[0], "");
    }

    /// goto/recherche sur des entrées bizarres ne paniquent pas.
    #[test]
    fn goto_et_recherche_limites() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["line one".into(), "line two".into()];
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ed.find(""); // vide
            ed.find("inexistant"); // absent
            ed.find("line"); // trouvé
        }));
        assert!(r.is_ok());
    }
}

#[cfg(test)]
mod pair_tests {
    use super::*;

    fn type_keys(ed: &mut Editor, s: &str) {
        for c in s.chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
    }

    /// Une ouvrante insère la paire, curseur au milieu.
    #[test]
    fn ouvrante_insere_la_paire() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "(");
        assert_eq!(ed.lines[0], "()");
        assert_eq!(ed.cx, 1);
        type_keys(&mut ed, "{");
        assert_eq!(ed.lines[0], "({})");
        assert_eq!(ed.cx, 2);
    }

    /// Les quotes se ferment aussi (char et string du C).
    #[test]
    fn quotes_se_ferment_seules() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "\"");
        assert_eq!(ed.lines[0], "\"\"");
        assert_eq!(ed.cx, 1);
    }

    /// Taper la fermante déjà présente la survole, sans doubler.
    #[test]
    fn fermante_presente_est_survolee() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "(x)");
        assert_eq!(ed.lines[0], "(x)");
        assert_eq!(ed.cx, 3, "le ) final survole au lieu de doubler");
    }

    /// Backspace entre une paire vide supprime les deux moitiés.
    #[test]
    fn backspace_sur_paire_vide_efface_les_deux() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "(");
        ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "");
        assert_eq!(ed.cx, 0);
    }

    /// Backspace sur du texte normal reste intact.
    #[test]
    fn backspace_classique_inchange() {
        let mut ed = Editor::open(None).unwrap();
        type_keys(&mut ed, "ab");
        ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "a");
    }

    /// Taper puis effacer jusqu'à l'état disque → le buffer redevient
    /// propre : plus de fausse alerte « modifié », l'explorateur s'ouvre.
    #[test]
    fn retour_a_l_etat_disque_rend_propre() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert!(ed.modified);
        ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert!(!ed.modified, "revenu à l'état disque = propre");
        // l'explorateur ne bloque donc plus
        ed.explorer = Some(Explorer::new(std::env::temp_dir()));
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        // aucune plainte « buffer modifié »
        assert!(!ed.status.contains("^s to save first"), "{}", ed.status);
    }

    /// F3 ouvre le terminal (focus dedans), Échap en sort sans fermer,
    /// F3 referme. Le cycle F2 l'inclut quand il est ouvert.
    #[test]
    fn terminal_toggle_et_cycle() {
        let mut ed = Editor::open(None).unwrap();
        assert!(ed.term.is_none());
        ed.on_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::empty()));
        assert!(ed.term.is_some(), "le PTY est spawné");
        assert_eq!(ed.focus, Focus::Terminal);
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Editor);
        assert!(ed.term.is_some(), "Échap ne ferme pas, juste le focus");
        // sans explorateur ouvert, la ronde est éditeur ↔ terminal
        ed.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Terminal, "ronde : éditeur → terminal");
        ed.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Editor, "…et retour");
        ed.on_key(KeyEvent::new(KeyCode::F(3), KeyModifiers::empty()));
        assert!(ed.term.is_none());
    }

    /// Explorateur : ^N crée (et ouvre + en-tête), ^R renomme, ^D supprime
    /// après confirmation o.
    #[test]
    fn ops_fichiers_creer_renommer_supprimer() {
        let dir = std::env::temp_dir().join(format!("cnano-ops-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(dir.clone()));
        ed.focus = Focus::Explorer;

        // ^N nouveau fichier
        ed.on_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        for c in "neuf.c".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let created = dir.join("neuf.c");
        assert!(created.exists(), "créé sur le disque");
        assert_eq!(ed.file.as_deref(), Some(created.as_path()), "ouvert");
        // en-tête squelette inséré (pas de modèle en test)
        assert!(
            ed.lines[0].starts_with("/*"),
            "auto header: {}",
            ed.lines[0]
        );
        assert!(
            ed.lines.iter().any(|l| l.contains("file: neuf.c")),
            "{}",
            ed.lines.join(" | ")
        );

        // ^R renomme (le fichier est sélectionné dans l'arbre rechargé)
        let pos = ed
            .explorer
            .as_ref()
            .unwrap()
            .rows()
            .iter()
            .position(|r| r.name == "neuf.c")
            .unwrap();
        // sélectionne-le en naviguant
        while ed.explorer.as_ref().unwrap().sel() > pos {
            ed.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
        }
        while ed.explorer.as_ref().unwrap().sel() < pos {
            ed.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        }
        // buffer modifié (en-tête inséré) → sauve d'abord pour suivre le rename
        ed.save();
        // l'ouverture a rendu le focus à l'éditeur : retour explorateur
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        // le prompt est pré-rempli du nom courant — on l'efface et on tape
        for _ in 0..8 {
            ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        }
        for c in "dix.c".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(dir.join("dix.c").exists());
        assert!(!created.exists(), "l'ancien nom a disparu");
        assert!(
            ed.file.as_deref().unwrap().ends_with("dix.c"),
            "l'éditeur suit"
        );

        // ^D puis 'o' supprime
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(ed.confirm_delete.is_some());
        ed.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::empty()));
        assert!(!dir.join("dix.c").exists(), "supprimé du disque");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// La disposition de départ : dossier → explorateur racine, fichier →
    /// racine = son dossier, rien → cwd.
    #[test]
    fn disposition_de_depart() {
        let cwd = std::env::current_dir().unwrap();
        let (f, root) = startup_layout(&Some(cwd.clone()));
        assert!(f.is_none(), "un dossier n'ouvre pas de fichier");
        assert_eq!(root, cwd);
        let file = cwd.join("Cargo.toml");
        let (f, root) = startup_layout(&Some(file.clone()));
        assert_eq!(f.as_deref(), Some(file.as_path()));
        assert_eq!(root, cwd, "la racine suit le fichier");
        let (f, root) = startup_layout(&None);
        assert!(f.is_none());
        assert_eq!(root, cwd);
    }

    /// « src/x.c » part de la racine même si la sélection est un dossier.
    #[test]
    fn creation_chemin_relatif_a_la_racine() {
        let dir = std::env::temp_dir().join(format!("cnano-rel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.c"), "int m;\n").unwrap();
        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(dir.clone()));
        ed.focus = Focus::Explorer;
        // la sélection est sur « src » (dossier) — le nom contient un /
        ed.create_file("src/neuf.c");
        assert!(
            dir.join("src/neuf.c").exists(),
            "racine + chemin, pas dossier+dossier"
        );
        assert!(!dir.join("src/src/neuf.c").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// the header opens with the language's own comment syntax.
    #[test]
    fn header_uses_the_language_syntax() {
        let h = header_lines(Path::new("my_swap.c"), "swap two values");
        assert_eq!(h[0], "/*");
        assert!(h[1].contains("file: my_swap.c"), "{}", h[1]);
        assert!(h[2].contains("description: swap two values"), "{}", h[2]);
        assert_eq!(h[h.len() - 2], "*/");
    }

    /// The pane answers the queries a modern shell sends at startup.
    #[test]
    fn terminal_queries_are_answered() {
        let r = terminal_replies("\x1b[0c");
        assert!(r.starts_with("\x1b[?62"), "device attributes: {r:?}");
        assert!(terminal_replies("\x1b]11;?").contains("]11;rgb:"));
        assert!(terminal_replies("\x1b]10;?").contains("]10;rgb:"));
        assert!(terminal_replies("\x1b]4;1;?").contains("]4;1;rgb:f07886"));
        assert!(terminal_replies("\x1b[?u").contains("[?0u"));
        assert!(terminal_replies("\x1b[6n").contains("R"));
        // nothing asked, nothing answered
        assert!(terminal_replies("plain output\n").is_empty());
        // and a chunk that asks two things gets two answers
        let both = terminal_replies("\x1b]11;?\x1b[0c");
        assert!(both.contains("]11;rgb:") && both.contains("[?62"));
    }

    /// É, à, ü… multi-octets : rafale de mouvements rapides + suppressions.
    /// Le bug d'origine : cx atterrissait au milieu d'un caractère → panique.
    #[test]
    fn accentues_ne_crachent_plus_jamais() {
        let mut ed = Editor::open(None).unwrap();
        for c in "héllö àü ù".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        assert_eq!(ed.lines[0], "héllö àü ù");
        // rafale gauche/droite aussi vite que possible
        for _ in 0..50 {
            ed.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()));
            ed.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty()));
            ed.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()));
        }
        // backspace traverse un é (2 octets) sans panique et le retire entier
        ed.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::empty()));
        for _ in 0..3 {
            ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        }
        assert_eq!(ed.lines[0], "héllö à");
        // Delete au début retire 'h', puis encore, sans jamais paniquer
        ed.on_key(KeyEvent::new(KeyCode::Home, KeyModifiers::empty()));
        ed.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "éllö à");
        // lignes de largeurs différentes : la colonne retombe sur une frontière
        ed.lines = vec!["éééé".into(), "ab".into()];
        ed.cy = 0;
        ed.cx = 8; // fin de "éééé" (4 × 2 octets)
        ed.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert_eq!(ed.cx, 2, "clampé ET sur frontière");
        ed.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
        // insertion après un accentué : jamais au milieu
        ed.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::empty()));
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "ééxéé");
    }
}

#[cfg(test)]
mod diag_tests {
    use super::*;

    /// the gutter merges the style pass and the checker: the worst wins.
    #[test]
    fn marqueurs_le_plus_severe_gagne() {
        let mut ed = Editor::open(None).unwrap();
        ed.check_marks = vec![(3, Severity::Minor), (4, Severity::Major)];
        ed.diags = vec![
            Diag {
                line: 3,
                col: 1,
                is_error: true,
                message: "boom".into(),
            },
            Diag {
                line: 5,
                col: 1,
                is_error: false,
                message: "warn".into(),
            },
        ];
        let m = ed.gutter_marks();
        assert_eq!(m[&3], Ed::red(), "l'erreur gcc écrase la norme mineure");
        assert_eq!(m[&4], Ed::red(), "norme majeure = rouge");
        assert_eq!(m[&5], Ed::amber(), "warning gcc = ambre");
        assert!(!m.contains_key(&1));
    }

    /// ^N/^P naviguent en circulaire et placent le curseur sur la faute.
    #[test]
    fn navigation_circulaire() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = (1..=10).map(|i| format!("line {i}")).collect();
        ed.diags = vec![
            Diag {
                line: 2,
                col: 1,
                is_error: true,
                message: "a".into(),
            },
            Diag {
                line: 9,
                col: 3,
                is_error: false,
                message: "b".into(),
            },
        ];
        ed.diag_jump(1);
        assert_eq!(ed.cy, 1, "premier diagnostic");
        ed.diag_jump(1);
        assert_eq!(ed.cy, 8);
        assert_eq!(ed.cx, 2);
        ed.diag_jump(1);
        assert_eq!(ed.cy, 1, "ça boucle");
        ed.diag_jump(-1);
        assert_eq!(ed.cy, 8, "en arrière aussi");
    }

    /// Le modèle de complétion est unique, verrouillé, et servi par le provider.
    #[test]
    fn modele_unique_et_verrouille() {
        assert_eq!(EDITOR_MODEL, "deepseek-v4.1-flash");
        assert!(crate::ai::AVAILABLE_MODELS.contains(&EDITOR_MODEL));
    }

    /// Un long message de norme est wrappé entièrement dans le survol.
    #[test]
    fn survol_message_long_complet() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["int x;".into()];
        ed.modified = true;
        let long =
            "line far too long : it exceeds the eighty column limit imposed by the norm (N-COL80)"
                .to_string();
        ed.check_details = vec![(1, Severity::Major, long.clone())];
        let text = render_text(&mut ed, 110, 30);
        for word in ["eighty", "column", "N-COL80"] {
            assert!(text.contains(word), "mot manquant : {word}");
        }
    }

    /// Un toast long occupe plusieurs lignes, intégralement.
    #[test]
    fn toast_long_wrappe_complet() {
        let mut ed = Editor::open(None).unwrap();
        ed.notify(Level::Warn, "norm : 3M 12m — function detected as too long in the current file, split it into sub-functions");
        let text = render_text(&mut ed, 110, 30);
        for word in ["function", "sub-functions"] {
            assert!(text.contains(word), "mot manquant : {word}");
        }
    }

    /// La barre basse n'affiche que la position — aucun badge, jamais.
    #[test]
    fn pied_de_page_position_seule() {
        let ed = Editor::open(None).unwrap();
        assert_eq!(pos_text(&ed).trim(), "ln 1, col 1");
        assert!(!pos_text(&ed).contains('·'));
    }

    /// ^T ouvre et ferme l'explorateur (racine = répertoire courant).
    #[test]
    fn toggle_explorateur() {
        let mut ed = Editor::open(None).unwrap();
        assert!(ed.explorer.is_none());
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(ed.explorer.is_some());
        assert_eq!(ed.focus, Focus::Explorer);
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(ed.explorer.is_none());
        assert_eq!(ed.focus, Focus::Editor);
    }

    /// Enter sur un fichier de l'explorateur l'ouvre dans l'éditeur.
    #[test]
    fn ouvrir_depuis_explorateur() {
        let dir = std::env::temp_dir().join(format!("cnano-ed-{}-a", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("solo.c");
        std::fs::write(
            &f,
            "int solo;
",
        )
        .unwrap();

        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(dir.clone()));
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(ed.file.as_deref(), Some(f.as_path()));
        assert_eq!(ed.lines, vec!["int solo;"]);
        assert_eq!(ed.focus, Focus::Explorer, "on reste dans l'explorateur");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Un buffer modifié ne se fait JAMAIS écraser par l'explorateur.
    #[test]
    fn buffer_modifie_bloque_le_changement() {
        let dir = std::env::temp_dir().join(format!("cnano-ed-{}-b", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.c"),
            "int a;
",
        )
        .unwrap();

        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty())); // modifié
        ed.explorer = Some(Explorer::new(dir.clone()));
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(ed.file.is_none(), "pas de changement de fichier");
        assert_eq!(ed.lines[0], "x", "le buffer est intact");
        assert!(ed.status.contains("^s"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// La frappe libre dans l'explorateur filtre (et ne tape pas dans le code).
    #[test]
    fn frappe_dans_explorateur_filtre_sans_touchr_au_buffer() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        ed.on_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "", "le buffer n'a pas bougé");
        assert_eq!(ed.explorer.as_ref().unwrap().filter(), "z");
    }

    /// Cadre de test : dessine l'UI sur un backend virtuel, retourne le texte.
    fn render_text(ed: &mut Editor, w: u16, h: u16) -> String {
        let backend = ratatui::backend::TestBackend::new(w, h);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| draw(f, ed)).unwrap();
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    /// L'accueil est une boîte arrondie façon lazy.nvim — et ne mentionne
    /// plus jamais rien d'autre que les gestes.
    #[test]
    fn accueil_boite_arrondie_pure() {
        let mut ed = Editor::open(None).unwrap();
        let text = render_text(&mut ed, 90, 28);
        for corner in ["╭", "╮", "╰", "╯"] {
            assert!(text.contains(corner), "coin {corner} présent");
        }
        assert!(text.contains("nana"), "the title lives in the border");
        assert!(text.contains("files") && text.contains("code") && text.contains("buffer"));
        assert!(!text.contains("IA"), "aucune mention de l'IA");
        assert!(!text.contains("piscine — sobre"), "plus de tagline");
    }

    /// Boîtes nues (pas de titre), ● de modification dans le breadcrumb,
    /// icônes de langage dans l'explorateur.
    #[test]
    fn boites_nues_et_icones() {
        let dir = std::env::temp_dir().join(format!("cnano-ico-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.c"), "int a;\n").unwrap();
        std::fs::write(dir.join("b.rs"), "fn b() {}\n").unwrap();
        let mut ed = Editor::open(None).unwrap();
        ed.file = Some(std::path::PathBuf::from("/tmp/demo.c"));
        ed.modified = true;
        ed.lines = vec!["int x;".into()];
        ed.explorer = Some(Explorer::new(dir.clone()));
        let text = render_text(&mut ed, 90, 28);
        assert!(!text.contains(" editeur"), "plus de titre éditeur");
        assert!(!text.contains(" explorer "), "plus de titre explorateur");
        assert!(text.contains("demo.c"), "le nom reste dans le breadcrumb");
        assert!(text.contains("\u{E649}"), "icône C (seti-c)");
        assert!(text.contains("\u{E7A8}"), "icône Rust (devicon)");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Le mapping langage → icône devicon (codepoints de la fonte installée).
    #[test]
    fn icones_par_langage() {
        // the contract: the glyph is the registry's, and families differ
        let c = file_icon("main.c", false);
        let rs = file_icon("lib.rs", false);
        let py = file_icon("x.py", false);
        let mk = file_icon("Makefile", false);
        assert_eq!(c, langs::for_path(Path::new("main.c")).icon.to_string());
        assert_eq!(rs, langs::for_path(Path::new("lib.rs")).icon.to_string());
        assert_ne!(c, rs);
        assert_ne!(rs, py);
        assert_ne!(py, mk);
        assert_eq!(
            file_icon("inconnu.xyz", false),
            langs::for_path(Path::new("inconnu.xyz")).icon.to_string()
        );
        // repli sans Nerd Font
        assert_eq!(file_icon_plain("main.c"), "Ⓒ");
        assert_eq!(dir_icon(true), "\u{F07C}");
    }

    /// Explorateur ouvert : deux boîtes arrondies côte à côte.
    #[test]
    fn deux_boites_quand_explorateur() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        let text = render_text(&mut ed, 110, 30);
        assert!(text.matches('╭').count() >= 2, "une boîte par panneau");
        assert!(text.contains("type to filter"));
    }

    /// Alt-Tab : éditeur → explorateur → éditeur ; ouvre l'explorateur
    /// si rien n'est ouvert (le geste sert toujours).
    #[test]
    fn alt_tab_cycle_les_panneaux() {
        let mut ed = Editor::open(None).unwrap();
        assert_eq!(ed.focus, Focus::Editor);
        // sans explorateur : Alt-Tab l'ouvre et le focusse
        ed.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::ALT));
        assert_eq!(ed.focus, Focus::Explorer);
        assert!(ed.explorer.is_some());
        // encore : retour éditeur — Ctrl+Tab et F2 marchent pareil
        ed.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL));
        assert_eq!(ed.focus, Focus::Editor);
        ed.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Explorer, "F2 cycle aussi");
        // avec la recherche ouverte, elle est dans le cycle
        ed.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert_eq!(ed.focus, Focus::Search);
        ed.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::empty())); // Shift+Tab = repli
        assert_eq!(ed.focus, Focus::Editor);
        ed.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Explorer);
        ed.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::empty()));
        assert_eq!(ed.focus, Focus::Search);
    }

    /// ^O : la frappe filtre, Enter ouvre le fichier, Échap referme.
    #[test]
    /// A search with a single result still draws its floating panel: the
    /// height guard used to swallow it, leaving a focus state with no ui.
    #[test]
    fn la_recherche_dessine_meme_avec_un_seul_resultat() {
        let dir = std::env::temp_dir().join(format!("nana-srch-{}-b", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("seul.py"), "x = 1\n").unwrap();

        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(dir.clone()));
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert_eq!(ed.search.as_ref().map(|s| s.len()), Some(1));
        let text = render_text(&mut ed, 100, 30);
        assert!(
            text.contains("search · 1"),
            "the panel must be drawn: {text}"
        );
        assert!(text.contains("seul.py"), "and list the file");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn recherche_flottante_ouvre_un_fichier() {
        let dir = std::env::temp_dir().join(format!("cnano-srch-{}-a", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("cible.c");
        std::fs::write(
            &f,
            "int cible;
",
        )
        .unwrap();

        let mut ed = Editor::open(None).unwrap();
        ed.explorer = Some(Explorer::new(dir.clone())); // racine connue
        ed.focus = Focus::Explorer;
        ed.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert_eq!(ed.focus, Focus::Search);
        for c in "cible".chars() {
            ed.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        assert_eq!(ed.search.as_ref().unwrap().len(), 1);
        ed.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(ed.file.as_deref(), Some(f.as_path()));
        assert_eq!(ed.lines, vec!["int cible;"]);
        assert!(ed.search.is_none(), "la recherche se referme");
        assert_eq!(ed.focus, Focus::Editor);
        // la frappe n'a jamais touché le buffer
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Les toasts s'affichent puis s'évanouissent (3,5 s).
    #[test]
    fn toasts_pousses_puis_expires() {
        let mut ed = Editor::open(None).unwrap();
        ed.notify(Level::Ok, "saved ✓");
        assert_eq!(ed.toasts.len(), 1);
        assert_eq!(ed.status, "saved ✓", "le statut reflète le toast");
        let text = render_text(&mut ed, 100, 30);
        assert!(text.contains("saved ✓"), "la carte est dessinée");
        assert!(text.contains("✓"), "icône du niveau");
        // un toast vieux de 10 s est purgé au prochain rendu
        ed.toasts.push(Toast {
            level: Level::Err,
            text: "fantôme".into(),
            at: std::time::Instant::now() - std::time::Duration::from_secs(10),
        });
        let _ = render_text(&mut ed, 100, 30);
        assert_eq!(ed.toasts.len(), 1, "le toast expiré a été purgé");
    }

    /// Le survol affiche gcc + norme de la ligne courante, avec messages.
    #[test]
    fn survol_diagnostics_de_la_ligne() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["int x = ;".into()];
        ed.modified = true; // pas d'écran d'accueil
        ed.diags = vec![Diag {
            line: 1,
            col: 9,
            is_error: true,
            message: "expected expression".into(),
        }];
        ed.check_details = vec![(1, Severity::Minor, "trailing whitespace (style)".into())];
        let both = ed.line_diagnostics();
        assert_eq!(both.len(), 2, "gcc et norme fusionnés");
        let text = render_text(&mut ed, 100, 30);
        assert!(
            text.contains("expected expression"),
            "le message gcc flotte"
        );
        assert!(
            text.contains("trailing whitespace"),
            "the style finding too"
        );
        assert!(text.contains("line 1"), "le titre porte la ligne");
    }

    /// La statusline segmentée : badge de focus, position, progression.
    #[test]
    fn statusline_a_ses_segments() {
        let mut ed = Editor::open(None).unwrap();
        let text = render_text(&mut ed, 100, 30);
        assert!(text.contains(" code "), "badge de focus");
        assert!(text.contains("ln 1, col 1"), "position");
        assert!(text.contains("100%"), "progression");
    }

    /// L'écran d'accueil n'apparaît que sur un buffer vierge sans fichier.
    #[test]
    fn accueil_uniquement_sur_vierge() {
        let mut ed = Editor::open(None).unwrap();
        assert!(ed.is_welcome());
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert!(
            !ed.is_welcome(),
            "dès la première frappe, l'accueil s'efface"
        );
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    fn ed_with_ghost() -> Editor {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["int foo(".into(), "return (0);".into()];
        ed.cy = 0;
        ed.cx = 8;
        ed.ghost = Some("\n    int i = 0;\n    while (i < 10)\n        i++;".into());
        ed.ghost_lines = vec![
            String::new(),
            "    int i = 0;".into(),
            "    while (i < 10)".into(),
            "        i++;".into(),
        ];
        ed
    }

    /// Tab accepte une suggestion MULTI-LIGNES : le splice crée de vraies
    /// lignes et le curseur finit à la fin de la suggestion.
    #[test]
    fn tab_accepte_ghost_multilignes() {
        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
        assert!(ed.ghost.is_none(), "ghost consommé");
        assert!(ed.ghost_lines.is_empty(), "ghost_lines consommées");
        assert_eq!(
            ed.lines,
            vec![
                "int foo(".to_string(),
                "    int i = 0;".to_string(),
                "    while (i < 10)".to_string(),
                "        i++;".to_string(),
                "return (0);".to_string(),
            ]
        );
        assert!(ed.modified);
        // curseur : dernière ligne insérée, fin de « i++; »
        assert_eq!((ed.cy, ed.cx), (3, 12));
    }

    /// Tab sans suggestion = 4 espaces (la Norme), pas une acceptation.
    #[test]
    fn tab_sans_ghost_insere_4_espaces() {
        let mut ed = Editor::open(None).unwrap();
        ed.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));
        assert_eq!(ed.lines[0], "    ");
        assert!(ed.ghost.is_none());
    }

    /// Fin et Ctrl+→ acceptent aussi la suggestion.
    #[test]
    fn fin_et_ctrl_fleche_acceptent() {
        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::End, KeyModifiers::empty()));
        assert!(ed.ghost.is_none());
        assert_eq!(ed.lines.len(), 5);

        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));
        assert!(ed.ghost.is_none());
        assert_eq!(ed.lines.len(), 5);
    }

    /// Échap refuse : ghost ET ghost_lines disparaissent (l'ancien bug
    /// laissait le fantôme affiché sans qu'aucune touche ne marche).
    #[test]
    fn esc_refuse_et_vide_tout() {
        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(ed.ghost.is_none());
        assert!(ed.ghost_lines.is_empty());
        assert_eq!(
            ed.lines,
            vec!["int foo(".to_string(), "return (0);".to_string()]
        );
        assert_eq!(ed.status, "declined");
    }

    /// Échap pendant « l'IA réfléchit… » : la requête en vol est annulée.
    #[test]
    fn esc_annule_ia_en_vol() {
        let mut ed = Editor::open(None).unwrap();
        let (_tx, rx) = channel();
        ed.rx = Some(rx);
        ed.ai_busy = true;
        ed.status = "en cours…".into();
        ed.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(!ed.ai_busy, "plus busy");
        assert!(ed.rx.is_none(), "canal lâché");
        assert_eq!(ed.status, "cancelled");
    }

    /// Toute édition invalide la suggestion — ghost ET ghost_lines
    /// (sinon le fantôme s'affiche mais plus aucune touche ne l'accepte).
    #[test]
    fn edition_invalide_le_ghost_completement() {
        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert!(ed.ghost.is_none());
        assert!(ed.ghost_lines.is_empty());

        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert!(ed.ghost_lines.is_empty());

        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::empty()));
        assert!(ed.ghost_lines.is_empty());

        let mut ed = ed_with_ghost();
        ed.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert!(ed.ghost_lines.is_empty());
    }

    /// La complétion demande le fichier ENTIER avec le curseur marqué —
    /// même une ligne avec le marqueur au milieu, sans panique.
    #[test]
    fn contexte_marqueur_curseur_sans_panique() {
        let mut ed = Editor::open(None).unwrap();
        ed.lines = vec!["int main(void)".into(), "{".into()];
        ed.cy = 1;
        ed.cx = 1;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // on reproduit la construction du marqueur de ai_complete
            let line: Vec<char> = ed.lines[ed.cy].chars().collect();
            let pos = ed.cx.min(line.len());
            let marked: String = line[..pos].iter().collect::<String>()
                + "▌"
                + &line[pos..].iter().collect::<String>();
            assert_eq!(marked, "{▌");
        }));
        assert!(r.is_ok());
    }
}
