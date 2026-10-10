//! the language registry.
//!
//! one row per language: extensions, comment syntax, icon, and the commands
//! nana runs for you (check, build+run, format). everything else in nana —
//! gutter, headers, syntax colours, the run key, the status line — reads
//! from this table, so adding a language is adding a row.
//!
//! placeholders expanded in every argument: `{file}` `{stem}` `{name}`
//! `{dir}` `{tmp}`.

use std::path::Path;

/// a command template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cmd {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

/// one language.
#[derive(Debug, Clone, Copy)]
pub struct Lang {
    /// stable id, also the config key
    pub id: &'static str,
    /// display name, lowercase
    pub name: &'static str,
    /// file extensions, without the dot
    pub exts: &'static [&'static str],
    /// exact file names that mean this language (Makefile, Dockerfile…)
    pub names: &'static [&'static str],
    /// line comment token
    pub line_comment: &'static str,
    /// block comment delimiters
    pub block: Option<(&'static str, &'static str)>,
    /// nerd font glyph for the file tree and the status line
    pub icon: char,
    /// 256-colour index for the icon
    pub color: u8,
    /// family, for grouping: systems, script, web, markup, data, shell, gpu…
    pub tag: &'static str,
    /// syntax / lint pass whose stderr becomes gutter diagnostics
    pub check: Option<Cmd>,
    /// compile step, when running needs one (C, rust, java, assembly…)
    pub run_build: Option<Cmd>,
    /// what `f5` executes — `{tmp}/nana-bin` for compiled languages
    pub run: Option<Cmd>,
    /// formatter, applied to the file in place
    pub fmt: Option<Cmd>,
}

