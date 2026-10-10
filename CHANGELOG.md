# changelog

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
