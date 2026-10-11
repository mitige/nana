//! file and directory icons and their colours.

use crate::editor::*;

/// Icônes activées ? `NANO_ICONS=0` → repli lettres cerclées (terminaux
/// sans Nerd Font). Les glyphes viennent des devicons de JetBrainsMono NF.
pub(in crate::editor) fn icons_enabled() -> bool {
    static ONCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ONCE.get_or_init(|| {
        std::env::var_os("NANO_ICONS")
            .map(|v| v != "0")
            .unwrap_or(true)
    })
}

/// Icône devicon du fichier (les vrais logos : C , Rust , Python …).
/// the glyph comes from the language registry, so a new language brings its
/// own icon instead of falling into a catch-all branch.
pub(in crate::editor) fn file_icon_dev(name: &str) -> &'static str {
    static TABLE: std::sync::LazyLock<HashMap<char, &'static str>> =
        std::sync::LazyLock::new(|| {
            let mut m: HashMap<char, &'static str> = HashMap::new();
            for l in langs::all() {
                m.entry(l.icon).or_insert_with(|| {
                    let s: &'static str = Box::leak(l.icon.to_string().into_boxed_str());
                    s
                });
            }
            m
        });
    let icon = langs::for_path(Path::new(name)).icon;
    TABLE.get(&icon).copied().unwrap_or("\u{F15B}")
}

/// Returns the one-letter fallback icon of a file for terminals without nerd font glyphs, picked by extension in a fixed table.
pub(in crate::editor) fn file_icon_plain(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "c" | "h" => "Ⓒ",
        "rs" => "Ⓡ",
        "py" => "Ⓟ",
        "js" | "ts" | "mjs" => "Ⓙ",
        "sh" | "bash" | "zsh" => "Ⓢ",
        "html" | "htm" => "Ⓗ",
        "css" => "ⓒ",
        "md" | "txt" => "Ⓜ",
        "toml" | "json" | "yaml" | "yml" | "lock" | "cfg" => "Ⓣ",
        _ if lower == "makefile" || ext == "mk" => "Ⓚ",
        _ => "◆",
    }
}

/// Mini-icône du fichier : devicon si possible, lettre cerclée sinon.
pub(in crate::editor) fn file_icon(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "";
    }
    if icons_enabled() {
        file_icon_dev(name)
    } else {
        file_icon_plain(name)
    }
}

/// Icône de dossier :  ouverte /  fermée (repli : ▾/▸).
pub(in crate::editor) fn dir_icon(expanded: bool) -> &'static str {
    if icons_enabled() {
        if expanded {
            "\u{F07C}"
        } else {
            "\u{F07B}"
        } // fa-folder-open / fa-folder
    } else if expanded {
        "▾"
    } else {
        "▸"
    }
}

/// Couleur d'un fichier selon son type — chaque famille a sa teinte.
pub(in crate::editor) fn file_color(name: &str, is_dir: bool) -> Color {
    if is_dir {
        return Ed::cyan();
    }
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        // build artefacts stay quiet
        "o" | "a" | "so" | "bin" | "o_" => Ed::gutter(),
        _ => Color::Indexed(langs::for_path(Path::new(name)).color),
    }
}