impl Lang {
    const fn new(
        id: &'static str,
        name: &'static str,
        exts: &'static [&'static str],
        line_comment: &'static str,
        icon: char,
        color: u8,
        tag: &'static str,
    ) -> Lang {
        Lang {
            id,
            name,
            exts,
            names: &[],
            line_comment,
            block: None,
            icon,
            color,
            tag,
            check: None,
            run_build: None,
            run: None,
            fmt: None,
        }
    }

    const fn names(mut self, names: &'static [&'static str]) -> Lang {
        self.names = names;
        self
    }

    const fn block(mut self, open: &'static str, close: &'static str) -> Lang {
        self.block = Some((open, close));
        self
    }

    const fn check(mut self, program: &'static str, args: &'static [&'static str]) -> Lang {
        self.check = Some(Cmd { program, args });
        self
    }

    const fn run_build(mut self, program: &'static str, args: &'static [&'static str]) -> Lang {
        self.run_build = Some(Cmd { program, args });
        self
    }

    const fn run(mut self, program: &'static str, args: &'static [&'static str]) -> Lang {
        self.run = Some(Cmd { program, args });
        self
    }

    const fn fmt(mut self, program: &'static str, args: &'static [&'static str]) -> Lang {
        self.fmt = Some(Cmd { program, args });
        self
    }

    /// true when the file needs a compile step before it can run
    pub fn compiled(&self) -> bool {
        self.run_build.is_some()
    }

    /// some languages indent with tabs *by convention*: flagging that would be
    /// flagging the language itself. the convention lives here, with the rest
    /// of what nana knows about a language.
    pub fn tabs_by_convention(&self) -> bool {
        matches!(self.id, "go" | "make" | "asm" | "cobol")
    }
}

// conveniences for the long python one-liners
const PY_AST: &[&str] = &[
    "-c",
    "import ast,sys;ast.parse(open(sys.argv[1]).read(),sys.argv[1])",
    "{file}",
];
const PY_JSON: &[&str] = &["-m", "json.tool", "{file}"];
const PY_YAML: &[&str] = &[
    "-c",
    "import sys,yaml;yaml.safe_load(open(sys.argv[1]))",
    "{file}",
];
const PY_TOML: &[&str] = &[
    "-c",
    "import sys,tomllib;tomllib.load(open(sys.argv[1],'rb'))",
    "{file}",
];

/// every language nana knows, in menu order.
pub const LANGS: &[Lang] = &[
    // ---------------------------------------------------------------- systems
    Lang::new("c", "c", &["c", "h"], "//", '\u{E649}', 4, "systems")
        .block("/*", "*/")
        .check(
            "gcc",
            &[
                "-fsyntax-only",
                "-Wall",
                "-Wextra",
                "-fdiagnostics-color=never",
                "{file}",
            ],
        )
        .run_build(
            "gcc",
            &[
                "-O2",
                "-Wall",
                "-Wextra",
                "{file}",
                "-o",
                "{tmp}/nana-bin",
                "-lm",
            ],
        )
        .run("{tmp}/nana-bin", &[])
        .fmt("clang-format", &["-i", "{file}"]),
    Lang::new(
        "cpp",
        "c++",
        &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "ipp"],
        "//",
        '\u{E61D}',
        4,
        "systems",
    )
    .block("/*", "*/")
    .check(
        "g++",
        &[
            "-fsyntax-only",
            "-std=c++20",
            "-Wall",
            "-Wextra",
            "-fdiagnostics-color=never",
            "{file}",
        ],
    )
    .run_build(
        "g++",
        &[
            "-O2",
            "-std=c++20",
            "-Wall",
            "{file}",
            "-o",
            "{tmp}/nana-bin",
        ],
    )
    .run("{tmp}/nana-bin", &[])
    .fmt("clang-format", &["-i", "{file}"]),
    Lang::new("rust", "rust", &["rs"], "//", '\u{E7A8}', 3, "systems")
        .block("/*", "*/")
        .check(
            "rustc",
            &[
                "--edition",
                "2021",
                "--emit",
                "metadata",
                "--crate-type",
                "lib",
                "-o",
                "{tmp}/nana-rs.rmeta",
                "{file}",
            ],
        )
        .run_build(
            "rustc",
            &["--edition", "2021", "-O", "{file}", "-o", "{tmp}/nana-bin"],
        )
        .run("{tmp}/nana-bin", &[])
        .fmt("rustfmt", &["--edition", "2021", "{file}"]),
    Lang::new("go", "go", &["go"], "//", '\u{E626}', 6, "systems")
        .block("/*", "*/")
        .check("gofmt", &["-e", "{file}"])
        .run("go", &["run", "{file}"])
        .fmt("gofmt", &["-w", "{file}"]),
    Lang::new(
        "zig",
        "zig",
        &["zig", "zon"],
        "//",
        '\u{E6A9}',
        3,
        "systems",
    )
    .check("zig", &["ast-check", "{file}"])
    .run("zig", &["run", "{file}"])
    .fmt("zig", &["fmt", "{file}"]),
    Lang::new(
        "nim",
        "nim",
        &["nim", "nims"],
        "#",
        '\u{E677}',
        3,
        "systems",
    )
    .check("nim", &["check", "--hints:off", "{file}"])
    .run("nim", &["r", "--hints:off", "{file}"])
    .fmt("nimpretty", &["{file}"]),
    Lang::new("d", "d", &["d"], "//", '\u{E7AF}', 1, "systems")
        .block("/*", "*/")
        .check("dmd", &["-o-", "-w", "{file}"])
        .run("dub", &["run", "--single", "{file}"]),
    Lang::new("swift", "swift", &["swift"], "//", '\u{E755}', 1, "systems")
        .block("/*", "*/")
        .check("swiftc", &["-typecheck", "{file}"])
        .run("swift", &["{file}"])
        .fmt("swift-format", &["format", "-i", "{file}"]),
    Lang::new(
        "objc",
        "objective-c",
        &["m", "mm"],
        "//",
        '\u{E61E}',
        4,
        "systems",
    )
    .block("/*", "*/")
    .check("clang", &["-fsyntax-only", "-x", "objective-c", "{file}"])
    .fmt("clang-format", &["-i", "{file}"]),
    Lang::new(
        "asm",
        "assembly",
        &["s", "S", "asm", "nasm"],
        ";",
        '\u{E637}',
        5,
        "systems",
    )
    .block("/*", "*/")
    .check("gcc", &["-fsyntax-only", "-x", "assembler", "{file}"])
    .run_build("gcc", &["-no-pie", "{file}", "-o", "{tmp}/nana-bin"])
    .run("{tmp}/nana-bin", &[]),
    Lang::new("cuda", "cuda", &["cu", "cuh"], "//", '\u{E64B}', 2, "gpu")
        .block("/*", "*/")
        .check("nvcc", &["-fsyntax-only", "{file}"])
        .fmt("clang-format", &["-i", "{file}"]),
    Lang::new(
        "glsl",
        "glsl",
        &["glsl", "vert", "frag", "comp", "geom"],
        "//",
        '\u{E64B}',
        5,
        "gpu",
    )
    .block("/*", "*/"),
    Lang::new("wgsl", "wgsl", &["wgsl"], "//", '\u{E64B}', 6, "gpu").block("/*", "*/"),
    Lang::new(
        "ada",
        "ada",
        &["adb", "ads"],
        "--",
        '\u{E6A9}',
        3,
        "systems",
    ),
    Lang::new(
        "fortran",
        "fortran",
        &["f90", "f95", "f03", "f08", "f"],
        "!",
        '\u{E6A9}',
        5,
        "systems",
    ),
    Lang::new(
        "pascal",
        "pascal",
        &["pas", "pp"],
        "//",
        '\u{E6A9}',
        5,
        "systems",
    )
    .block("{", "}"),
    Lang::new("v", "v", &["v"], "//", '\u{E6A9}', 6, "systems")
        .block("/*", "*/")
        .run("v", &["run", "{file}"]),
    Lang::new("crystal", "crystal", &["cr"], "#", '\u{E62F}', 5, "systems")
        .check("crystal", &["build", "--no-codegen", "{file}"])
        .run("crystal", &["run", "{file}"]),
    Lang::new(
        "haskell",
        "haskell",
        &["hs", "lhs"],
        "--",
        '\u{E777}',
        5,
        "functional",
    )
    .block("{-", "-}")
    .check("ghc", &["-fno-code", "{file}"])
    .run("runghc", &["{file}"])
    .fmt("ormolu", &["-i", "{file}"]),
    Lang::new(
        "ocaml",
        "ocaml",
        &["ml", "mli"],
        "//",
        '\u{E67A}',
        3,
        "functional",
    )
    .block("(*", "*)")
    .check("ocamlfind", &["ocamlc", "-stop-after", "parsing", "{file}"]),
    Lang::new(
        "elixir",
        "elixir",
        &["ex", "exs"],
        "#",
        '\u{E62D}',
        5,
        "functional",
    )
    .check("elixir", &["-c", "{file}"])
    .run("elixir", &["{file}"]),
    Lang::new(
        "erlang",
        "erlang",
        &["erl", "hrl"],
        "%",
        '\u{E7B1}',
        1,
        "functional",
    )
    .check("erlc", &["-o", "{tmp}", "{file}"]),
    Lang::new(
        "clojure",
        "clojure",
        &["clj", "cljs", "cljc", "edn"],
        ";",
        '\u{E768}',
        3,
        "functional",
    ),
    Lang::new(
        "lisp",
        "common lisp",
        &["lisp", "lsp", "cl"],
        ";",
        '\u{E6B0}',
        5,
        "functional",
    ),
    Lang::new(
        "scheme",
        "scheme",
        &["scm", "ss", "rkt"],
        ";",
        '\u{E6B0}',
        1,
        "functional",
    ),
    Lang::new("elm", "elm", &["elm"], "--", '\u{E62C}', 4, "functional")
        .block("{-", "-}")
        .check("elm", &["make", "--output=/dev/null", "{file}"]),
    Lang::new(
        "scala",
        "scala",
        &["scala", "sc"],
        "//",
        '\u{E737}',
        1,
        "systems",
    )
    .check("scalac", &["-Ystop-after:parser", "{file}"]),
    Lang::new(
        "kotlin",
        "kotlin",
        &["kt", "kts"],
        "//",
        '\u{E634}',
        5,
        "systems",
    )
    .run("kotlin", &["{file}"]),
    Lang::new("java", "java", &["java"], "//", '\u{E738}', 1, "systems")
        .block("/*", "*/")
        .check("javac", &["-d", "{tmp}", "-proc:none", "{file}"])
        .run_build("javac", &["-d", "{tmp}", "{file}"])
        .run("java", &["-cp", "{tmp}", "{stem}"]),
    Lang::new(
        "csharp",
        "c#",
        &["cs", "csx"],
        "//",
        '\u{E7B2}',
        5,
        "systems",
    )
    .block("/*", "*/")
    .run("dotnet", &["script", "{file}"]),
    Lang::new(
        "fsharp",
        "f#",
        &["fs", "fsx", "fsi"],
        "//",
        '\u{E7A7}',
        4,
        "functional",
    )
    .block("(*", "*)"),
    Lang::new("vb", "visual basic", &["vb"], "'", '\u{E6A9}', 5, "systems"),
    Lang::new(
        "solidity",
        "solidity",
        &["sol"],
        "//",
        '\u{E6A9}',
        3,
        "systems",
    )
    .block("/*", "*/")
    .check("solc", &["--ast-compact-json", "{file}"]),
    // ------------------------------------------------------------------ script
    Lang::new(
        "python",
        "python",
        &["py", "pyi", "pyw"],
        "#",
        '\u{E606}',
        3,
        "script",
    )
    .check("python3", PY_AST)
    .run("python3", &["{file}"])
    .fmt("ruff", &["format", "{file}"]),
    Lang::new(
        "ruby",
        "ruby",
        &["rb", "rake", "gemspec"],
        "#",
        '\u{E739}',
        1,
        "script",
    )
    .check("ruby", &["-c", "{file}"])
    .run("ruby", &["{file}"])
    .names(&["Rakefile", "Gemfile"]),
    Lang::new(
        "perl",
        "perl",
        &["pl", "pm", "t"],
        "#",
        '\u{E769}',
        5,
        "script",
    )
    .check("perl", &["-c", "{file}"])
    .run("perl", &["{file}"]),
    Lang::new("php", "php", &["php", "phtml"], "//", '\u{E73D}', 5, "web")
        .block("/*", "*/")
        .check("php", &["-l", "{file}"])
        .run("php", &["{file}"])
        .names(&["composer.json"]),
    Lang::new("lua", "lua", &["lua"], "--", '\u{E620}', 4, "script")
        .block("--[[", "]]")
        .check("luac", &["-p", "{file}"])
        .run("lua", &["{file}"])
        .fmt("stylua", &["{file}"]),
    Lang::new(
        "raku",
        "raku",
        &["raku", "p6", "pm6"],
        "#",
        '\u{E6A9}',
        5,
        "script",
    )
    .run("raku", &["{file}"]),
    Lang::new("tcl", "tcl", &["tcl"], "#", '\u{E6A9}', 5, "script"),
    Lang::new("r", "r", &["r", "R", "rmd"], "#", '\u{E881}', 4, "data").run("Rscript", &["{file}"]),
    Lang::new("julia", "julia", &["jl"], "#", '\u{E624}', 5, "data").run("julia", &["{file}"]),
    Lang::new("matlab", "matlab", &["m"], "%", '\u{E6A9}', 5, "data").block("%{", "%}"),
    Lang::new("octave", "octave", &["oct"], "%", '\u{E6A9}', 5, "data"),
    Lang::new("dart", "dart", &["dart"], "//", '\u{E798}', 4, "web")
        .block("/*", "*/")
        .check("dart", &["analyze", "{file}"])
        .run("dart", &["run", "{file}"])
        .fmt("dart", &["format", "{file}"]),
    Lang::new(
        "groovy",
        "groovy",
        &["groovy", "gradle"],
        "//",
        '\u{E775}',
        5,
        "script",
    )
    .names(&["build.gradle", "settings.gradle"]),
    Lang::new("hack", "hack", &["hack", "hh"], "//", '\u{E73D}', 5, "web").block("/*", "*/"),
    Lang::new(
        "smalltalk",
        "smalltalk",
        &["st"],
        "\"",
        '\u{E6A9}',
        5,
        "script",
    ),
    Lang::new(
        "forth",
        "forth",
        &["fth", "4th", "forth"],
        "\\",
        '\u{E6A9}',
        5,
        "systems",
    ),
    Lang::new(
        "cobol",
        "cobol",
        &["cob", "cbl"],
        "*>",
        '\u{E6A9}',
        5,
        "systems",
    ),
    Lang::new(
        "prolog",
        "prolog",
        &["pro", "prolog"],
        "%",
        '\u{E6A9}',
        5,
        "functional",
    ),
    Lang::new(
        "wasm",
        "webassembly",
        &["wat", "wast"],
        ";;",
        '\u{E6A1}',
        5,
        "systems",
    )
    .block("(;", ";)"),
    Lang::new("llvm", "llvm ir", &["ll"], ";", '\u{E6A9}', 5, "systems"),
    // --------------------------------------------------------------------- web
    Lang::new(
        "javascript",
        "javascript",
        &["js", "mjs", "cjs", "jsx"],
        "//",
        '\u{E74E}',
        3,
        "web",
    )
    .block("/*", "*/")
    .check("node", &["--check", "{file}"])
    .run("node", &["{file}"])
    .fmt("prettier", &["--write", "{file}"]),
    Lang::new(
        "typescript",
        "typescript",
        &["ts", "tsx", "mts", "cts"],
        "//",
        '\u{E628}',
        4,
        "web",
    )
    .block("/*", "*/")
    .check("tsc", &["--noEmit", "--pretty", "false", "{file}"])
    .fmt("prettier", &["--write", "{file}"]),
    Lang::new("vue", "vue", &["vue"], "//", '\u{E62C}', 3, "web").block("<!--", "-->"),
    Lang::new("svelte", "svelte", &["svelte"], "//", '\u{E697}', 1, "web").block("<!--", "-->"),
    Lang::new("astro", "astro", &["astro"], "//", '\u{E6B3}', 5, "web").block("<!--", "-->"),
    Lang::new(
        "html",
        "html",
        &[
            "html", "htm", "xhtml", "ejs", "hbs", "twig", "jinja", "j2", "erb", "pug",
        ],
        "<!--",
        '\u{E736}',
        1,
        "markup",
    )
    .block("<!--", "-->"),
    Lang::new("css", "css", &["css"], "/*", '\u{E749}', 4, "web")
        .block("/*", "*/")
        .fmt("prettier", &["--write", "{file}"]),
    Lang::new(
        "scss",
        "scss",
        &["scss", "sass", "less", "styl"],
        "//",
        '\u{E74B}',
        5,
        "web",
    )
    .block("/*", "*/")
    .fmt("prettier", &["--write", "{file}"]),
    Lang::new(
        "graphql",
        "graphql",
        &["graphql", "gql"],
        "#",
        '\u{E662}',
        5,
        "query",
    ),
    Lang::new(
        "coffee",
        "coffeescript",
        &["coffee"],
        "#",
        '\u{E751}',
        5,
        "web",
    ),
    // ------------------------------------------------------------------ markup
    Lang::new(
        "markdown",
        "markdown",
        &["md", "markdown", "mdx"],
        "<!--",
        '\u{E73E}',
        6,
        "markup",
    )
    .block("<!--", "-->")
    .fmt("prettier", &["--write", "{file}"]),
    Lang::new(
        "rst",
        "restructuredtext",
        &["rst"],
        "..",
        '\u{E73E}',
        6,
        "markup",
    ),
    Lang::new(
        "asciidoc",
        "asciidoc",
        &["adoc", "asciidoc"],
        "//",
        '\u{E73E}',
        6,
        "markup",
    ),
    Lang::new(
        "latex",
        "latex",
        &["tex", "sty", "cls", "bib"],
        "%",
        '\u{E600}',
        2,
        "markup",
    ),
    Lang::new("typst", "typst", &["typ"], "//", '\u{E600}', 5, "markup").block("/*", "*/"),
    Lang::new("org", "org", &["org"], "#", '\u{E633}', 5, "markup"),
    Lang::new("svg", "svg", &["svg"], "<!--", '\u{E698}', 5, "markup").block("<!--", "-->"),
    Lang::new(
        "xml",
        "xml",
        &["xml", "xsd", "xsl", "plist", "csproj", "props", "targets"],
        "<!--",
        '\u{E619}',
        5,
        "data",
    )
    .block("<!--", "-->"),
    // -------------------------------------------------------------------- data
    Lang::new(
        "json",
        "json",
        &["json", "jsonc", "json5", "geojson", "ndjson"],
        "//",
        '\u{E60B}',
        3,
        "data",
    )
    .check("python3", PY_JSON)
    .fmt("prettier", &["--write", "{file}"]),
    Lang::new("yaml", "yaml", &["yaml", "yml"], "#", '\u{E60B}', 5, "data")
        .check("python3", PY_YAML)
        .fmt("prettier", &["--write", "{file}"]),
    Lang::new("toml", "toml", &["toml"], "#", '\u{E6B2}', 3, "data")
        .check("python3", PY_TOML)
        .names(&["Cargo.toml", "pyproject.toml", "Cargo.lock"]),
    Lang::new(
        "ini",
        "ini",
        &["ini", "cfg", "conf", "desktop", "editorconfig"],
        "#",
        '\u{E615}',
        5,
        "data",
    ),
    Lang::new("csv", "csv", &["csv", "tsv"], "#", '\u{E64A}', 5, "data"),
    Lang::new(
        "sql",
        "sql",
        &["sql", "psql", "mysql"],
        "--",
        '\u{E706}',
        5,
        "query",
    )
    .block("/*", "*/"),
    Lang::new(
        "protobuf",
        "protobuf",
        &["proto"],
        "//",
        '\u{E6B2}',
        5,
        "data",
    ),
    Lang::new("thrift", "thrift", &["thrift"], "//", '\u{E6B2}', 5, "data"),
    Lang::new(
        "diff",
        "diff",
        &["diff", "patch"],
        "#",
        '\u{E728}',
        5,
        "data",
    ),
    Lang::new("log", "log", &["log"], "#", '\u{E615}', 8, "data"),
    Lang::new(
        "regex",
        "regex",
        &["regex", "re"],
        "#",
        '\u{E6A9}',
        5,
        "data",
    ),
    // ------------------------------------------------------------------- shell
    Lang::new(
        "bash",
        "bash",
        &["sh", "bash", "zsh", "ksh"],
        "#",
        '\u{E795}',
        2,
        "shell",
    )
    .check("bash", &["-n", "{file}"])
    .run("bash", &["{file}"])
    .fmt("shfmt", &["-w", "{file}"])
    .names(&["bashrc", ".bashrc", ".zshrc", ".profile", ".bash_profile"]),
    Lang::new("fish", "fish", &["fish"], "#", '\u{E795}', 6, "shell")
        .check("fish", &["--no-execute", "{file}"])
        .run("fish", &["{file}"]),
    Lang::new(
        "powershell",
        "powershell",
        &["ps1", "psm1", "psd1"],
        "#",
        '\u{E795}',
        4,
        "shell",
    )
    .block("<#", "#>")
    .check(
        "pwsh",
        &["-NoProfile", "-Command", "Get-Command -Syntax {file}"],
    ),
    Lang::new(
        "batch",
        "batch",
        &["bat", "cmd"],
        "REM",
        '\u{E795}',
        5,
        "shell",
    ),
    Lang::new("awk", "awk", &["awk"], "#", '\u{E795}', 5, "shell"),
    Lang::new("sed", "sed", &["sed"], "#", '\u{E795}', 5, "shell"),
    Lang::new("nushell", "nushell", &["nu"], "#", '\u{E795}', 6, "shell"),
    Lang::new("elvish", "elvish", &["elv"], "#", '\u{E795}', 6, "shell"),
    // ------------------------------------------------------------------ config
    Lang::new("make", "make", &["mk", "mak"], "#", '\u{E673}', 5, "config")
        .names(&["Makefile", "makefile", "GNUmakefile"])
        .run("make", &[])
        .fmt("clang-format", &["-i", "{file}"]),
    Lang::new("cmake", "cmake", &["cmake"], "#", '\u{E794}', 5, "config")
        .names(&["CMakeLists.txt"])
        .check("cmake", &["-P", "{file}"]),
    Lang::new("meson", "meson", &["meson"], "#", '\u{E794}', 5, "config").names(&["meson.build"]),
    Lang::new("just", "just", &["just"], "#", '\u{E673}', 5, "config")
        .names(&["justfile", "Justfile"]),
    Lang::new(
        "docker",
        "dockerfile",
        &["dockerfile", "containerfile"],
        "#",
        '\u{E7B0}',
        4,
        "config",
    )
    .names(&["Dockerfile", "Containerfile"]),
    Lang::new("nix", "nix", &["nix"], "#", '\u{E843}', 6, "config")
        .block("/*", "*/")
        .check("nix-instantiate", &["--parse", "{file}"])
        .fmt("nixfmt", &["{file}"]),
    Lang::new(
        "terraform",
        "terraform",
        &["tf", "tfvars"],
        "#",
        '\u{E69A}',
        5,
        "config",
    )
    .block("/*", "*/"),
    Lang::new("hcl", "hcl", &["hcl"], "#", '\u{E69A}', 5, "config"),
    Lang::new(
        "ansible",
        "ansible",
        &["ansible"],
        "#",
        '\u{E69A}',
        5,
        "config",
    ),
    Lang::new("nginx", "nginx", &["nginx"], "#", '\u{E6A9}', 5, "config").names(&["nginx.conf"]),
    Lang::new(
        "gitconfig",
        "git config",
        &["gitconfig", "gitignore", "gitattributes"],
        "#",
        '\u{E702}',
        5,
        "config",
    )
    .names(&[".gitignore", ".gitattributes", ".gitconfig"]),
    Lang::new("env", "dotenv", &["env"], "#", '\u{E615}', 5, "config")
        .names(&[".env", ".env.local"]),
    Lang::new(
        "vim",
        "vim script",
        &["vim", "vimrc"],
        "\"",
        '\u{E62B}',
        2,
        "config",
    )
    .names(&[".vimrc", "vimrc"]),
    Lang::new(
        "emacs",
        "emacs lisp",
        &["el", "elc"],
        ";",
        '\u{E632}',
        5,
        "config",
    )
    .names(&[".emacs", "init.el"]),
];

/// the fallback language: plain text, no commands, no colours.
pub const PLAIN: Lang = Lang {
    id: "text",
    name: "text",
    exts: &["txt"],
    names: &[],
    line_comment: "#",
    block: None,
    icon: '\u{F0219}',
    color: 8,
    tag: "text",
    check: None,
    run_build: None,
    run: None,
    fmt: None,
};

/// every language, plus the fallback.
pub fn all() -> impl Iterator<Item = &'static Lang> {
    LANGS.iter().chain(std::iter::once(&PLAIN))
}

/// lookup by id.
pub fn by_id(id: &str) -> Option<&'static Lang> {
    LANGS.iter().find(|l| l.id == id)
}

