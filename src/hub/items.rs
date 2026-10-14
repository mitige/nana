use super::pages::page_web;
use super::*;

impl Hub {
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

    /// Lists the memory pages as items, each one opening its page when chosen.
    /// the label stays `class: name`, which the rest of the hub matches on; the
    /// kind of the page and the sage that keeps it go in the note, where they
    /// are read without breaking a lookup by label.
    pub(super) fn memory_items(&self) -> Vec<Item> {
        Memory::open(&self.root)
            .list()
            .into_iter()
            .map(|e| {
                let kind = magi::Kind::of(e.class, &e.name);
                Item {
                    label: format!("{}: {}", e.class.id(), e.name),
                    note: format!(
                        "{} — {} — kept by {}",
                        e.title,
                        kind.label(),
                        kind.keeper().id()
                    ),
                    action: Action::ReadMemory(e.class, e.name),
                }
            })
            .collect()
    }

    /// every memory page, as a row the map can point at
    pub(super) fn world_items(&self) -> Vec<Item> {
        world::nodes(&self.root)
            .into_iter()
            .map(|n| Item {
                label: format!(
                    "{} {}: {}",
                    world::glyph(n.confidence),
                    n.class.id(),
                    n.name
                ),
                note: n.day.map(world::iso).unwrap_or_else(|| "undated".into()),
                action: Action::ReadMemory(n.class, n.name),
            })
            .collect()
    }

    /// the wiki: written pages first, then the audit trail.
    pub(super) fn knowledge_items(&self) -> Vec<Item> {
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

    /// Lists the endpoints as items: every built-in, then every custom one you
    /// added. The one answering the current model wears a dot, and enter
    /// routes the model to the endpoint you pick.
    pub(super) fn provider_items(&self) -> Vec<Item> {
        let s = self.settings();
        let current = s
            .model
            .clone()
            .unwrap_or_else(|| crate::provider::DEFAULT_MODEL.into());
        let answering = crate::provider::Endpoint::resolve(&s, &current).id;
        crate::provider::Endpoint::all(&s)
            .into_iter()
            .map(|e| {
                let picked = e.id == answering;
                let mut note = format!("{} · {}", e.label, e.base_url);
                if e.custom {
                    note.push_str(" · custom");
                }
                Item {
                    label: format!("{}{}", if picked { "● " } else { "  " }, e.id),
                    note,
                    action: Action::SetProvider(e.id.clone()),
                }
            })
            .collect()
    }

    /// Lists the discovered skills as items, each one running its skill when chosen.
    pub(super) fn skill_items(&self) -> Vec<Item> {
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

    /// Lists the personas as items, the current one marked with a dot, each one selectable.
    pub(super) fn persona_items(&self) -> Vec<Item> {
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
    pub(super) fn agent_items(&self) -> Vec<Item> {
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
    pub(super) fn refresh_detail(&mut self) {
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
            Action::SetProvider(id) => {
                let s = self.settings();
                match crate::provider::Endpoint::by_id(id, &s) {
                    Some(e) => format!(
                        "{}\\nkind: {}\\nendpoint: {}\\nkey: {}\\n\\nenter: route the model here\\nctrl+w in this box: add a custom endpoint",
                        e.label,
                        e.kind_name(),
                        e.base_url,
                        e.key_state()
                    ),
                    None => item.note.clone(),
                }
            }
            Action::None => {
                if self.current() == Section::Agents {
                    crate::company::describe(&self.root)
                } else {
                    item.note.clone()
                }
            }
        };
    }
}
