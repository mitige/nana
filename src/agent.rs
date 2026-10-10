//! the agent: a system prompt assembled from sections, a tool loop, and a
//! transcript that survives the process.
//!
//! the prompt is built the way the harness does it — ordered sections, the
//! persona near the top, the project's own instructions next, then what is
//! available (skills, memory, tools). nothing is hidden: `nana --agent
//! --show-prompt` prints exactly what the model receives.

use crate::persona::{self, Persona};
use crate::provider::{Client, Msg, Reply, Role};
use crate::settings::{self, Settings};
use crate::skills::{self, Skill};
use crate::tools::{self, Ctx};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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
}

impl Agent {
    pub fn new(root: &Path, settings: Settings) -> Result<Agent, String> {
        let client = Client::resolve(&settings, root)?;
        Ok(Agent::with_client(root, settings, client))
    }

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
                tool_calls: Vec::new(),
                tool_call_id: v
                    .get("tool_call_id")
                    .and_then(|c| c.as_str())
                    .map(str::to_string),
            });
        }
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

    /// the ordered sections of the system prompt.
    pub fn sections(&self, request: &str) -> Vec<Section> {
        let mut out: Vec<Section> = Vec::new();
        out.push(Section {
            order: -1000,
            name: "identity",
            text:
                "you are the agent inside nana, a terminal editor. you work in the user's project, \
                   you use the tools you are given, and you say what you did."
                    .into(),
        });
        // the persona sits right after the identity, before everything else
        if let Some(p) = &self.persona {
            out.push(Section {
                order: 0,
                name: "persona",
                text: format!("{}\n\n{}", p.title, p.prompt),
            });
        }
        if let Some(instr) = &self.instructions {
            out.push(Section {
                order: 200,
                name: "project-instructions",
                text: format!("instructions of this project (AGENTS.md):\n{instr}"),
            });
        }
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
        // always present: an agent that never hears about memory never writes any
        let mem = self.ctx.memory.index();
        let known = if mem.is_empty() {
            "the memory of this project is empty so far.\n".to_string()
        } else {
            let recalled = self.ctx.memory.recall(request, RECALL_BUDGET);
            let checks = self.touched_checks(request);
            if recalled.is_empty() {
                mem
            } else {
                format!("{mem}the pages this request touches, read for you:\n{recalled}{checks}")
            }
        };
        out.push(Section {
            order: 400,
            name: "memory",
            text: format!(
                "{known}memory is a world model of this project: what is true of its state, \
                 its conventions, and how it behaves. a wrong page is a bug to fix, not a fact to trust.\n\
                 memory rules, each one is a duty, not a suggestion:\n\
                 - when a task starts: call memory_list, then memory_read on every page that touches the \
                 task. before you act on a page, say what you expect from it.\n\
                 - when you check a fact: after acting, verify the result with a tool against that \
                 expectation, and run memory_check on any page that carries a check.\n\
                 - when a page is wrong: correct the page with memory_write at once, and say which belief was wrong.\n\
                 - when the user states a preference or a rule: write it as a feedback or user page with memory_write.\n\
                 - when a decision is made: write a project page with the reason, so the next run does not reopen it.\n\
                 - when a task ends: write what was learned, the conventions met and the pitfalls hit, \
                 with memory_write, before your final words.\n\
                 - before your final words: ask yourself whether anything durable happened in this run; \
                 if yes, the memory write comes first, the answer comes after."
            ),
        });
        let wiki = crate::knowledge::index(&self.root);
        if !wiki.is_empty() {
            out.push(Section {
                order: 450,
                name: "knowledge",
                text: format!(
                    "{wiki}read a page with the read_file tool when it matters. when a session \
                     teaches something durable, it is worth a page."
                ),
            });
        }
        out.push(Section {
            order: 1000,
            name: "tools",
            text: "tools: files and the shell are scoped to this project. a destructive \
                   command is refused unless the user approved it — never try to work around that."
                .into(),
        });
        // the model stops early when it is unsure it may go on; say plainly that it may
        out.push(Section {
            order: 1100,
            name: "autonomy",
            text: "work in one unbroken run until the task is finished. do not stop to ask \
                   whether to continue, do not ask permission between steps, and do not end \
                   your turn with a plan. act, check the result with a tool, and keep going. \
                   only answer in words when the task is done or when you are truly blocked."
                .into(),
        });
        out.push(Section {
            order: 10200,
            name: "suffix",
            text: format!("your working directory is {}.", self.root.display()),
        });
        out.sort_by_key(|s| s.order);
        out
    }

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
        // durable work ends in a memory page: a run that changed files and
        // answers without one is sent back once, never more, so it cannot loop
        let mut changed_files = false;
        let mut remembered = false;
        let mut sent_back = false;
        loop {
            step += 1;
            self.steps = step;
            shrink_old_tool_results(&mut self.messages, KEEP_WHOLE_RESULTS);
            cap_context(&system, &mut self.messages, CONTEXT_CAP_CHARS);
            let reply: Reply = match self.client.chat(&system, &self.messages, &specs) {
                Ok(r) => r,
                Err(e) => {
                    on(Event::Error(e.clone()));
                    return Err(e);
                }
            };
            if !reply.text.trim().is_empty() {
                on(Event::Text(reply.text.clone()));
            }
            if reply.tool_calls.is_empty() && changed_files && !remembered && !sent_back {
                sent_back = true;
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
                continue;
            }
            if reply.tool_calls.is_empty() {
                self.messages.push(Msg {
                    role: Role::Assistant,
                    content: reply.text.clone(),
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                });
                self.log("assistant", &reply.text, None, None);
                on(Event::Finished { steps: step });
                return Ok(reply.text);
            }
            // the model wants tools: answer every call, then loop
            self.messages.push(Msg {
                role: Role::Assistant,
                content: reply.text.clone(),
                tool_calls: reply.tool_calls.clone(),
                tool_call_id: None,
            });
            for call in &reply.tool_calls {
                on(Event::ToolCall {
                    name: call.name.clone(),
                    args: call.args.clone(),
                });
                if let Some((path, content, write)) = file_event(&call.name, &call.args, &self.ctx)
                {
                    on(Event::File {
                        path,
                        content,
                        write,
                    });
                }
                let result = tools::run(&call.name, &call.args, &self.ctx);
                let (ok, text) = match result {
                    Ok(t) => (true, t),
                    Err(e) => (false, format!("error: {e}")),
                };
                if ok {
                    match call.name.as_str() {
                        "write_file" | "edit_file" => changed_files = true,
                        "memory_write" => remembered = true,
                        _ => {}
                    }
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
    }

    /// append one line to the transcript of this project.
    fn log(&self, role: &str, content: &str, tool: Option<String>, id: Option<String>) {
        if let Some(parent) = self.session.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let line = json!({
            "role": role,
            "content": content,
            "tool": tool,
            "tool_call_id": id,
            "at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        });
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

/// the file a tool is about to touch, with the text it will write. only the
/// writes are shown before they land: a read shows what the tool returns.
/// how much memory the prompt may carry for one request, in characters.
const RECALL_BUDGET: usize = 6000;

/// the newest tool results stay whole; older ones keep a head so the model
/// still sees what they were, and it can run the tool again to see the rest.
const KEEP_WHOLE_RESULTS: usize = 4;
const OLD_RESULT_HEAD: usize = 300;
/// ceiling on the characters sent in one turn (system prompt included).
const CONTEXT_CAP_CHARS: usize = 200_000;

/// a long session re-sends every message on each turn. an old tool result
/// (a file read, a grep) is the bulk of that, and it is stale by then: cut it
/// to its head, keep the call id so the pairing with the call stays valid.
fn shrink_old_tool_results(msgs: &mut [Msg], keep_whole: usize) {
    let tools: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::Tool)
        .map(|(i, _)| i)
        .collect();
    let old = tools.len().saturating_sub(keep_whole);
    for &i in &tools[..old] {
        let m = &mut msgs[i];
        if m.content.chars().count() <= OLD_RESULT_HEAD {
            continue;
        }
        let head: String = m.content.chars().take(OLD_RESULT_HEAD).collect();
        m.content = format!(
            "{head}\n[… {} characters elided from an older result; run the tool again to see them]",
            m.content.chars().count() - OLD_RESULT_HEAD
        );
    }
}

/// what one turn actually sends: the system prompt and every message. the
/// cap is measured on this, not on the messages alone, or the prompt grows
/// past the budget unseen.
fn context_chars(system: &str, msgs: &[Msg]) -> usize {
    system.chars().count()
        + msgs
            .iter()
            .map(|m| m.content.chars().count())
            .sum::<usize>()
}

/// a hard ceiling on the context of one turn. when the sum is over it, the
/// oldest tool results are cut to their head first, one after the other,
/// until the sum fits. the user messages are never cut, and the newest result
/// is the last one touched.
fn cap_context(system: &str, msgs: &mut [Msg], cap: usize) {
    let mut over = context_chars(system, msgs).saturating_sub(cap);
    if over == 0 {
        return;
    }
    let tools: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == Role::Tool)
        .map(|(i, _)| i)
        .collect();
    for i in tools {
        if over == 0 {
            break;
        }
        let len = msgs[i].content.chars().count();
        if len <= OLD_RESULT_HEAD {
            continue;
        }
        let head: String = msgs[i].content.chars().take(OLD_RESULT_HEAD).collect();
        let cut = len - OLD_RESULT_HEAD;
        msgs[i].content = format!(
            "{head}\n[... {cut} characters elided to keep the context under its cap; run the tool again to see them]"
        );
        over = over.saturating_sub(cut);
    }
}

fn file_event(name: &str, args: &Value, ctx: &crate::tools::Ctx) -> Option<(String, String, bool)> {
    match name {
        "write_file" => Some((
            args.get("path")?.as_str()?.to_string(),
            args.get("content")?.as_str()?.to_string(),
            true,
        )),
        // a read shows the file as it is now, before the tool runs, through the
        // same path check as the tool: a sandboxed read is never shown either
        "read_file" => {
            let raw = args.get("path")?.as_str()?;
            let path = crate::tools::resolve(ctx, raw).ok()?;
            let text = std::fs::read_to_string(&path).ok()?;
            Some((raw.to_string(), text, false))
        }
        _ => None,
    }
}

/// AGENTS.md, at the root or inside the project directory.
fn read_instructions(root: &Path, project_dir: &str) -> Option<String> {
    for candidate in [
        root.join("AGENTS.md"),
        root.join(project_dir).join("AGENTS.md"),
    ] {
        if let Ok(t) = std::fs::read_to_string(&candidate) {
            if !t.trim().is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn sessions_dir(root: &Path) -> PathBuf {
    root.join(settings::PROJECT_DIR).join("sessions")
}

fn new_session_path(root: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    sessions_dir(root).join(format!("{stamp}-{}.jsonl", std::process::id()))
}

/// the most recent transcript of this project.
pub fn latest_session(root: &Path) -> Option<PathBuf> {
    let dir = sessions_dir(root);
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .collect();
    all.sort();
    all.pop()
}

/// every transcript, newest first — what the memory box lists.
pub fn sessions(root: &Path) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = std::fs::read_dir(sessions_dir(root))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    all.retain(|p| p.extension().and_then(|x| x.to_str()) == Some("jsonl"));
    all.sort();
    all.reverse();
    all
}

#[cfg(test)]
mod tests {

    /// read a whole http request: the headers, then exactly as many body bytes
    /// as content-length promised. deterministic, no waiting on timeouts.
    fn read_request(sock: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .ok();
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut head_len: Option<usize> = None;
        let mut want: usize = 0;
        loop {
            if let Some(h) = head_len {
                if buf.len() >= h + want {
                    break;
                }
            }
            match sock.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => break,
            }
            if head_len.is_none() {
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    head_len = Some(i + 4);
                    let head = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                    want = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                }
            }
        }
        String::from_utf8_lossy(&buf).to_string()
    }
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nana-agent-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// a provider that answers with a scripted list of bodies, one per call.
    fn scripted(bodies: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for body in bodies {
                // a run that makes fewer model calls than scripted must fail
                // the test, not leave the server waiting forever
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let mut sock = loop {
                    match listener.accept() {
                        Ok((sock, _)) => break sock,
                        Err(_) if std::time::Instant::now() < deadline => {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(_) => return seen,
                    }
                };
                sock.set_nonblocking(false).unwrap();
                seen.push(read_request(&mut sock));
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes());
            }
            seen
        });
        (format!("http://{addr}"), handle)
    }

    fn agent_at(root: &Path, url: &str) -> Agent {
        let mut s = Settings::default();
        s.sandbox = Some(true);
        Agent::with_client(root, s, Client::local(url, "mock"))
    }

    #[test]
    fn the_prompt_asks_for_one_unbroken_run() {
        let d = tmp("autonomy");
        let a = Agent::with_client(
            &d,
            Settings::default(),
            Client::local("http://127.0.0.1:1", "m"),
        );
        let p = a.system_prompt("refactor the parser");
        assert!(p.contains("one unbroken run"), "no autonomy nudge: {p}");
        assert!(
            p.contains("do not stop to ask"),
            "the agent must not ask permission between steps: {p}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_prompt_carries_the_memory_the_request_touches_not_only_its_names() {
        let d = tmp("recall-prompt");
        let m = crate::memory::Memory::open(&d);
        m.write(
            crate::memory::Class::Project,
            "parser",
            "# parser\n\npratt loop, no recursion past depth 8",
        )
        .unwrap();
        let a = Agent::with_client(
            &d,
            Settings::default(),
            Client::local("http://127.0.0.1:1", "m"),
        );
        let p = a.system_prompt("refactor the parser");
        assert!(
            p.contains("pratt loop"),
            "the page body is in the prompt: {p}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_checks_of_the_pages_a_request_touches_run_before_the_answer() {
        let d = tmp("auto-check");
        std::fs::write(d.join("Cargo.toml"), "[package]\n").unwrap();
        let m = crate::memory::Memory::open(&d);
        m.write_checked(
            crate::memory::Class::Project,
            "manifest",
            "# manifest\n\na cargo manifest exists",
            crate::memory::Confidence::Medium,
            Some("test -f Cargo.toml"),
        )
        .unwrap();
        m.write_checked(
            crate::memory::Class::Project,
            "parser",
            "# parser\n\nthe parser lives in one file",
            crate::memory::Confidence::Medium,
            Some("test -f missing-parser.rs"),
        )
        .unwrap();
        let a = Agent::with_client(
            &d,
            Settings::default(),
            Client::local("http://127.0.0.1:1", "m"),
        );
        let p = a.system_prompt("refactor the parser and the manifest");
        assert!(p.contains("checks run for you"), "no check report: {p}");
        assert!(
            p.contains("holds: manifest"),
            "a true page is not reported: {p}"
        );
        assert!(
            p.contains("does not hold: parser"),
            "a false page is not reported: {p}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn old_tool_results_are_shrunk_so_the_context_stops_growing() {
        let big = "x".repeat(20_000);
        let mut msgs = vec![Msg::user("q")];
        for i in 0..6 {
            msgs.push(Msg::tool(&format!("c{i}"), big.clone()));
        }
        shrink_old_tool_results(&mut msgs, 2);
        assert_eq!(msgs[0].content, "q", "the question is never shrunk");
        for (i, m) in msgs[1..].iter().enumerate() {
            assert_eq!(
                m.tool_call_id.as_deref(),
                Some(format!("c{i}").as_str()),
                "the call id is kept so the pairing stays valid"
            );
        }
        assert!(msgs[1].content.len() < 1000, "an old result is shrunk");
        assert!(
            msgs[1].content.contains("elided"),
            "the model is told it was cut and can read it again: {}",
            &msgs[1].content
        );
        assert_eq!(msgs[5].content.len(), 20_000, "the last two stay whole");
        assert_eq!(msgs[6].content.len(), 20_000, "the last two stay whole");
    }

    #[test]
    fn the_context_sent_each_turn_stays_under_the_cap() {
        let big = "x".repeat(100_000);
        let mut msgs = vec![Msg::user("q")];
        for i in 0..6 {
            msgs.push(Msg::tool(&format!("c{i}"), big.clone()));
        }
        let system = "s".repeat(1_000);
        assert!(
            context_chars(&system, &msgs) > CONTEXT_CAP_CHARS,
            "the fixture really is over the cap"
        );
        cap_context(&system, &mut msgs, CONTEXT_CAP_CHARS);
        let sent = context_chars(&system, &msgs);
        assert!(
            sent <= CONTEXT_CAP_CHARS,
            "the context sent is {sent} chars, over the cap of {CONTEXT_CAP_CHARS}"
        );
        assert_eq!(msgs[0].content, "q", "the question is never cut");
        assert_eq!(
            msgs[6].content.len(),
            100_000,
            "the newest result is the last to be cut"
        );
    }

    #[test]
    fn an_empty_memory_still_asks_the_agent_to_remember() {
        let d = tmp("memory-empty");
        let a = Agent::with_client(
            &d,
            Settings::default(),
            Client::local("http://127.0.0.1:1", "m"),
        );
        let p = a.system_prompt("refactor the parser");
        assert!(p.contains("memory_write"), "no write nudge: {p}");
        assert!(p.contains("memory_read"), "no read nudge: {p}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn memory_is_a_world_model_the_agent_checks_against_reality() {
        let d = tmp("world");
        let a = Agent::with_client(
            &d,
            Settings::default(),
            Client::local("http://127.0.0.1:1", "m"),
        );
        let p = a.system_prompt("refactor the parser");
        assert!(
            p.contains("world model"),
            "memory is not framed as a model: {p}"
        );
        assert!(p.contains("expect"), "no expectation before acting: {p}");
        assert!(
            p.contains("correct the page"),
            "a wrong belief is never corrected: {p}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn memory_rules_name_each_moment_that_calls_for_a_memory_act() {
        let d = tmp("memory-rules");
        let a = Agent::with_client(
            &d,
            Settings::default(),
            Client::local("http://127.0.0.1:1", "m"),
        );
        let p = a.system_prompt("refactor the parser");
        assert!(p.contains("memory rules"), "the rules have no heading: {p}");
        assert!(
            p.contains("when a task starts"),
            "no start-of-task trigger: {p}"
        );
        assert!(
            p.contains("when a task ends"),
            "no end-of-task trigger: {p}"
        );
        assert!(
            p.contains("when a page is wrong"),
            "no correction trigger: {p}"
        );
        assert!(
            p.contains("when the user states a preference"),
            "no user-preference trigger: {p}"
        );
        assert!(
            p.contains("before your final words"),
            "the agent may end without recording: {p}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// a run that changed files and then answers without a memory page is
    /// sent back once: the loop does not let durable work end unrecorded.
    #[test]
    fn durable_work_without_a_memory_page_is_sent_back_before_the_answer() {
        let d = tmp("gate");
        let write = serde_json::json!({"path": "src/a.rs", "content": "fn a() {}"}).to_string();
        let mem =
            serde_json::json!({"class": "project", "name": "a", "content": "a is new"}).to_string();
        let call = |id: &str, name: &str, args: &str| {
            serde_json::json!({"choices":[{"message":{"content":"","tool_calls":[
                {"id":id,"type":"function","function":{"name":name,"arguments":args}}]}}]})
            .to_string()
        };
        let (url, server) = scripted(vec![
            call("c1", "write_file", &write),
            r#"{"choices":[{"message":{"content":"written"}}]}"#.to_string(),
            call("c2", "memory_write", &mem),
            r#"{"choices":[{"message":{"content":"all recorded"}}]}"#.to_string(),
        ]);
        let mut a = agent_at(&d, &url);
        let answer = a.run("add a", |_| {}).unwrap();
        let seen = server.join().unwrap();
        assert_eq!(answer, "all recorded", "the run went on after the gate");
        assert_eq!(seen.len(), 4, "the gate added one more model call");
        assert!(
            seen[2].contains("changed files without a memory page"),
            "the model was sent back to memory: {}",
            &seen[2][..seen[2].len().min(300)]
        );
        assert!(d.join(".nana/memory/project/a.md").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_read_only_run_is_not_sent_back_to_memory() {
        let d = tmp("gate-read");
        let (url, server) = scripted(vec![
            r#"{"choices":[{"message":{"content":"the answer"}}]}"#.to_string(),
        ]);
        let mut a = agent_at(&d, &url);
        assert_eq!(a.run("what is here?", |_| {}).unwrap(), "the answer");
        assert_eq!(server.join().unwrap().len(), 1, "no extra call for a read");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_prompt_is_ordered_identity_persona_then_the_rest() {
        let d = tmp("prompt");
        std::fs::create_dir_all(d.join(".nana/personas")).unwrap();
        std::fs::write(
            d.join(".nana/personas/lead.md"),
            "---\ntitle: Lead\ndescription: d\n---\nbe terse.\n",
        )
        .unwrap();
        std::fs::write(d.join("AGENTS.md"), "always write tests\n").unwrap();
        std::fs::create_dir_all(d.join(".nana/skills")).unwrap();
        crate::knowledge::write(&d, "build", "# build\n\ncargo build --release").unwrap();
        std::fs::write(
            d.join(".nana/skills/rel.md"),
            "---\nname: rel\ndescription: releases\ntrigger: release\n---\nstep one\n",
        )
        .unwrap();
        let mut s = Settings::default();
        s.persona = Some("lead".into());
        let a = Agent::with_client(&d, s, Client::local("http://127.0.0.1:1", "m"));
        let p = a.system_prompt("cut a release");
        let ipos = p.find("you are the agent inside nana").unwrap();
        let ppos = p.find("be terse.").unwrap();
        let apos = p.find("always write tests").unwrap();
        let spos = p.find("- rel: releases").unwrap();
        let tpos = p.find("tools:").unwrap();
        assert!(ipos < ppos, "identity before persona");
        assert!(ppos < apos, "persona before project instructions");
        assert!(apos < spos, "instructions before skills");
        assert!(spos < tpos, "skills before the tool note");
        // a matching trigger brings the skill body in
        assert!(p.contains("step one"), "the triggered skill is included");
        assert!(p.contains("- build: build"), "the wiki is announced: {p}");
        let q = a.system_prompt("write a poem");
        assert!(!q.contains("step one"), "and only when it matches");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_loop_runs_a_tool_then_answers() {
        let d = tmp("loop");
        std::fs::write(d.join("hello.txt"), "the secret word is: rutabaga\n").unwrap();
        let (url, server) = scripted(vec![
            r#"{"choices":[{"message":{"content":"","tool_calls":[
                 {"id":"c1","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"hello.txt\"}"}}]}}]}"#
                .to_string(),
            r#"{"choices":[{"message":{"content":"the word is rutabaga"}}]}"#.to_string(),
        ]);
        let mut a = agent_at(&d, &url);
        let mut events = Vec::new();
        let answer = a
            .run("what is the secret word?", |e| events.push(e))
            .unwrap();
        assert_eq!(answer, "the word is rutabaga");
        // the tool really ran: the model was shown the file's contents
        let seen = server.join().unwrap();
        assert_eq!(seen.len(), 2, "two model calls");
        assert!(
            seen[1].contains("rutabaga"),
            "the file content went back to the model: {}",
            &seen[1][..seen[1].len().min(400)]
        );
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::ToolCall { name, .. } if name == "read_file")));
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Finished { steps: 2 })));
        // and the transcript is on disk, in the project
        let log = std::fs::read_to_string(&a.session).unwrap();
        assert!(log.contains("\"role\":\"user\""), "{log}");
        assert!(log.contains("\"role\":\"tool\""), "{log}");
        assert!(log.contains("rutabaga"), "{log}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_write_is_announced_with_its_text_before_it_lands() {
        let d = tmp("live");
        let body = "fn one() {}\nfn two() {}";
        let args = serde_json::json!({"path": "src/new.rs", "content": body}).to_string();
        let reply = serde_json::json!({"choices":[{"message":{"content":"","tool_calls":[
            {"id":"c1","type":"function","function":{"name":"write_file","arguments": args}}]}}]})
        .to_string();
        let (url, server) = scripted(vec![
            reply,
            r#"{"choices":[{"message":{"content":"done"}}]}"#.to_string(),
            // the gate sends the run back once for a memory page
            r#"{"choices":[{"message":{"content":"noted"}}]}"#.to_string(),
        ]);
        let mut a = agent_at(&d, &url);
        let mut events = Vec::new();
        a.run("write it", |e| events.push(e)).unwrap();
        let _ = server.join();
        let file = events
            .iter()
            .find_map(|e| match e {
                Event::File {
                    path,
                    content,
                    write,
                } => Some((path.clone(), content.clone(), *write)),
                _ => None,
            })
            .expect("the write is announced");
        assert_eq!(file, ("src/new.rs".to_string(), body.to_string(), true));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_read_is_announced_with_the_text_it_read() {
        let d = tmp("live-read");
        std::fs::write(d.join("notes.txt"), "alpha\nbeta\n").unwrap();
        let args = serde_json::json!({"path": "notes.txt"}).to_string();
        let reply = serde_json::json!({"choices":[{"message":{"content":"","tool_calls":[
            {"id":"c1","type":"function","function":{"name":"read_file","arguments": args}}]}}]})
        .to_string();
        let (url, server) = scripted(vec![
            reply,
            r#"{"choices":[{"message":{"content":"done"}}]}"#.to_string(),
        ]);
        let mut a = agent_at(&d, &url);
        let mut events = Vec::new();
        a.run("read it", |e| events.push(e)).unwrap();
        let _ = server.join();
        let file = events
            .iter()
            .find_map(|e| match e {
                Event::File {
                    path,
                    content,
                    write,
                } => Some((path.clone(), content.clone(), *write)),
                _ => None,
            })
            .expect("the read is announced, like a write");
        assert_eq!(
            file,
            ("notes.txt".to_string(), "alpha\nbeta\n".to_string(), false)
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_refused_tool_is_reported_to_the_model_not_hidden() {
        let d = tmp("refuse");
        let (url, server) = scripted(vec![
            r#"{"choices":[{"message":{"content":"","tool_calls":[
                 {"id":"c1","type":"function","function":{"name":"run_shell","arguments":"{\"command\":\"rm -rf .\"}"}}]}}]}"#
                .to_string(),
            r#"{"choices":[{"message":{"content":"understood, i will not"}}]}"#.to_string(),
        ]);
        let mut a = agent_at(&d, &url);
        let answer = a.run("delete everything", |_| {}).unwrap();
        assert_eq!(answer, "understood, i will not");
        let seen = server.join().unwrap();
        assert!(
            seen[1].contains("needs your explicit approval"),
            "the refusal is visible to the model"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_session_can_be_resumed() {
        let d = tmp("resume");
        let (url, _s) = scripted(vec![
            r#"{"choices":[{"message":{"content":"first answer"}}]}"#.to_string(),
        ]);
        let mut a = agent_at(&d, &url);
        a.run("first question", |_| {}).unwrap();
        let mut b = agent_at(&d, "http://127.0.0.1:1");
        let path = b.resume_latest().expect("a previous session");
        assert!(path.starts_with(&d), "{path:?}");
        assert_eq!(b.messages.len(), 2, "question and answer came back");
        assert_eq!(b.messages[0].content, "first question");
        assert_eq!(b.messages[1].content, "first answer");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_long_session_keeps_going_until_the_model_answers() {
        let d = tmp("long");
        // far past the old 12-step cap: the agent must not stop on its own
        let calls = 30;
        let mut bodies = Vec::new();
        for i in 0..calls {
            bodies.push(format!(
                r#"{{"choices":[{{"message":{{"content":"","tool_calls":[
                    {{"id":"c{i}","type":"function","function":{{"name":"list_dir","arguments":"{{}}"}}}}]}}}}]}}"#
            ));
        }
        bodies.push(r#"{"choices":[{"message":{"content":"all done"}}]}"#.to_string());
        let (url, server) = scripted(bodies);
        let mut a = agent_at(&d, &url);
        let mut events = Vec::new();
        let answer = a
            .run("keep going", |e| events.push(e))
            .expect("a long session must not be cut off");
        assert_eq!(answer, "all done");
        assert_eq!(
            server.join().unwrap().len(),
            calls + 1,
            "every step was sent"
        );
        assert!(events
            .iter()
            .any(|e| matches!(e, Event::Finished { steps } if *steps == calls + 1)));
        let _ = std::fs::remove_dir_all(&d);
    }
}
