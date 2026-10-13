//! the agent: a system prompt assembled from sections, a tool loop, and a
//! transcript that survives the process.
//!
//! the prompt is built the way the harness does it — ordered sections, the
//! persona near the top, the project's own instructions next, then what is
//! available (skills, memory, tools). nothing is hidden: `nana --agent
//! --show-prompt` prints exactly what the model receives.

use crate::persona::{self, Persona};
use crate::provider::{Client, Msg, Reply, Role, ToolCall};
use crate::settings::{self, Settings};
use crate::skills::{self, Skill};
use crate::tools::{self, Ctx};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

mod blank_turn;
pub(crate) mod context;
mod conversation;
mod live_event;
mod prompt_rules;
mod recall;
mod retry;
mod sessions;
use context::{
    cap_context, pair_tool_messages, shrink_old_tool_results, CONTEXT_CAP_CHARS, KEEP_WHOLE_RESULTS,
};
pub use conversation::Conversation;
use live_event::file_event;
use prompt_rules::{
    autonomy_section, identity_section, read_instructions, structure_section, tools_section,
    MEMORY_RULES,
};
use recall::RECALL_BUDGET;
use sessions::new_session_path;
pub use sessions::{latest_session, sessions};

/// a section of the system prompt, with its position. lower order = earlier.
#[derive(Debug, Clone)]
pub struct Section {
    pub order: i32,
    pub name: &'static str,
    pub text: String,
}

/// what the agent is doing, as it happens — the ui and the cli both consume it.
#[derive(Debug, Clone)]
pub enum Event {
    Started {
        model: String,
        provider: String,
        step: usize,
    },
    Text(String),
    ToolCall {
        name: String,
        args: Value,
    },
    ToolResult {
        name: String,
        ok: bool,
        text: String,
    },
    /// a file the agent is about to write or has just read, with its text: the
    /// ui plays it back line by line, so the work is seen as it happens.
    File {
        path: String,
        content: String,
        write: bool,
    },
    /// a model call failed for a passing reason and is asked again after
    /// `wait_secs`: the run is alive, the ui says so instead of going quiet.
    Retry {
        attempt: usize,
        of: usize,
        wait_secs: u64,
        reason: String,
    },
    Finished {
        steps: usize,
    },
    Error(String),
}

pub struct Agent {
    pub root: PathBuf,
    pub settings: Settings,
    pub client: Client,
    pub ctx: Ctx,
    pub persona: Option<Persona>,
    pub skills: Vec<Skill>,
    /// AGENTS.md, if the project has one
    pub instructions: Option<String>,
    pub messages: Vec<Msg>,
    pub steps: usize,
    pub session: PathBuf,
    pub silent: bool,
    /// the waits between the retries of a failed model call
    pub retry_delays: Vec<Duration>,
    /// raised by the ui to stop a run between two steps
    pub stop: Arc<AtomicBool>,
}

impl Agent {
    /// Builds the agent for a project: resolves the client from the settings, then
    /// assembles the agent around it. Hands back the agent, or the reason no client could start.
    pub fn new(root: &Path, settings: Settings) -> Result<Agent, String> {
        let client = Client::resolve(&settings, root)?;
        Ok(Agent::with_client(root, settings, client))
    }

    /// Builds the agent around a ready client, loading the persona, skills and project
    /// instructions once so every turn reuses them.
    pub fn with_client(root: &Path, settings: Settings, client: Client) -> Agent {
        let persona = settings
            .persona
            .as_deref()
            .and_then(|id| persona::load(Some(root), id));
        let skills = skills::discover(root, &settings.skill_dirs);
        let instructions = read_instructions(root, settings::PROJECT_DIR);
        let session = new_session_path(root);
        Agent {
            root: root.to_path_buf(),
            ctx: Ctx::new(root, settings.sandbox_enabled()),
            settings,
            client,
            persona,
            skills,
            instructions,
            messages: Vec::new(),
            steps: 0,
            session,
            silent: false,
            retry_delays: retry::backoff(),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// pick up where the last conversation in this project stopped.
    pub fn resume_latest(&mut self) -> Option<PathBuf> {
        let path = latest_session(&self.root)?;
        let text = std::fs::read_to_string(&path).ok()?;
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if v.get("kind").and_then(|k| k.as_str()) == Some("header") {
                continue;
            }
            let role = match v.get("role").and_then(|r| r.as_str()) {
                Some("user") => Role::User,
                Some("assistant") => Role::Assistant,
                Some("tool") => Role::Tool,
                _ => continue,
            };
            self.messages.push(Msg {
                role,
                content: v
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default()
                    .to_string(),
                tool_calls: sessions::logged_tool_calls(&v),
                tool_call_id: v
                    .get("tool_call_id")
                    .and_then(|c| c.as_str())
                    .map(str::to_string),
            });
        }
        pair_tool_messages(&mut self.messages);
        self.session = path.clone();
        Some(path)
    }

    /// runs the check of each page the request touches, before the model
    /// answers, so a false page is seen at once. it goes through memory_check,
    /// so a destructive check meets the same approval gate as any shell call.
    fn touched_checks(&self, request: &str) -> String {
        let req = request.to_lowercase();
        let mut lines = Vec::new();
        for e in self.ctx.memory.list() {
            if e.check.is_none() {
                continue;
            }
            if e.class == crate::memory::Class::User {
                continue;
            }
            let name = e.name.to_lowercase();
            let mentioned = req.contains(&name)
                || req
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w.len() > 3 && name.split('-').any(|p| p == w));
            if !mentioned {
                continue;
            }
            let args = serde_json::json!({"class": e.class.id(), "name": e.name});
            if let Ok(verdict) = crate::tools::run("memory_check", &args, &self.ctx) {
                lines.push(verdict);
            }
        }
        if lines.is_empty() {
            return String::new();
        }
        format!("checks run for you:\n{}\n", lines.join("\n"))
    }

