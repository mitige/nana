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

    /// Hands back the label of a section, lowercase as every user-visible text is, used by the section menu and its titles.
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

/// Builds the menu lines for the section list, the current section marked with a triangle.
pub fn section_titles(current: Section) -> Vec<String> {
    Section::ALL
        .iter()
        .map(|s| format!("{}{}", if *s == current { "▸ " } else { "  " }, s.title()))
        .collect()
}
