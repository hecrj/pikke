# PIKKE — build plan (v1)

The engine design is `DESIGN.md` (the spec: the op API, the
execution model, the color space, the units, the invariants). This is
the build plan for `pikke` v1: the engine in Rust, with the example
design run end to end. It records the settled decisions, the contract
deltas still owed to the spec, the v1 harness and architecture, and
the phase breakdown with its verification gates.

## ⚠ Linear U8 is ALWAYS WRONG

The buffer is linear **f32** (0–1), and every op runs on it by
default.

**Never a linear u8.** A *linear u8* — the linear light quantized to
256 uniform steps (`u8 = linear × 255`) — is *always* wrong: it is a
256-level *lossy* compression of a value that must carry continuous
linear light — the 1/255 quantization throws away the buffer's f32
precision and bands smooth gradients (worst in the shadows, where the
sRGB encode keeps fine steps). Any resample, composite or filter run
on it inherits the quantization, so the error is baked into everything
downstream. There is no circumstance that justifies it.

**An sRGB u8, when justified.** The sRGB u8 is the *correct* 8-bit
representation (the sRGB encode) — the default at the 8-bit boundary
(the PNG/MP4 output), the single rounding the plan allows. An op may
*also* work on the sRGB u8 mid-pipeline, but only with a valid reason
and an explicit justification at the op's defining site: the sRGB
encode is a bounded, legitimate loss, whereas a linear quantization
is data destruction. The default is the f32; an sRGB-u8 shortcut has
to earn it.

Consequently the linear-f32 resample is home-grown — the `image`
crate's resampler is u8-only (it cannot resample the buffer's
precision). The default, the cubic, is GIMP's resize as-is (GEGL's
cubic sampler: the BC-spline b = 1/2, c = 1/4, the downsample's
sub-block average of its fixed 4-tap, the abyss at the edge) — the
B-spline (its b = 1.0 corner) proved too soft against it, and its
mild lobe keeps the fringing at GIMP's level; the lanczos is a
Lanczos-3 (support-scaled taps, renormalized to their gathered
weight, a unit-scale axis copied). See Phase 5a.

## 1. Scope

**Goal.** `pikke` (`DESIGN.md`) in Rust — focused on color accuracy
and correctness: v1 loads a design script, binds its parameters,
executes it against the op table, and encodes the yielded frames
(one frame → `output.png`, many → `output.mp4`).

**Non-goals for v1** (explicit):

- `size = "auto"` (dropped from the API; sizing is a Lua-side
  computation);
- producer retry (a failed `spawn` is an error; a transient failure
  is re-run by hand);
- run presets — the run is a `--scale` against the logical canvas;
- batch-driven runs — v1 is invoked per render from the command
  line; a driver is v2.

## 2. Settled decisions

From the design review (2026-11-08):

- **Language: Rust** (one language for the whole binary stack).
- **Scope: stills only** for v1. The example design yields one frame
  (`time` given), so v1's real outputs are PNGs; the MP4 path exists
  and is verified with a synthetic producer.
- **Units: the script is in logical px** — the run's canvas scale,
  what `width()`/`height()` report. The engine receives the run's
  scale and performs the logical → physical transform internally; the
  script never scales.
  `physical_width()`/`physical_height()` are the only physical-px
  interface: a frame producer renders in physical px and takes them
  as its viewport.
- **`spawn` protocol:** the script passes the producer's protocol
  flag (`--stdout`); frames are raw RGBA8 on the producer's stdout,
  canvas-sized. No retry.
- **Internal buffer: premultiplied** RGBA, f32 linear (0–1) — the op
  boundary stays straight RGBA8 sRGB (§3b). The linear composite is
  `layer + base·(1 − layer.a)`: no division, and a 0 or 1 alpha on
  either side exact. Premultiplication discards a fully transparent
  pixel's color — the stored-channel ops (`luma_at`, the contrast
  histogram) read it as colorless — exact while the region is opaque,
  a representation limit otherwise.
- **`text` anchor:** the tight ink box's top-left lands at `(x, y)`.
- **No `size = "auto"`** — numeric px only.
- **`fill`'s space is automatic:** a `Text` region composites in
  sRGB, a `Rectangle` region in linear — no parameter (the best
  strategy per region kind). `blend` keeps its explicit
  `color_space` (default linear).
- **Outputs hardcoded:** `output.png` / `output.mp4` in the working
  directory.
