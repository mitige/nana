# working in this repository

nana is a rust terminal editor with an agent inside it. when you change code
here, keep these rules:

- no dependencies are added lightly: every new crate must earn its place.
- every feature gets a test, and the test must fail before the feature exists.
- comments explain *why*, never *what*: the code already says what.
- user-visible text is lowercase, in english, including headings and messages.
- the ui has no square corners: boxes, bands and segments are rounded.
- run `cargo test` before you say anything is done, and paste the result line.
- the language registry (src/langs.rs) is the single place a language is
  described; never special-case a language anywhere else.

code shape (atomic code, asked by the user):

- one function does one job. a function over 100 lines is split into named
  helpers, each with one responsibility.
- every function has a doc comment (`///`) above it, in english, that says
  why it exists and what it hands back. the test functions need none.
- keep the syntax clean: no dead code, no commented-out code, no `unwrap()`
  on user input.
- after a split, run `cargo fmt`, then `cargo test`, then `cargo clippy` if
  it is installed. a refactor must keep every test green.

file structure (one responsibility per file, not only per function):

- a file holds one concept: one type and its impl, or one screen, or one
  family of helpers. when a file mixes two concepts, it is split.
- a file over 1200 lines is split by concept into a folder of the same name
  (src/editor.rs and src/editor/render.rs). tests live in a folder beside
  the code (src/editor/tests/), one file per topic, never one giant file.
- a test file mirrors the module it tests; a shared helper goes in the
  folder's mod.rs, not copied into each topic file.
- a module declares its children in mod.rs or in its own file, never both.
- the shape test (tests/code_shape.rs) enforces the function rules and the
  file limit; a file listed there as oversized is debt, and it shrinks.
- a split is by concept, never by line count alone: first list the functions
  and their groups, then give each group one file named after its concept.
- every function sits in exactly one file: a function moved twice, or dropped
  during a split, is a bug. count the functions before and after the split.
- a source file that no `mod` declares is dead code: it is deleted, not kept
  "for later". the shape test fails on it.
- the parent module file only declares its children and re-exports what the
  rest of the crate uses; it holds no function of its own.
- a child file names the concept, not the position: `statusbar.rs`, never
  `part2.rs` or `misc.rs`.
- one function does one job; a helper that is used once and hides a second
  job is split out with a name that says that job.
- a refactor is done only when cargo build has zero warnings, cargo test is
  green, and cargo clippy is clean.
