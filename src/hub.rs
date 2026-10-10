//! the hub: every box in one panel.
//!
//! a workspace should not be a pile of commands. this is the panel that holds
//! the boxes — memory, providers, skills, personas (and, next, the agents of
//! the company) — navigated with the arrow keys, with the detail of what you
//! selected on the right.
//!
//! the logic lives here and knows nothing about terminals: it is a list of
//! sections, each producing items, each item carrying the action it performs.
//! the tui draws it and hands the actions back.

use crate::company::{dispatch, plan_hiring};
use crate::memory::{Class, Memory};
use crate::persona;
use crate::settings::{self, Settings};
use crate::skills;
use crate::world;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Memory,
    Knowledge,
    Providers,
    Skills,
    Personas,
    Agents,
    World,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::Memory,
        Section::Providers,
        Section::Skills,
        Section::Personas,
        Section::Agents,
        Section::Knowledge,
        Section::World,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Section::Memory => "memory",
            Section::Knowledge => "knowledge",
            Section::World => "world",
            Section::Providers => "providers",
            Section::Skills => "skills",
            Section::Personas => "personas",
            Section::Agents => "agents",
        }
    }

    /// one line explaining what the box is for, shown at the top of the panel.
    pub fn hint(&self) -> &'static str {
        match self {
            Section::Memory => "what this project has learned — read, write, forget",
            Section::Knowledge => "the project's wiki — hand written, or dreamed from the sessions",
            Section::World => "the memory as a map: each class a lane, time running left to right",
            Section::Providers => "who can answer, and which model is current",
            Section::Skills => "packaged workflows, picked up by their trigger",
            Section::Personas => "saved system prompts, project or user",
            Section::Agents => "the company: a ceo, and the employees it hires",
        }
    }
}

/// what happens when you press enter on an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// nothing to do, it is information
    None,
    /// open a knowledge page in the editor
    ReadKnowledge(String),
    /// open a memory page in the editor
    ReadMemory(Class, String),
    /// use this model from now on
    SetModel(String),
    /// use this persona by default
    SetPersona(String),
    /// hand the skill to the agent
    RunSkill(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// the short description shown next to the label
    pub note: String,
    pub action: Action,
}

/// what the box is asking you for, when it is asking for something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// typing filters the list (the resting state)
    Filter,
    /// a name for a new memory page of that class
    NewMemory(Class),
    /// a name for a new knowledge page
    NewKnowledge,
    /// consolidate the latest sessions into the wiki
    Dream,
    /// a name for a new persona
    NewPersona,
    /// a name for a new skill
    NewSkill,
    /// the mission you give the ceo
    Hire,
    /// a task for that employee
    Task(String),
    /// the model to use from now on
    SetModel,
}

impl Purpose {
    /// the question shown in the box while you type.
    pub fn label(&self) -> String {
        match self {
            Purpose::Filter => "filter".into(),
            Purpose::NewMemory(c) => format!("new {} page", c.id()),
            Purpose::NewKnowledge => "new knowledge page".into(),
            Purpose::Dream => "dreaming".into(),
            Purpose::NewPersona => "new persona".into(),
            Purpose::NewSkill => "new skill".into(),
            Purpose::Hire => "mission for the ceo".into(),
            Purpose::Task(who) => format!("task for {who}"),
            Purpose::SetModel => "model".into(),
        }
    }

    pub fn takes_text(&self) -> bool {
        !matches!(self, Purpose::Filter)
    }
}

/// what the editor must do after the box acted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// nothing, the box redrew itself
    None,
    /// open this file in the editor, so you write it there
    OpenFile(PathBuf),
    /// say this in the status line
    Say(String),
}

pub struct Hub {
    pub root: PathBuf,
    pub section: usize,
    /// cursor per section, so moving around does not lose your place
    cursors: Vec<usize>,
    /// the filter, in browse mode
    pub query: String,
    pub purpose: Purpose,
    /// the text being typed for that purpose
    pub input: String,
    /// what is running in the background, if anything
    pub busy: Option<String>,
    /// what the workers reported, shown in the right card
    pub activity: Vec<(String, String)>,
    rx: Option<std::sync::mpsc::Receiver<crate::agent::Event>>,
    items: Vec<Item>,
    /// the right pane: the full detail of the selection
    detail: String,
}

