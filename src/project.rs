//! project detection: the part that makes nana feel like it knows your repo.
//!
//! a file alone tells you the language; the *project* tells you the command.
//! nana walks up from the file, finds the first marker it understands
//! (Cargo.toml, package.json, pyproject.toml, go.mod, Makefile, CMakeLists…),
//! and derives run / build / test / format from it — including the common
//! framework shapes (next, vite, django, flask, spring, gradle…).

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cargo,
    Node,
    Python,
    Go,
    Make,
    CMake,
    Meson,
    Gradle,
    Maven,
    Dotnet,
    Zig,
    Docker,
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Cargo => "cargo",
            Kind::Node => "node",
            Kind::Python => "python",
            Kind::Go => "go",
            Kind::Make => "make",
            Kind::CMake => "cmake",
            Kind::Meson => "meson",
            Kind::Gradle => "gradle",
            Kind::Maven => "maven",
            Kind::Dotnet => "dotnet",
            Kind::Zig => "zig",
            Kind::Docker => "docker",
        }
    }
}

/// a shell line, kept as text so it can be typed into the terminal pane —
/// no argument-quoting puzzles, and pipelines (`a && b`) just work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line(pub String);

impl Line {
    pub fn new(s: impl Into<String>) -> Self {
        Line(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn line(s: &str) -> Option<Line> {
    Some(Line(s.to_string()))
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub kind: Kind,
    /// what the project calls itself, for the status line
    pub name: String,
    /// the framework we recognised, if any: "next", "django", "vite"…
    pub framework: Option<String>,
    pub run: Option<Line>,
    pub build: Option<Line>,
    pub test: Option<Line>,
    pub fmt: Option<Line>,
}

/// marker files, in priority order.
const MARKERS: &[(&str, Kind)] = &[
    ("Cargo.toml", Kind::Cargo),
    ("package.json", Kind::Node),
    ("pyproject.toml", Kind::Python),
    ("setup.py", Kind::Python),
    ("requirements.txt", Kind::Python),
    ("Pipfile", Kind::Python),
    ("go.mod", Kind::Go),
    ("meson.build", Kind::Meson),
    ("CMakeLists.txt", Kind::CMake),
    ("Makefile", Kind::Make),
    ("build.zig", Kind::Zig),
    ("pom.xml", Kind::Maven),
    ("build.gradle", Kind::Gradle),
    ("build.gradle.kts", Kind::Gradle),
    ("docker-compose.yml", Kind::Docker),
    ("compose.yml", Kind::Docker),
];

/// walk up from `start` and describe the first project found.
pub fn detect(start: &Path) -> Option<Project> {
    let start = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let mut dir = start.canonicalize().unwrap_or(start);
    for _ in 0..16 {
        if let Some(p) = describe(&dir) {
            return Some(p);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn describe(dir: &Path) -> Option<Project> {
    if let Some(entry) = first_with_ext(dir, &["csproj", "sln", "fsproj"]) {
        let name = entry
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("dotnet")
            .to_string();
        return Some(Project {
            root: dir.to_path_buf(),
            kind: Kind::Dotnet,
            name,
            framework: Some("dotnet".into()),
            run: line("dotnet run"),
            build: line("dotnet build"),
            test: line("dotnet test"),
            fmt: line("dotnet format"),
        });
    }
    for (marker, kind) in MARKERS {
        if !dir.join(marker).exists() {
            continue;
        }
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("project")
            .to_string();
        return Some(match kind {
            Kind::Cargo => cargo(dir, name),
            Kind::Node => node(dir, name),
            Kind::Python => python(dir, name),
            Kind::Go => go(dir, name),
            Kind::Make => make(dir, name),
            Kind::CMake => cmake(dir, name),
            Kind::Meson => meson(dir, name),
            Kind::Zig => zig(dir, name),
            Kind::Maven => maven(dir, name),
            Kind::Gradle => gradle(dir, name),
            Kind::Docker => docker(dir, name),
            // handled above: dotnet is detected by file extension, not marker
            Kind::Dotnet => continue,
        });
    }
    None
}

fn first_with_ext(dir: &Path, exts: &[&str]) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if let Some(x) = p.extension().and_then(|x| x.to_str()) {
            if exts.contains(&x) {
                return Some(p);
            }
        }
    }
    None
}

fn cargo(dir: &Path, name: String) -> Project {
    Project {
        root: dir.to_path_buf(),
        kind: Kind::Cargo,
        name,
        framework: None,
        run: line("cargo run"),
        build: line("cargo build"),
        test: line("cargo test"),
        fmt: line("cargo fmt"),
    }
}

/// node: read package.json for the script and the framework.
fn node(dir: &Path, name: String) -> Project {
    let text = std::fs::read_to_string(dir.join("package.json")).unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let scripts = json
        .get("scripts")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let has_script = |n: &str| scripts.get(n).is_some();
    let deps = {
        let mut s = String::new();
        for key in ["dependencies", "devDependencies"] {
            if let Some(o) = json.get(key).and_then(|v| v.as_object()) {
                for k in o.keys() {
                    s.push_str(k);
                    s.push(' ');
                }
            }
        }
        s
    };
    let framework = [
        "next", "nuxt", "vite", "svelte", "astro", "react", "vue", "angular", "express", "nestjs",
        "remix", "solid", "electron", "tauri", "expo",
    ]
    .iter()
    .find(|f| deps.contains(**f))
    .map(|f| f.to_string());

    let pm = if dir.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if dir.join("yarn.lock").exists() {
        "yarn"
    } else if dir.join("bun.lockb").exists() {
        "bun"
    } else {
        "npm"
    };
    let run_prefix = match pm {
        "pnpm" => "pnpm run",
        "yarn" => "yarn",
        "bun" => "bun run",
        _ => "npm run",
    };
    let script = if has_script("dev") {
        Some("dev")
    } else if has_script("start") {
        Some("start")
    } else if has_script("serve") {
        Some("serve")
    } else {
        None
    };

    Project {
        root: dir.to_path_buf(),
        kind: Kind::Node,
        name,
        framework,
        run: script.map(|s| Line::new(format!("{run_prefix} {s}"))),
        build: has_script("build").then(|| Line::new(format!("{run_prefix} build"))),
        test: has_script("test").then(|| Line::new(format!("{run_prefix} test"))),
        fmt: Some(match pm {
            "pnpm" => Line::new("pnpm exec prettier --write ."),
            "yarn" => Line::new("yarn prettier --write ."),
            "bun" => Line::new("bunx prettier --write ."),
            _ => Line::new("npx --yes prettier --write ."),
        }),
    }
}

/// python: django, flask, or a plain entry point.
fn python(dir: &Path, name: String) -> Project {
    let (framework, run) = if dir.join("manage.py").exists() {
        (
            Some("django".to_string()),
            Line::new("python3 manage.py runserver"),
        )
    } else if let Some(entry) = ["main.py", "app.py", "run.py", "src/main.py"]
        .iter()
        .find(|e| dir.join(e).exists())
    {
        let guess = if *entry == "app.py" {
            "flask"
        } else {
            "script"
        };
        (
            Some(guess.to_string()),
            Line::new(format!("python3 {entry}")),
        )
    } else {
        (None, Line::new(format!("python3 -m {name}")))
    };
    let test = if dir.join("uv.lock").exists() {
        Line::new("uv run pytest")
    } else {
        Line::new("python3 -m pytest")
    };
    let fmt = if dir.join("ruff.toml").exists() {
        Line::new("ruff format .")
    } else {
        Line::new("python3 -m black .")
    };
    Project {
        root: dir.to_path_buf(),
        kind: Kind::Python,
        name,
        framework,
        run: Some(run),
        build: None,
        test: Some(test),
        fmt: Some(fmt),
    }
}

fn go(dir: &Path, name: String) -> Project {
    Project {
        root: dir.to_path_buf(),
        kind: Kind::Go,
        name,
        framework: None,
        run: line("go run ."),
        build: line("go build ./..."),
        test: line("go test ./..."),
        fmt: line("gofmt -w ."),
    }
}

fn make(dir: &Path, name: String) -> Project {
    let has_test = std::fs::read_to_string(dir.join("Makefile"))
        .map(|t| t.contains("\ntest:") || t.starts_with("test:"))
        .unwrap_or(false);
    Project {
        root: dir.to_path_buf(),
        kind: Kind::Make,
        name,
        framework: None,
        run: line("make"),
        build: line("make"),
        test: has_test.then(|| Line::new("make test")),
        fmt: None,
    }
}

fn cmake(dir: &Path, name: String) -> Project {
    Project {
        root: dir.to_path_buf(),
        kind: Kind::CMake,
        name,
        framework: None,
        run: None,
        build: line("cmake -S . -B build && cmake --build build"),
        test: line("ctest --test-dir build --output-on-failure"),
        fmt: None,
    }
}

fn meson(dir: &Path, name: String) -> Project {
    Project {
        root: dir.to_path_buf(),
        kind: Kind::Meson,
        name,
        framework: None,
        run: None,
        build: line("meson setup build && meson compile -C build"),
        test: line("meson test -C build"),
        fmt: None,
    }
}

fn zig(dir: &Path, name: String) -> Project {
    Project {
        root: dir.to_path_buf(),
        kind: Kind::Zig,
        name,
        framework: None,
        run: line("zig build run"),
        build: line("zig build"),
        test: line("zig build test"),
        fmt: line("zig fmt ."),
    }
}

fn maven(dir: &Path, name: String) -> Project {
    Project {
        root: dir.to_path_buf(),
        kind: Kind::Maven,
        name,
        framework: Some("maven".into()),
        run: line("mvn -q compile exec:java"),
        build: line("mvn -q package"),
        test: line("mvn -q test"),
        fmt: None,
    }
}

fn gradle(dir: &Path, name: String) -> Project {
    let g = if dir.join("gradlew").exists() {
        "./gradlew"
    } else {
        "gradle"
    };
    Project {
        root: dir.to_path_buf(),
        kind: Kind::Gradle,
        name,
        framework: Some("gradle".into()),
        run: line(&format!("{g} run")),
        build: line(&format!("{g} build")),
        test: line(&format!("{g} test")),
        fmt: None,
    }
}

fn docker(dir: &Path, name: String) -> Project {
    Project {
        root: dir.to_path_buf(),
        kind: Kind::Docker,
        name,
        framework: Some("compose".into()),
        run: line("docker compose up"),
        build: line("docker compose build"),
        test: None,
        fmt: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nana-proj-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn cargo_project_is_recognised() {
        let d = tmpdir("cargo");
        std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let p = detect(&d).unwrap();
        assert_eq!(p.kind, Kind::Cargo);
        assert_eq!(p.run.unwrap().as_str(), "cargo run");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn node_scripts_and_framework_are_read() {
        let d = tmpdir("node");
        std::fs::write(
            d.join("package.json"),
            r#"{"name":"x","scripts":{"dev":"next dev","build":"next build"},"dependencies":{"next":"14"}}"#,
        )
        .unwrap();
        let p = detect(&d).unwrap();
        assert_eq!(p.kind, Kind::Node);
        assert_eq!(p.framework.as_deref(), Some("next"));
        assert_eq!(p.run.unwrap().as_str(), "npm run dev");
        assert_eq!(p.build.unwrap().as_str(), "npm run build");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pnpm_lock_selects_pnpm() {
        let d = tmpdir("pnpm");
        std::fs::write(d.join("package.json"), r#"{"scripts":{"dev":"vite"}}"#).unwrap();
        std::fs::write(d.join("pnpm-lock.yaml"), "").unwrap();
        let p = detect(&d).unwrap();
        assert_eq!(p.run.unwrap().as_str(), "pnpm run dev");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn django_is_recognised() {
        let d = tmpdir("django");
        std::fs::write(d.join("manage.py"), "").unwrap();
        std::fs::write(d.join("pyproject.toml"), "").unwrap();
        let p = detect(&d).unwrap();
        assert_eq!(p.framework.as_deref(), Some("django"));
        assert_eq!(p.run.unwrap().as_str(), "python3 manage.py runserver");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn detection_walks_up_to_the_root() {
        let d = tmpdir("walk");
        std::fs::write(d.join("go.mod"), "module x\n").unwrap();
        let deep = d.join("cmd").join("app");
        std::fs::create_dir_all(&deep).unwrap();
        let p = detect(&deep).unwrap();
        assert_eq!(p.kind, Kind::Go);
        assert_eq!(p.root.canonicalize().unwrap(), d.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bare_directory_has_no_project() {
        let d = tmpdir("bare");
        assert!(detect(&d).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
