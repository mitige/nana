//! Explorateur de fichiers — panneau latéral à la neo-tree, filtre flou à
//! la Telescope, sobriété à la Apple.
//!
//! Modèle pur (aucune IO terminal) : l'arbre est lu à la demande, les
//! répertoires se déplient paresseusement, la frappe filtre en flou sur tout
//! le projet, et `git status` colore les fichiers touchés.

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Une ligne visible du panneau (arbre aplati ou résultat de filtre).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Chemin absolu.
    pub path: PathBuf,
    /// Profondeur dans l'arbre (0 en mode filtre).
    pub depth: usize,
    pub is_dir: bool,
    /// Nom affiché : nom court en mode arbre, chemin relatif en mode filtre.
    pub name: String,
    /// Vrai si le répertoire est déplié.
    pub expanded: bool,
}

pub struct Explorer {
    /// Racine du panneau (canonisée).
    pub root: PathBuf,
    rows: Vec<Row>,
    sel: usize,
    scroll: usize,
    expanded: HashSet<PathBuf>,
    filter: String,
    /// Chemin absolu → marqueur git ('M' modifié, 'A' ajouté, '?' non suivi).
    git: HashMap<PathBuf, char>,
    /// bascule « . » : montrer les dotfiles (.gitignore, .env…)
    show_hidden: bool,
}

/// Bruit de build, jamais montré (même avec la bascule dotfiles).
fn is_noise(name: &str) -> bool {
    matches!(
        name,
        "target" | "node_modules" | "__pycache__" | "dist" | ".git"
    )
}

/// Dotfile : visible seulement si la bascule `.` est active.
fn is_dotfile(name: &str) -> bool {
    name.starts_with('.')
}

/// Lit un répertoire : dossiers d'abord, puis fichiers, alphabétique
/// insensible à la casse (comme Finder).
fn read_sorted(dir: &Path, show_hidden: bool) -> Vec<(PathBuf, bool)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(PathBuf, bool)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            !is_noise(name) && (show_hidden || !is_dotfile(name))
        })
        .map(|p| (p.clone(), p.is_dir()))
        .collect();
    entries.sort_by(|a, b| {
        b.1.cmp(&a.1).then_with(|| {
            a.0.file_name()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .cmp(&b.0.file_name().unwrap_or_default().to_ascii_lowercase())
        })
    });
    entries
}

/// Parse `git status --porcelain` : chemin absolu → marqueur.
fn parse_porcelain(text: &str, repo_root: &Path) -> HashMap<PathBuf, char> {
    let mut out = HashMap::new();
    for l in text.lines() {
        if l.len() < 4 {
            continue;
        }
        let (x, y) = (l.as_bytes()[0] as char, l.as_bytes()[1] as char);
        let raw = l[3..].trim().trim_matches('"');
        // renommage : « ancien -> nouveau » — on suit le nouveau
        let rel = raw.rsplit(" -> ").next().unwrap_or(raw);
        let mark = if x == '?' || y == '?' {
            '?'
        } else if x == 'A' || y == 'A' {
            'A'
        } else if matches!(x, 'M' | 'D' | 'R') || matches!(y, 'M' | 'D') {
            'M'
        } else {
            continue;
        };
        out.insert(repo_root.join(rel), mark);
    }
    out
}

/// Statuts git de la racine (vide hors dépôt — jamais bloquant).
fn scan_git(root: &Path) -> HashMap<PathBuf, char> {
    let top = std::process::Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| PathBuf::from(s.trim()));
    let Some(repo) = top else {
        return HashMap::new();
    };
    std::process::Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| parse_porcelain(&s, &repo))
        .unwrap_or_default()
}

impl Explorer {
    pub fn new(root: PathBuf) -> Self {
        // absolute() : normalise sans produire le préfixe \\?\ que
        // canonicalize() collerait aux chemins sous Windows
        let root = std::path::absolute(&root).unwrap_or(root);
        let mut ex = Self {
            expanded: HashSet::from([root.clone()]),
            git: scan_git(&root),
            root,
            rows: Vec::new(),
            sel: 0,
            scroll: 0,
            filter: String::new(),
            show_hidden: false,
        };
        ex.refresh();
        ex
    }

    // -------------------------------------------------------------- arbre

    /// Recharge l'arbre depuis le disque (après création/suppression/renommage)
    /// et rescanne les marqueurs git.
    pub fn reload(&mut self) {
        self.git = scan_git(&self.root);
        self.refresh();
    }