impl Hub {
    pub fn open(root: &Path) -> Hub {
        let mut hub = Hub {
            root: root.to_path_buf(),
            section: 0,
            cursors: vec![0; Section::ALL.len()],
            query: String::new(),
            purpose: Purpose::Filter,
            input: String::new(),
            busy: None,
            activity: Vec::new(),
            rx: None,
            items: Vec::new(),
            detail: String::new(),
        };
        hub.refresh();
        hub
    }

    pub fn current(&self) -> Section {
        Section::ALL[self.section.min(Section::ALL.len() - 1)]
    }

    /// where we are in the section list, for the drawing.
    pub fn section_index(&self) -> usize {
        self.section
    }

    /// how many sections there are, so the tui can place things.
    pub fn section_count(&self) -> usize {
        Section::ALL.len()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn cursor(&self) -> usize {
        self.cursors[self.section]
    }

    pub fn selected(&self) -> Option<&Item> {
        self.items.get(self.cursor())
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// the settings of this project, loaded fresh: the boxes write to them.
    pub fn settings(&self) -> Settings {
        Settings::load(&self.root)
    }

    pub fn next_section(&mut self) {
        self.section = (self.section + 1) % Section::ALL.len();
        self.query.clear();
        self.refresh();
    }

    pub fn prev_section(&mut self) {
        self.section = (self.section + Section::ALL.len() - 1) % Section::ALL.len();
        self.query.clear();
        self.refresh();
    }

    pub fn down(&mut self) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        self.cursors[self.section] = (self.cursor() + 1) % n;
        self.refresh_detail();
    }

    pub fn up(&mut self) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        self.cursors[self.section] = (self.cursor() + n - 1) % n;
        self.refresh_detail();
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.cursors[self.section] = 0;
        self.refresh();
    }

    pub fn pop_char(&mut self) {
        self.query.pop();
        self.cursors[self.section] = 0;
        self.refresh();
    }

    /// rebuild the items of the current section, honouring the filter.
    pub fn refresh(&mut self) {
        let mut items = match self.current() {
            Section::Memory => self.memory_items(),
            Section::World => self.world_items(),
            Section::Knowledge => self.knowledge_items(),
            Section::Providers => self.provider_items(),
            Section::Skills => self.skill_items(),
            Section::Personas => self.persona_items(),
            Section::Agents => self.agent_items(),
        };
        if !self.query.is_empty() {
            let q = self.query.to_lowercase();
            items.retain(|i| {
                i.label.to_lowercase().contains(&q) || i.note.to_lowercase().contains(&q)
            });
        }
        if self.cursor() >= items.len() {
            self.cursors[self.section] = items.len().saturating_sub(1);
        }
        self.items = items;
        self.refresh_detail();
    }

    fn memory_items(&self) -> Vec<Item> {
        Memory::open(&self.root)
            .list()
            .into_iter()
            .map(|e| Item {
                label: format!("{}: {}", e.class.id(), e.name),
                note: e.title,
                action: Action::ReadMemory(e.class, e.name),
            })
            .collect()
    }

    /// every memory page, as a row the map can point at
    fn world_items(&self) -> Vec<Item> {
        world::nodes(&self.root)
            .into_iter()
            .map(|n| Item {
                label: format!("{} {}: {}", world::glyph(n.confidence), n.class.id(), n.name),
                note: n.day.map(world::iso).unwrap_or_else(|| "undated".into()),
                action: Action::ReadMemory(n.class, n.name),
            })
            .collect()
    }

    /// the wiki: written pages first, then the audit trail.
    fn knowledge_items(&self) -> Vec<Item> {
        let mut out: Vec<Item> = crate::knowledge::list(&self.root)
            .into_iter()
            .map(|p| Item {
                label: p.name.clone(),
                note: format!("{} — {} lines", p.title, p.lines),
                action: Action::ReadKnowledge(p.name),
            })
            .collect();
        for a in crate::knowledge::audit_list(&self.root).into_iter().rev() {
            out.push(Item {
                label: format!("audit/{}", a.name),
                note: format!("{} lines", a.lines),
                action: Action::ReadKnowledge(format!("audit/{}", a.name)),
            });
        }
        out
    }