- **ffmpeg flags hardcoded** (the default run).
- The example design's simplifications (a draft) stand for now — it
  is an example, and the Phase 10 inspection is what distinguishes
  its legitimate differences from engine bugs.

## 3. Contract deltas

**(a) Folded into DESIGN.md.** The op-level decisions the script's
evolution settled: `resize` (the target semantics), `blend`'s `at`,
the physical canvas queries, the logical/physical units model (the
engine takes the run's scale and transforms internally; the script is
logical px throughout), `fill`'s automatic space, and the `auto`-size
removal.

**(b) Open spec deltas** (op-level details DESIGN.md does not yet pin
down). The spec is expected to evolve as the engine is implemented —
**deviations from these are allowed**: they are noted in the phase
record where they occur and folded back into DESIGN.md later:

- **`spawn`'s protocol:** args are strings/numbers (numbers rendered
  in decimal; a `nil` arg is an engine error). Frames: raw RGBA8, the
  canvas's physical dims, straight alpha, sRGB; the iterator yields
  one frame per step; a truncated stream (a partial frame) or a
  non-zero exit is an error carrying the producer's drained stderr
  tail. The producer's own protocol flag is the script's business.
- **`text`'s anchor and value:** the tight ink box's top-left is at
  `(x, y)` (the engine measures the rasterized ink's offsets and
  compensates). A `Text` value carries its ink-cropped pixels plus
  the laid-out ink position; `bounds()` is that ink box as a
  `Rectangle`. `weight` sets the variable font's `wght` axis; a
  static font ignores it.
- **`luma_at`:** the mean over the region (clipped to the canvas) of
  the Rec. 709 luma of the region's **stored sRGB** RGB channels
  (alpha ignored), 0–255.
- **Geometry rounding:** positions, sizes and resize targets are real
  px; every geometric result — and the logical → physical transform
  itself — rounds to integer px, ties away from zero (Rust's
  `round` — the first implementation's native rounding).
- **`yield`'s invariant:** a frame is canvas-sized — every op
  preserves the receiver's dims (`resize` is the only dim-changing
  op), so the engine asserts canvas dims on `yield` and on `blend`'s
  receiver.
- **Buffer representation:** at the op boundary a buffer is
  straight-alpha RGBA8 sRGB; internally the buffer is f32 linear
  (0–1), premultiplied (§2), and the sRGB ↔ linear transfer is
  evaluated directly at the boundaries (load, yield) and in the small
  sRGB-composite and `luma_at` regions — no LUTs. Rounding happens
  exactly once, at the linear → 8-bit encode (nearest 1/255 step,
  ties away from zero — Rust's `round`; out-of-range linear values
  clamp).
- **Parameters and entry:** the script's globals expose the op table
  and an `arguments` table declaring the entry's parameters (in CLI
  order) — a table read, so the engine needs no `debug` library. The
  engine loads the script (paths — `image`/`font`/`require` —
  resolve against the script's directory), creates a `--<name>` CLI
  flag for each declared parameter (the CLI, §5), and calls
  `render(args)` with a single table of the bound values. v1: a
  flag's value is a string; an omitted or empty flag binds `nil`
  (optionality is the script's — it guards the optional ones); the
  script converts where it needs a number.

## 4. The script's state

- **Fixed this round:** the spawn args' `[...]` → `{...}` (Lua arrays
  are brace-delimited); the `resize` calls' `.` → `:` (the spec's
  method call syntax); the optional `--animation`/`--time` args are
  appended only when the parameter is present — `nil`, bound from the
  omitted or empty flag of that name ("the run names no value") — so
  an absent term omits the flag; a `nil` that ever reaches `spawn` is
  the backstop error (§3b).
- **Fixed by the script's evolution:** `height = height()` in the
  resize; the producer's viewport via `physical_width()`/
  `physical_height()`.
- **Subsumed, no change needed:** the logos' `resize({width=200})` —
  200 is now a logical constant, correct at every scale; the text's
  `fill` — sRGB by the region's kind.

## 5. The v1 harness

