# AGENTS.md

PIKKE is a modern, Lua-driven image and video compositing engine.
Inspired by [GEGL](https://gitlab.gnome.org/GNOME/gegl).

## Positioning

The engine is generic: it interprets Lua design scripts and holds
no content knowledge.

## Rust

- Modules around single-noun types; no `ops`/`utils` catch-alls.
- The Lua boundary is `install`: argument sanitization lives there;
  the type's methods are pure, decoupled from Lua.
- Shadowing is fine: a function may share its name with the value
  it reads or produces, and a local may shadow a function name.
  Name things by what they are; do not rename to dodge a
  collision.
- No shortened names: `width` and `height`, not `w` and `h`.
- Inference carries the types: no annotation where the use fixes
  them; a value keeps its name through conversion
  (`let value: u32 = value.parse()…`).
- The standard's iterators and combinators over hand-rolled index
  management (`sequence_values().enumerate()`, `transpose`);
  `std::env::args()` directly, not collected.
- Make unbuildable states unrepresentable: a constructor that
  returns the built value (`Engine::load`) over a `new()` plus
  `Option` fields.
- Import groups, separated by blank lines: `std`, external crates,
  `crate` (last). Shared types are importable from the crate root
  (`pub use`).

## Comments

No comments allowed, unless explicitly requested during review.

## Commits

Every change must be reviewed by the user. Under no circumstance
you are allowed to commit or push until explicitly asked to.

- **Short subjects:** one imperative clause, backticked identifiers; details in the body.

## Hygiene

`cargo fmt --check` and `cargo lint` must be clean.

## Benchmarks

Run `cargo bench --bench lua -- --baseline golden --quick` for a quick benchmark
against the golden measurements. Only needed when working on `image` or `color`
hot paths.