    fn provider_items(&self) -> Vec<Item> {
        let s = self.settings();
        let current = s
            .model
            .clone()
            .unwrap_or_else(|| crate::provider::DEFAULT_MODEL.into());
        crate::provider::PROVIDERS
            .iter()
            .map(|p| {
                let picked = crate::provider::detect(&current).id == p.id;
                Item {
                    label: format!("{}{}", if picked { "● " } else { "  " }, p.id),
                    note: format!("{} · {}", p.label, p.base_url),
                    action: Action::None,
                }
            })
            .collect()
    }

    fn skill_items(&self) -> Vec<Item> {
        let s = self.settings();
        skills::discover(&self.root, &s.skill_dirs)
            .into_iter()
            .map(|sk| Item {
                label: sk.name.clone(),
                note: if sk.description.is_empty() {
                    sk.trigger.clone()
                } else {
                    sk.description.clone()
                },
                action: Action::RunSkill(sk.name),
            })
            .collect()
    }

    fn persona_items(&self) -> Vec<Item> {
        let current = self.settings().persona.unwrap_or_default();
        persona::list(Some(&self.root))
            .into_iter()
            .map(|p| Item {
                label: format!("{}{}", if p.id == current { "● " } else { "  " }, p.id),
                note: p.title,
                action: Action::SetPersona(p.id),
            })
            .collect()
    }

    /// the company: employees already on the payroll, plus the ceo's entry.
    fn agent_items(&self) -> Vec<Item> {
        let mut items = vec![Item {
            label: "● ceo".to_string(),
            note: "hires the team, then delegates the work".to_string(),
            action: Action::None,
        }];
        for a in crate::company::roster(&self.root) {
            items.push(Item {
                label: format!("  {}", a.name),
                note: format!("{} — {}", a.role, a.mission),
                action: Action::None,
            });
        }
        items
    }

    /// the right pane: what the selection actually is.
    fn refresh_detail(&mut self) {
        if !self.activity.is_empty() || self.busy.is_some() {
            let mut out = String::new();
            if let Some(what) = &self.busy {
                out.push_str(&format!("{what}…\n\n"));
            }
            for (kind, text) in &self.activity {
                let head = match kind.as_str() {
                    "tool" => "-> ",
                    "ok" => "ok ",
                    "no" => "no ",
                    "error" => "err ",
                    _ => "   ",
                };
                out.push_str(&format!("{head}{text}\n"));
            }
            self.detail = out;
            return;
        }
        let Some(item) = self.selected().cloned() else {
            self.detail = format!("({} is empty)", self.current().title());
            return;
        };
        if self.current() == Section::World {
            self.detail = world::render(&self.root, self.selected().map(|i| &i.action));
            return;
        }
        self.detail = match &item.action {
            Action::ReadMemory(class, name) => {
                let memory = Memory::open(&self.root);
                let mut text = memory
                    .read(*class, name)
                    .unwrap_or_else(|e| format!("unreadable: {e}"));
                text.push_str(&page_web(&memory, *class, name));
                text
            }
            Action::ReadKnowledge(name) => crate::knowledge::read(&self.root, name)
                .unwrap_or_else(|e| format!("unreadable: {e}")),
            Action::SetPersona(id) => persona::load(Some(&self.root), id)
                .map(|p| {
                    format!(
                        "{}\n\n{}\n\nenter: use this persona by default",
                        p.title, p.prompt
                    )
                })
                .unwrap_or_else(|| item.note.clone()),
            Action::RunSkill(name) => {
                let s = self.settings();
                skills::discover(&self.root, &s.skill_dirs)
                    .into_iter()
                    .find(|sk| sk.name == *name)
                    .map(|sk| {
                        format!(
                            "{}\ntrigger: {}\n\n{}\n\nenter: hand it to the agent",
                            sk.description, sk.trigger, sk.body
                        )
                    })
                    .unwrap_or_else(|| item.note.clone())
            }
            Action::SetModel(m) => format!("model {m}"),
            Action::None => {
                if self.current() == Section::Providers {
                    let s = self.settings();
                    format!(
                        "current model: {}\nprovider: {}\nsandbox: {}\n\nset a model in .nana/settings.json:\n  {{\"model\": \"gpt-4o\", \"provider\": \"openai\"}}",
                        s.model.clone().unwrap_or_else(|| format!("{} (default)", crate::provider::DEFAULT_MODEL)),
                        s.provider
                            .clone()
                            .unwrap_or_else(|| "auto, from the model name".into()),
                        if s.sandbox_enabled() { "on" } else { "off" },
                    )
                } else if self.current() == Section::Agents {
                    crate::company::describe(&self.root)
                } else {
                    item.note.clone()
                }
            }
        };
    }
}