    /// the ordered sections of the system prompt: each one is built by its own
    /// helper, then they are sorted so the prompt reads in a fixed order.
    pub fn sections(&self, request: &str) -> Vec<Section> {
        let mut out: Vec<Section> = vec![identity_section()];
        out.extend(self.persona_section());
        out.extend(self.instructions_section());
        out.extend(self.skill_sections(request));
        out.push(self.memory_section(request));
        out.extend(self.knowledge_section());
        out.push(tools_section());
        out.push(structure_section());
        out.push(autonomy_section());
        out.push(self.suffix_section());
        out.sort_by_key(|s| s.order);
        out
    }

    /// the persona sits right after the identity, before everything else.
    fn persona_section(&self) -> Option<Section> {
        self.persona.as_ref().map(|p| Section {
            order: 0,
            name: "persona",
            text: format!("{}\n\n{}", p.title, p.prompt),
        })
    }

    /// the project's AGENTS.md, read as the project's own rules.
    fn instructions_section(&self) -> Option<Section> {
        self.instructions.as_ref().map(|instr| Section {
            order: 200,
            name: "project-instructions",
            text: format!("instructions of this project (AGENTS.md):\n{instr}"),
        })
    }

    /// the skill index, then the full body of every skill the request matches.
    fn skill_sections(&self, request: &str) -> Vec<Section> {
        let mut out = Vec::new();
        let idx = skills::index(&self.skills);
        if !idx.is_empty() {
            out.push(Section {
                order: 300,
                name: "skills",
                text: idx,
            });
        }
        for s in skills::matching(&self.skills, request) {
            out.push(Section {
                order: 310,
                name: "skill",
                text: format!("skill « {} » — follow it:\n{}", s.name, s.body),
            });
        }
        out
    }

    /// always present: an agent that never hears about memory never writes any.
    fn memory_section(&self, request: &str) -> Section {
        Section {
            order: 400,
            name: "memory",
            text: format!("{}{MEMORY_RULES}", self.memory_known(request)),
        }
    }

    /// the memory index, plus the pages this request touches and their checks.
    fn memory_known(&self, request: &str) -> String {
        let mem = self.ctx.memory.index();
        if mem.is_empty() {
            return "the memory of this project is empty so far.\n".to_string();
        }
        let recalled = self.ctx.memory.recall(request, RECALL_BUDGET);
        if recalled.is_empty() {
            return mem;
        }
        let checks = self.touched_checks(request);
        format!("{mem}the pages this request touches, read for you:\n{recalled}{checks}")
    }

    /// the knowledge wiki index, when the project has one.
    fn knowledge_section(&self) -> Option<Section> {
        let wiki = crate::knowledge::index(&self.root);
        if wiki.is_empty() {
            return None;
        }
        Some(Section {
            order: 450,
            name: "knowledge",
            text: format!(
                "{wiki}read a page with the read_file tool when it matters. when a session \
                 teaches something durable, it is worth a page."
            ),
        })
    }

    /// the working directory, the last line of the prompt.
    fn suffix_section(&self) -> Section {
        Section {
            order: 10200,
            name: "suffix",
            text: format!("your working directory is {}.", self.root.display()),
        }
    }

