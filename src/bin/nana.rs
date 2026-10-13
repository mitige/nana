//! the `nana` binary: the editor, and the agentic surfaces around it.
//!
//! everything the tui can do is also reachable from the shell, which is how a
//! user (and a test) can exercise it end to end:
//!
//!   nana                          the editor
//!   nana --agent "…"              one agent request, steps printed as they run
//!   nana --agent --resume "…"     continue the project's last conversation
//!   nana --show-prompt "…"        print exactly what the model will receive
//!   nana --providers              who can answer, and how they are picked
//!   nana --memory …               list, search, read, write, forget, consolidate
//!   nana --persona …              list, show, write, delete
//!   nana --skills                 what this project can hand the agent
//!   nana --languages              the language registry

use nana::{agent, memory, persona, provider, settings, skills};
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Entry point of the binary: reads the arguments and dispatches to a subcommand, or opens the editor when there is none, and exits with the code it returns.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let code = match args.first().map(String::as_str) {
        Some("-h") | Some("--help") => {
            help();
            0
        }
        Some("-V") | Some("--version") => {
            println!("nana {VERSION}");
            0
        }
        Some("--languages") => {
            print_languages();
            0
        }
        Some("--providers") => {
            providers();
            0
        }
        Some("--skills") => {
            print_skills(&root);
            0
        }
        Some("--company") => company_cmd(&root, &args[1..]),
        Some("--knowledge") => knowledge_cmd(&root, &args[1..]),
        Some("--dream") => dream_cmd(&root, &args[1..]),
        Some("--memory") => memory_cmd(&root, &args[1..]),
        Some("--magi") => magi_cmd(&root, &args[1..]),
        Some("--persona") => persona_cmd(&root, &args[1..]),
        Some("--show-prompt") => show_prompt(&root, &args[1..]),
        Some("--agent") => run_agent(&root, &args[1..]),
        Some(flag) if flag.starts_with("--") => {
            eprintln!("nana: unknown flag {flag} — try nana --help");
            2
        }
        _ => {
            let target = args.first().map(PathBuf::from);
            match nana::editor::run(target) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("nana: {e}");
                    1
                }
            }
        }
    };
    std::process::exit(code);
}

/// Prints the command-line help, the one place the list of subcommands is written for the user.
fn help() {
    print!(
        "nana {VERSION} — an adaptive terminal editor, now with an agent

usage: nana [file|dir]          open the editor
       nana --agent \"…\"        ask the agent to do something here
       nana --providers         list the model providers
       nana --memory …          the project's memory
       nana --magi [--apply]      three sages judge the memory
       nana --company …         the company: hire, task, fire, mail
       nana --knowledge …       the project's wiki: list, show, search, write
       nana --dream [n]         consolidate the last sessions into the wiki
       nana --persona …         saved system prompts
       nana --skills            skills this project offers
       nana --show-prompt \"…\"   print the assembled system prompt
       nana --languages         every language nana knows

the editor adapts to the file you open: syntax colours, the checker, the run
recipe and the auto header all come from a language registry (src/langs.rs).
in a project, nana runs the project's own commands (cargo, npm, django…).

editing
  ctrl+s        save            ctrl+q  quit
  ctrl+z        undo            ctrl+k  cut line
  ctrl+u        paste           ctrl+f  search
  ctrl+r        replace         ctrl+g  go to line
  tab           accept suggestion, else indent

panels
  ctrl+t        file explorer   ctrl+o  fuzzy file search
  f2            cycle panels    f3      terminal
  ctrl+n/p      next/previous diagnostic

the language
  ctrl+b        check (compiler / linter / syntax)
  f5            run (project command, or the file itself)
  f6            format (prettier, rustfmt, gofmt, black…)
  f4            auto header in the language's comment syntax
  ctrl+e        ghost completion

the agent
  settings: .nana/settings.json (project) over ~/.config/nana/settings.json
  memory:   .nana/memory/  personas: .nana/personas/  skills: .nana/skills/
"
    );
}

