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
sub-block average of its fixed 4-tap, the out-of-domain taps
clamped to the boundary — GIMP's CLAMP abyss — so a constant plane
stays constant at the edge) — the
B-spline (its b = 1.0 corner) proved too soft against it, and its
mild lobe keeps the fringing at GIMP's level; the lanczos is a
Lanczos-3 (support-scaled taps, renormalized to their gathered
weight, a unit-scale axis copied). See Phase 5a.

## 1. Scope

**Goal.** `pikke` (`DESIGN.md`) in Rust — focused on color accuracy
and correctness: v1 loads a design script, binds its parameters,
executes it against the op table, and encodes the yielded frames
(one frame → a PNG, many → an MP4).

**Non-goals for v1** (explicit):

- `size = "auto"` (dropped from the API; sizing is a Lua-side
  computation);
- producer retry (a failed `stream` is an error; a transient failure
  is re-run by hand);
- run presets — the run is a `--scale` against the script's dims;
- batch-driven runs — v1 is invoked per render from the command
  line; a driver is v2.

## 2. Settled decisions

From the design review (2026-11-08):

- **Language: Rust** (one language for the whole binary stack).
- **Scope: stills only** for v1. The example design yields one frame
  (`time` given), so v1's real outputs are PNGs; the MP4 path exists
  and is verified with a synthetic producer.
- **Units: the script is in logical px** — at the run's scale. The
  engine holds no canvas: the script decides its dims, and the run's
  `--scale` is the caller's only knob. The engine performs the
  logical → physical transform internally; the script never scales.
  An image's `physical_width`/`physical_height` fields are the only
  physical-px interface: a frame producer renders in physical px and
  takes them as its viewport.
- **`stream` protocol:** the script passes the producer's protocol
  flag (`--stdout`); frames are raw RGBA8 on the producer's stdout,
  at the producer's viewport. No retry.
- **Internal buffer: premultiplied** RGBA, f32 linear (0–1) — the op
  boundary stays straight RGBA8 sRGB (§3b). The linear composite is
  `layer + base·(1 − layer.a)`: no division, and a 0 or 1 alpha on
  either side exact. Premultiplication discards a fully transparent
  pixel's color — the stored-channel ops (`luma_in`, the autocontrast
  histogram) read it as colorless — exact while the region is opaque,
  a representation limit otherwise.
- **`text` anchor:** the tight ink box's top-left lands at `(x, y)`.
- **No `size = "auto"`** — numeric px only.
- **`fill`'s space is automatic:** a `Text` region composites in
  sRGB, a `Rectangle` region in linear — no parameter (the best
  strategy per region kind). `blend` keeps its explicit
  `color_space` (default linear).
- **Output path:** the working directory, named after the script
  (`<script>.png` / `<script>.mp4`); `--output` overrides the path.
- **ffmpeg flags hardcoded** (the default run).
- The example design's simplifications (a draft) stand for now — it
  is an example, and the Phase 10 inspection is what distinguishes
  its legitimate differences from engine bugs.

## 3. Contract deltas

**(a) Folded into DESIGN.md.** The op-level decisions the script's
evolution settled: `resize` (the target semantics), `blend`'s `at`,
the physical-dim fields, the logical/physical units model (the
engine takes the run's scale and transforms internally; the script is
logical px throughout), `fill`'s automatic space, and the `auto`-size
removal.

**(b) Open spec deltas** (op-level details DESIGN.md does not yet pin
down). The spec is expected to evolve as the engine is implemented —
**deviations from these are allowed**: they are noted in the phase
record where they occur and folded back into DESIGN.md later:

- **`stream`'s protocol:** args are strings/numbers (numbers rendered
  in decimal; a `nil` arg is an engine error). Frames: raw RGBA8 at
  the producer's viewport, straight alpha, sRGB; the iterator yields
  one frame per step; a truncated stream (a partial frame) or a
  non-zero exit is an error carrying the producer's drained stderr
  tail. The producer's own protocol flag is the script's business.
  The frame-size channel is open: the engine holds no canvas, so the
  protocol must carry the frame's dims.
- **`text`'s anchor and value:** the tight ink box's top-left is at
  `(x, y)` (the engine measures the rasterized ink's offsets and
  compensates). A `Text` value carries its ink-cropped pixels plus
  the laid-out ink position; `bounds()` is that ink box as a
  `Rectangle`. `weight` sets the variable font's `wght` axis; a
  static font ignores it.
