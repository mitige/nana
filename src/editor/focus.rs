//! alt-tab : passage du focus entre les zones.

use super::*;

impl Editor {
    /// Alt-Tab : éditeur → explorateur → recherche → éditeur.
    /// Les panneaux absents sont sautés ; sans aucun panneau, ouvre
    /// l'explorateur (le geste sert toujours).
    pub(super) fn cycle_focus(&mut self) {
        // ronde : éditeur → explorateur → recherche → terminal (les absents
        // sont sautés) ; sans aucun panneau, ouvre l'explorateur
        let mut order = vec![Focus::Editor];
        if self.explorer.is_some() {
            order.push(Focus::Explorer);
        }
        if self.search.is_some() {
            order.push(Focus::Search);
        }
        if self.term.is_some() {
            order.push(Focus::Terminal);
        }
        if order.len() == 1 {
            self.toggle_explorer();
            return;
        }
        let pos = order.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = order[(pos + 1) % order.len()];
    }
}
