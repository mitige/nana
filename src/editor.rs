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

/// how long a transient line lives before the bar goes quiet again.
const STATUS_TTL: Duration = Duration::from_secs(4);
/// the last stretch of that life, during which the line dims out.
const FADE_OUT: Duration = Duration::from_millis(900);
/// how long the verdict stays in the corner after a check. it is news, not
/// furniture: it fades, and the next save brings it back.
const CHECK_TTL: Duration = Duration::from_secs(10);
/// a toast slides in over this long, then sits still.
const SLIDE_IN: Duration = Duration::from_millis(160);
/// how long a toast lives, and how long it spends fading out.
const TOAST_TTL: Duration = Duration::from_millis(3500);
/// one playback tick is one frame of the animation loop, at about 60 fps.
const TICK: Duration = Duration::from_millis(16);
/// how long a new view takes to sweep in from the left.
const VIEW_SWEEP: Duration = Duration::from_millis(240);
/// how long the agent's cursor takes to glide from the line it left to the next.
const CURSOR_GLIDE: Duration = Duration::from_millis(160);
/// the spinner: ten braille frames, one every 80 ms.
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// the rounded ends of a pill segment (powerline half circles).
const CAP_L: &str = "\u{E0B6}";
const CAP_R: &str = "\u{E0B4}";

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
    /// Survol de la hub : une bande sarcelle, nettement distincte du noir des
    /// autres sélections, pour que la ligne visée se voie au premier regard.
    fn hover() -> Color {
        Color::Indexed(23)
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

/// the agent panel: a rounded card with the conversation on top and a prompt
/// line at the bottom. it owns the keyboard while it is open — esc closes it,
/// and esc again while it is thinking cancels the request.
struct AgentPane {
    input: String,
    /// the transcript as it arrives: (kind, text)
    lines: Vec<(String, String)>,
    scroll: usize,
    rx: Option<Receiver<crate::agent::Event>>,
    busy: bool,
    status: String,
    /// the persona this conversation runs under, read from the project
    persona: Option<String>,
    /// file writes announced by the agent, drained by the editor each frame
    writes: Vec<LiveWrite>,
    /// every action of this conversation, placed in time: the background of the card
    trail: Vec<crate::trail::Step>,
    /// when the card opened: the trail's clock starts here
    started: std::time::Instant,
    /// when the agent last produced anything: the time it took to reach the
    /// next write is the time that write lasts on screen
    last_event: std::time::Instant,
    /// the tool call waiting for its answer, and whether its file was already
    /// announced (a write or a read plays as the file, not as a tool action)
    pending: Option<(String, serde_json::Value, bool)>,
    /// the transcript already wrapped to the card's width, so a frame only
    /// wraps the lines that arrived since the last one
    transcript: TranscriptRows,
    /// project root and session id, to know where the conversation is kept
    root: PathBuf,
    session: String,
    /// how the conversation opened (the first message sent) and how the
    /// agent's last reply closed it; both go to memory as they are known
    opening: Option<String>,
    closing: Option<String>,
}

impl AgentPane {
    /// keeps the opening and the closing of this conversation in the project
    /// memory. the memory is best effort: a failed write must not break a chat.
    fn save_turn_now(&self) -> bool {
        let (Some(opening), Some(closing)) = (&self.opening, &self.closing) else {
            return false;
        };
        crate::memory::Memory::open(&self.root)
            .save_turn(&self.session, opening, closing)
            .is_ok()
    }
}

/// the transcript as rows ready to paint. lines are only ever appended, so the
/// rows of the lines already wrapped stay valid; a new width starts over.
#[derive(Default)]
struct TranscriptRows {
    width: usize,
    /// how many transcript lines are wrapped into `rows`
    wrapped: usize,
    rows: Vec<Line<'static>>,
}

impl TranscriptRows {
    /// wrap the lines that are new since the last call, at `width`.
    fn sync(&mut self, lines: &[(String, String)], width: usize) {
        if width != self.width || lines.len() < self.wrapped {
            self.width = width;
            self.wrapped = 0;
            self.rows.clear();
        }
        for (kind, text) in &lines[self.wrapped..] {
            let (label, color) = transcript_label(kind);
            let mut first = true;
            for chunk in wrap(text, width.saturating_sub(6)) {
                self.rows.push(Line::from(vec![
                    Span::styled(
                        if first { label } else { "     " },
                        Style::default().fg(color),
                    ),
                    Span::styled(chunk, Style::default().fg(Ed::text())),
                ]));
                first = false;
            }
        }
        self.wrapped = lines.len();
    }
}

/// the label and colour a transcript line is painted with, by its kind.
fn transcript_label(kind: &str) -> (&'static str, ratatui::style::Color) {
    match kind {
        "you" => ("you  ", Ed::accent()),
        "agent" => ("nana ", Ed::cyan()),
        "tool" => ("->   ", Ed::amber()),
        "ok" => ("ok   ", Ed::green()),
        "refused" => ("no   ", Ed::red()),
        "error" => ("err  ", Ed::red()),
        _ => ("     ", Ed::gutter()),
    }
}

impl AgentPane {
    /// the transcript's rows at `width`, wrapping only what is new.
    fn rendered_rows(&mut self, width: usize) -> &[Line<'static>] {
        self.transcript.sync(&self.lines, width);
        &self.transcript.rows
    }

    fn new() -> AgentPane {
        AgentPane {
            input: String::new(),
            lines: vec![(
                "note".into(),
                "ask for something in this project. enter sends, esc closes.".into(),
            )],
            scroll: 0,
            rx: None,
            busy: false,
            status: String::new(),
            persona: None,
            writes: Vec::new(),
            root: PathBuf::new(),
            session: String::new(),
            opening: None,
            closing: None,
            trail: Vec::new(),
            started: std::time::Instant::now(),
            last_event: std::time::Instant::now(),
            pending: None,
            transcript: TranscriptRows::default(),
        }
    }

    /// one action on the trail, stamped with the time since the card opened.
    fn record(&mut self, lane: crate::trail::Lane, label: String, ok: bool) {
        let at = self.started.elapsed().as_secs_f32();
        self.trail.push(crate::trail::Step {
            lane,
            label: safe_text(&label).replace('\n', " "),
            at,
            ok,
        });
    }

    /// start a request on a worker thread; the events come back one by one.
    fn send(&mut self, root: &Path) {
        let request = self.input.trim().to_string();
        if request.is_empty() || self.busy {
            return;
        }
        self.input.clear();
        if self.opening.is_none() {
            self.opening = Some(request.clone());
            self.root = root.to_path_buf();
            self.session = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "0".into());
        }
        self.lines.push(("you".into(), request.clone()));
        self.scroll = 0;
        self.busy = true;
        self.status = "thinking".into();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let root = root.to_path_buf();
        crate::guard::spawn_streaming(tx, move |tx| {
            let settings = crate::settings::Settings::load(&root);
            match crate::agent::Agent::new(&root, settings) {
                Ok(mut agent) => {
                    let _ = agent.run(&request, |event| {
                        let _ = tx.send(event);
                    });
                }
                Err(e) => {
                    let _ = tx.send(crate::agent::Event::Error(e));
                }
            }
        });
    }

    /// fold whatever the worker produced since the last frame.
    fn poll(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };
        let mut done = false;
        while let Ok(event) = rx.try_recv() {
            let waited = self.last_event.elapsed();
            if !matches!(event, crate::agent::Event::ToolCall { .. }) {
                self.last_event = Instant::now();
            }
            match event {
                crate::agent::Event::Started {
                    model, provider, ..
                } => {
                    self.status = format!("{model} via {provider}");
                }
                crate::agent::Event::Text(t) => {
                    let t = safe_text(&t);
                    self.record(crate::trail::Lane::Say, t.clone(), true);
                    self.closing = Some(t.clone());
                    self.lines.push(("agent".into(), t));
                }
                crate::agent::Event::ToolCall { name, args } => {
                    let label = format!("{name} {}", short_json(&args));
                    self.record(crate::trail::Lane::Tool, label.clone(), true);
                    self.lines.push(("tool".into(), label));
                    self.pending = Some((name, args, false));
                }
                crate::agent::Event::ToolResult { name, ok, text } => {
                    self.record(
                        crate::trail::Lane::Result,
                        format!("{name}: {}", text.trim()),
                        ok,
                    );
                    // every action is played, not only the files: a tool that
                    // did not announce a file shows its call and its answer
                    if let Some((call, args, shown)) = self.pending.take() {
                        if call == name && !shown {
                            let mut text_lines = vec![terminal_header(&name, &args)];
                            text_lines.extend(text.trim().lines().map(safe_text));
                            self.writes.push(LiveWrite {
                                path: PathBuf::from(format!("{ACTIONS_VIEW}: {name}")),
                                text: text_lines,
                                shown: 0,
                                col: 0,
                                ticks: ticks_for(waited),
                                elapsed: 0,
                                write: false,
                            });
                        }
                    }
                    self.lines.push((
                        if ok { "ok".into() } else { "refused".into() },
                        format!("{name}: {}", safe_text(text.trim())),
                    ));
                }
                crate::agent::Event::File {
                    path,
                    content,
                    write,
                } => {
                    self.record(crate::trail::Lane::File, path.clone(), true);
                    if let Some(call) = &mut self.pending {
                        call.2 = true;
                    }
                    self.writes.push(LiveWrite {
                        path: PathBuf::from(&path),
                        text: content.lines().map(safe_text).collect(),
                        shown: 0,
                        col: 0,
                        ticks: ticks_for(waited),
                        elapsed: 0,
                        write,
                    });
                    self.lines.push(("file".into(), safe_text(&path)));
                }
                crate::agent::Event::Finished { steps } => {
                    self.status = format!("done in {steps} step(s)");
                    self.save_turn_now();
                    done = true;
                }
                crate::agent::Event::Error(e) => {
                    self.record(crate::trail::Lane::Result, e.clone(), false);
                    self.lines.push(("error".into(), safe_text(&e)));
                    self.status = "failed".into();
                    done = true;
                }
            }
        }
        if !done {
            self.rx = Some(rx);
        } else {
            self.busy = false;
        }
    }
}

