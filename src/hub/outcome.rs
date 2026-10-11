use super::*;

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
