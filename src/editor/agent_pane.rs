//! the agent panel: the card that floats over the ide while the agent works,
//! the transcript it writes, and the text rules that keep it readable.

use std::path::{Path, PathBuf};

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::*;

/// the agent panel: a rounded card with the conversation on top and a prompt
/// line at the bottom. it owns the keyboard while it is open — esc closes it,
/// and esc again while it is thinking cancels the request.
pub(super) struct AgentPane {
    pub(super) input: String,
    /// the transcript as it arrives: (kind, text)
    pub(super) lines: Vec<(String, String)>,
    pub(super) scroll: usize,
    pub(super) rx: Option<Receiver<crate::agent::Event>>,
    /// the conversation every message of this card continues: one agent,
    /// one history, so the model remembers the turns before
    pub(super) conversation: crate::agent::Conversation,
    pub(super) busy: bool,
    pub(super) status: String,
    /// the persona this conversation runs under, read from the project
    pub(super) persona: Option<String>,
    /// file writes announced by the agent, drained by the editor each frame
    pub(super) writes: Vec<LiveWrite>,
    /// every action of this conversation, placed in time: the background of the card
    pub(super) trail: Vec<crate::trail::Step>,
    /// when the card opened: the trail's clock starts here
    pub(super) started: std::time::Instant,
    /// the tool calls waiting for their answer, oldest first, and whether each
    /// one's file was already announced (a write or a read plays as the file,
    /// not as a tool action). A queue, not one slot: the agent may call a
    /// second tool before the first one answers, and the first must still play.
    pub(super) pending: VecDeque<(String, serde_json::Value, bool)>,
    /// the transcript already wrapped to the card's width, so a frame only
    /// wraps the lines that arrived since the last one
    pub(super) transcript: TranscriptRows,
    /// project root and session id, to know where the conversation is kept
    pub(super) root: PathBuf,
    pub(super) session: String,
    /// how the conversation opened (the first message sent) and how the
    /// agent's last reply closed it; both go to memory as they are known
    pub(super) opening: Option<String>,
    pub(super) closing: Option<String>,
}

