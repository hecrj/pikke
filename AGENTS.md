# AGENTS.md

PIKKE is a modern, Lua-driven image and video compositing engine.
Inspired by [GEGL](https://gitlab.gnome.org/GNOME/gegl).

## Positioning

The engine is generic: it interprets Lua design scripts and holds
no content knowledge.

## Rust

- Modules around single-noun types; no `ops`/`utils` catch-alls.
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
- Errors are user-facing `anywho!` messages; `main` returns
  `anywho::Result` (the default rendering, exit 1).
- Import groups, separated by blank lines: `std`, external crates,
  `crate` (last). Shared types are importable from the crate root
  (`pub use`).

## Comments and doc comments

Cover only what the code does not say: invariants, protocol
semantics, units, gotchas.

- Never values: numbers, named lists, restated defaults.
- Never restated code: step-by-step narration, per-parameter docs,
  restated names.
- Write each fact once, at its defining site; reference it
  elsewhere.
- No history: do not justify a choice by negating a previous or
  rejected approach.
- Keep them as concise as possible: only the important ones.

## Hygiene

`cargo fmt --check` and `cargo lint` must be clean.
