use super::*;

pub struct Hub {
    pub root: PathBuf,
    /// when the map was last opened, so it sweeps once per visit
    pub(super) sweep_started: Option<std::time::Instant>,
    pub section: usize,
    /// cursor per section, so moving around does not lose your place
    pub(super) cursors: Vec<usize>,
    /// the filter, in browse mode
    pub query: String,
    pub purpose: Purpose,
    /// the text being typed for that purpose
    pub input: String,
    /// what is running in the background, if anything
    pub busy: Option<String>,
    /// what the workers reported, shown in the right card
    pub activity: Vec<(String, String)>,
    pub(super) rx: Option<std::sync::mpsc::Receiver<crate::agent::Event>>,
    pub(super) items: Vec<Item>,
    /// the right pane: the full detail of the selection
    pub(super) detail: String,
}

impl Hub {
    /// Builds the hub for a project with the filter purpose and empty lists, so the menu opens before anything is loaded.
    pub fn open(root: &Path) -> Hub {
        let mut hub = Hub {
            root: root.to_path_buf(),
            sweep_started: None,
            section: 0,
            cursors: vec![0; Section::ALL.len()],
            query: String::new(),
            purpose: Purpose::Filter,
            input: String::new(),
            busy: None,
            activity: Vec::new(),
            rx: None,
            items: Vec::new(),
            detail: String::new(),
        };
        hub.refresh();
        hub
    }

    /// Hands back the section under the cursor, clamped so a stale index can never leave the list.
    pub fn current(&self) -> Section {
        Section::ALL[self.section.min(Section::ALL.len() - 1)]
    }

    /// where we are in the section list, for the drawing.
    pub fn section_index(&self) -> usize {
        self.section
    }

    /// how many sections there are, so the tui can place things.
    pub fn section_count(&self) -> usize {
        Section::ALL.len()
    }

    /// Hands back the filter text typed for the current section.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Hands back the items of the current section, as last refreshed.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Hands back the cursor of the current section, one cursor kept per section.
    pub fn cursor(&self) -> usize {
        self.cursors[self.section]
    }

    /// Hands back the item under the cursor, or None when the section is empty.
    pub fn selected(&self) -> Option<&Item> {
        self.items.get(self.cursor())
    }

    /// Hands back the detail text of the selected item, shown beside the list.
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// the settings of this project, loaded fresh: the boxes write to them.
    pub fn settings(&self) -> Settings {
        Settings::load(&self.root)
    }
}
