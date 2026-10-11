use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// the short description shown next to the label
    pub note: String,
    pub action: Action,
}
