# changelog

## 2.6.1

- fixed the providers pane printing escapes on screen: the detail of a provider
  was built with a literal `\n` instead of a line break, so hovering a row showed
  `\nkind: anthropic\nendpoint: ...` as one unreadable line. the pane now breaks
  properly, and a test refuses any literal escape in it.
- `refresh_detail` was split in two: `detail_of` builds the pane for one item,
  `refresh_detail` only chooses between it and the busy view.

## 2.6.0

- custom endpoints: `ctrl+w` in the providers box adds one of your own — `id url [key env] [kind]`, kind being openai, anthropic or gemini. it lands in `.nana/settings.json`, joins the built-in rows, and can be picked with enter like any other. a custom id that matches a built-in one shadows it. `nana --providers` lists them too, custom rows last.

## 2.5.0

- magi: three sages judge the project's memory, each with one lens. balthasar keeps the conversations, casper the cards (user, feedback, project), melchior the world model. a page another page links to survives whatever its date, a sage never buries a page a brother kept, and a tie keeps the page. a retired page keeps its history. reachable as `nana --magi [--apply]` and as the `magi` tool of the agent.
- the agent's view can no longer fall behind: a write is shown whole the moment the agent makes it. the playback queue, its per-write durations and the catch-up ceiling are gone, so no amount of work by the agent can build a lag. the cursor stays on the last line where the agent stopped, then the view is a file like any other.

## 2.2.9

- the agent's playback (writes, reads, actions) advances by wall-clock time, one step per 16 ms tick. before, every wake-up of the loop (a key, a mouse move) played a step, so the write jumped ahead in bursts.

## 2.2.8

- the playback no longer shortens a write because the agent wrote a lot: a single long write plays at its own pace. the queue is still bounded (at most 24 waiting writes).
- release: a macos binary (apple silicon) is built and published with the linux and windows ones; install.sh fetches it on macos.

## 2.2.7

- the live view opens one window per agent action, its badge follows the action, and the editor is split beside it.
- an action shows the tool's own words; run_shell runs in a terminal, with no gutter.
- memory: a new task opens its own page, a failed task is kept, and pages are recalled by the files they name.
- bars and selected rows drop the powerline half-circles; the white edge is gone.
- boxes have no title; a second shell command no longer erases the first one.

## 2.2.6

- install.sh names the real repository in its error message, instead of an undefined variable.
- the README test count follows cargo test.

## 2.2.5

- the agent saves each conversation in memory: its opening message and its closing reply, as a project page.
- ctrl+y in the agent panel keeps the conversation so far in memory at once.

## 2.2.4

- the release job checks out the repository before it publishes, so the changelog is found.

## 2.2.3

- the windows test suite builds the expected absolute paths with the platform rules.

## 2.2.2

- the windows test suite compares paths with forward slashes, as the code emits them.

## 2.2.1

- the test suite runs on windows: paths are built with the platform separator.

## 2.2.0

- one-line installs: `install.sh` for linux, `install.ps1` for windows. both
  fetch the latest release and put `nana` on your PATH.
- every tag builds and publishes the two archives, after the tests pass.
- the agent menu is a centred card on a clean screen: the trail lane is gone.
- the agent's sandbox is off by default.
- README: 107 languages, 312 tests, install section rewritten.

## 2.1.0

- memory is a world model: each page carries a confidence, a date and an
  optional check the agent runs before trusting it.
- the world map: memory is drawn as lanes over time, with a section in the hub.
- the agent's trail: each action sits in time behind its card, and a file the
  agent reads is announced like a write.
- the agent reports each file it writes.
- `cargo fmt --check` is clean again: the ci no longer fails before the tests.

## 2.0.0

- nana opens on the hub: the menu is the only thing on screen at start, and
  `esc` gives back the ide behind it.
- the hub (`ctrl+w`): memory, knowledge, providers, skills, personas and the
  company, in rounded boxes, with the keys to act on each of them.
- the company (`agents` box, `nana --company`): a ceo hires the team, each
  employee runs an ordinary agent task under its own persona.
- the wiki (`.nana/knowledge/`) and `nana --dream` consolidate sessions into
  pages, with a dated audit.
- providers: `cheapmodels/...` is routed to its own endpoint (`OPPP_API_KEY`),
  and it is the default model, from one constant shared by hub, cli and client.
- `super+t` opens the terminal, like `f3`.
- rounded corners on every box, band and segment; no square corner left.
- `cargo fmt --check` is clean, so the ci passes.
- the agent is always told to check memory before a task and to write what it
  learns, even when the memory is still empty; the memory tools say so too.
- the readme shows the hub, the editor and the terminal, captured from the
  real binary.
- the agent menu and the file search are windows of their own, like the hub:
  nothing of the ide is drawn around them any more.

## 1.0.0

- one verdict, a bar that breathes, and movement.
