# changelog

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

## 1.0.0

- one verdict, a bar that breathes, and movement.