    /// Joins the prompt sections for one request into the system text sent to the model.
    pub fn system_prompt(&self, request: &str) -> String {
        self.sections(request)
            .into_iter()
            .map(|s| s.text)
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// one request, tool calls included, until the model answers in words.
    pub fn run<F: FnMut(Event)>(&mut self, request: &str, mut on: F) -> Result<String, String> {
        let system = self.system_prompt(request);
        self.messages.push(Msg::user(request));
        self.log("user", request, None, None);
        on(Event::Started {
            model: self.client.model.clone(),
            provider: self.client.provider.id.to_string(),
            step: 0,
        });
        let specs = tools::specs();
        // no step cap: a long session runs until the model answers in words
        // (or the provider fails); stopping on a count left people re-prompting.
        let mut step = 0;
        let mut progress = Progress::default();
        loop {
            step += 1;
            self.steps = step;
            if self.stop.swap(false, Ordering::SeqCst) {
                return Err("cancelled".to_string());
            }
            shrink_old_tool_results(&mut self.messages, KEEP_WHOLE_RESULTS);
            cap_context(&system, &mut self.messages, CONTEXT_CAP_CHARS);
            let reply = match self.chat_with_retry(&system, &specs, &mut on) {
                Ok(r) => r,
                Err(e) => {
                    on(Event::Error(e.clone()));
                    return Err(e);
                }
            };
            if !reply.text.trim().is_empty() {
                on(Event::Text(reply.text.clone()));
            }
            if self.owes_memory_page(&reply, &mut progress) {
                continue;
            }
            if reply.tool_calls.is_empty() {
                self.record_answer(&reply.text);
                on(Event::Finished { steps: step });
                return Ok(reply.text);
            }
            self.record_tool_request(&reply);
            self.answer_tool_calls(&reply.tool_calls, &mut progress, &mut on);
        }
    }

    /// a run that changed files and answers without a memory page is sent back
    /// once, never more, so it cannot loop. returns true when it was sent back.
    fn owes_memory_page(&mut self, reply: &Reply, progress: &mut Progress) -> bool {
        let owed = reply.tool_calls.is_empty()
            && progress.changed_files
            && !progress.remembered
            && !progress.sent_back;
        if !owed {
            return false;
        }
        progress.sent_back = true;
        self.messages.push(Msg {
            role: Role::Assistant,
            content: reply.text.clone(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        });
        let nudge = "you changed files without a memory page. before you answer, \
                      write what this change taught you with memory_write: \
                      a convention, a decision, or a pitfall you hit.";
        self.messages.push(Msg::user(nudge));
        self.log("assistant", &reply.text, None, None);
        self.log("user", nudge, None, None);
        true
    }

    /// keeps the final answer of a run in the conversation and the transcript.
    fn record_answer(&mut self, text: &str) {
        self.messages.push(Msg {
            role: Role::Assistant,
            content: text.to_string(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        });
        self.log("assistant", text, None, None);
    }

    /// keeps the model's tool request, so every call below has its answer after it.
    fn record_tool_request(&mut self, reply: &Reply) {
        self.messages.push(Msg {
            role: Role::Assistant,
            content: reply.text.clone(),
            tool_calls: reply.tool_calls.clone(),
            tool_call_id: None,
        });
        self.log_tool_request(reply);
    }

    /// answers every call the model asked for, in order, then the loop goes on.
    fn answer_tool_calls<F: FnMut(Event)>(
        &mut self,
        calls: &[ToolCall],
        progress: &mut Progress,
        on: &mut F,
    ) {
        for call in calls {
            on(Event::ToolCall {
                name: call.name.clone(),
                args: call.args.clone(),
            });
            if let Some((path, content, write)) = file_event(&call.name, &call.args, &self.ctx) {
                on(Event::File {
                    path,
                    content,
                    write,
                });
            }
            let (ok, text) = match tools::run(&call.name, &call.args, &self.ctx) {
                Ok(t) => (true, t),
                Err(e) => (false, format!("error: {e}")),
            };
            if ok {
                progress.note(&call.name);
            }
            on(Event::ToolResult {
                name: call.name.clone(),
                ok,
                text: text.clone(),
            });
            self.messages.push(Msg::tool(&call.id, text.clone()));
            self.log(
                "tool",
                &text,
                Some(call.name.clone()),
                Some(call.id.clone()),
            );
        }
    }

    /// append one line to the transcript of this project.
    fn log(&self, role: &str, content: &str, tool: Option<String>, id: Option<String>) {
        self.append_line(json!({
            "role": role,
            "content": content,
            "tool": tool,
            "tool_call_id": id,
            "at": now_secs(),
        }));
    }

    /// keeps the model's tool request in the transcript, calls included: a
    /// resumed session must give each tool answer the call it answers.
    fn log_tool_request(&self, reply: &Reply) {
        let calls: Vec<Value> = reply
            .tool_calls
            .iter()
            .map(|c| json!({"id": c.id, "name": c.name, "args": c.args}))
            .collect();
        self.append_line(json!({
            "role": "assistant",
            "content": reply.text,
            "tool_calls": calls,
            "at": now_secs(),
        }));
    }

    /// writes one json line at the end of this session's transcript.
    fn append_line(&self, line: Value) {
        if let Some(parent) = self.session.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut text = serde_json::to_string(&line).unwrap_or_default();
        text.push('\n');
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.session)
        {
            let _ = f.write_all(text.as_bytes());
        }
    }
}

/// the time of a transcript line, in seconds since the epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// the file a tool is about to touch, with the text it will write. only the
/// writes are shown before they land: a read shows what the tool returns.
/// how much memory the prompt may carry for one request, in characters.
/// what a run has done so far, to decide whether it owes a memory page.
/// the flags only move forward: once a run wrote memory, it is never owed again.
#[derive(Default)]
struct Progress {
    changed_files: bool,
    remembered: bool,
    sent_back: bool,
}

impl Progress {
    /// records a tool that succeeded: file edits and memory writes count.
    fn note(&mut self, tool: &str) {
        match tool {
            "write_file" | "edit_file" => self.changed_files = true,
            "memory_write" => self.remembered = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
