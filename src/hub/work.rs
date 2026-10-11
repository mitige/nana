use super::pages::today;
use super::*;

impl Hub {
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