/// Prints the model in use and the provider it resolves to, then one row per provider with its key state, so the user sees what will answer.
fn providers() {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let s = settings::Settings::load(&root);
    let model = s
        .model
        .clone()
        .unwrap_or_else(|| format!("(none set — the default is {})", provider::DEFAULT_MODEL));
    let picked = provider::detect(&model);
    println!("model:  {model}");
    println!("picked: {} [{}]", picked.label, picked.kind_name());
    println!();
    println!("{:<20} {:<11} {:<48} key", "provider", "kind", "endpoint");
    for p in provider::PROVIDERS {
        let key = if p.keyless {
            "not needed".to_string()
        } else {
            match p.key_env {
                Some(env) => {
                    let set = std::env::var(env).is_ok()
                        || provider::dsh_has(env)
                        || (p.id == "dashscope" && provider::dsh_has("OPP_API_KEY"));
                    format!("{env}{}", if set { " (set)" } else { " (missing)" })
                }
                None => "-".to_string(),
            }
        };
        println!(
            "{:<20} {:<11} {:<48} {}",
            p.id,
            p.kind_name(),
            p.base_url,
            key
        );
    }
    println!(
        "\nmodels are routed by name: claude→anthropic, gemini→gemini, qwen/kimi/deepseek→dashscope,\n\
         gpt→openai, llama→ollama, anything else→openai-compatible. override in settings.json:\n\
         {{\"provider\":\"openai\",\"model\":\"gpt-4o\",\"base_url\":\"https://api.openai.com/v1\"}}"
    );
}

/// Prints the skills this project offers, or says where to add one when there are none.
fn print_skills(root: &Path) {
    let s = settings::Settings::load(root);
    let found = skills::discover(root, &s.skill_dirs);
    if found.is_empty() {
        println!(
            "no skills in this project — add markdown files to .nana/skills/ (or skills/<name>/SKILL.md)"
        );
        return;
    }
    for s in found {
        println!(
            "{:<16} {:<40} trigger: {}",
            s.name,
            s.description,
            if s.trigger.is_empty() {
                "-"
            } else {
                &s.trigger
            }
        );
    }
}

/// the project's wiki: read it, write it, search it.
fn knowledge_cmd(root: &Path, rest: &[String]) -> i32 {
    use nana::knowledge;
    match rest.first().map(String::as_str) {
        None | Some("list") => {
            print!("{}", knowledge::describe(root));
            0
        }
        Some("show") => {
            let Some(name) = rest.get(1) else {
                eprintln!("usage: nana --knowledge show <page>");
                return 2;
            };
            match knowledge::read(root, name) {
                Ok(text) => {
                    print!("{text}");
                    0
                }
                Err(e) => {
                    eprintln!("nana: {e}");
                    1
                }
            }
        }
        Some("search") => {
            let Some(q) = rest.get(1) else {
                eprintln!("usage: nana --knowledge search <text>");
                return 2;
            };
            let hits = knowledge::search(root, q);
            for h in &hits {
                println!("{}:{}: {}", h.page, h.line, h.text);
            }
            if hits.is_empty() {
                println!("no match in the wiki");
            }
            0
        }
        Some("write") => {
            let Some(name) = rest.get(1) else {
                eprintln!("usage: nana --knowledge write <page> <<'EOF' … EOF");
                return 2;
            };
            let mut body = String::new();
            use std::io::Read;
            if std::io::stdin().read_to_string(&mut body).is_err() || body.trim().is_empty() {
                eprintln!("nana: nothing on stdin");
                return 2;
            }
            match knowledge::write(root, name, &body) {
                Ok(p) => {
                    println!("wrote {}", p.display());
                    0
                }
                Err(e) => {
                    eprintln!("nana: {e}");
                    1
                }
            }
        }
        Some(other) => {
            eprintln!("nana: unknown knowledge command {other}");
            2
        }
    }
}

/// /dream: consolidate the latest sessions into the wiki.
fn dream_cmd(root: &Path, rest: &[String]) -> i32 {
    let how_many: usize = rest.first().and_then(|s| s.parse().ok()).unwrap_or(10);
    let settings = settings::Settings::load(root);
    let mut runner = match agent::Agent::new(root, settings) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nana: {e}");
            return 1;
        }
    };
    eprintln!("dreaming over the {how_many} latest session(s)…");
    let date = today();
    match nana::knowledge::dream(&mut runner, how_many, &date) {
        Ok(report) => {
            println!(
                "read {} session(s), wrote {} page(s): {}",
                report.read_sessions,
                report.pages.len(),
                report.pages.join(", ")
            );
            if let Some(a) = report.audit {
                println!("audit: {}", a.display());
            }
            0
        }
        Err(e) => {
            eprintln!("nana: {e}");
            1
        }
    }
}

