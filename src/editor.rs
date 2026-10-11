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
use std::collections::{HashMap, VecDeque};
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

/// Reads the completion agent saved in the config, falling back to the code explainer so completion always has a role.
fn load_complete_agent() -> AgentRole {
    let cfg = crate::config::load();
    let name = cfg.nana_agent.as_deref().unwrap_or("code explainer");
    COMPLETE_AGENTS
        .iter()
        .find(|a| a.name() == name)
        .copied()
        .unwrap_or(AgentRole::CodeExplainer)
}

/// Stores the chosen completion agent in the config so the next start keeps the same choice.
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
    /// Foreground colour of body text: the terminal default, so the user's own theme shows through.
    fn text() -> Color {
        Color::Reset
    }
    /// Muted colour for secondary text such as hints and inactive labels.
    fn dim() -> Color {
        Color::Indexed(8)
    }
    /// Colour of the line numbers, kept muted so they never compete with the code.
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
    /// Colour of the ghost completion, dim enough to read as a suggestion and not as code.
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

/// the name prefix of the windows that show the agent's tool actions (memory,
/// search, shell): they are not files. each action has its own window named
/// `agent actions: <tool>`, so a later action never erases an earlier one.
pub(super) const ACTIONS_VIEW: &str = "agent actions";

/// whether a window is an action window. paths compare by components, so this
/// must look at the text: "agent actions: run_shell" does not start a path
/// component-wise with "agent actions".
pub(super) fn is_action_view(path: &std::path::Path) -> bool {
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

    /// Opens a file into a buffer, or an empty welcome buffer when there is no path, and returns the editor or the read error.
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

mod agent_focus;
mod agent_pane;
mod assist;
mod build;
mod editing;
mod event_loop;
mod explorer_focus;
mod files;
mod focus;
mod header_bar;
mod hub_mode;
mod render;
mod search;
pub(crate) mod session;
mod terminal;
mod terminal_focus;
#[cfg(test)]
use agent_pane::TranscriptRows;
use agent_pane::{safe_text, AgentPane};
use render::*;
#[cfg(test)]
use terminal::terminal_replies;
use terminal::TermPane;

pub use session::run;

#[cfg(test)]
mod tests;
