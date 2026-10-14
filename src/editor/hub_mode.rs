//! le hub : écran d'accueil et ses actions.

use super::*;

impl Editor {
    /// the hub is modal too: it browses the boxes, and enter acts on the
    /// selected item.
    pub(super) fn hub_key(&mut self, key: KeyEvent) {
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
            // ctrl+w: add an endpoint of your own, from the providers box
            KeyCode::Char('w') if ctrl => {
                let providers = self
                    .hub
                    .as_ref()
                    .map(|h| h.current() == crate::hub::Section::Providers)
                    .unwrap_or(false);
                if providers {
                    if let Some(h) = self.hub.as_mut() {
                        h.begin(crate::hub::Purpose::NewProvider);
                    }
                } else {
                    self.status = "ctrl+w adds a provider — open the providers box first".into();
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
    pub(super) fn hub_edit(&mut self) {
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
    pub(super) fn hub_delete(&mut self) {
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
    pub(super) fn hub_task(&mut self) {
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
    pub(super) fn open_path(&mut self, path: std::path::PathBuf) {
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
    pub(super) fn hub_act(&mut self) {
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
                pane.conversation = self.conversation.clone();
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
}