- **CLI:**
  `pikke <script> --width <W> --height <H> [--scale <s>]
  [--<param> <value>]...` — the script is the first argument (its
  parameter flags only exist once it is loaded); one named flag per
  entry-function parameter, created on the fly from the introspected
  names (§3b; for the example design: `--id --title --subtitle
  --animation --time`).
  `--width`/`--height` are the **logical** canvas (the run's canvas
  dims) — required: the engine is generic and holds no content
  defaults (not even a default canvas); `--scale` the run's scale,
  defaulting to 1 (the default run is the canvas at native scale).
  The physical canvas is `W×s`/`H×s`, each dim rounded (Rust's
  `round`); the MP4 path needs even physical dims (the caller's
  choice; 1640×2048 at 0.5/1.0/2.0 are all even). A flag's value is a
  string; an omitted or empty flag binds `nil`. Errors go to stderr
  with a non-zero exit.
- **Run size:** the default run is without `--scale` (i.e. 1); a
  scaled run (Phase 10) runs `--width 1640 --height 2048 --scale 0.5`
  (physical 820×1024). The engine supplies `width()`/`height()` = the
  logical canvas and `physical_width()`/`physical_height()` = the
  derived physical.
- **Outputs:** `output.png` / `output.mp4` in the working directory.
  The engine does not know the kind in advance: frame 1 is held, and
  frame 2's arrival starts the MP4 path (flushing frame 1 into the
  encoder). One yielded frame → the PNG.
- **PNG:** RGBA — the buffer's alpha as-is, no need to drop it (the
  base is opaque, so 255 throughout). Written atomically.
- **MP4:** the frame stream piped to ffmpeg, hardcoded (the default
  run):
  `-f rawvideo -pix_fmt rgba -s <W>x<H> -framerate 60 -i - -c:v
  libx264 -preset slow -crf 16 -pix_fmt yuv420p -movflags +faststart
  -f mp4 output.mp4`

## 6. Architecture

A standalone crate, one binary. Each module is built around the
single type it carries (the type's Lua bindings included) — a noun,
no underscores, no `ops`/`utils` catch-alls. Hygiene: `cargo fmt
--check` and `cargo clippy` clean.

```
pikke/
  Cargo.toml
  src/
    main.rs      the binary's entry: the CLI (the named flags created
                 from the introspected entry parameters, §3b) and the
                 dispatch into the Engine
    engine.rs    the `Engine`: the Lua VM (mlua) and the script's
                 environment — the op table (each global bound to its
                 type's userdata methods; the binding lives with the
                 type), the canvas queries (`width`/`height` logical,
                 `physical_width`/`physical_height` the derived
                 physical) —, the entry's parameter introspection
                 (§3b), the execution, and the logical → physical
                 transform at the op boundary (× the scale, Rust's
                 `round`)
    image.rs     the `Image`: the RGBA buffer — buffers are at their
                 own scale (the file's at load, the run's after a
                 resample; §2's form); the API reports logical. Its
                 ops: load (PNG, the straight RGBA8 sRGB boundary,
                 §3b), resize / contrast / blend / fill / luma_at /
                 width / height / size.
    color.rs     the `Color` and the pinned transfer math: the IEC
                 61966-2-1 sRGB ↔ linear conversion (evaluated
                 directly — no LUTs), the single encode rounding
                 (nearest 1/255, ties away from zero — Rust's
                 `round`), Porter-Duff over
                 in linear and sRGB, the Rec. 709 luma, the
                 tone-preserving full-range stretch
    font.rs      the `Font`: the file loaded via cosmic-text's
                 `FontSet` (from the file's bytes; the path relative
                 to the script), the `wght` weight axis (a swash
                 variation, set when the file carries it)
    text.rs      the `Text`: the layout (the ink-offset probe, the
                 anchor), the ink-cropped pixels, `bounds`
    rectangle.rs the `Rectangle` and `padded`
    stream.rs    the `Stream`: the `spawn`'d producer protocol — the
                 stdout frame reader, the stderr drain thread (bounded
                 tail, 1 MiB, for error reports), the lazy iterator,
                 the failure semantics
```

Dependencies: `mlua` with the vendored Lua 5.4 (no system Lua),
`image` (PNG codec only — its resampler is u8-only, so the linear-f32
Lanczos is home-grown; see the Linear-U8-Is-Always-Wrong notice),
`cosmic-text` for the text (swash shaping/rasterization + fontdb file
loading — pure Rust).
No argument-parsing crate: the CLI is hand-rolled — one `while let`
pass over `std::env::args()` with a `match` per argument: boolean
flags, `--flag value` arms (the value is the next argument), unknown
options and extra positionals are errors, `--help` prints after the
pass; the script is the first argument (loaded before the pass, for
the dynamic flags); the dynamic parameter flags enter the match as a
membership test against the introspected names.

Frame flow: the script threads `base` → `output` explicitly (no
hidden canvas); the engine only interprets. Compositing is
single-threaded in v1 (no thread-count-dependent behavior). Memory: a
still keeps one frame; the animation case is bounded by the
producer's loop, and the MP4 path streams frames to the encoder as
they are yielded.

## 7. Phases

Each phase is a small, self-contained step with its gate — the
verification that closes it.

**Phase 0 — the open spec deltas.** The script is fixed (§4);
DESIGN.md carries the settled op semantics (§3a). §3b (the spawn
protocol, the text anchor and value, the luma note, the rounding, the
yield invariant, the buffer boundary, the parameters and entry) stays
open for now: the spec evolves with the implementation, and
deviations from it are allowed — noted where they occur, folded back
into DESIGN.md later.
*Gate:* DESIGN.md and the script agree on every op the script uses;
`luac5.4 -p` clean.

**Phase 1 — the CLI, the VM, and the flags.** The `pikke` crate
(mlua); the script loads into a dedicated environment; the entry's
parameters are read from the script's `arguments` declaration (§3b)
and the hand-rolled CLI is built from them (the single-pass style —
the dependencies, §6). The script is the first argument, so the
flags exist before the pass (and the help text) happens; flag values
bind to the parameters as strings (an omitted or empty flag binds
`nil`).
*Gate:* `pikke <script> --help` prints the `--id --title --subtitle
--animation --time` flags alongside `--width`/`--height`/`--scale`;
with a bound `--id`, `pikke <script> --width 1640 --height 2048`
binds the values and fails cleanly at the entry's first op (the op
table does not exist yet — expected here).