/// lookup by extension (no dot, case-insensitive).
pub fn by_ext(ext: &str) -> Option<&'static Lang> {
    let ext = ext.trim_start_matches('.');
    LANGS
        .iter()
        .find(|l| l.exts.iter().any(|e| e.eq_ignore_ascii_case(ext)))
}

/// the language of a path: exact file name first, then extension.
pub fn for_path(path: &Path) -> &'static Lang {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if let Some(l) = LANGS
        .iter()
        .find(|l| l.names.iter().any(|n| n.eq_ignore_ascii_case(name)))
    {
        return l;
    }
    // dotfiles such as .zshrc: look at the whole name as an extension too
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if let Some(l) = by_ext(ext) {
            return l;
        }
    }
    if let Some(l) = by_ext(name) {
        return l;
    }
    &PLAIN
}

/// the language of an extension string as stored in the editor.
pub fn for_ext(ext: &str) -> &'static Lang {
    by_ext(ext).unwrap_or(&PLAIN)
}

/// the absolute form of a path, without touching the filesystem — a checker
/// run from another directory must still find the file.
pub fn absolute(path: &Path) -> std::path::PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// expand `{file} {stem} {name} {dir} {tmp}` in one argument.
pub fn expand_arg(arg: &str, path: &Path, tmp: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("main");
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("main");
    let dir = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(p) => absolute(p),
        None => std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf()),
    };
    arg.replace("{file}", &absolute(path).display().to_string())
        .replace("{stem}", stem)
        .replace("{name}", name)
        .replace("{tmp}", &tmp.display().to_string())
        .replace("{dir}", &dir.display().to_string())
}

