//! Coloration syntaxique via syntect — thème « Minuit » sur mesure, embarqué.
//!
//! Le thème vit dans `assets/c-nano.tmTheme` (compilé dans le binaire via
//! `include_str!`) : plus aucune dépendance à un thème externe, les couleurs
//! ne peuvent plus disparaître. Repli : base16-mocha.dark si le plist est
//! injouable (ne devrait jamais arriver — le fichier est testé).

use std::io::Cursor;
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

pub static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);

/// Thème doc (sobre) — utilisé par le visualiseur de fiches, pas par l'éditeur.
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let set = ThemeSet::load_defaults();
    set.themes["base16-ocean.dark"].clone()
});

/// Thème de l'éditeur : « Minuit », palette Apple sobre pensée pour le C.
/// Embarqué à la compilation — aucun fichier requis à l'exécution.
static EDITOR_THEME: LazyLock<Theme> = LazyLock::new(|| {
    const MINUIT: &str = include_str!("../assets/minuit.tmTheme");
    let mut cursor = Cursor::new(MINUIT.as_bytes());
    ThemeSet::load_from_reader(&mut cursor).unwrap_or_else(|_| {
        let set = ThemeSet::load_defaults();
        set.themes["base16-mocha.dark"].clone()
    })
});

fn c_syntax() -> &'static SyntaxReference {
    SYNTAXES
        .find_syntax_by_extension("c")
        .expect("syntaxe C introuvable")
}

/// Syntaxe par extension de fichier (éditeur multi-langage).
fn syntax_for(ext: &str) -> &'static SyntaxReference {
    SYNTAXES
        .find_syntax_by_extension(ext)
        .unwrap_or_else(|| c_syntax())
}

/// Couleur de fond du thème doc (pour harmoniser les blocs de code).
pub fn theme_bg() -> syntect::highlighting::Color {
    THEME
        .settings
        .background
        .unwrap_or(syntect::highlighting::Color {
            r: 20,
            g: 24,
            b: 32,
            a: 255,
        })
}

/// Couleur de fond de l'éditeur (thème Minuit) — unifie toute la surface.
pub fn editor_bg() -> syntect::highlighting::Color {
    EDITOR_THEME
        .settings
        .background
        .unwrap_or(syntect::highlighting::Color {
            r: 0x14,
            g: 0x14,
            b: 0x16,
            a: 255,
        })
}

/// Retourne, par ligne, les segments (style, texte) colorés.
pub fn highlight_c(code: &str) -> Vec<Vec<(Style, String)>> {
    highlight_code(code, "c")
}

/// Coloration par langage (extension du fichier : c, js, py, rs, html, css…).
/// Utilise le thème éditeur (Minuit) pour le code, l'autre pour la doc.
pub fn highlight_code(code: &str, ext: &str) -> Vec<Vec<(Style, String)>> {
    highlight_with_theme(code, ext, &EDITOR_THEME)
}

/// Coloration pour la doc (thème sobre).
pub fn highlight_code_doc(code: &str, ext: &str) -> Vec<Vec<(Style, String)>> {
    highlight_with_theme(code, ext, &THEME)
}

fn highlight_with_theme(code: &str, ext: &str, theme: &Theme) -> Vec<Vec<(Style, String)>> {
    let mut hl = HighlightLines::new(syntax_for(ext), theme);
    code.lines()
        .map(|line| {
            let regions = hl
                .highlight_line(line, &SYNTAXES)
                .unwrap_or_else(|_| vec![(Style::default(), line)]);
            regions
                .into_iter()
                .map(|(style, text)| {
                    // retire l'italique (mal rendu dans beaucoup de terminaux)
                    let style = Style {
                        font_style: style.font_style - FontStyle::ITALIC,
                        ..style
                    };
                    (style, text.to_string())
                })
                .collect()
        })
        .collect()
}

/// Rend une ligne colorée en séquences ANSI 24 bits (fond transparent).
pub fn line_to_ansi(segments: &[(Style, String)]) -> String {
    let refs: Vec<(Style, &str)> = segments.iter().map(|(s, t)| (*s, t.as_str())).collect();
    syntect::util::as_24_bit_terminal_escaped(&refs[..], false)
}

#[cfg(test)]
mod theme_tests {
    use super::*;

    /// Le thème Minuit embarqué se charge et porte ses couleurs signature.
    #[test]
    fn minuit_se_charge_avec_la_bonne_palette() {
        let bg = editor_bg();
        assert_eq!(
            (bg.r, bg.g, bg.b),
            (0x14, 0x14, 0x16),
            "fond Minuit, version profonde"
        );
        let hl = highlight_c("int main(void) { return (0); }");
        let flat: Vec<String> = hl[0].iter().map(|(_, t)| t.clone()).collect();
        assert!(flat.join("").contains("main"), "le texte survit");
        // au moins 3 couleurs distinctes sur une ligne de C riche
        let colors: std::collections::HashSet<_> =
            hl[0].iter().map(|(s, _)| s.foreground).collect();
        assert!(
            colors.len() >= 3,
            "la ligne doit porter plusieurs couleurs, pas une seule ({colors:?})"
        );
    }

    /// Les grandes familles du C ont chacune leur couleur Minuit.
    #[test]
    fn chaque_famille_a_sa_couleur() {
        let cases = [
            ("// silence", 0x6E, 0x6E, 0x73),         // commentaire
            ("return (0);", 0xFF, 0x7A, 0x93),        // mot-clé
            ("int i = 0;", 0x64, 0xD2, 0xFF),         // type
            ("char *s = \"oui\";", 0xA6, 0xDA, 0x95), // chaîne
            ("int x = 42;", 0xC6, 0xA0, 0xF6),        // nombre
        ];
        for (code, r, g, b) in cases {
            let hl = highlight_c(code);
            let hit = hl[0]
                .iter()
                .any(|(s, _)| (s.foreground.r, s.foreground.g, s.foreground.b) == (r, g, b));
            assert!(
                hit,
                "couleur #{r:02X}{g:02X}{b:02X} absente pour « {code} »"
            );
        }
    }
}
