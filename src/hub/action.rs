use super::*;

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
    /// route the model to this endpoint from now on, by its id
    SetProvider(String),
}