impl Hub {
    /// start asking for something: the keystrokes go to `input` from now on.
    pub fn begin(&mut self, purpose: Purpose) {
        self.purpose = purpose;
        self.input.clear();
        self.activity.clear();
    }

    pub fn cancel_input(&mut self) {
        self.purpose = Purpose::Filter;
        self.input.clear();
    }

    pub fn typing(&self) -> bool {
        self.purpose.takes_text()
    }

    /// type one character into the box, wherever the box is.
    pub fn type_char(&mut self, c: char) {
        if self.typing() {
            self.input.push(c);
        } else {
            self.push_char(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.typing() {
            self.input.pop();
        } else {
            self.pop_char();
        }
    }

    /// enter: do what the box is asking, and say what the editor must do next.
    pub fn submit(&mut self) -> Result<Outcome, String> {
        if !self.typing() {
            return self.act();
        }
        let value = self.input.trim().to_string();
        if value.is_empty() {
            self.cancel_input();
            return Ok(Outcome::None);
        }
        let purpose = self.purpose.clone();
        self.purpose = Purpose::Filter;
        self.input.clear();
        match purpose {
            Purpose::NewKnowledge => {
                let path = crate::knowledge::write(
                    &self.root,
                    &value,
                    &format!("# {value}\n\ntodo: what this project should know"),
                )
                .map_err(|e| e)?;
                self.refresh();
                Ok(Outcome::OpenFile(path))
            }
            Purpose::NewMemory(class) => {
                let m = Memory::open(&self.root);
                let path = m
                    .write(class, &value, "todo: what this project should remember")
                    .map_err(|e| e)?;
                self.refresh();
                Ok(Outcome::OpenFile(path))
            }
            Purpose::NewPersona => {
                let p = persona::Persona {
                    id: value.clone(),
                    title: value.clone(),
                    description: "written in nana".into(),
                    prompt: "you are…\n\nwrite here what this persona should do.".into(),
                    path: PathBuf::new(),
                };
                let path = persona::save(Some(&self.root), &p, true).map_err(|e| e)?;
                self.refresh();
                Ok(Outcome::OpenFile(path))
            }
            Purpose::NewSkill => {
                let dir = self.root.join(".nana").join("skills");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let path = dir.join(format!("{}.md", value.replace(' ', "-")));
                let body = format!(
                    "---\nname: {value}\ndescription: what this skill does\ntrigger: {value}\n---\n1. first step\n2. second step\n"
                );
                std::fs::write(&path, body).map_err(|e| e.to_string())?;
                self.refresh();
                Ok(Outcome::OpenFile(path))
            }
            Purpose::Hire => {
                self.hire(&value);
                Ok(Outcome::Say(format!("the ceo is hiring for: {value}")))
            }
            Purpose::Task(who) => {
                self.give_task(&who, &value);
                Ok(Outcome::Say(format!("{who} is on it")))
            }
            Purpose::SetModel => {
                let msg = apply(&self.root, &Action::SetModel(value.clone()))?;
                self.refresh();
                Ok(Outcome::Say(msg))
            }
            Purpose::Dream => {
                self.dream();
                Ok(Outcome::Say("dreaming over the latest sessions".into()))
            }
            Purpose::Filter => Ok(Outcome::None),
        }
    }

    /// the dream: read the latest sessions, write back what they taught.
    /// it runs in the background, like the hiring.
    pub fn dream(&mut self) {
        let root = self.root.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.busy = Some("dreaming over the sessions".into());
        self.activity.clear();
        std::thread::spawn(move || {
            let settings = Settings::load(&root);
            let date = today();
            match crate::agent::Agent::new(&root, settings) {
                Ok(mut runner) => match crate::knowledge::dream(&mut runner, 10, &date) {
                    Ok(report) => {
                        let mut text = format!(
                            "read {} session(s), wrote {} page(s)",
                            report.read_sessions,
                            report.pages.len()
                        );
                        if !report.pages.is_empty() {
                            text.push_str(": ");
                            text.push_str(&report.pages.join(", "));
                        }
                        if let Some(a) = &report.audit {
                            text.push_str(&format!("\naudit: {}", a.display()));
                        }
                        let _ = tx.send(crate::agent::Event::Text(text));
                        let _ = tx.send(crate::agent::Event::Finished { steps: 1 });
                    }
                    Err(e) => {
                        let _ = tx.send(crate::agent::Event::Error(e));
                    }
                },
                Err(e) => {
                    let _ = tx.send(crate::agent::Event::Error(e));
                }
            }
        });
    }

    /// enter in browse mode: the selected item's action.
    pub fn act(&mut self) -> Result<Outcome, String> {
        let Some(action) = self.selected().map(|i| i.action.clone()) else {
            return Ok(Outcome::None);
        };
        match action {
            Action::SetPersona(id) => {
                let msg = apply(&self.root, &Action::SetPersona(id))?;
                self.refresh();
                Ok(Outcome::Say(msg))
            }
            Action::SetModel(m) => {
                let msg = apply(&self.root, &Action::SetModel(m))?;
                self.refresh();
                Ok(Outcome::Say(msg))
            }
            Action::RunSkill(name) => Ok(Outcome::Say(format!("skill {name}"))),
            Action::ReadMemory(_, _) | Action::ReadKnowledge(_) | Action::None => Ok(Outcome::None),
        }
    }

    /// edit the file behind the selection, in the editor.
    pub fn edit_selected(&mut self) -> Result<Outcome, String> {
        let Some(item) = self.selected().cloned() else {
            return Ok(Outcome::None);
        };
        let path = match &item.action {
            Action::ReadMemory(class, name) => Memory::open(&self.root)
                .root()
                .join(class.id())
                .join(format!("{name}.md")),
            Action::ReadKnowledge(name) => crate::knowledge::root_of(&self.root)
                .join(format!("{}.md", name.trim_end_matches(".md"))),
            Action::SetPersona(id) | Action::RunSkill(id) => {
                // personas and skills are files too: find the one that owns it
                let mut found = None;
                for dir in [
                    self.root.join(".nana/personas"),
                    self.root.join(".nana/skills"),
                ] {
                    let candidate = dir.join(format!("{id}.md"));
                    if candidate.exists() {
                        found = Some(candidate);
                    }
                }
                match found {
                    Some(p) => p,
                    None => return Err(format!("no file for « {id} »")),
                }
            }
            _ => return Ok(Outcome::None),
        };
        Ok(Outcome::OpenFile(path))
    }

    /// remove what is selected: a page, a persona, or an employee.
    pub fn delete_selected(&mut self) -> Result<String, String> {
        let Some(item) = self.selected().cloned() else {
            return Ok(String::new());
        };
        let msg = match &item.action {
            Action::ReadMemory(class, name) => {
                Memory::open(&self.root).forget(*class, name)?;
                format!("forgotten: {name}")
            }
            Action::ReadKnowledge(name) => {
                let path = crate::knowledge::root_of(&self.root)
                    .join(format!("{}.md", name.trim_end_matches(".md")));
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
                format!("forgotten: {name}")
            }
            Action::SetPersona(id) => {
                persona::delete(Some(&self.root), id)?;
                format!("persona {id} deleted")
            }
            Action::RunSkill(name) => {
                let path = self.root.join(".nana/skills").join(format!("{name}.md"));
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
                format!("skill {name} deleted")
            }
            _ => {
                // in the company box, delete means fire
                let name = item.label.trim().trim_start_matches('●').trim().to_string();
                if name == "ceo" {
                    return Err("you cannot fire the ceo".into());
                }
                crate::company::fire(&self.root, &name)?;
                format!("{name} was let go")
            }
        };
        self.refresh();
        Ok(msg)
    }

    /// what `n` should start, depending on the box you are in.
    pub fn new_purpose(&self) -> Purpose {
        match self.current() {
            Section::Memory => Purpose::NewMemory(Class::Project),
            // the map is a view of the memory: a new page is written from the memory box
            Section::World => Purpose::NewMemory(Class::Project),
            Section::Knowledge => Purpose::NewKnowledge,
            Section::Personas => Purpose::NewPersona,
            Section::Skills => Purpose::NewSkill,
            Section::Agents => Purpose::Hire,
            Section::Providers => Purpose::SetModel,
        }
    }

    /// the ceo hires in the background: the box stays usable while it thinks.
    pub fn hire(&mut self, mission: &str) {
        let root = self.root.clone();
        let mission = mission.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.busy = Some("the ceo is hiring".into());
        self.activity.clear();
        std::thread::spawn(move || {
            let settings = Settings::load(&root);
            let skills = skills::discover(&root, &settings.skill_dirs)
                .into_iter()
                .map(|s| s.name)
                .collect::<Vec<_>>();
            match crate::agent::Agent::new(&root, settings) {
                Ok(mut runner) => match plan_hiring(&mut runner, &mission, &skills) {
                    Ok(people) => {
                        let names = people
                            .iter()
                            .map(|p| format!("{} ({})", p.name, p.role))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let _ = crate::company::hire(&root, &people);
                        let _ = tx.send(crate::agent::Event::Text(format!("hired: {names}")));
                        let _ = tx.send(crate::agent::Event::Finished { steps: 1 });
                    }
                    Err(e) => {
                        let _ = tx.send(crate::agent::Event::Error(e));
                    }
                },
                Err(e) => {
                    let _ = tx.send(crate::agent::Event::Error(e));
                }
            }
        });
    }

    /// give one employee one task, in the background.
    pub fn give_task(&mut self, who: &str, task: &str) {
        let root = self.root.clone();
        let name = who.to_string();
        let task = task.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.busy = Some(format!("{name} is working"));
        self.activity.clear();
        std::thread::spawn(move || {
            let Some(employee) = crate::company::employee(&root, &name) else {
                let _ = tx.send(crate::agent::Event::Error(format!("nobody named {name}")));
                return;
            };
            let settings = Settings::load(&root);
            let sender = tx.clone();
            let result = dispatch(&root, settings, &employee, &task, |e| {
                let _ = sender.send(e);
            });
            match result {
                Ok(_) => {
                    let _ = tx.send(crate::agent::Event::Finished { steps: 1 });
                }
                Err(e) => {
                    let _ = tx.send(crate::agent::Event::Error(e));
                }
            }
        });
    }

    /// fold what the workers reported since the last frame.
    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        let mut finished = false;
        while let Ok(event) = rx.try_recv() {
            match event {
                crate::agent::Event::Text(t) => self.activity.push(("text".into(), t)),
                crate::agent::Event::ToolCall { name, args } => self
                    .activity
                    .push(("tool".into(), format!("{name} {}", args))),
                crate::agent::Event::ToolResult { name, ok, text } => self.activity.push((
                    if ok { "ok".into() } else { "no".into() },
                    format!("{name}: {text}"),
                )),
                crate::agent::Event::Finished { .. } => finished = true,
                crate::agent::Event::Error(e) => {
                    self.activity.push(("error".into(), e));
                    finished = true;
                }
                crate::agent::Event::Started { .. } | crate::agent::Event::File { .. } => {}
            }
        }
        if finished {
            self.busy = None;
            self.rx = None;
            self.refresh();
        }
    }
}

/// today, as a page name: 2026-10-10.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (mut y, mut d) = (1970i64, days as i64);
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if d < len {
            break;
        }
        d -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    while m < 12 && d >= months[m] {
        d -= months[m];
        m += 1;
    }
    format!("{y}-{:02}-{:02}", m + 1, d + 1)
}

