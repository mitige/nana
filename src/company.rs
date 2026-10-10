//! the company: a ceo that hires, and the employees it hires.
//!
//! a mission is given to the ceo. the ceo answers with a hiring plan — the
//! smallest team that can do the work — and those employees are written to
//! the project's roster, each with a role and a system prompt of their own.
//! a task is then dispatched to one of them, which is an ordinary agent run
//! with that employee's persona on top.
//!
//! everything is stored as json and jsonl under `<project>/.nana/agents/`, so
//! the org chart is a file you can read, edit and version — or delete.

use crate::agent::{Agent as Runner, Event};
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Employee {
    pub name: String,
    /// what they do, in two words
    pub role: String,
    /// the one line that says why they exist
    pub mission: String,
    /// their system prompt
    pub persona: String,
    /// skills they are expected to use
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub employee: String,
    pub task: String,
    /// queued | running | done | failed
    pub state: String,
    pub at: u64,
    #[serde(default)]
    pub result: String,
}

fn dir(root: &Path) -> PathBuf {
    root.join(".nana").join("agents")
}

fn roster_path(root: &Path) -> PathBuf {
    dir(root).join("roster.json")
}

fn mail_path(root: &Path) -> PathBuf {
    dir(root).join("tasks.jsonl")
}

/// the employees of this project, in the order they were hired.
pub fn roster(root: &Path) -> Vec<Employee> {
    let Ok(text) = std::fs::read_to_string(roster_path(root)) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// replace the roster (the caller decides who stays).
pub fn save_roster(root: &Path, people: &[Employee]) -> Result<PathBuf, String> {
    let path = roster_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(people).map_err(|e| e.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|e| e.to_string())?;
    Ok(path)
}

/// hire: append, without losing whoever is already there.
pub fn hire(root: &Path, people: &[Employee]) -> Result<usize, String> {
    let mut all = roster(root);
    let mut hired = 0;
    for p in people {
        if all.iter().any(|e| e.name == p.name) {
            continue;
        }
        all.push(p.clone());
        hired += 1;
    }
    save_roster(root, &all)?;
    Ok(hired)
}

pub fn fire(root: &Path, name: &str) -> Result<(), String> {
    let mut all = roster(root);
    let before = all.len();
    all.retain(|e| e.name != name);
    if all.len() == before {
        return Err(format!("nobody named « {name} » works here"));
    }
    save_roster(root, &all).map(|_| ())
}

pub fn employee(root: &Path, name: &str) -> Option<Employee> {
    roster(root).into_iter().find(|e| e.name == name)
}

/// the roster as text, for the hub's detail pane and for the shell.
pub fn describe(root: &Path) -> String {
    let people = roster(root);
    let mut out = String::from(
        "the ceo runs this project's company.\n\
         hire a team with:  nana --company hire \"the mission\"\n\
         give work with:    nana --company task <name> \"the task\"\n\n",
    );
    if people.is_empty() {
        out.push_str(
            "no employees yet — the roster is empty.\n\
                      the ceo hires the smallest team that can do the mission.",
        );
        return out;
    }
    out.push_str(&format!("{} employee(s):\n", people.len()));
    for p in people {
        out.push_str(&format!("  {:<14} {:<24} {}\n", p.name, p.role, p.mission));
    }
    out
}

// ------------------------------------------------------------------ the ceo

/// the prompt that turns a mission into a hiring plan.
pub fn ceo_prompt(mission: &str, skills: &[String]) -> String {
    let known = if skills.is_empty() {
        "none declared in this project".to_string()
    } else {
        skills.join(", ")
    };
    format!(
        "you are the ceo of the small software company that lives inside this \
         project. you do not write code yourself: you hire the smallest team \
         that can finish the mission, and you give each of them one clear job.\n\n\
         the mission: {mission}\n\n\
         skills this project offers: {known}\n\n\
         answer with json only, an array of employees, at most four of them, \
         no prose around it:\n\
         [{{\"name\": \"short-lowercase-name\", \"role\": \"two words\", \
         \"mission\": \"one line, what they own\", \
         \"persona\": \"the system prompt you would give them, two or three \
         sentences, precise\", \"skills\": [\"skill names they should use\"]}}]"
    )
}

/// read a hiring plan out of whatever the model answered: json, with or
/// without a markdown fence around it.
pub fn parse_plan(text: &str) -> Result<Vec<Employee>, String> {
    let trimmed = text.trim();
    // strip a fence if the model wrapped its json
    let body = if let Some(start) = trimmed.find('[') {
        match trimmed.rfind(']') {
            Some(end) if end > start => &trimmed[start..=end],
            _ => trimmed,
        }
    } else {
        trimmed
    };
    let people: Vec<Employee> = serde_json::from_str(body)
        .map_err(|e| format!("the ceo did not answer with a hiring plan ({e})"))?;
    if people.is_empty() {
        return Err("the ceo hired nobody".into());
    }
    for p in &people {
        if p.name.trim().is_empty() || p.persona.trim().is_empty() {
            return Err("an employee came without a name or a persona".into());
        }
    }
    Ok(people)
}

/// ask the ceo to plan the team for a mission.
pub fn plan_hiring(
    runner: &mut Runner,
    mission: &str,
    skills: &[String],
) -> Result<Vec<Employee>, String> {
    let prompt = ceo_prompt(mission, skills);
    let answer = runner.run(&prompt, |_| {})?;
    parse_plan(&answer)
}

// ------------------------------------------------------------------ the work

/// queue a task for an employee: the mailbox is a file, so nothing is lost.
pub fn queue_task(root: &Path, employee: &str, task: &str) -> Result<PathBuf, String> {
    let path = mail_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let entry = Task {
        employee: employee.to_string(),
        task: task.to_string(),
        state: "queued".into(),
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        result: String::new(),
    };
    let mut line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    line.push('\n');
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}

/// the mailbox, oldest first.
pub fn tasks(root: &Path) -> Vec<Task> {
    let Ok(text) = std::fs::read_to_string(mail_path(root)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<Task>(l).ok())
        .collect()
}

/// the last state of each task, keyed by (employee, task).
pub fn mailbox(root: &Path) -> Vec<Task> {
    let mut out: Vec<Task> = Vec::new();
    for t in tasks(root) {
        if let Some(prev) = out
            .iter_mut()
            .find(|p| p.employee == t.employee && p.task == t.task)
        {
            *prev = t;
        } else {
            out.push(t);
        }
    }
    out
}

/// record how a task ended.
pub fn close_task(
    root: &Path,
    employee: &str,
    task: &str,
    state: &str,
    result: &str,
) -> Result<(), String> {
    let path = mail_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let entry = Task {
        employee: employee.to_string(),
        task: task.to_string(),
        state: state.to_string(),
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        result: result.chars().take(400).collect(),
    };
    let mut line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
    line.push('\n');
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

/// give one task to one employee: an agent run with their persona on top,
/// their skills, and the memory of the project. the result is written to the
/// mailbox as it goes, so a crashed run still leaves a trace.
pub fn dispatch<F: FnMut(Event)>(
    root: &Path,
    settings: Settings,
    who: &Employee,
    task: &str,
    mut on: F,
) -> Result<String, String> {
    queue_task(root, &who.name, task)?;
    // an employee is a persona: their prompt goes in the system message, where
    // the model actually reads it, and it stays on disk as a file
    write_persona(root, who)?;
    let mut settings = settings;
    settings.persona = Some(format!("employee-{}", who.name));
    let mut runner = Runner::new(root, settings)?;
    let request = if who.mission.trim().is_empty() {
        task.to_string()
    } else {
        format!("{task}\n\nyour job in this company: {}", who.mission)
    };
    let answer = runner.run(&request, |e| on(e));
    match &answer {
        Ok(text) => close_task(root, &who.name, task, "done", text)?,
        Err(e) => close_task(root, &who.name, task, "failed", e)?,
    }
    answer
}

/// the employee's persona, as a persona file the agent can be run with.
pub fn write_persona(root: &Path, who: &Employee) -> Result<PathBuf, String> {
    let p = crate::persona::Persona {
        id: format!("employee-{}", who.name),
        title: format!("{} — {}", who.name, who.role),
        description: who.mission.clone(),
        prompt: format!("{}\n\nyour job: {}", who.persona, who.mission),
        path: PathBuf::new(),
    };
    crate::persona::save(Some(root), &p, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn project(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nana-co-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        d
    }

    fn alice() -> Employee {
        Employee {
            name: "alice".into(),
            role: "backend engineer".into(),
            mission: "owns the protocol work".into(),
            persona: "write small, tested functions; never guess.".into(),
            skills: vec!["release".into()],
        }
    }

    #[test]
    fn hiring_appends_and_never_duplicates() {
        let d = project("hire");
        assert_eq!(hire(&d, &[alice()]).unwrap(), 1);
        assert_eq!(hire(&d, &[alice()]).unwrap(), 0, "already on the payroll");
        assert_eq!(roster(&d).len(), 1);
        assert_eq!(roster(&d)[0].name, "alice");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_roster_is_readable_json_on_disk() {
        let d = project("json");
        hire(&d, &[alice()]).unwrap();
        let path = d.join(".nana/agents/roster.json");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"name\": \"alice\""), "{text}");
        assert!(text.contains("\"persona\""), "{text}");
        // and a hand-edited roster is honoured
        let edited = text.replace("alice", "bob");
        std::fs::write(&path, edited).unwrap();
        assert_eq!(roster(&d)[0].name, "bob");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn firing_removes_one_and_says_so_when_unknown() {
        let d = project("fire");
        hire(&d, &[alice()]).unwrap();
        fire(&d, "alice").unwrap();
        assert!(roster(&d).is_empty());
        assert!(fire(&d, "alice").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_hiring_plan_is_parsed_from_json_even_fenced() {
        let plain =
            r#"[{"name":"alice","role":"backend","mission":"protocol","persona":"be precise"}]"#;
        let people = parse_plan(plain).unwrap();
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].persona, "be precise");
        let fenced = format!("here is the team:\n```json\n{plain}\n```\nhope that helps");
        assert_eq!(parse_plan(&fenced).unwrap().len(), 1);
        assert!(parse_plan("i cannot help with that").is_err());
        assert!(parse_plan("[]").is_err(), "an empty team is not a plan");
        let missing = r#"[{"name":"","role":"x","mission":"y","persona":"z"}]"#;
        assert!(parse_plan(missing).is_err(), "an employee needs a name");
    }

    /// a provider that answers with a hiring plan, to drive the ceo end to end.
    fn ceo_server(body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 8192];
            sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .ok();
            let mut head: Option<usize> = None;
            let mut want = 0usize;
            loop {
                if let Some(h) = head {
                    if buf.len() >= h + want {
                        break;
                    }
                }
                match sock.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
                if head.is_none() {
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        head = Some(i + 4);
                        let h = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                        want = h
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                    }
                }
            }
            let req = String::from_utf8_lossy(&buf).to_string();
            let payload = format!(
                "{{\"choices\":[{{\"message\":{{\"content\":{}}}}}]}}",
                serde_json::to_string(body).unwrap()
            );
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            let _ = sock.write_all(resp.as_bytes());
            req
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn the_ceo_plans_the_team_end_to_end() {
        let d = project("ceo");
        let (url, server) = ceo_server(
            r#"[{"name":"alice","role":"backend","mission":"the protocol","persona":"be precise"},
                {"name":"bob","role":"reviewer","mission":"read the diffs","persona":"be blunt"}]"#,
        );
        let mut runner = Runner::with_client(
            &d,
            Settings::default(),
            crate::provider::Client::local(&url, "mock"),
        );
        let people = plan_hiring(
            &mut runner,
            "add a protocol layer",
            &["release".to_string()],
        )
        .unwrap();
        assert_eq!(people.len(), 2);
        assert_eq!(hire(&d, &people).unwrap(), 2);
        assert_eq!(roster(&d).len(), 2);
        // the mission really reached the model
        let req = server.join().unwrap();
        assert!(
            req.contains("add a protocol layer"),
            "the mission is in the prompt"
        );
        assert!(req.contains("release"), "and so are the project's skills");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_dispatched_task_reaches_the_model_with_the_employee_persona() {
        let d = project("dispatch");
        let (url, server) = ceo_server("done: the protocol layer is in");
        let who = alice();
        // dispatch builds its own client from the settings, so point those at
        // the mock for this run
        let mut s = Settings::default();
        s.base_url = Some(url.clone());
        s.model = Some("mock".into());
        s.provider = Some("openai-compatible".into());
        let answer = dispatch(&d, s, &who, "write the parser", |_| {}).unwrap();
        assert!(answer.contains("protocol"), "{answer}");
        // the employee's persona travelled with the request
        let req = server.join().unwrap();
        assert!(req.contains("backend engineer"), "the role is stated");
        assert!(
            req.contains("write small, tested functions"),
            "the persona too"
        );
        // and the mailbox kept a trace
        let mail = mailbox(&d);
        assert_eq!(mail.len(), 1);
        assert_eq!(mail[0].state, "done");
        assert_eq!(mail[0].employee, "alice");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_mailbox_keeps_the_last_state_of_a_task() {
        let d = project("mail");
        queue_task(&d, "alice", "do the thing").unwrap();
        close_task(&d, "alice", "do the thing", "done", "it is done").unwrap();
        queue_task(&d, "bob", "another thing").unwrap();
        let mail = mailbox(&d);
        assert_eq!(mail.len(), 2);
        let alice_task = mail.iter().find(|t| t.employee == "alice").unwrap();
        assert_eq!(alice_task.state, "done");
        assert_eq!(alice_task.result, "it is done");
        let bob_task = mail.iter().find(|t| t.employee == "bob").unwrap();
        assert_eq!(bob_task.state, "queued");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn describing_the_company_tells_you_how_to_use_it() {
        let d = project("describe");
        let empty = describe(&d);
        assert!(empty.contains("no employees yet"), "{empty}");
        hire(&d, &[alice()]).unwrap();
        let full = describe(&d);
        assert!(full.contains("alice"), "{full}");
        assert!(full.contains("backend engineer"), "{full}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_employee_gets_a_persona_file_of_their_own() {
        let d = project("persona");
        let path = write_persona(&d, &alice()).unwrap();
        assert!(
            path.ends_with(".nana/personas/employee-alice.md"),
            "{path:?}"
        );
        let p = crate::persona::load(Some(&d), "employee-alice").unwrap();
        assert!(p.prompt.contains("never guess"), "{}", p.prompt);
        assert!(p.prompt.contains("owns the protocol work"), "{}", p.prompt);
        let _ = std::fs::remove_dir_all(&d);
    }
}
