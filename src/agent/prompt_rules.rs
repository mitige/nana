//! the fixed parts of the system prompt: the rules the agent follows, the sections
//! that never change, and the project instructions read from AGENTS.md.

use super::Section;
use std::path::Path;

/// the rules the agent follows about memory, appended after the memory index.
pub(super) const MEMORY_RULES: &str = "memory is a world model of this project: what is true of its state, \
     its conventions, and how it behaves. a wrong page is a bug to fix, not a fact to trust.\n\
     memory rules, each one is a duty, not a suggestion:\n\
     - when a task starts: call memory_list, then memory_read on every page that touches the \
     task. before you act on a page, say what you expect from it.\n\
     - when you check a fact: after acting, verify the result with a tool against that \
     expectation, and run memory_check on any page that carries a check.\n\
     - when a page is wrong: correct the page with memory_write at once, and say which belief was wrong.\n\
     - when the user states a preference or a rule: write it as a feedback or user page with memory_write.\n\
     - when a decision is made: write a project page with the reason, so the next run does not reopen it.\n\
     - when a task ends: write what was learned, the conventions met and the pitfalls hit, \
     with memory_write, before your final words.\n\
     - before your final words: ask yourself whether anything durable happened in this run; \
     if yes, the memory write comes first, the answer comes after.";

/// the structure rules the agent follows when it writes code: one job per function,
/// one responsibility per file, so a project stays easy to read and split.
pub(super) const STRUCTURE_RULES: &str =
    "code structure rules, to apply to every file you touch:\n\
     - one responsibility per file: a file holds one concept; split it when it mixes several.\n\
     - one job per function: a function over about 100 lines is split into named helpers.\n\
     - group by responsibility, not by size: a module holds the code of one feature, \
     and its tests live next to it or in their own file.\n\
     - every function has a doc comment that says why it exists and what it hands back.\n\
     - no dead code, no commented-out code, no unwrap() on user input.\n\
     - after a split, run the formatter, then the tests, and keep them green.";

/// the fixed opening of the prompt: who the agent is.
pub(super) fn identity_section() -> Section {
    Section {
        order: -1000,
        name: "identity",
        text: "you are the agent inside nana, a terminal editor. you work in the user's project, \
               you use the tools you are given, and you say what you did."
            .into(),
    }
}

/// the tools rule: the file and shell scope, and the destructive-command guard.
pub(super) fn tools_section() -> Section {
    Section {
        order: 1000,
        name: "tools",
        text: "tools: files and the shell are scoped to this project. a destructive \
               command is refused unless the user approved it — never try to work around that."
            .into(),
    }
}

/// the structure section: the code rules, placed before the autonomy rule so the
/// model reads how to shape its code before it is told to keep going.
pub(super) fn structure_section() -> Section {
    Section {
        order: 1050,
        name: "structure",
        text: STRUCTURE_RULES.into(),
    }
}

/// the autonomy rule: the model stops early when unsure it may go on, so say plainly it may.
pub(super) fn autonomy_section() -> Section {
    Section {
        order: 1100,
        name: "autonomy",
        text: "work in one unbroken run until the task is finished. do not stop to ask \
               whether to continue, do not ask permission between steps, and do not end \
               your turn with a plan. act, check the result with a tool, and keep going. \
               only answer in words when the task is done or when you are truly blocked."
            .into(),
    }
}

/// AGENTS.md, at the root or inside the project directory.
pub(super) fn read_instructions(root: &Path, project_dir: &str) -> Option<String> {
    for candidate in [
        root.join("AGENTS.md"),
        root.join(project_dir).join("AGENTS.md"),
    ] {
        if let Ok(t) = std::fs::read_to_string(&candidate) {
            if !t.trim().is_empty() {
                return Some(t);
            }
        }
    }
    None
}