- **`luma_in`:** the mean over the region (clipped to the receiver) of
  the Rec. 709 luma of the region's **stored (linear,
  premultiplied)** RGB channels (alpha ignored — a transparent
  pixel is colorless), × 255 (the linear scale — the same scale as
  the autocontrast histogram's luma); a region with no receiver
  overlap — or a negative size — reads 0 (colorless).
- **`fill`'s paint and span:** the color is a `#RRGGBB` hex (exactly
  six digits after `#`), decoded sRGB → linear at full opacity. The
  region's span — each edge × the run scale — is painted **unrounded**
  through `vello_cpu`'s coverage rasterizer (the region in logical
  real px under the run-scale transform; its alpha mask is the
  sub-pixel coverage) and the pinned paint composited over the buffer
  with the pinned `over` — a paint span is sub-pixel geometry, so the
  geometry-rounding clause does not apply to it. A `Text` region is
  painted by `fill` (the skrifa outlines → the vello raster, §7)
  with its ink's top-left pinned exactly to `(x, y)`. A
  non-finite or non-positive span paints nothing. Image dims are
  capped at 65535 physical px (the rasterizer's surface bound).
- **Geometry rounding:** positions, sizes and resize targets are real
  px; every geometric result — and the logical → physical transform
  itself — rounds to integer px, ties away from zero (Rust's
  `round` — the first implementation's native rounding).
- **`yield`:** the engine holds no canvas — the frame is what the
  script built. Every op preserves the receiver's dims (`resize` is
  the only dim-changing op), so a frame's dims are the script's
  own; `yield` asserts that every frame of a run shares the first
  frame's dims (a differing dim is an error; lands in Phase 8).
- **Buffer representation:** at the op boundary a buffer is
  straight-alpha RGBA8 sRGB; internally the buffer is f32 linear
  (0–1), premultiplied (§2), and the sRGB ↔ linear transfer is
  evaluated directly at the boundaries (load, yield) and in the small
  sRGB-composite and `luma_in` regions — no LUTs. Rounding happens
  exactly once, at the linear → 8-bit encode (nearest 1/255 step,
  ties away from zero — Rust's `round`; out-of-range linear values
  clamp).
- **Parameters and entry:** the script's globals expose the op table
  and, when the entry takes parameters, an `ARGUMENTS` table
  declaring them (in CLI order; absent — no parameters) — a table
  read, so the engine needs no `debug` library. The engine loads the
  script (paths — `image`/`font`/`require` — resolve against the
  script's directory) with the op table installed before the chunk
  runs (the script's top level may call ops), creates a `--<name>`
  CLI flag for each declared parameter (the CLI, §5), and calls
  `render(args)` with a single table of the bound values. v1: a
  flag's value is a string; an omitted or empty flag binds `nil`
  (optionality is the script's — it guards the optional ones); the
  script converts where it needs a number. The engine's own flag
  names (`scale`, `output`) are reserved argument names — a script
  declaring one fails the load.

## 4. The script's state

- **Fixed this round:** the stream args' `[...]` → `{...}` (Lua arrays
  are brace-delimited); the `resize` calls' `.` → `:` (the spec's
  method call syntax); the optional `--animation`/`--time` args are
  appended only when the parameter is present — `nil`, bound from the
  omitted or empty flag of that name ("the run names no value") — so
  an absent term omits the flag; a `nil` that ever reaches `stream` is
  the backstop error (§3b).
- **Fixed by the script's evolution:** `height = height()` in the
  resize; the producer's viewport via the image's `physical_width`/
  `physical_height` fields.
- **Subsumed, no change needed:** the logos' `resize({width=200})` —
  200 is now a logical constant, correct at every scale; the text's
  `fill` — sRGB by the region's kind.

## 5. The v1 harness

- **CLI:**
  `pikke <script> [--scale <s>] [--output <path>]
  [--<param> <value>]...` — the script is the first argument (its
  parameter flags only exist once it is loaded); one named flag per
  entry-function parameter, created on the fly from the introspected
  names (§3b; for the example design: `--id --title --subtitle
  --animation --time`).
  The engine holds no canvas — the script decides its dims — so
  `--scale` (the run's scale, defaulting to 1) is the caller's only
  sizing knob; each logical dim × the scale, rounded (Rust's
  `round`), is the physical dim. `--output` the output's path
  (default the script's file name with a `.png`/`.mp4` extension).
  The MP4 path needs even physical dims (the script's choice;
  1640×2048 at 0.5/1.0/2.0 are all even). A flag's value is a
  string; an omitted or empty flag binds `nil`. Errors go to stderr
  with a non-zero exit.
- **Run size:** the default run is without `--scale` (i.e. 1); a
  scaled run (Phase 10) runs `--scale 0.5` against the example
  design's own 1640×2048 (physical 820×1024). The script's dims are
  its own; an image's `physical_width`/`physical_height` report the
  derived physical.
- **Outputs:** in the working directory, named after the script —
  `hello.lua` → `hello.png` / `hello.mp4`; `--output <path>`
  overrides the still's path (the video is the same path as
  `.mp4`). The engine does not know the kind in advance:
  the first frame is always written as a PNG, and frame 2's arrival
  starts the MP4 path (flushing frame 1 into the encoder). One
  yielded frame → the PNG alone; a sequence → the PNG and the MP4.
  All frames share frame 1's dims (`yield`'s invariant, §3b), so the
  encoder's size is fixed at frame 1.
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
                 type), installed before the chunk runs so the
                 script's top level may call ops —, the entry's
                 parameter introspection (§3b), the execution, and
                 the logical → physical transform at the op boundary
                 (× the scale, Rust's `round`)
    image.rs     the `Image`: the RGBA buffer — buffers are at their
                 own scale (the file's at load, the run's after a
                 resample; §2's form); the API reports logical. Its
                 ops: blank (solid or transparent) / load (PNG, the
                 straight RGBA8 sRGB boundary, §3b) / resize /
                 autocontrast / blend / fill / luma_in / width /
                 height / size / physical_width / physical_height.
    color.rs     the `Color` and the pinned transfer math: the IEC
                 61966-2-1 sRGB ↔ linear conversion (evaluated
                 directly — no LUTs), the single encode rounding
                 (nearest 1/255, ties away from zero — Rust's
                 `round`), Porter-Duff over
                 in linear and sRGB, the Rec. 709 luma, the
                 tone-preserving full-range stretch
    font.rs      the `Font`: the face registered in the fontique
                 collection (the file's bytes; the path relative to
                 the script)
    text.rs      the `Text`: the parley layout at the run scale,
                 the pixel-free ink probe, the ink box, the
                 `bounds`/`width`/`height` accessors (logical px),
                 `translate` (the logical offsets, rounded at the
                 run scale) — `fill` does the skrifa outlines →
                 vello rasterization
    rectangle.rs the `Rectangle` and `padded`
    stream.rs    the `Stream`: the producer protocol — the
                 stdout frame reader, the stderr drain thread (bounded
                 tail, 1 MiB, for error reports), the lazy iterator,
                 the failure semantics
```

Dependencies: `mlua` with the vendored Lua 5.4 (no system Lua),
`image` (PNG codec only — its resampler is u8-only, so the linear-f32
Lanczos is home-grown; see the Linear-U8-Is-Always-Wrong notice),
`parley` for the text layout, `fontique` for the font file loading,
`skrifa` for the glyph outlines (the existing `vello` rasterizes the
fill) — pure Rust.
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
DESIGN.md carries the settled op semantics (§3a). §3b (the stream
protocol, the text anchor and value, the luma note, the rounding, the
yield invariant, the buffer boundary, the parameters and entry) stays
open for now: the spec evolves with the implementation, and
deviations from it are allowed — noted where they occur, folded back
into DESIGN.md later.
*Gate:* DESIGN.md and the script agree on every op the script uses;
`luac5.4 -p` clean.

**Phase 1 — the CLI, the VM, and the flags.** The `pikke` crate
(mlua); the script runs in the state's globals, extended with the
op table (§8); the entry's parameters are read from the script's
`ARGUMENTS` declaration (§3b)
and the hand-rolled CLI is built from them (the single-pass style —
the dependencies, §6). The script is the first argument, so the
flags exist before the pass (and the help text) happens; flag values
bind to the parameters as strings (an omitted or empty flag binds
`nil`).
*Gate:* `pikke <script> --help` prints the `--id --title --subtitle
--animation --time` flags alongside `--scale`; with a bound `--id`,
`pikke <script>` binds the values and fails cleanly at the entry's
first op (the op table does not exist yet — expected here).

**Phase 2 — the API surface, mocked.** Scaffold the crate's final
structure (§6) and draft the entire Lua API with empty mocks: every
op returns a placeholder; the mock `stream` returns a lazy stream that
yields one frame; `yield` records the frame and writes no file,
encodes nothing.
*Gate:* with its parameters bound, `pikke <script>` invokes the
script end to end — every op is called, and `yield` was called with
the right output (the mock's one frame) — producing no image.

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
op-boundary transform is exact (the 200 px target → 100, 20 → 10,
150 → 75).

**Phase 5a — `resize`.** The real resize (the three target
semantics; the default cubic and a lanczos — free ops), run in
linear.
*Gate:* on the sample backgrounds, headless: the three modes'
geometry exact (aspect preserved, cover center-cropped, exact box),
within a few units.

**Phase 5b — `autocontrast`.** The real autocontrast (the
tone-preserving stretch; a free op), run in linear: the histogram's
extent is the Rec. 709 luma of the stored (premultiplied) channels,
and the ramp — the identity off the extent (the no-clip), the
stretch to the full range within it — runs on the f32 values, so the
buffer's single encode rounding stays at the boundary.
*Gate:* on the sample backgrounds, headless: the stretch is
full-range and tone-preserving (no highlight/shadow clip).

**Phase 6a — `rectangle`.** The real `Rectangle`: the
`rectangle({x, y, size})` constructor (top-left, real px; `size` a
scalar or the `{w, h}` pair) and `padded(n)`; the geometry is real
px (f32) — the integer rounding (ties away from zero, §3b) is the
consumers' logical → physical transform. `img:size()` returns the
single `{w, h}` pair it is the convenience for.
*Gate:* the geometry exact in real px: the positions and sizes
preserved, `padded` growing all sides by `n`.

**Phase 6b — `blend`.** The real `blend`: the Porter-Duff `over`
(pinned; linear on the buffer, or sRGB by decoding both sides,
`over`'ing, and re-encoding), `other`'s top-left at `at` (a
`Rectangle`, default `(0, 0)`, the signed physical transform for the
off-receiver halves), clipped to the receiver; a layer still at its
file scale resamples to the run scale here, to its logical dims (the
composite the one-resample's second trigger, §8).
*Gate:* headless, on real pairs: the `over` exact at 8-bit (the
pinned math — byte-identical where an alpha is 0 or 255), the `at`
offset and the receiver clip exact.

**Phase 6c — `luma_in`.** The real `luma_in`: the mean, over the
region (clipped to the receiver), of the Rec. 709 luma of the
region's stored (linear, premultiplied) RGB channels (alpha ignored —
a transparent pixel is colorless), × 255 (the linear scale, §3b — no
sRGB round-trip; the thresholds run on the same scale as the
autocontrast histogram).
*Gate:* on the sample backgrounds, headless: the `luma_in` matching
the 709 mean.

**Phase 6d — `fill` (done).** The real `fill` on `vello_cpu` 0.3
(`std` + `u8_pipeline`; no `png`, `text` or `multithreading`): the
rasterizer is used for **coverage only** — the region's span in
logical real px under the run-scale transform (vello's CTM, unrounded
— the §3b `fill` delta) filled
opaque, its alpha mask read, and the pinned hex paint (sRGB →
linear, full opacity) composited over the buffer with the pinned
`over` in linear — the `Rectangle` kind's space. A `Text` region is
accepted but paints nothing while `text` is mocked (the real sRGB
ink paint lands in Phase 7); `standard.lua` fills one. `#RRGGBB`
only (six hex digits after `#`); a non-finite or non-positive span
paints nothing. Image dims are capped at 65535 physical px (vello's
surface bound).
*Gate (met):* an aligned region paints byte-exactly (the interior
the hex's own bytes, the outside untouched) — pinning vello's exact
0/255 aligned coverage; fractional edges match the pinned composite
at frozen u8-pipeline coverages (exact area, rounded to u8: ¼ → 64,
½ → 128 at scale 1; ¾² → 143 at scale 0.5 — frozen from a one-off
probe mirroring the engine); a degenerate span is a no-op; the
binding rejects bad hex and wrong-kind regions with user-facing
errors and accepts `Text`; the smoke's `padded(30)` box lands
byte-exact at the corner (PIL). The `Text` region's real sRGB ink
paint is verified with the real text in Phase 7.

**Phase 7 — text (done).** `font.rs` + `text.rs` on `parley`/
`fontique`/`skrifa`: `font` registers the face in the shared
`fontique` collection (the file's bytes, the path relative to the
script) and hands back a `Font`; `text` lays the string out with
`parley` at the run scale (quantized, one run styled from the
`font`/`size`/`weight` opts — `weight` optional, default 400; the
variable font's `wght` mapped to normalized coords, the static font
unaffected) and probes the ink without pixels (the skrifa outline
bboxes). `Text` stores the ink box, the scale, the ink offset and
the parley `Layout` — no color, no pixels; an empty string or an
unknown family is a degenerate (empty-layout) `Text`, and `bounds`
reports the logical box. The paint lands in `fill`: the per-glyph
skrifa outlines (y-flipped to image space, the variable location
applied) rasterized through `vello` into the ink box plus a 1 px AA
margin, the ink's top-left pinned exactly to `(x, y)`, composited
with the pinned sRGB `over`.
*Gate:* `personal/gate.lua` — the script's two texts (an `Oswald`
variable font at weight 300, the `AireExterior` static font) on a
real plate: the ink's top-left exactly `(x, y)` (pinned), the fill
the hex's own bytes (the fully-covered pixels exact), the plate
byte-identical outside the ink (PIL).

**Phase 8 — the real `Stream` (done).** `stream`'s producer
protocol: the logical viewport (the script passes the frame's
viewport in logical px; the engine transforms it at the stream
boundary and passes `--width`/`--height` to the producer, so the
script stays unaware of physical coordinates end to end), the
stdout frame reader (raw RGBA8 at the producer's viewport — the
frame-size channel), the stderr drain (the bounded 1 MiB tail), the
lazy iterator (one frame per step), the failure semantics (a
truncated stream or a non-zero exit → an error carrying the stderr
tail), and the `yield` frame-size invariant (every frame shares the
first frame's dims, §3b).
*Gate (no GPU, met):* a synthetic producer (a small script writing
N raw RGBA8 frames to stdout) — N frames yielded byte-for-byte, the
viewport flags exact at scale 1 and 0.5; a truncated stream and a
non-zero exit each surface as an error carrying the stderr tail; a
stream whose frames differ in dims fails with the invariant's error.
With the GPU: one real still (a frame producer through a minimal
`background:blend(frame)` script) — the yielded frame equals the raw
stream's frame byte-for-byte (no free op between them).

**Phase 9 — the encoder (done).** The `Video`: the always-PNG first
frame (the PNG writer — RGBA, atomic) and the ffmpeg MP4 pipe from
frame 2 (the default flags, §5), driven by a worker thread over a
bounded channel (backpressure; the RGBA transfer is off the
script's thread); the even-dims precheck guards yuv420p.
*Gate (met):* one frame → `<script>.png` decoding back to the frame
byte-for-byte (lossless); N frames → the first frame's
`<script>.png` and `<script>.mp4` with N frames (ffprobe).

**Phase 10 — the example design, end to end.** Run the example
design at a run scale: `pikke <script> --scale 0.5 <params>` (the
script's own 1640×2048; the omitted optional parameters bind `nil`,
and the script omits the flags they feed) and inspect `<script>.png`.
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

- **Free ops are chosen, not copied.** The Lanczos resize, the
  skrifa-outlines → vello rasterization, and the ffmpeg flags are
  practical choices — none is replicated detail for its own sake.
  Color accuracy is paramount.
- **No text supersampling for now:** the glyphs rasterize directly at
  the run scale. skrifa draws unhinted by default, so the small text
  (a caption line, ~10 physical px at 0.5×) is the risk case — the
  first fix, if its edges are ragged, is 4× supersampling with a
  Lanczos downscale.
- **The engine runs on mlua's safe standard-library subset:**
  the script runs in the state's globals, extended with the op
  table — the full standard library is available to it (`os`, `io`,
  and `package` for `require`); the subset withholds only `debug`,
  which introspection doesn't need (it's a table read of the
  script's `ARGUMENTS` declaration, §3b).
- **The resample happens once.** A layer is resampled to the run
  scale at the first op that needs it there (the `resize`, or the
  composite when the layer is still at its file scale), so no path
  pays a double resampling.
- **`autocontrast` order:** the script resizes (cover) then stretches —
  the histogram's tail moves slightly with the crop.
- **No producer retry** (settled): a transient producer failure
  (truncated stream, crash) fails the run and is re-run by hand.
- **No system-Lua dependency** (mlua's vendored 5.4); the text stack
  (`parley`/`fontique`/`skrifa`) is pure Rust — no fontconfig, no
  libfreetype.
- **v1's designs are stills-only,** so the MP4 path is exercised
  synthetically; a real animated run is v2.

## 9. Next

The phase ladder is settled (§7); the next working step is Phase 10
(the example design, end to end, at `--scale 0.5`).