/// today, for the audit page name.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (mut y, mut d) = (1970i64, days as i64);
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if d < len {
            break;
        }
        d -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0;
    while m < 12 && d >= months[m] {
        d -= months[m];
        m += 1;
    }
    format!("{y}-{:02}-{:02}", m + 1, d + 1)
}

/// the company: a ceo that hires, employees that work.
/// Runs `nana --company <sub>`: the staff of the project, their mailbox and the
/// hiring, dispatch and firing of employees. Hands back the process exit code.
fn company_cmd(root: &Path, rest: &[String]) -> i32 {
    match rest.first().map(String::as_str) {
        None | Some("list") => company_list(root),
        Some("hire") => company_hire(root, rest),
        Some("task") => company_task(root, rest),
        Some("fire") => company_fire(root, rest),
        Some("mail") => company_mail(root),
        Some(other) => {
            eprintln!("nana: unknown company command {other}");
            2
        }
    }
}

/// Prints the staff and the last eight mailbox entries. Hands back 0.
fn company_list(root: &Path) -> i32 {
    use nana::company;
    print!("{}", company::describe(root));
    let mail = company::mailbox(root);
    if !mail.is_empty() {
        println!("\nmailbox:");
        for t in mail.iter().rev().take(8) {
            println!("  {:<8} {:<10} {}", t.employee, t.state, short(&t.task, 60));
        }
    }
    0
}

