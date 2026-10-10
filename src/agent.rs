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
            mem
        };
        out.push(Section {
            order: 400,
            name: "memory",
            text: format!(
                "{known}read a page with the memory_read tool when it matters. \
                 write one with memory_write as soon as you learn something durable: \
                 a convention, a decision, a fact about the user, a pitfall you hit. \
                 check memory before you start a task, and record what you learned at the end."
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
        loop {
            step += 1;
            self.steps = step;
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
                let result = tools::run(&call.name, &call.args, &self.ctx);
                let (ok, text) = match result {
                    Ok(t) => (true, t),
                    Err(e) => (false, format!("error: {e}")),
                };
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
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for body in bodies {
                let (mut sock, _) = listener.accept().unwrap();
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
