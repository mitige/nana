use super::*;

impl Hub {
    /// start asking for something: the keystrokes go to `input` from now on.
    pub fn begin(&mut self, purpose: Purpose) {
        self.purpose = purpose;
        self.input.clear();
        self.activity.clear();
    }

    /// Leaves the input mode and clears the typed text, so the next key goes back to the list.
    pub fn cancel_input(&mut self) {
        self.purpose = Purpose::Filter;
        self.input.clear();
    }

    /// Tells whether the hub is taking typed text right now, so the keyboard feeds the input and not the list.
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

    /// Removes the last typed character in input mode, or the last filter character otherwise.
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
                )?;
                self.refresh();
                Ok(Outcome::OpenFile(path))
            }
            Purpose::NewMemory(class) => {
                let m = Memory::open(&self.root);
                let path = m.write(class, &value, "todo: what this project should remember")?;
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
                let path = persona::save(Some(&self.root), &p, true)?;
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
            Purpose::NewProvider => {
                let msg = super::apply::add_provider(&self.root, &value)?;
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
}
