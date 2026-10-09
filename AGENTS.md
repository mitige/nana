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