/// Plans and records new hires for a mission given after `hire`. Hands back
/// the exit code: 2 for a missing mission, 1 when planning or saving fails.
fn company_hire(root: &Path, rest: &[String]) -> i32 {
    use nana::company;
    let mission = rest[1..].join(" ");
    if mission.trim().is_empty() {
        eprintln!("usage: nana --company hire \"the mission\"");
        return 2;
    }
    let settings = settings::Settings::load(root);
    let mut runner = match agent::Agent::new(root, settings) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nana: {e}");
            return 1;
        }
    };
    let project = settings::Settings::load(root);
    let known = skills::discover(root, &project.skill_dirs)
        .into_iter()
        .map(|s| s.name)
        .collect::<Vec<_>>();
    eprintln!("the ceo is thinking about: {mission}");
    match company::plan_hiring(&mut runner, &mission, &known) {
        Ok(people) => match company::hire(root, &people) {
            Ok(n) => {
                println!("hired {n}:");
                for p in &people {
                    println!("  {:<12} {:<22} {}", p.name, p.role, p.mission);
                }
                0
            }
            Err(e) => {
                eprintln!("nana: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("nana: {e}");
            1
        }
    }
}

/// Gives a task to a named employee and streams the run: tool calls and results
/// go to stderr, the answer to stdout. Hands back 2 for bad usage, 1 on failure.
fn company_task(root: &Path, rest: &[String]) -> i32 {
    use nana::company;
    let (Some(name), true) = (rest.get(1), rest.len() > 2) else {
        eprintln!("usage: nana --company task <name> \"the task\"");
        return 2;
    };
    let task = rest[2..].join(" ");
    let Some(employee) = company::employee(root, name) else {
        eprintln!("nana: nobody named « {name} » works here");
        return 1;
    };
    let settings = settings::Settings::load(root);
    eprintln!("{} is on it", employee.name);
    let result = company::dispatch(root, settings, &employee, &task, print_company_event);
    match result {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

/// Shows one event of an employee's run: calls and results on stderr, the
/// text on stdout, errors on stderr. Other events are not shown.
fn print_company_event(e: agent::Event) {
    match e {
        agent::Event::ToolCall { name, args } => {
            eprintln!("→ {name} {}", short(&args.to_string(), 100))
        }
        agent::Event::ToolResult { name, ok, text } => eprintln!(
            "{} {name} {}",
            if ok { "ok" } else { "no" },
            short(text.trim(), 120)
        ),
        agent::Event::Text(t) => println!("{t}"),
        agent::Event::Error(e) => eprintln!("nana: {e}"),
        _ => {}
    }
}

/// Lets an employee go by name. Hands back 2 without a name, 1 when it fails.
fn company_fire(root: &Path, rest: &[String]) -> i32 {
    use nana::company;
    let Some(name) = rest.get(1) else {
        eprintln!("usage: nana --company fire <name>");
        return 2;
    };
    match company::fire(root, name) {
        Ok(()) => {
            println!("{name} was let go");
            0
        }
        Err(e) => {
            eprintln!("nana: {e}");
            1
        }
    }
}

/// Prints every mailbox entry with its result line when it has one. Hands back 0.
fn company_mail(root: &Path) -> i32 {
    use nana::company;
    for t in company::mailbox(root) {
        println!("{:<8} {:<8} {}", t.employee, t.state, short(&t.task, 70));
        if !t.result.is_empty() {
            println!("         {}", short(&t.result, 90));
        }
    }
    0
}

/// Runs `nana --magi [--apply]`: the three sages read the project's memory and
/// say what each of them would keep or forget. With `--apply` the pages the
/// majority forgets are retired, their history kept; without it nothing is
/// written at all. Hands back the process exit code.
fn magi_cmd(root: &Path, rest: &[String]) -> i32 {
    let mem = memory::Memory::open(root);
    let apply = rest.iter().any(|a| a == "--apply");
    let decisions = if apply {
        memory::magi::run(&mem)
    } else {
        memory::magi::report(&mem)
    };
    let decisions = match decisions {
        Ok(d) => d,
        Err(e) => {
            eprintln!("nana: magi: {e}");
            return 1;
        }
    };
    if decisions.is_empty() {
        println!(
            "magi: this project has no memory to judge ({})",
            mem.root().display()
        );
        return 0;
    }
    print!("{}", memory::magi::format_report(&decisions));
    if apply {
        let forgotten = decisions.iter().filter(|d| d.verdict.forgets()).count();
        println!("magi: {forgotten} page(s) retired, each kept in the history");
    } else {
        println!("magi: nothing was changed; add --apply to retire");
    }
    0
}

/// Runs `nana --memory <sub>`: list, search, show, write and forget the pages of
/// this project's memory. Hands back the process exit code.
fn memory_cmd(root: &Path, rest: &[String]) -> i32 {
    let mem = memory::Memory::open(root);
    match rest.first().map(String::as_str) {
        None | Some("list") => memory_list(&mem),
        Some("search") => memory_search(&mem, rest),
        Some("show") | Some("read") => memory_show(&mem, rest),
        Some("write") => memory_write(&mem, rest),
        Some("forget") => memory_forget(&mem, rest),
        Some("consolidate") => memory_consolidate(&mem, root),
        Some(other) => {
            eprintln!("nana: unknown memory command {other}");
            2
        }
    }
}

/// Lists the pages of the project's memory, or says it has none. Hands back 0.
fn memory_list(mem: &memory::Memory) -> i32 {
    let entries = mem.list();
    if entries.is_empty() {
        println!("this project has no memory yet ({})", mem.root().display());
        return 0;
    }
    for e in entries {
        println!("{:<10} {:<24} {}", e.class.id(), e.name, e.title);
    }
    0
}

/// Searches the memory for the text after `search`, printing each hit as
/// class:line. Hands back 2 without a query, 0 otherwise.
fn memory_search(mem: &memory::Memory, rest: &[String]) -> i32 {
    let Some(q) = rest.get(1) else {
        eprintln!("usage: nana --memory search <text>");
        return 2;
    };
    let hits = mem.search(q);
    for h in &hits {
        println!("{}:{}: {}", h.class.id(), h.line, h.text);
    }
    if hits.is_empty() {
        println!("no match in this project's memory");
    }
    0
}

/// Merges the memory pages into fewer, sharper ones with the agent, and says
/// which pages were retired. Hands back 1 when the agent or the merge fails.
fn memory_consolidate(mem: &memory::Memory, root: &Path) -> i32 {
    let settings = settings::Settings::load(root);
    let mut runner = match agent::Agent::new(root, settings) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nana: {e}");
            return 1;
        }
    };
    eprintln!("consolidating the memory...");
    match memory::consolidate(mem, |prompt| runner.run(prompt, |_| {})) {
        Ok(retired) if retired.is_empty() => {
            println!("nothing to consolidate");
            0
        }
        Ok(retired) => {
            println!("consolidated, retired {}", retired.join(", "));
            0
        }
        Err(e) => {
            eprintln!("nana: {e}");
            1
        }
    }
}

/// Parses `<class> <name>` from the arguments after a sub-command. Hands back
/// the class and name, or the usage error message and exit code 2.
fn class_and_name<'a>(rest: &'a [String], usage: &str) -> Result<(memory::Class, &'a str), i32> {
    let (Some(class), Some(name)) = (rest.get(1), rest.get(2)) else {
        eprintln!("{usage}");
        return Err(2);
    };
    let Some(class) = memory::Class::parse(class) else {
        eprintln!("classes are user, feedback, project, reference");
        return Err(2);
    };
    Ok((class, name))
}

