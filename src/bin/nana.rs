//! the `nana` binary: usage, version, then the editor.

use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let arg = std::env::args().nth(1);
    if arg.as_deref() == Some("--languages") {
        print_languages();
        return;
    }
    if matches!(
        arg.as_deref(),
        Some("-h") | Some("--help") | Some("-V") | Some("--version")
    ) {
        if matches!(arg.as_deref(), Some("-V") | Some("--version")) {
            println!("nana {VERSION}");
            return;
        }
        print!(
            "nana {VERSION} — an adaptive terminal editor\n\n\
             usage: nana [file|dir]\n\n\
             the editor adapts to the file you open: syntax colours, the\n\
             checker, the run recipe and the auto header all come from a\n\
             language registry (see src/langs.rs). in a project, nana runs\n\
             the project's own commands (cargo, npm, django, make, cmake…).\n\n\
             editing\n\
             \x20 ctrl+s        save            ctrl+q  quit\n\
             \x20 ctrl+z        undo            ctrl+k  cut line\n\
             \x20 ctrl+u        paste           ctrl+f  search\n\
             \x20 ctrl+r        replace         ctrl+g  go to line\n\
             \x20 tab           accept suggestion, else indent\n\n\
             panels\n\
             \x20 ctrl+t        file explorer   ctrl+o  fuzzy file search\n\
             \x20 f2            cycle panels    f3      terminal\n\
             \x20 ctrl+n/p      next/previous diagnostic\n\n\
             the language\n\
             \x20 ctrl+b        check (compiler / linter / syntax)\n\
             \x20 f5            run (project command, or the file itself)\n\
             \x20 f6            format (prettier, rustfmt, gofmt, black…)\n\
             \x20 f4            auto header in the language's comment syntax\n\
             \x20 ctrl+e        ghost completion (needs an api key)\n\n\
             config: ~/.config/nana/config.toml\n\n\
             nana --languages lists every language it knows\n"
        );
        return;
    }
    let target = arg.map(PathBuf::from);
    if let Err(e) = nana::editor::run(target) {
        eprintln!("nana: {e}");
        std::process::exit(1);
    }
}

/// the registry, printed as a table — the same table the readme carries.
fn print_languages() {
    let mut rows: Vec<(String, String, String, String, String)> = Vec::new();
    for l in nana::langs::all() {
        let files = l.names.join(", ");
        let run = match (&l.run_build, &l.run) {
            (Some(b), Some(r)) => format!("{} + {}", b.program, r.program),
            (None, Some(r)) => r.program.to_string(),
            _ => "-".to_string(),
        };
        rows.push((
            l.id.to_string(),
            l.exts.join(" "),
            files,
            l.check
                .map(|c| c.program.to_string())
                .unwrap_or_else(|| "-".into()),
            run,
        ));
    }
    println!(
        "{:<12} {:<28} {:<22} {:<10} run",
        "language", "extensions", "file names", "checker"
    );
    for r in &rows {
        println!("{:<12} {:<28} {:<22} {:<10} {}", r.0, r.1, r.2, r.3, r.4);
    }
    println!("\n{} languages", rows.len());
}