/// expand a whole command template.
pub fn expand(cmd: &Cmd, path: &Path, tmp: &Path) -> (String, Vec<String>) {
    let args = cmd
        .args
        .iter()
        .map(|a| expand_arg(a, path, tmp))
        .collect::<Vec<_>>();
    let program = expand_arg(cmd.program, path, tmp);
    (program, args)
}

/// the scratch directory nana compiles into: $TMPDIR/nana-<pid>.
pub fn tmp_dir() -> std::path::PathBuf {
    let base = std::env::temp_dir().join(format!("nana-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&base);
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_extension_resolves_deterministically() {
        // a few extensions are genuinely shared (.m is objective-c and matlab,
        // .pl is perl and prolog): the first language listed wins, and that
        // order is the contract.
        let mut first: std::collections::HashMap<String, &str> = std::collections::HashMap::new();
        for l in LANGS {
            for e in l.exts {
                first.entry(e.to_ascii_lowercase()).or_insert(l.id);
            }
        }
        for (ext, id) in first {
            let got = by_ext(&ext).unwrap_or_else(|| panic!("extension {ext} must resolve"));
            assert_eq!(
                got.id, id,
                "extension {ext} must map to the first language listed"
            );
        }
    }

    #[test]
    fn c_and_cpp_do_not_collide() {
        assert_eq!(by_ext("c").unwrap().id, "c");
        assert_eq!(by_ext("cpp").unwrap().id, "cpp");
        assert_eq!(by_ext("hpp").unwrap().id, "cpp");
    }

    #[test]
    fn names_win_over_extensions() {
        let mk = for_path(Path::new("Makefile"));
        assert_eq!(mk.id, "make");
        let cargo = for_path(Path::new("Cargo.toml"));
        assert_eq!(cargo.id, "toml");
        let docker = for_path(Path::new("Dockerfile"));
        assert_eq!(docker.id, "docker");
    }

    #[test]
    fn dotfiles_resolve() {
        assert_eq!(for_path(Path::new(".zshrc")).id, "bash");
        assert_eq!(for_path(Path::new(".gitignore")).id, "gitconfig");
    }

    #[test]
    fn unknown_files_fall_back_to_plain_text() {
        let l = for_path(Path::new("weird.xyzzy"));
        assert_eq!(l.id, "text");
        assert!(l.run.is_none());
    }

    #[test]
    fn placeholders_expand() {
        let tmp = Path::new("/tmp/x");
        let (prog, args) = expand(
            &Cmd {
                program: "gcc",
                args: &["{file}", "-o", "{tmp}/bin"],
            },
            Path::new("/home/me/main.c"),
            tmp,
        );
        assert_eq!(prog, "gcc");
        let abs = std::path::absolute(Path::new("/home/me/main.c")).unwrap();
        let slash = |p: &Path| p.display().to_string().replace('\\', "/");
        assert_eq!(
            args,
            vec![slash(&abs), "-o".to_string(), slash(&tmp.join("bin"))]
        );
        // a relative path is handed over absolute, so any cwd works
        let (_, rel) = expand(
            &Cmd {
                program: "gcc",
                args: &["{file}"],
            },
            Path::new("src/main.c"),
            tmp,
        );
        assert!(Path::new(&rel[0]).is_absolute(), "{}", rel[0]);
        assert!(
            Path::new(&rel[0]).ends_with(Path::new("src").join("main.c")),
            "{}",
            rel[0]
        );
        assert_eq!(
            expand_arg("{stem}", Path::new("/a/b/hello.py"), tmp),
            "hello"
        );
        let dir = std::path::absolute(Path::new("/a/b")).unwrap();
        assert_eq!(
            expand_arg("{dir}", Path::new("/a/b/hello.py"), tmp).replace('\\', "/"),
            dir.display().to_string().replace('\\', "/")
        );
    }

    #[test]
    fn every_language_has_a_comment_token_and_an_id() {
        for l in all() {
            assert!(!l.id.is_empty(), "id missing");
            assert!(!l.name.is_empty(), "{} has no display name", l.id);
            assert!(!l.line_comment.is_empty(), "{} has no comment token", l.id);
            assert!(!l.tag.is_empty(), "{} has no tag", l.id);
        }
    }

    #[test]
    fn tab_indented_languages_say_so() {
        assert!(by_id("go").unwrap().tabs_by_convention());
        assert!(by_id("make").unwrap().tabs_by_convention());
        assert!(!by_id("python").unwrap().tabs_by_convention());
        assert!(!by_id("c").unwrap().tabs_by_convention());
    }

    #[test]
    fn ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for l in LANGS {
            assert!(seen.insert(l.id), "duplicate id {}", l.id);
        }
    }

    #[test]
    fn the_families_the_user_asked_for_are_present() {
        for id in [
            "c",
            "cpp",
            "rust",
            "java",
            "javascript",
            "typescript",
            "python",
            "lua",
            "asm",
            "html",
            "css",
            "markdown",
            "bash",
            "sql",
            "json",
            "yaml",
            "toml",
            "go",
            "php",
            "ruby",
            "swift",
            "kotlin",
            "dart",
            "haskell",
            "elixir",
            "docker",
            "make",
            "cmake",
        ] {
            assert!(by_id(id).is_some(), "language {id} must exist");
        }
    }
}
