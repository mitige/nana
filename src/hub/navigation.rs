use super::*;

impl Hub {
    /// Moves to the next section, clearing the filter and refreshing the items, the world map starting its sweep when entered.
    pub fn next_section(&mut self) {
        self.section = (self.section + 1) % Section::ALL.len();
        self.query.clear();
        self.enter_section();
        self.refresh();
    }

    /// Moves to the previous section, clearing the filter and refreshing the items, the world map starting its sweep when entered.
    pub fn prev_section(&mut self) {
        self.section = (self.section + Section::ALL.len() - 1) % Section::ALL.len();
        self.query.clear();
        self.enter_section();
        self.refresh();
    }

    /// opening the map starts its sweep, and leaving it forgets the sweep.
    pub(super) fn enter_section(&mut self) {
        self.sweep_started = (self.current() == Section::World).then(std::time::Instant::now);
    }

    /// the sweep's progress, while the map is still being drawn. None once the
    /// map is at rest, or when another box is open: nothing to animate then.
    pub fn sweep_progress(&self) -> Option<f32> {
        let started = self.sweep_started?;
        let p = sweep_at(started, std::time::Instant::now());
        (p < 1.0).then_some(p)
    }

    /// Moves the cursor down one item, wrapping at the end, and refreshes the detail.
    pub fn down(&mut self) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        self.cursors[self.section] = (self.cursor() + 1) % n;
        self.refresh_detail();
    }

    /// Moves the cursor up one item, wrapping at the start, and refreshes the detail.
    pub fn up(&mut self) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        self.cursors[self.section] = (self.cursor() + n - 1) % n;
        self.refresh_detail();
    }

    /// Appends a typed character to the filter, puts the cursor back on the first item and refreshes the list.
    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.cursors[self.section] = 0;
        self.refresh();
    }

    /// Removes the last filter character, puts the cursor back on the first item and refreshes the list.
    pub fn pop_char(&mut self) {
        self.query.pop();
        self.cursors[self.section] = 0;
        self.refresh();
    }
}