**Phase 2 — the API surface, mocked.** Scaffold the crate's final
structure (§6) and draft the entire Lua API with empty mocks: every
op returns a placeholder; the mock `spawn` returns a lazy stream that
yields one canvas-sized frame; `yield` records the frame (asserting
the canvas dims, §3b) and writes no file, encodes nothing.
*Gate:* with its parameters bound, `pikke <script> --width 1640
--height 2048` invokes the script end to end — every op is called,
and `yield` was called with the right output (the mock's one
canvas-sized frame) — producing no image.

**Phase 3 — the pinned color math.** `color.rs`: the IEC 61966-2-1
sRGB ↔ linear conversion (evaluated directly, 0–1 — no LUTs),
Porter-Duff over (linear and sRGB), the single encode rounding
(nearest 1/255 step, ties away from zero — Rust's `round`), the Rec.
709 luma, the
tone-preserving full-range stretch.
*Gate:* the pinned math against its test vectors — the encode within
±1 at rounding boundaries (our tie-break and the last-ulp evaluation
noise differ); the decode/encode round-trip exact for all 256 stored
values; over within ±1 on random RGBA pairs, exact on the round-trip
cases (alpha 0/255) — pinned math is right up to integer rounding;
the luma matching the 709 mean.

**Phase 4 — the real Image buffer.** `image.rs` replaces its mock:
the buffer (§2's form, at its own scale), `image(path)` load (PNG
decode, the straight RGBA8 sRGB boundary in, §3b), `width`/
`height`/`size`, and the logical → physical transform at the op
boundary (× the scale, Rust's `round`).
*Gate:* a real asset loads at its file dims; at `--scale 0.5` the
canvas is 820×1024 and the op-boundary transform is exact (the 200
px target → 100, 20 → 10, 150 → 75).

**Phase 5a — `resize`.** The real resize (the three target
semantics; the default cubic and a lanczos — free ops), run in
linear.
*Gate:* on the sample backgrounds, headless: the three modes'
geometry exact (aspect preserved, cover center-cropped, exact box),
within a few units.

**Phase 5b — `contrast`.** The real contrast (auto, the
tone-preserving stretch; a free op), run in linear.
*Gate:* on the sample backgrounds, headless: the stretch is
full-range and tone-preserving (no highlight/shadow clip).

**Phase 6 — compositing: `blend`, `fill`, `luma_at`, the
Rectangle.** The real blend (over, `color_space`, `at` + the canvas
clip), fill (the space by the region's kind), luma_at (the region
mean), and `rectangle.rs` (the Rectangle + `padded`).
*Gate:* `blend` and `fill` exact at 8-bit on real pairs (pinned
math); `luma_at` matching the 709 mean on the sample backgrounds.

**Phase 7 — text.** `font.rs` + `text.rs` on `cosmic-text` (the face
via `FontSet` from the file's bytes — the path relative to the
script —, swash shaping + rasterization at the run scale): the `wght`
axis (a swash variation, set when the file carries it — the
example's variable font does, its static font does not), the layout,
the ink-offset probe (the union of the glyphs' ink), the anchor (the
tight ink box's top-left at `(x, y)`), `bounds`. Shaping hint
`Basic` — plain advances, no GPOS (swash kerns by default).
*Gate:* the script's two texts (a 300-weight variable font, a
700-weight static font): the ink box's top-left exactly `(x, y)`
(pinned), the ink box's size within ~1 px of the layout's ink probe,
the rasterized text clean at its size (free).

**Phase 8 — the real Stream.** `spawn`'s producer protocol: the
stdout frame reader (raw RGBA8, the canvas's physical dims), the
stderr drain (the bounded 1 MiB tail), the lazy iterator (one frame
per step), the failure semantics (a truncated stream or a non-zero
exit → an error carrying the stderr tail).
*Gate (no GPU):* a synthetic producer (a small script writing N raw
RGBA8 frames to stdout) — N frames yielded byte-for-byte; a
truncated stream and a non-zero exit each surface as an error
carrying the stderr tail. With the GPU: one real still (a frame
producer through a minimal `background:blend(frame)` script, the
viewport from `physical_width()`/`physical_height()`) — the yielded
frame equals the raw stream's frame byte-for-byte (no free op
between them).

**Phase 9 — the encoder.** The PNG writer (RGBA, atomic) and the
ffmpeg MP4 pipe (the default flags, §5); the PNG/MP4 decision at
frame 2 (§5) replaces the Phase 2 yield recorder.
*Gate:* one frame → `output.png` decoding back to the frame
byte-for-byte (lossless); N frames → `output.mp4` with N frames
(ffprobe).

**Phase 10 — the example design, end to end.** Run the example
design at a run scale: `pikke <script> --width 1640 --height 2048
--scale 0.5 <params>` (the omitted optional parameters bind `nil`,
and the script omits the flags they feed) and inspect `output.png`.
*Gate:* the composite is correct end to end — the geometry exact
(the fit, the crop, the placed elements' positions, all logical px
at the run scale), the effects visible (the stretch, the frame
composite), no artifacts — judged by eye and an ad-hoc diff at that
time.

**After v1 (preview, not scheduled here):** the standing measurement
tooling (per-element regression checks, the engine's `--report`
sidecar), the run presets, the batch driver, and the hash-embedding
question for engine outputs.

## 8. Risks and notes

- **Free ops are chosen, not copied.** The Lanczos resize, the swash
  rasterization, and the ffmpeg flags are practical choices — none
  is replicated detail for its own sake. Color accuracy is paramount.
- **No text supersampling for now:** the glyphs rasterize directly at
  the run scale. Swash is unhinted, so the small text (a caption
  line, ~10 physical px at 0.5×) is the risk case — the first fix,
  if its edges are ragged, is 4× supersampling with a Lanczos
  downscale.
- **The engine runs on mlua's safe standard-library subset:**
  parameter introspection is a table read of the script's `arguments`
  declaration (§3b), so no `debug` library is needed — and `package`
  (for `require`) is in the safe set. Isolation from the base
  libraries (`os`, `io`, `debug`) comes from the dedicated
  environment (a fresh globals table), not the subset.
- **The resample happens once.** A layer is resampled to the run
  scale at the first op that needs it there (the `resize`, or the
  composite when the layer is still at its file scale), so no path
  pays a double resampling.
- **`contrast` order:** the script resizes (cover) then stretches —
  the histogram's tail moves slightly with the crop.
- **No producer retry** (settled): a transient producer failure
  (truncated stream, crash) fails the run and is re-run by hand.
- **No system-Lua dependency** (mlua's vendored 5.4); the text stack
  (`cosmic-text`/`swash`/`fontdb`) is pure Rust — no fontconfig, no
  libfreetype.
- **v1's designs are stills-only,** so the MP4 path is exercised
  synthetically; a real animated run is v2.

## 9. Next

The phase ladder is settled (§7); the next working step is Phase 5b
(`contrast` — the auto stretch).