/// the page's place among the others: what it links to, what links to it,
/// and how many times it was written, with the day of each version.
fn page_web(memory: &Memory, class: Class, name: &str) -> String {
    let mut out = String::new();
    let links = memory.links(class, name).unwrap_or_default();
    let back = memory.backlinks(name);
    if !links.is_empty() {
        out.push_str(&format!("\n\nlinks to: {}", links.join(", ")));
    }
    if !back.is_empty() {
        out.push_str(&format!("\n\nlinked from: {}", back.join(", ")));
    }
    let history = memory.history(class, name);
    if !history.is_empty() {
        out.push_str(&format!("\n\nhistory: {} version(s)", history.len()));
        for (day, _) in history.iter().rev() {
            out.push_str(&format!("\n  {day}"));
        }
    }
    out
}

pub fn section_titles(current: Section) -> Vec<String> {
    Section::ALL
        .iter()
        .map(|s| format!("{}{}", if *s == current { "▸ " } else { "  " }, s.title()))
        .collect()
}

/// the settings a box writes when you act on an item.
pub fn apply(root: &Path, action: &Action) -> Result<String, String> {
    let mut s = settings::Settings::load(root);
    match action {
        Action::SetModel(m) => {
            s.model = Some(m.clone());
            s.provider = Some(crate::provider::detect(m).id.to_string());
            s.save_project(root).map_err(|e| e.to_string())?;
            Ok(format!("model is now {m}"))
        }
        Action::SetPersona(id) => {
            s.persona = Some(id.clone());
            s.save_project(root).map_err(|e| e.to_string())?;
            Ok(format!("persona is now {id}"))
        }
        Action::RunSkill(name) => Ok(format!("run the skill {name}")),
        Action::ReadMemory(_, _) | Action::ReadKnowledge(_) | Action::None => Ok(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a unique scratch project per call: tests run in parallel inside one
    /// process, and a shared path means one test deletes another's fixture.
    fn project(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!(
            "nana-hub-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".nana")).unwrap();
        d
    }

    fn seeded() -> PathBuf {
        let d = project("seeded");
        let m = Memory::open(&d);
        m.write(Class::Project, "stack", "rust only, no async")
            .unwrap();
        m.write(Class::User, "tone", "blunt, no filler").unwrap();
        std::fs::create_dir_all(d.join(".nana/skills")).unwrap();
        std::fs::write(
            d.join(".nana/skills/release.md"),
            "---\nname: release\ndescription: cut a release\ntrigger: release\n---\nsteps\n",
        )
        .unwrap();
        std::fs::create_dir_all(d.join(".nana/personas")).unwrap();
        std::fs::write(
            d.join(".nana/personas/reviewer.md"),
            "---\ntitle: The Reviewer\ndescription: reads code\n---\nbe precise.\n",
        )
        .unwrap();
        d
    }

    #[test]
    fn the_memory_box_lists_what_the_project_remembers() {
        let d = seeded();
        let hub = Hub::open(&d);
        assert_eq!(hub.current(), Section::Memory);
        let names: Vec<&str> = hub.items().iter().map(|i| i.label.as_str()).collect();
        assert!(names.contains(&"project: stack"), "{names:?}");
        assert!(names.contains(&"user: tone"), "{names:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_knowledge_box_lists_the_wiki_pages() {
        let d = seeded();
        crate::knowledge::write(&d, "build", "# how it builds\n\ncargo build").unwrap();
        let mut hub = Hub::open(&d);
        while hub.current() != Section::Knowledge {
            hub.next_section();
        }
        let labels: Vec<&str> = hub.items().iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["build"], "{labels:?}");
        assert!(
            hub.items()[0].note.contains("how it builds"),
            "{}",
            hub.items()[0].note
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_detail_pane_shows_the_page_itself() {
        let d = seeded();
        let mut hub = Hub::open(&d);
        // find the stack page and show it
        while hub.selected().map(|i| i.label.as_str()) != Some("project: stack") {
            hub.down();
        }
        assert!(
            hub.detail().contains("rust only, no async"),
            "{}",
            hub.detail()
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn sections_cycle_and_keep_their_own_cursor() {
        let d = seeded();
        let mut hub = Hub::open(&d);
        hub.down();
        let first_cursor = hub.cursor();
        assert_eq!(first_cursor, 1);
        hub.next_section();
        assert_eq!(hub.current(), Section::Providers);
        assert_eq!(hub.cursor(), 0, "a fresh section starts at the top");
        hub.prev_section();
        assert_eq!(hub.current(), Section::Memory);
        assert_eq!(hub.cursor(), first_cursor, "and remembers where you were");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_page_shows_its_links_its_backlinks_and_its_history() {
        let d = seeded();
        let m = Memory::open(&d);
        m.write(Class::Project, "stack", "rust only, see [[deploy]]").unwrap();
        m.write(Class::Project, "deploy", "ships from [[stack]]").unwrap();
        m.write(Class::Project, "stack", "rust only, no async, see [[deploy]]")
            .unwrap();
        let mut hub = Hub::open(&d);
        while hub.selected().map(|i| i.label.as_str()) != Some("project: stack") {
            hub.down();
        }
        let detail = hub.detail().to_string();
        assert!(detail.contains("links to: deploy"), "{detail}");
        assert!(detail.contains("linked from: deploy"), "{detail}");
        assert!(detail.contains("history:"), "{detail}");
        assert!(detail.contains("3 version"), "{detail}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn typing_filters_the_list() {
        let d = seeded();
        let mut hub = Hub::open(&d);
        for c in "tone".chars() {
            hub.push_char(c);
        }
        assert_eq!(hub.items().len(), 1, "{:?}", hub.items());
        assert_eq!(hub.items()[0].label, "user: tone");
        hub.pop_char();
        hub.pop_char();
        hub.pop_char();
        hub.pop_char();
        assert_eq!(hub.items().len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_provider_box_marks_the_current_model() {
        let d = seeded();
        let mut s = Settings::load(&d);
        s.model = Some("claude-sonnet-4".into());
        s.save_project(&d).unwrap();
        let mut hub = Hub::open(&d);
        hub.next_section();
        assert_eq!(hub.current(), Section::Providers);
        let marked: Vec<&str> = hub
            .items()
            .iter()
            .filter(|i| i.label.starts_with("● "))
            .map(|i| i.label.as_str())
            .collect();
        assert_eq!(marked, vec!["● anthropic"], "{:?}", hub.items());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_skill_box_offers_to_run_it() {
        let d = seeded();
        let mut hub = Hub::open(&d);
        hub.next_section();
        hub.next_section();
        assert_eq!(hub.current(), Section::Skills);
        assert_eq!(hub.selected().map(|i| i.label.as_str()), Some("release"));
        assert_eq!(
            hub.selected().map(|i| i.action.clone()),
            Some(Action::RunSkill("release".into()))
        );
        assert!(hub.detail().contains("cut a release"), "{}", hub.detail());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn picking_a_persona_writes_it_to_the_project_settings() {
        let d = seeded();
        let mut hub = Hub::open(&d);
        for _ in 0..3 {
            hub.next_section();
        }
        assert_eq!(hub.current(), Section::Personas);
        assert_eq!(hub.selected().map(|i| i.label.as_str()), Some("  reviewer"));
        let action = hub.selected().unwrap().action.clone();
        let msg = apply(&d, &action).unwrap();
        assert_eq!(msg, "persona is now reviewer");
        assert_eq!(Settings::load(&d).persona.as_deref(), Some("reviewer"));
        // and the box now marks it as the current one
        hub.refresh();
        assert_eq!(hub.selected().map(|i| i.label.as_str()), Some("● reviewer"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn picking_a_model_writes_it_and_its_provider() {
        let d = seeded();
        let msg = apply(&d, &Action::SetModel("gemini-2.5-pro".into())).unwrap();
        assert!(msg.contains("gemini-2.5-pro"), "{msg}");
        let s = Settings::load(&d);
        assert_eq!(s.model.as_deref(), Some("gemini-2.5-pro"));
        assert_eq!(
            s.provider.as_deref(),
            Some("gemini"),
            "the provider follows the model name"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_empty_project_still_opens_every_box() {
        let d = project("empty");
        let mut hub = Hub::open(&d);
        for _ in 0..Section::ALL.len() {
            assert!(!hub.current().title().is_empty());
            assert!(
                !hub.detail().is_empty(),
                "{} has no detail",
                hub.current().title()
            );
            hub.next_section();
        }
        assert_eq!(hub.current(), Section::Memory, "it comes back around");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_world_box_draws_the_memory_as_lanes_in_time() {
        let d = seeded();
        let mut hub = Hub::open(&d);
        while hub.current() != Section::World {
            hub.next_section();
        }
        let lanes: Vec<&str> = hub.detail().lines().map(str::trim).collect();
        for class in ["user", "feedback", "project", "reference"] {
            assert!(
                lanes.iter().any(|l| l.starts_with(class)),
                "no lane for {class}: {}",
                hub.detail()
            );
        }
        assert!(hub.detail().contains("today"), "{}", hub.detail());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_agents_box_starts_with_the_ceo_alone() {
        let d = project("company");
        let mut hub = Hub::open(&d);
        hub.next_section();
        hub.next_section();
        hub.next_section();
        hub.next_section();
        assert_eq!(hub.current(), Section::Agents);
        assert_eq!(hub.items().len(), 1, "{:?}", hub.items());
        assert_eq!(hub.items()[0].label, "● ceo");
        let _ = std::fs::remove_dir_all(&d);
    }
}