impl AgentPane {
    /// keeps the opening and the closing of this conversation in the project
    /// memory. the memory is best effort: a failed write must not break a chat.
    pub(super) fn save_turn_now(&self) -> bool {
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
pub(super) struct TranscriptRows {
    pub(super) width: usize,
    /// how many transcript lines are wrapped into `rows`
    pub(super) wrapped: usize,
    pub(super) rows: Vec<Line<'static>>,
}

impl TranscriptRows {
    /// wrap the lines that are new since the last call, at `width`.
    pub(super) fn sync(&mut self, lines: &[(String, String)], width: usize) {
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
pub(super) fn transcript_label(kind: &str) -> (&'static str, ratatui::style::Color) {
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
    pub(super) fn rendered_rows(&mut self, width: usize) -> &[Line<'static>] {
        self.transcript.sync(&self.lines, width);
        &self.transcript.rows
    }

    /// Creates an empty agent card with its welcome note, so the first open says what the card is for.
    pub(super) fn new() -> AgentPane {
        AgentPane {
            input: String::new(),
            lines: vec![(
                "note".into(),
                "ask for something in this project. enter sends, esc closes.".into(),
            )],
            scroll: 0,
            rx: None,
            conversation: crate::agent::Conversation::default(),
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
            pending: VecDeque::new(),
            transcript: TranscriptRows::default(),
        }
    }

    /// one action on the trail, stamped with the time since the card opened.
    pub(super) fn record(&mut self, lane: crate::trail::Lane, label: String, ok: bool) {
        let at = self.started.elapsed().as_secs_f32();
        self.trail.push(crate::trail::Step {
            lane,
            label: safe_text(&label).replace('\n', " "),
            at,
            ok,
        });
    }

    /// a prompt opens a new task whenever the previous one is closed: the
    /// opening of the last task must not be filed under the first one.
    pub(super) fn begin_task(&mut self, request: &str, root: &Path) {
        if self.opening.is_none() || self.closing.is_some() {
            self.opening = Some(request.to_string());
            self.closing = None;
            self.root = root.to_path_buf();
            self.session = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "0".into());
        }
    }

    /// a task that ends in error is still a task that closed: its closing
    /// message is the failure, so the page is kept rather than lost.
    pub(super) fn end_failed(&mut self, e: &str) {
        if self.closing.is_none() {
            self.closing = Some(format!("failed: {e}"));
        }
        self.save_turn_now();
    }

    /// start a request on a worker thread; the events come back one by one.
    pub(super) fn send(&mut self, root: &Path) {
        let request = self.input.trim().to_string();
        if request.is_empty() || self.busy {
            return;
        }
        self.input.clear();
        self.begin_task(&request, root);
        self.lines.push(("you".into(), request.clone()));
        self.scroll = 0;
        self.busy = true;
        self.status = "thinking".into();
        let (tx, rx) = channel();
        self.rx = Some(rx);
        let root = root.to_path_buf();
        let conversation = self.conversation.clone();
        crate::guard::spawn_streaming(tx, move |tx| {
            let _ = conversation.turn(&root, &request, |event| {
                let _ = tx.send(event);
            });
        });
    }

    /// stops the turn in progress: the worker is told to halt at its next
    /// step, and the card stops listening. the history so far is kept.
    pub(super) fn cancel(&mut self) {
        self.conversation.cancel();
        self.busy = false;
        self.rx = None;
        self.lines
            .push(("note".into(), "cancelled — the answer is dropped".into()));
        self.status = "cancelled".into();
    }

    /// fold whatever the worker produced since the last frame.
    pub(super) fn poll(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };
        let mut done = false;
        while let Ok(event) = rx.try_recv() {
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
                    let label = terminal_header(&name, &args);
                    self.record(crate::trail::Lane::Tool, label.clone(), true);
                    self.lines.push(("tool".into(), label));
                    self.pending.push_back((name, args, false));
                }
                crate::agent::Event::ToolResult { name, ok, text } => {
                    self.tool_answered(&name, ok, &text);
                }
                crate::agent::Event::File { path, content, .. } => {
                    self.record(crate::trail::Lane::File, path.clone(), true);
                    if let Some(call) = self.pending.back_mut() {
                        call.2 = true;
                    }
                    self.writes.push(LiveWrite {
                        path: PathBuf::from(&path),
                        text: content.lines().map(safe_text).collect(),
                    });
                    self.lines.push(("file".into(), safe_text(&path)));
                }
                crate::agent::Event::Retry {
                    attempt,
                    of,
                    wait_secs,
                    reason,
                } => self.retrying(&format!("{reason} — retry {attempt}/{of} in {wait_secs}s")),
                crate::agent::Event::Finished { steps } => {
                    self.status = format!("done in {steps} step(s)");
                    self.save_turn_now();
                    done = true;
                }
                crate::agent::Event::Error(e) => {
                    self.record(crate::trail::Lane::Result, e.clone(), false);
                    self.lines.push(("error".into(), safe_text(&e)));
                    self.status = "failed".into();
                    self.end_failed(&e);
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

    /// a tool answered: it goes on the trail and in the transcript, and a
    /// tool that announced no file plays its call and its answer on screen.
    fn tool_answered(&mut self, name: &str, ok: bool, text: &str) {
        self.record(
            crate::trail::Lane::Result,
            format!("{name}: {}", text.trim()),
            ok,
        );
        let answered = self.pending.iter().position(|(call, _, _)| call == name);
        if let Some((_, args, shown)) = answered.and_then(|i| self.pending.remove(i)) {
            if !shown {
                let mut text_lines = vec![terminal_header(name, &args)];
                text_lines.extend(text.trim().lines().map(safe_text));
                self.writes.push(LiveWrite {
                    path: PathBuf::from(format!("{ACTIONS_VIEW}: {name}")),
                    text: text_lines,
                });
            }
        }
        self.lines.push((
            if ok { "ok".into() } else { "refused".into() },
            format!("{name}: {}", safe_text(text.trim())),
        ));
    }

    /// a model call failed for a passing reason and will be asked again: the
    /// card says so, so a wait reads as a retry and not as a frozen agent.
    fn retrying(&mut self, note: &str) {
        self.status = note.to_string();
        self.lines.push(("note".into(), safe_text(note)));
    }
}

/// one line of json, short enough for the transcript.
pub(super) fn short_json(v: &serde_json::Value) -> String {
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
pub(super) fn terminal_header(name: &str, args: &serde_json::Value) -> String {
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
pub(super) fn safe_text(s: &str) -> String {
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
pub(super) fn is_invisible(c: char) -> bool {
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
pub(super) fn skip_escape(chars: &mut std::str::Chars) {
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
