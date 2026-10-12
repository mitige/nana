//! the typescript bootcamp keys: the check runs tsc then ts-node on the file.

use super::*;

/// F7 on a typescript file runs the bootcamp check in the background and
/// reports its verdict in the status line; it must not open the terminal, so
/// the errors are read in the status line, not typed into a shell.
#[test]
fn f7_sur_un_fichier_ts_rend_le_verdict_dans_la_barre_sans_terminal() {
    let dir = std::env::temp_dir().join("nana-bootcamp-f7");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("hello.ts");
    std::fs::write(&file, "console.log(1);\n").unwrap();
    let mut ed = Editor::open(Some(file.as_path())).unwrap();
    ed.bootcamp_check();
    assert!(ed.term.is_none(), "le terminal ne doit pas s'ouvrir");
    assert!(ed.status.contains("bootcamp"), "statut : {}", ed.status);
    let started = std::time::Instant::now();
    while ed.bootcamp_rx.is_some() && started.elapsed().as_secs() < 60 {
        ed.poll_bootcamp();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(ed.bootcamp_rx.is_none(), "le verdict doit arriver");
    assert!(ed.status.starts_with("bootcamp"), "verdict : {}", ed.status);
}

/// a forbidden `any` must show in the gutter of a typescript file, so the
/// rule is seen while the student types, not only at the compile.
#[test]
fn any_interdit_apparait_dans_la_gouttiere() {
    let dir = std::env::temp_dir().join("nana-bootcamp-any");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bad.ts");
    std::fs::write(&file, "let x: any = 1;\n").unwrap();
    let mut ed = Editor::open(Some(file.as_path())).unwrap();
    ed.refresh_bootcamp_marks();
    assert!(ed.gutter_marks().contains_key(&1));
}

/// the bootcamp marks are a typescript rule: a rust file that happens to hold
/// the word `var` must not be marked by it.
#[test]
fn le_bootcamp_ne_marque_pas_un_fichier_rust() {
    let dir = std::env::temp_dir().join("nana-bootcamp-rs");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("ok.rs");
    std::fs::write(&file, "let var = 1;\n").unwrap();
    let mut ed = Editor::open(Some(file.as_path())).unwrap();
    ed.refresh_bootcamp_marks();
    assert!(!ed.gutter_marks().contains_key(&1));
}

/// the template command writes tsconfig.json and .gitignore into the exercise
/// folder, and never overwrites a file the student already wrote.
#[test]
fn le_modele_ecrit_sans_ecraser() {
    let dir = std::env::temp_dir().join("nana-bootcamp-template");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".gitignore"), "mon contenu\n").unwrap();
    let written = crate::bootcamp::write_templates(&dir).unwrap();
    assert_eq!(written, vec!["tsconfig.json".to_string()]);
    assert_eq!(
        std::fs::read_to_string(dir.join(".gitignore")).unwrap(),
        "mon contenu\n"
    );
}

/// the package check reads the package.json next to the file and says in the
/// status line what departs from the pins, or that the folder is in order.
#[test]
fn le_package_json_signale_un_ecart() {
    let dir = std::env::temp_dir().join("nana-bootcamp-package");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("package.json"), r#"{"type":"module"}"#).unwrap();
    let file = dir.join("hello.ts");
    std::fs::write(&file, "\n").unwrap();
    let mut ed = Editor::open(Some(file.as_path())).unwrap();
    ed.check_bootcamp_package();
    assert!(ed.status.contains("type"), "statut : {}", ed.status);
}

/// the forbidden word must stand out in the text itself, not only in the gutter
/// mark: on the line `let x: any = 1;` the cells of `any` carry a background
/// that the cells of `let` do not.
#[test]
fn le_mot_interdit_est_surligne_dans_le_texte() {
    let dir = std::env::temp_dir().join("nana-bootcamp-highlight");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("types.ts");
    std::fs::write(&file, "let x: any = 1;\n").unwrap();
    let mut ed = Editor::open(Some(file.as_path())).unwrap();
    ed.refresh_bootcamp_marks();
    let backend = ratatui::backend::TestBackend::new(120, 40);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    term.draw(|f| draw(f, &mut ed)).unwrap();
    let buf = term.backend().buffer().clone();
    let row = (0..40u16)
        .find(|&y| {
            (0..120u16)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .contains("let x: any")
        })
        .unwrap();
    let cells: Vec<String> = (0..120u16)
        .map(|x| buf[(x, row)].symbol().to_string())
        .collect();
    let column_of = |word: &str| -> u16 {
        (0..cells.len())
            .find(|&i| cells[i..].concat().starts_with(word))
            .unwrap() as u16
    };
    let any_at = column_of("any");
    let let_at = column_of("let");
    assert_ne!(
        buf[(any_at, row)].bg,
        buf[(let_at, row)].bg,
        "any doit avoir un fond que let n'a pas"
    );
}

/// a typescript file the compiler rejects must put the first error in the
/// status line, so the student reads it there and not in a shell.
#[test]
#[ignore = "needs tsc and ts-node on the PATH: run with cargo test -- --ignored"]
fn f7_sur_un_fichier_ts_faux_affiche_la_premiere_erreur() {
    let dir = std::env::temp_dir().join("nana-bootcamp-f7-bad");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bad.ts");
    std::fs::write(&file, "let x: number = \"oops\";\n").unwrap();
    let mut ed = Editor::open(Some(file.as_path())).unwrap();
    ed.bootcamp_check();
    let started = std::time::Instant::now();
    while ed.bootcamp_rx.is_some() && started.elapsed().as_secs() < 60 {
        ed.poll_bootcamp();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        ed.status.contains("error"),
        "la première erreur doit apparaître : {}",
        ed.status
    );
}