/// one line of json, short enough for the transcript.
fn short_json(v: &serde_json::Value) -> String {
    let s = v.to_string().replace('\n', " ");
    if s.chars().count() <= 70 {
        s
    } else {
        s.chars().take(70).collect::<String>() + "…"
    }
}

/// the line that opens an action in the live view. a shell command reads as a
/// terminal prompt, `$ ls`, so the eye knows the agent ran it; any other tool
/// keeps its name and its json arguments.
fn terminal_header(name: &str, args: &serde_json::Value) -> String {
    let arg = |key: &str| args.get(key).and_then(|c| c.as_str()).unwrap_or("");
    let command = match name {
        "run_shell" => Some(arg("command").replace('\n', " ")),
        "list_dir" => Some(format!("ls {}", arg("path"))),
        "read_file" => Some(format!("cat {}", arg("path"))),
        "grep" => Some(format!("grep {} {}", arg("pattern"), arg("path"))),
        _ => None,
    };
    match command {
        Some(command) => format!("$ {}", command.trim_end()),
        None => format!("> {name} {}", short_json(args)),
    }
}

/// what a cell is allowed to show. text from the agent or from a file can carry
/// bytes a terminal acts on (escape sequences, carriage returns, bells) or marks
/// that take no room but still move things about. they are removed where the
/// text comes in; tabs become spaces to the next stop, and a line break is kept.
fn safe_text(s: &str) -> String {
    let mut clean = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => skip_escape(&mut chars),
            '\t' | '\n' => clean.push(c),
            c if c.is_control() || is_invisible(c) => {}
            c => clean.push(c),
        }
    }
    clean
        .split('\n')
        .map(expand_tabs)
        .collect::<Vec<_>>()
        .join("\n")
}