/// Prints one memory page. Hands back 2 for bad usage, 1 when the page is missing.
fn memory_show(mem: &memory::Memory, rest: &[String]) -> i32 {
    let (class, name) = match class_and_name(rest, "usage: nana --memory show <class> <name>") {
        Ok(v) => v,
        Err(code) => return code,
    };
    match mem.read(class, name) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(e) => {
            eprintln!("nana: {e}");
            1
        }
    }
}

/// Writes a memory page whose text comes on stdin, so a page can be long and
/// multi-line. Hands back 2 for bad usage or empty input, 1 when saving fails.
fn memory_write(mem: &memory::Memory, rest: &[String]) -> i32 {
    use std::io::Read;
    let (class, name) = match class_and_name(
        rest,
        "usage: nana --memory write <class> <name> <<'EOF' … EOF",
    ) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut body = String::new();
    if std::io::stdin().read_to_string(&mut body).is_err() || body.trim().is_empty() {
        eprintln!("nana: nothing on stdin — pipe the page in");
        return 2;
    }
    match mem.write(class, name, &body) {
        Ok(path) => {
            println!("wrote {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("nana: {e}");
            1
        }
    }
}

/// Deletes one memory page. Hands back 2 for bad usage, 1 when the page cannot go.
fn memory_forget(mem: &memory::Memory, rest: &[String]) -> i32 {
    let (class, name) = match class_and_name(rest, "usage: nana --memory forget <class> <name>") {
        Ok(v) => v,
        Err(code) => return code,
    };
    match mem.forget(class, name) {
        Ok(()) => {
            println!("forgotten");
            0
        }
        Err(e) => {
            eprintln!("nana: {e}");
            1
        }
    }
}

/// Handles the persona subcommand: lists the saved personas, or says how to add one when there are none, and returns the exit code.
fn persona_cmd(root: &Path, rest: &[String]) -> i32 {
    match rest.first().map(String::as_str) {
        None | Some("list") => {
            let all = persona::list(Some(root));
            if all.is_empty() {
                println!(
                    "no personas yet — nana --persona write <id>, or drop a .md in .nana/personas/"
                );
                return 0;
            }
            for p in all {
                println!("{:<16} {:<32} {}", p.id, p.title, p.description);
            }
            0
        }
        Some("show") => {
            let Some(id) = rest.get(1) else {
                eprintln!("usage: nana --persona show <id>");
                return 2;
            };
            match persona::load(Some(root), id) {
                Some(p) => {
                    println!("{}", p.prompt);
                    0
                }
                None => {
                    eprintln!("nana: no persona « {id} »");
                    1
                }
            }
        }
        Some("write") => {
            let Some(id) = rest.get(1) else {
                eprintln!("usage: nana --persona write <id> [--user] <<'EOF' … EOF");
                return 2;
            };
            let to_user = rest.iter().any(|a| a == "--user");
            let mut body = String::new();
            use std::io::Read;
            if std::io::stdin().read_to_string(&mut body).is_err() || body.trim().is_empty() {
                eprintln!("nana: nothing on stdin — pipe the prompt in");
                return 2;
            }
            // the first line titles it, the rest is the prompt itself
            let mut lines = body.lines();
            let title = lines.next().unwrap_or(id).to_string();
            let prompt: String = lines.collect::<Vec<_>>().join("\n");
            let p = persona::Persona {
                id: id.clone(),
                title,
                description: String::new(),
                prompt: if prompt.trim().is_empty() {
                    body.clone()
                } else {
                    prompt
                },
                path: PathBuf::new(),
            };
            let store = if to_user { None } else { Some(root) };
            match persona::save(store, &p, !to_user) {
                Ok(path) => {
                    println!("wrote {}", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("nana: {e}");
                    1
                }
            }
        }
        Some("delete") => {
            let Some(id) = rest.get(1) else {
                eprintln!("usage: nana --persona delete <id>");
                return 2;
            };
            match persona::delete(Some(root), id) {
                Ok(()) => {
                    println!("deleted");
                    0
                }
                Err(e) => {
                    eprintln!("nana: {e}");
                    1
                }
            }
        }
        Some(other) => {
            eprintln!("nana: unknown persona command {other}");
            2
        }
    }
}

/// Prints the system prompt the agent would send for a request, falling back to the same prompt when no client is usable, so it can be checked offline.
fn show_prompt(root: &Path, rest: &[String]) -> i32 {
    let request = rest.join(" ");
    let s = settings::Settings::load(root);
    match agent::Agent::new(root, s.clone()) {
        Ok(a) => {
            println!("{}", a.system_prompt(&request));
            0
        }
        Err(e) => {
            // the prompt can still be shown without a usable client
            eprintln!("nana: {e} (showing the prompt anyway)\n");
            let a = agent::Agent::with_client(
                root,
                s,
                provider::Client::local("http://127.0.0.1:1", "none"),
            );
            println!("{}", a.system_prompt(&request));
            0
        }
    }
}

/// Runs the agent on one request given on the command line, with the quiet and resume flags read out of the arguments, and returns its exit code.
fn run_agent(root: &Path, rest: &[String]) -> i32 {
    let resume = rest.iter().any(|a| a == "--resume");
    let quiet = rest.iter().any(|a| a == "--quiet");
    let request: String = rest
        .iter()
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    if request.trim().is_empty() {
        eprintln!("usage: nana --agent [--resume] \"what you want done\"");
        return 2;
    }
    let s = settings::Settings::load(root);
    let mut a = match agent::Agent::new(root, s) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("nana: {e}");
            return 1;
        }
    };
    if resume {
        if let Some(path) = a.resume_latest() {
            if !quiet {
                eprintln!("resumed {}", path.display());
            }
        }
    }
    let session = a.session.clone();
    if !quiet {
        eprintln!(
            "model {} via {} — project {}",
            a.client.model,
            a.client.provider.label,
            root.display()
        );
    }
    let result = a.run(&request, |e| match e {
        agent::Event::Started { .. } => {}
        agent::Event::Text(t) => {
            if !quiet {
                println!("{t}");
            }
        }
        agent::Event::ToolCall { name, args } => {
            if !quiet {
                eprintln!("→ {name} {}", short(&args.to_string(), 120));
            }
        }
        agent::Event::ToolResult { name, ok, text } => {
            if !quiet {
                eprintln!(
                    "{} {name} {}",
                    if ok { "✓" } else { "✗" },
                    short(text.trim(), 160)
                );
            }
        }
        agent::Event::File { path, .. } => {
            if !quiet {
                eprintln!("✎ {path}");
            }
        }
        agent::Event::Retry {
            attempt,
            of,
            wait_secs,
            reason,
        } => eprintln!("nana: {reason} — retry {attempt}/{of} in {wait_secs}s"),
        agent::Event::Finished { steps } => {
            if !quiet {
                eprintln!("done in {steps} step(s) — transcript {}", session.display());
            }
        }
        agent::Event::Error(e) => eprintln!("nana: {e}"),
    });
    match result {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

/// Cuts a text to at most n characters after joining its lines, adding dots when it was cut, so a long message fits one line.
fn short(s: &str, n: usize) -> String {
    let flat = s.replace('\n', " ");
    if flat.chars().count() <= n {
        flat
    } else {
        flat.chars().take(n).collect::<String>() + "…"
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