    /// Reconstruit les lignes visibles (après pliage, filtre, rafraîchissement).
    fn refresh(&mut self) {
        let keep = self.rows.get(self.sel).map(|r| r.path.clone());
        self.rows.clear();
        if self.filter.is_empty() {
            self.push_rows(&self.root.clone(), 0);
        } else {
            self.refresh_filtered();
        }
        // la sélection suit le même chemin à travers le rafraîchissement
        self.sel = keep
            .and_then(|p| self.rows.iter().position(|r| r.path == p))
            .unwrap_or(0)
            .min(self.rows.len().saturating_sub(1).max(0));
        if self.rows.is_empty() {
            self.sel = 0;
        }
    }

    fn push_rows(&mut self, dir: &Path, depth: usize) {
        for (path, is_dir) in read_sorted(dir, self.show_hidden) {
            let expanded = self.expanded.contains(&path);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            self.rows.push(Row {
                path: path.clone(),
                depth,
                is_dir,
                name,
                expanded,
            });
            if is_dir && expanded {
                self.push_rows(&path, depth + 1);
            }
        }
    }

    /// Mode filtre : tous les fichiers du projet, triés par pertinence floue.
    fn refresh_filtered(&mut self) {
        let mut all = Vec::new();
        Self::walk(&self.root, &mut all);
        let matcher = SkimMatcherV2::default();
        let needle = self.filter.clone();
        let mut scored: Vec<(i64, PathBuf)> = all
            .into_iter()
            .filter_map(|p| {
                let rel = p
                    .strip_prefix(&self.root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .to_string();
                matcher.fuzzy_match(&rel, &needle).map(|score| (score, p))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        self.rows = scored
            .into_iter()
            .take(300)
            .map(|(_, p)| Row {
                name: p
                    .strip_prefix(&self.root)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .to_string(),
                path: p,
                depth: 0,
                is_dir: false,
                expanded: false,
            })
            .collect();
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for (path, is_dir) in read_sorted(dir, false) {
            if is_dir {
                Self::walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }

    // -------------------------------------------------------------- état

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn sel(&self) -> usize {
        self.sel
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn git_mark(&self, path: &Path) -> Option<char> {
        self.git.get(path).copied()
    }

    /// Fenêtre visible : le scroll suit la sélection (appelé au rendu).
    pub fn window(&mut self, height: usize) -> (usize, &[Row]) {
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + height {
            self.scroll = self.sel - height + 1;
        }
        let end = (self.scroll + height).min(self.rows.len());
        (
            self.scroll,
            &self.rows[self.scroll.min(self.rows.len())..end],
        )
    }

    // -------------------------------------------------------------- actions

    pub fn move_down(&mut self) {
        if !self.rows.is_empty() {
            self.sel = (self.sel + 1).min(self.rows.len() - 1);
        }
    }

    pub fn move_up(&mut self) {
        self.sel = self.sel.saturating_sub(1);
    }

    pub fn home(&mut self) {
        self.sel = 0;
    }

    pub fn end(&mut self) {
        if !self.rows.is_empty() {
            self.sel = self.rows.len() - 1;
        }
    }

    /// Enter : déplie/replie un dossier, ou signale le fichier à ouvrir.
    pub fn enter(&mut self) -> Option<PathBuf> {
        let row = self.rows.get(self.sel)?.clone();
        if row.is_dir && self.filter.is_empty() {
            if self.expanded.contains(&row.path) {
                self.expanded.remove(&row.path);
            } else {
                self.expanded.insert(row.path.clone());
            }
            self.refresh();
            None
        } else {
            Some(row.path)
        }
    }

    /// → : déplie un dossier fermé (sinon sans effet).
    pub fn expand(&mut self) {
        if let Some(row) = self.rows.get(self.sel).cloned() {
            if row.is_dir && !row.expanded && self.filter.is_empty() {
                self.expanded.insert(row.path);
                self.refresh();
            }
        }
    }

    /// ← : replie le dossier, ou remonte à sa ligne parente.
    pub fn collapse_or_parent(&mut self) {
        let Some(row) = self.rows.get(self.sel).cloned() else {
            return;
        };
        if !self.filter.is_empty() {
            return;
        }
        if row.is_dir && row.expanded {
            self.expanded.remove(&row.path);
            self.refresh();
            return;
        }
        // saute à la ligne du dossier parent
        if let Some(parent) = row.path.parent() {
            if let Some(pos) = self.rows.iter().position(|r| r.path == parent) {
                self.sel = pos;
            }
        }
    }

    pub fn push_filter(&mut self, c: char) {
        self.filter.push(c);
        self.sel = 0;
        self.refresh();
    }

    pub fn pop_filter(&mut self) {
        self.filter.pop();
        self.sel = 0;
        self.refresh();
    }

    /// « . » : montre/cache les dotfiles dans l'arbre.
    pub fn toggle_hidden(&mut self) -> bool {
        self.show_hidden = !self.show_hidden;
        self.refresh();
        self.show_hidden
    }

    /// Vide le filtre ; vrai s'il y avait quelque chose à effacer.
    pub fn clear_filter(&mut self) -> bool {
        let had = !self.filter.is_empty();
        self.filter.clear();
        self.refresh();
        had
    }
}

// ------------------------------------------------------------------ recherche

/// Recherche de fichiers flottante (façon Telescope) : filtre flou sur tout
/// le projet, résultats navigables. Modèle pur, zéro IO terminal.
pub struct FileSearch {
    pub root: PathBuf,
    filter: String,
    results: Vec<PathBuf>,
    sel: usize,
    scroll: usize,
}

impl FileSearch {
    pub fn new(root: PathBuf) -> Self {
        // absolute() : normalise sans produire le préfixe \\?\ que
        // canonicalize() collerait aux chemins sous Windows
        let root = std::path::absolute(&root).unwrap_or(root);
        let mut fs = Self {
            root,
            filter: String::new(),
            results: Vec::new(),
            sel: 0,
            scroll: 0,
        };
        fs.refresh();
        fs
    }

    fn refresh(&mut self) {
        let mut all = Vec::new();
        Self::collect(&self.root, &mut all);
        if self.filter.is_empty() {
            self.results = all.into_iter().take(200).collect();
        } else {
            let matcher = SkimMatcherV2::default();
            let needle = self.filter.clone();
            let mut scored: Vec<(i64, PathBuf)> = all
                .into_iter()
                .filter_map(|p| {
                    let rel = self.rel(&p);
                    matcher.fuzzy_match(&rel, &needle).map(|s| (s, p))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            self.results = scored.into_iter().take(200).map(|(_, p)| p).collect();
        }
        self.sel = 0;
        self.scroll = 0;
    }

    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        for (path, is_dir) in read_sorted(dir, false) {
            if is_dir {
                Self::collect(&path, out);
            } else {
                out.push(path);
            }
        }
    }

    /// Chemin relatif à la racine, pour l'affichage.
    pub fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .to_string_lossy()
            .to_string()
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn len(&self) -> usize {
        self.results.len()
    }

    pub fn sel(&self) -> usize {
        self.sel
    }

    pub fn sel_path(&self) -> Option<PathBuf> {
        self.results.get(self.sel).cloned()
    }

    pub fn push(&mut self, c: char) {
        self.filter.push(c);
        self.refresh();
    }

    pub fn pop(&mut self) {
        self.filter.pop();
        self.refresh();
    }

    pub fn move_down(&mut self) {
        if !self.results.is_empty() {
            self.sel = (self.sel + 1).min(self.results.len() - 1);
        }
    }

    pub fn move_up(&mut self) {
        self.sel = self.sel.saturating_sub(1);
    }

    pub fn home(&mut self) {
        self.sel = 0;
    }

    pub fn end(&mut self) {
        if !self.results.is_empty() {
            self.sel = self.results.len() - 1;
        }
    }

    /// Fenêtre visible : le scroll suit la sélection (appelé au rendu).
    pub fn window(&mut self, height: usize) -> (usize, Vec<PathBuf>) {
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + height {
            self.scroll = self.sel - height + 1;
        }
        let end = (self.scroll + height).min(self.results.len());
        (
            self.scroll,
            self.results[self.scroll.min(self.results.len())..end].to_vec(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Petit projet jetable : src/main.c, src/util.c, README.md, .cache/x, target/o.
    /// Nom unique par appel — les tests tournent en parallèle dans le même
    /// processus, un nom par PID seulement se ferait piétiner.
    fn fixture() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "cnano-expl-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join(".cache")).unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::write(dir.join("src/main.c"), "int main;\n").unwrap();
        fs::write(dir.join("src/util.c"), "int util;\n").unwrap();
        fs::write(dir.join("README.md"), "# ok\n").unwrap();
        fs::write(dir.join(".cache/x"), "hidden\n").unwrap();
        fs::write(dir.join("target/o"), "build\n").unwrap();
        dir
    }

    #[test]
    fn arbre_trie_dossiers_dabord_et_cache_le_bruit() {
        let root = fixture();
        let ex = Explorer::new(root.clone());
        let names: Vec<&str> = ex.rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["src", "README.md"], "dossiers d'abord, alpha");
        assert!(!names.contains(&".cache"), "dotfiles cachés");
        assert!(!names.contains(&"target"), "build caché");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deplier_replier_un_dossier() {
        let root = fixture();
        let mut ex = Explorer::new(root.clone());
        assert_eq!(ex.rows().len(), 2);
        ex.enter(); // déplie src
        let names: Vec<&str> = ex.rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["src", "main.c", "util.c", "README.md"]);
        assert_eq!(ex.rows()[1].depth, 1);
        ex.collapse_or_parent(); // replie
        assert_eq!(ex.rows().len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fleche_gauche_remonte_au_parent() {
        let root = fixture();
        let mut ex = Explorer::new(root.clone());
        ex.enter(); // déplie src, sélection reste sur src
        ex.move_down(); // → main.c
        assert_eq!(ex.rows()[ex.sel()].name, "main.c");
        ex.collapse_or_parent(); // pas un dossier → remonte à src
        assert_eq!(ex.rows()[ex.sel()].name, "src");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn filtre_flou_trouve_partout() {
        let root = fixture();
        let mut ex = Explorer::new(root.clone());
        for c in "util".chars() {
            ex.push_filter(c);
        }
        let names: Vec<&str> = ex.rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["src/util.c"], "chemin relatif affiché");
        assert!(ex.clear_filter());
        assert_eq!(ex.rows().len(), 2, "retour à l'arbre");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn enter_sur_fichier_le_renvoie() {
        let root = fixture();
        let mut ex = Explorer::new(root.clone());
        ex.move_down(); // README.md
        let p = ex.enter().expect("un fichier s'ouvre");
        assert!(p.ends_with("README.md"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recherche_floue_et_selection() {
        let root = fixture();
        let mut fs = FileSearch::new(root.clone());
        assert_eq!(fs.len(), 3, "tous les fichiers au départ (filtre vide)");
        for c in "util".chars() {
            fs.push(c);
        }
        assert_eq!(fs.len(), 1);
        assert!(fs.sel_path().unwrap().ends_with("util.c"));
        for _ in 0..4 {
            fs.pop();
        }
        assert_eq!(fs.len(), 3, "filtre vide → tout le projet revient");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recherche_navigation_bornee() {
        let root = fixture();
        let mut fs = FileSearch::new(root.clone());
        fs.move_up();
        assert_eq!(fs.sel(), 0, "pas de dépassement en haut");
        fs.end();
        assert_eq!(fs.sel(), fs.len() - 1);
        fs.move_down();
        assert_eq!(fs.sel(), fs.len() - 1, "borné en bas");
        fs.home();
        assert_eq!(fs.sel(), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn point_bascule_les_dotfiles() {
        let root = fixture(); // contient .cache/x
        let mut ex = Explorer::new(root.clone());
        let names: Vec<&str> = ex.rows().iter().map(|r| r.name.as_str()).collect();
        assert!(!names.contains(&".cache"), "caché par défaut");
        assert!(ex.toggle_hidden(), "activé");
        let names: Vec<&str> = ex.rows().iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&".cache"), "visible après « . »");
        assert!(!names.contains(&"target"), "le bruit reste caché");
        assert!(!ex.toggle_hidden(), "désactivé");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn porcelain_git_est_parse() {
        let txt = " M src/main.c\n?? src/neuf.c\nA  src/ajout.c\nR  vieux.c -> nouveau.c\n";
        let m = parse_porcelain(txt, Path::new("/repo"));
        assert_eq!(m[Path::new("/repo/src/main.c")], 'M');
        assert_eq!(m[Path::new("/repo/src/neuf.c")], '?');
        assert_eq!(m[Path::new("/repo/src/ajout.c")], 'A');
        assert_eq!(m[Path::new("/repo/nouveau.c")], 'M');
    }
}