/// marks that take no room on screen and still change what a terminal shows.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{feff}'
    )
}

/// the end of an escape sequence, already past its ESC: a CSI runs to its final
/// byte, a string (OSC, DCS...) to its bell or its terminator, and any other
/// pair is two bytes.
fn skip_escape(chars: &mut std::str::Chars) {
    match chars.next() {
        Some('[') => {
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    break;
                }
            }
        }
        Some(']' | 'P' | 'X' | '^' | '_') => {
            while let Some(c) = chars.next() {
                if c == '\u{7}' {
                    break;
                }
                if c == '\u{1b}' {
                    chars.next();
                    break;
                }
            }
        }
        _ => {}
    }
}

/// the name prefix of the windows that show the agent's tool actions (memory,
/// search, shell): they are not files. each action has its own window named
/// `agent actions: <tool>`, so a later action never erases an earlier one.
const ACTIONS_VIEW: &str = "agent actions";

/// whether a window is an action window. paths compare by components, so this
/// must look at the text: "agent actions: run_shell" does not start a path
/// component-wise with "agent actions".
fn is_action_view(path: &std::path::Path) -> bool {
    path.to_string_lossy().starts_with(ACTIONS_VIEW)
}

/// the agent's view: the file it is writing or reading, line by line, with its
/// own cursor. it is a separate buffer from the user's, so neither overrides
/// the other.
#[derive(Debug, Clone, Default)]
struct AgentView {
    path: Option<PathBuf>,
    lines: Vec<String>,
    cy: usize,
    cx: usize,
    write: bool,
    /// when this view was swept in: it slides in from the left over VIEW_SWEEP
    opened: Option<Instant>,
    /// the line the cursor left, and when it left it: the cursor glides from there
    glide: Option<(usize, Instant)>,
}

