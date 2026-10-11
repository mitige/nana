use super::*;

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

    /// Tells whether the purpose takes typed text: the filter does not, every other purpose does, so the keyboard feeds the input box.
    pub fn takes_text(&self) -> bool {
        !matches!(self, Purpose::Filter)
    }
}