/// a write in progress, played back: the text the agent is putting in a file,
/// and how many of its lines are on screen so far.
#[derive(Debug, Clone)]
struct LiveWrite {
    path: PathBuf,
    text: Vec<String>,
    shown: usize,
    /// characters of line `shown` already on screen
    col: usize,
    /// ticks the whole text must take; 0 means one letter per tick
    ticks: usize,
    /// ticks played so far
    elapsed: usize,
    /// false for a read: the buffer shows the file, it is not changed by it
    write: bool,
}

/// the frames a write lasts: the time the agent took to produce it, in ticks.
/// a write that followed a long pause plays slowly; one that followed a quick
/// step plays fast. nothing here is a fixed pace.
fn ticks_for(waited: Duration) -> usize {
    (waited.as_millis() / TICK.as_millis()).max(1) as usize
}

/// the most ticks the playback may owe the agent: about a second and a half at
/// 60 fps. past it, the waiting writes are shortened so the screen catches up
/// with the agent instead of showing what it did a minute ago.
const LAG_BUDGET: usize = 94;
/// the most writes that may wait behind the one on screen. beyond it the oldest
/// land at once: a flood must not turn into a queue the user watches for minutes.
const MAX_QUEUE: usize = 24;

/// keeps the playback in step with the agent. the text of every write still
/// lands in full; only the time it takes shrinks. writes without a duration
/// (ticks 0, one letter per tick) are left as they were.
fn catch_up(live: &mut [LiveWrite]) {
    let n = live.len();
    if n > MAX_QUEUE {
        for w in live[..n - MAX_QUEUE].iter_mut().filter(|w| w.ticks > 0) {
            w.ticks = w.ticks.min(w.elapsed + 1);
        }
    }
    let owed: usize = live
        .iter()
        .filter(|w| w.ticks > 0)
        .map(|w| w.ticks.saturating_sub(w.elapsed))
        .sum();
    if owed <= LAG_BUDGET {
        return;
    }
    for w in live.iter_mut().filter(|w| w.ticks > 0) {
        let left = w.ticks.saturating_sub(w.elapsed);
        let fit = (left * LAG_BUDGET / owed).max(1);
        w.ticks = w.elapsed + fit;
    }
}

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
    /// when that check landed: the indicator pulses for a moment
    check_at: Option<Instant>,
    /// the animation clock — frames come from here, never from wall time
    epoch: Instant,
    /// when the transient line was last written, and what it said
    status_at: Instant,
    status_seen: String,
    /// the user's typing rhythm: when the quiet is long enough, the memory is looked at
    quiet: crate::quiet::Quiet,
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
    /// the agent panel (^a): a conversation with the project's agent
    agent_pane: Option<AgentPane>,
    /// writes the agent announced, played back one line per step; the first is
    /// the one on screen
    live: Vec<LiveWrite>,
    /// the agent's own view of the file it writes or reads: a buffer and a
    /// cursor of its own, drawn behind the card, never in the user's buffer
    agent_view: AgentView,
    /// the hub (^w): the boxes — memory, providers, skills, personas, agents
    hub: Option<crate::hub::Hub>,
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
    /// consolidation de la mémoire en arrière-plan, et son déclencheur adaptatif
    consolidation: crate::memory::ConsolidationTrigger,
    consolidation_rx: Option<Receiver<Result<usize, String>>>,
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
    /// the start screen is the hub, for the project at `root`: the ide stays
    /// behind it until the user closes the menu.
    pub fn open_hub_at_start(&mut self, root: &Path) {
        self.hub = Some(crate::hub::Hub::open(root));
    }

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
        let status = "f2 panels · ^b check · f5 run · f6 format · ^s save · ^q quit".to_string();
        let status_seen = status.clone();
        let mut ed = Self {
            lines,
            cx: 0,
            cy: 0,
            scroll_x: 0,
            scroll_y: 0,
            file,
            modified: false,
            saved,
            status,
            ghost: None,
            ghost_lines: Vec::new(),
            ai_busy: false,
            rx: None,
            clipboard: String::new(),
            should_quit: false,
            confirm_quit: false,
            complete_agent: load_complete_agent(),
            check_note: None,
            check_at: None,
            epoch: Instant::now(),
            status_at: Instant::now(),
            status_seen: status_seen.clone(),
            quiet: crate::quiet::Quiet::default(),
            check_marks: Vec::new(),
            check_rx: None,
            diags: Vec::new(),
            diag_rx: None,
            diag_idx: None,
            prompt: None,
            history: Vec::new(),
            ext,
            ai: None,
            agent_pane: None,
            live: Vec::new(),
            agent_view: AgentView::default(),
            hub: None,
            explorer: None,
            search: None,
            focus: Focus::Editor,
            toasts: Vec::new(),
            confirm_delete: None,
            header_rx: None,
            consolidation: crate::memory::ConsolidationTrigger::default(),
            consolidation_rx: None,
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

    /// split the line here, with no auto-indent: pasting must not reformat.
    fn split_line_here(&mut self) {
        let rest = self.lines[self.cy].split_off(self.cx);
        self.cy += 1;
        self.lines.insert(self.cy, rest);
        self.cx = 0;
    }

    /// Paste a block of text verbatim. Typing enter auto-indents, which is
    /// right for writing code and wrong for pasting it: what you copied is
    /// what you get, newlines included, and nothing grows a tab of its own.
    fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // the agent card owns the keyboard: its paste goes into the prompt,
        // line breaks kept, and never into the user's buffer behind the card
        if let Some(pane) = self.agent_pane.as_mut() {
            let cleaned = safe_text(&text.replace("\r\n", "\n").replace('\r', "\n"));
            pane.input.push_str(&cleaned);
            return;
        }
        // a modal card owns the keyboard: a paste goes nowhere while it is open
        if self.hub.is_some() {
            return;
        }
        self.snapshot();
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut lines = 0;
        for (i, part) in normalized.split('\n').enumerate() {
            if i > 0 {
                self.split_line_here();
                lines += 1;
            }
            if part.is_empty() {
                continue;
            }
            let line = &mut self.lines[self.cy];
            line.insert_str(self.cx, part);
            self.cx += part.len();
        }
        self.sync_modified();
        self.clear_ghost();
        self.status = format!("pasted {lines} line(s)");
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
                // no file name in the message: it is already in the tree and
                // in the breadcrumb — the bar only says what just happened
                self.notify(Level::Ok, "saved");
                // the check runs off the ui thread after every save: the
                // language's own tool plus the universal style pass
                let p = path.clone();
                let (tx, rx) = channel();
                crate::guard::spawn_guarded(tx, move || {
                    let cfg = crate::config::load().style.to_cfg();
                    crate::check::check_path(&p, &cfg)
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

    /// collect what the agent worker produced, each frame.
    fn poll_agent(&mut self) {
        if let Some(pane) = &mut self.agent_pane {
            pane.poll();
            self.live.extend(pane.writes.drain(..));
            catch_up(&mut self.live);
        }
        if let Some(hub) = &mut self.hub {
            hub.poll();
        }
    }

    /// one step of a write in progress: the next line of the agent's text
    /// lands in the buffer, the cursor follows it, and when the text is whole
    /// the buffer is the saved file, so nothing reads as an unsaved edit.
    /// the user has moved on (card closed, another file opened, the hub
    /// taken): the playback stops where it is, what was shown stays shown.
    fn interrupt_live(&mut self) {
        self.live.clear();
    }

    fn play_live(&mut self) {
        let Some(w) = self.live.first_mut() else {
            return;
        };
        // the agent's text goes to the agent's own view. the user's buffer,
        // its file, its cursor and its unsaved work are never touched here.
        // every action has its own window, as a file does: nothing is piled
        // into a view shared with another action
        let old_cy = self.agent_view.cy;
        let mut fresh = false;
        if w.shown == 0 && w.col == 0 {
            fresh = true;
            self.agent_view = AgentView {
                path: Some(w.path.clone()),
                lines: Vec::new(),
                cy: 0,
                cx: 0,
                write: w.write,
                opened: Some(Instant::now()),
                glide: None,
            };
            // the tree shows the file being worked on, as a person's cursor would
            if let Some(ex) = &mut self.explorer {
                if !is_action_view(&w.path) {
                    ex.select_path(&w.path);
                }
            }
        }
        // the text lands against a cumulative target: after tick n of `ticks`,
        // total * n / ticks letters are on screen, so the write ends on its
        // last tick whatever its length. ticks 0 is one letter per tick, so
        // the cursor moves along the line the way a person types.
        let total: usize = w.text.iter().map(|l| l.chars().count()).sum();
        let before: usize = w.text[..w.shown.min(w.text.len())]
            .iter()
            .map(|l| l.chars().count())
            .sum::<usize>()
            + w.col;
        w.elapsed += 1;
        let target = if w.ticks == 0 {
            before + 1
        } else {
            (total * w.elapsed / w.ticks).min(total)
        };
        let last_tick = w.ticks > 0 && w.elapsed >= w.ticks;
        let mut revealed = before;
        while w.shown < w.text.len() && (revealed < target || last_tick) {
            let line = &w.text[w.shown];
            let len = line.chars().count();
            if w.col == 0 {
                self.agent_view.lines.push(String::new());
            }
            if w.col < len {
                if let (Some(ch), Some(out)) =
                    (line.chars().nth(w.col), self.agent_view.lines.last_mut())
                {
                    out.push(ch);
                }
                w.col += 1;
                revealed += 1;
            }
            if w.col >= len {
                w.shown += 1;
                w.col = 0;
            }
        }
        // the cursor moved down a line: it glides from the line it left
        let new_cy = self.agent_view.lines.len().saturating_sub(1);
        if !fresh && !self.agent_view.lines.is_empty() && new_cy != old_cy {
            self.agent_view.glide = Some((old_cy, Instant::now()));
        }
        if !self.agent_view.lines.is_empty() {
            self.agent_view.cy = new_cy;
            self.agent_view.cx = self.agent_view.lines[self.agent_view.cy].chars().count();
        }
        if w.shown >= w.text.len() {
            if self.agent_view.lines.is_empty() {
                self.agent_view.lines.push(String::new());
            }
            self.live.remove(0);
        }
    }

    fn poll_check(&mut self) {
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
    /// the transient line ages: stamped whenever `status` actually changes,
    /// so any action brings it back and it fades on its own afterwards.
    fn tick_status(&mut self) {
        if self.status != self.status_seen {
            self.status_seen = self.status.clone();
            self.status_at = Instant::now();
        }
    }

    fn status_age(&self) -> Duration {
        self.status_at.elapsed()
    }

    /// is a background job running? then the spinner turns and says what.
    fn busy(&self) -> Option<&'static str> {
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
    fn spinner(&self) -> &'static str {
        let i = (self.epoch.elapsed().as_millis() / 80) as usize % SPINNER.len();
        SPINNER[i]
    }

    /// redraw fast while something moves, slowly when the screen is still.
    fn animating(&self) -> bool {
        !self.live.is_empty()
            || self.busy().is_some()
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

    fn notify(&mut self, level: Level, text: impl Into<String>) {
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
    fn poll_quiet(&mut self, now: Instant) {
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
    fn poll_consolidation(&mut self) {
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
        self.notify(Level::Ok, "header inserted");
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

    // -------------------------------------------------------------------- hub

    /// the hub is modal too: it browses the boxes, and enter acts on the
    /// selected item.
    fn hub_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.hub.is_none() {
            return;
        }
        // each branch takes the borrow and gives it straight back: acting on an
        // item touches the rest of the editor, and a held borrow would forbid it
        match key.code {
            KeyCode::Esc => self.hub = None,
            KeyCode::Up => {
                if let Some(h) = self.hub.as_mut() {
                    h.up();
                }
            }
            KeyCode::Down => {
                if let Some(h) = self.hub.as_mut() {
                    h.down();
                }
            }
            KeyCode::Left => {
                if let Some(h) = self.hub.as_mut() {
                    h.prev_section();
                }
            }
            KeyCode::Right | KeyCode::Tab => {
                if let Some(h) = self.hub.as_mut() {
                    h.next_section();
                }
            }
            KeyCode::Backspace => {
                if let Some(h) = self.hub.as_mut() {
                    h.backspace();
                }
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(h) = self.hub.as_mut() {
                    h.type_char(c);
                }
            }
            KeyCode::Enter => self.hub_act(),
            // the box does the work: no shell needed to write a persona, cut a
            // skill or hire a team
            KeyCode::Char('n') if ctrl => {
                if let Some(h) = self.hub.as_mut() {
                    let p = h.new_purpose();
                    h.begin(p);
                }
            }
            KeyCode::Char('e') if ctrl => self.hub_edit(),
            KeyCode::Char('d') if ctrl => self.hub_delete(),
            KeyCode::Char('t') if ctrl => self.hub_task(),
            // ctrl+r: dream over the latest sessions, in the knowledge box
            KeyCode::Char('r') if ctrl => {
                if let Some(h) = self.hub.as_mut() {
                    if h.current() == crate::hub::Section::Knowledge {
                        h.begin(crate::hub::Purpose::Dream);
                        let outcome = h.submit();
                        if let Ok(crate::hub::Outcome::Say(m)) = outcome {
                            self.status = m;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// ctrl+e: open the file behind the selection, to edit it here.
    fn hub_edit(&mut self) {
        let Some(hub) = self.hub.as_mut() else {
            return;
        };
        match hub.edit_selected() {
            Ok(crate::hub::Outcome::OpenFile(path)) => {
                self.hub = None;
                self.open_path(path);
            }
            Ok(crate::hub::Outcome::Say(m)) => self.status = m,
            Ok(crate::hub::Outcome::None) => {}
            Err(e) => {
                if let Some(h) = self.hub.as_mut() {
                    h.activity.push(("error".into(), e));
                }
            }
        }
    }

    /// ctrl+d: forget a page, delete a persona or a skill, fire an employee.
    fn hub_delete(&mut self) {
        let Some(hub) = self.hub.as_mut() else {
            return;
        };
        match hub.delete_selected() {
            Ok(m) if !m.is_empty() => hub.activity.push(("text".into(), m)),
            Ok(_) => {}
            Err(e) => hub.activity.push(("error".into(), e)),
        }
    }

    /// ctrl+t: give the selected employee a task, typed right here.
    fn hub_task(&mut self) {
        let Some(hub) = self.hub.as_mut() else {
            return;
        };
        let name = hub
            .selected()
            .map(|i| i.label.trim().trim_start_matches('●').trim().to_string())
            .unwrap_or_default();
        if name.is_empty() || name == "ceo" {
            hub.activity
                .push(("error".into(), "pick an employee first".into()));
            return;
        }
        hub.begin(crate::hub::Purpose::Task(name));
    }

    /// load a file into the buffer, wherever it comes from.
    fn open_path(&mut self, path: std::path::PathBuf) {
        self.interrupt_live();
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.snapshot();
                self.lines = text.lines().map(str::to_string).collect();
                if self.lines.is_empty() {
                    self.lines.push(String::new());
                }
                self.ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("txt")
                    .to_string();
                self.file = Some(path);
                self.saved = self.lines.clone();
                self.modified = false;
                self.cx = 0;
                self.cy = 0;
                self.status = "written in nana — ctrl+s saves it".into();
            }
            Err(e) => self.notify(Level::Err, format!("cannot open: {e}")),
        }
    }

    /// do what the selected item says.
    fn hub_act(&mut self) {
        // if the box is asking for something, enter answers it
        if self.hub.as_ref().map(|h| h.typing()).unwrap_or(false) {
            let outcome = self.hub.as_mut().map(|h| h.submit());
            match outcome {
                Some(Ok(crate::hub::Outcome::OpenFile(path))) => {
                    self.hub = None;
                    self.open_path(path);
                }
                Some(Ok(crate::hub::Outcome::Say(m))) => self.status = m,
                Some(Ok(crate::hub::Outcome::None)) => {}
                Some(Err(e)) => {
                    if let Some(h) = self.hub.as_mut() {
                        h.activity.push(("error".into(), e));
                    }
                }
                None => {}
            }
            return;
        }
        let Some((root, action)) = self
            .hub
            .as_ref()
            .and_then(|h| h.selected().map(|i| (h.root.clone(), i.action.clone())))
        else {
            return;
        };
        match action {
            crate::hub::Action::ReadMemory(class, name) => {
                let path = crate::memory::Memory::open(&root)
                    .root()
                    .join(class.id())
                    .join(format!("{name}.md"));
                self.hub = None;
                self.open_path(path);
                self.status = format!("{name} — from this project's memory");
            }
            crate::hub::Action::RunSkill(name) => {
                self.hub = None;
                let mut pane = AgentPane::new();
                pane.persona = crate::settings::Settings::load(&root).persona;
                pane.input = format!("follow the {name} skill");
                self.agent_pane = Some(pane);
                self.status = format!("skill {name} — enter sends it to the agent");
            }
            other => match crate::hub::apply(&root, &other) {
                Ok(m) => {
                    if !m.is_empty() {
                        self.notify(Level::Ok, m);
                    }
                    if let Some(h) = self.hub.as_mut() {
                        h.refresh();
                    }
                }
                Err(e) => self.notify(Level::Err, e),
            },
        }
    }

    // ------------------------------------------------------------------ agent

    fn agent_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(pane) = self.agent_pane.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                if pane.busy {
                    pane.busy = false;
                    pane.rx = None;
                    pane.lines
                        .push(("note".into(), "cancelled — the answer is dropped".into()));
                    pane.status = "cancelled".into();
                } else {
                    self.agent_pane = None;
                    self.interrupt_live();
                }
            }
            // alt or shift + enter starts a new line of the prompt; enter sends it
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
            {
                if !pane.busy {
                    pane.input.push('\n');
                }
            }
            KeyCode::Enter => {
                let root = file_dir(self.file.as_deref());
                pane.send(&root);
            }
            KeyCode::Backspace => {
                pane.input.pop();
            }
            KeyCode::Up => pane.scroll = pane.scroll.saturating_add(1),
            KeyCode::Down => pane.scroll = pane.scroll.saturating_sub(1),
            KeyCode::Char(c) if !ctrl => pane.input.push(c),
            // ctrl+y keeps the conversation in memory now, without waiting for the turn to end
            KeyCode::Char('y') if ctrl => {
                pane.status = if pane.save_turn_now() {
                    "conversation saved to memory".into()
                } else {
                    "nothing to save yet: send a message and get an answer first".into()
                };
            }
            _ => {}
        }
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
        if ctrl && key.code == KeyCode::Char('a') {
            let root = crate::project::detect(self.file.as_deref().unwrap_or(Path::new(".")))
                .map(|p| p.root)
                .unwrap_or_else(|| file_dir(self.file.as_deref()));
            let mut pane = AgentPane::new();
            pane.persona = crate::settings::Settings::load(&root).persona;
            self.agent_pane = Some(pane);
            self.status = "agent — type a request, enter sends, esc closes".into();
            return;
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
            return;
        }

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
        // super+t : le même terminal que F3, pour qui vient d'un bureau
        if key.code == KeyCode::F(3)
            || (key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::SUPER))
        {
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
                self.status = "profile changed".into();
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

mod render;
use render::*;

pub use render::run;

#[cfg(test)]
mod tests;
