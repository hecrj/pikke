# PIKKE

A modern, Lua-driven image and video compositing engine.
Inspired by [GEGL](https://gitlab.gnome.org/GNOME/gegl).

`pikke` composites images and video by executing a **Lua script**
against a fixed table of **ops** on a pixel buffer with one canonical
form (linear light, premultiplied, f32). A composition emits a
**sequence of frames** — one frame is a still image, many are a video.

This document specifies the engine's design — the op API, the
execution model, the color-space and units conventions, and the
invariants (correctness and determinism, not byte-exactness) — in a
**language-agnostic** way. The first implementation is Rust
(`pikke/`); the op API is the portability boundary, and the same
script must run unchanged on any conforming engine.

## Design

The architecture follows GEGL's: a pixel buffer with one canonical
form, ops with exact documented semantics, and a program that is the
composition. Where GEGL's composition is a node graph of operations,
pikke's is a Lua script — the same separation (the content is data
and program; the engine is generic), expressed as a program instead
of a graph.

- **One buffer form.** Every op reads and writes the same pixel form:
  linear light, **premultiplied** RGBA, f32 (0–1). The storage form
  (8-bit sRGB) exists only at the boundaries — loading and encoding —
  with a single rounding on the way out. (GEGL: the f16 buffer and
  the format conversions at the boundary; pikke: f32.)
- **Linear light is the working space.** Compositing and the color
  ops run in linear light by default; sRGB is the authoring and
  storage space.
- **Ops are the units of composition.** A fixed table of ops, each
  with exact, documented semantics (§ The op API). The engine holds
  no content — no layout knowledge, no asset lists: the content is
  the script's data and its op calls.
- **Pull, not push.** Frames are produced on demand: the script's
  frame loop pulls exactly one frame per iteration from the
  producers, and the encoder consumes the frames as they are
  yielded — nothing is produced ahead of its consumption (GEGL's
  tile-pull model in miniature).
- **The default interpolation is GEGL's.** `resize`'s default
  `cubic` filter is GEGL's cubic sampler as-is (the BC-spline
  b = 1/2, c = 1/4; the downsample's sub-block average of its fixed
  4-tap; the abyss at the edge) — the filter GIMP's resize runs. The
  `lanczos` is a Lanczos-3 (support-scaled taps, renormalized to
  their gathered weight, a unit-scale axis copied).

What pikke is not (yet): a node-graph API (the script is the
program), color management (sRGB in and out), or a tiled, parallel
machine (one frame in flight, single-threaded).

## The split

- **The script** is the *composition*: what is drawn, where, in what
  order, with what style — one script serves every run of a design,
  the per-run data passed as **parameters** to the script's entry
  function. The script **decides its own dims** (plain values — a
  top-level `width = 1640`, say) and positions everything relative to
  them (see *Units*), so one script serves every run scale.
- **The engine** is a *generic interpreter*: it loads the script,
  binds the parameters, executes it, and encodes the yielded frames.
  It knows the op semantics and **nothing** about what is composed —
  no asset lists, no layout knowledge.
- **The op API** is the *contract*: each op's signature and exact
  semantics. A conforming engine implements the same API; the script
  is the portable artifact.

The composition owns the *what*; the engine owns the *how*. A layout
change is a script edit; an op-behavior change is an engine edit.

## The op API

### Constructors

| op | signature | meaning |
|---|---|---|
| `image` | `image(path) → Image` | Load a bitmap (path resolved against the calling module's directory). sRGB, with alpha. |
| `blank` | `blank({width, height, color}) → Image` | A solid-color image (logical px); `color` a `#RRGGBB` hex, transparent when absent. |
| `font` | `font(path) → Font` | Load a font (path resolved against the calling module's directory); used by `text`. |
| `stream` | `stream(program, args) → Stream` | Run an external frame producer (`args`: an array of strings/numbers, numbers rendered for the command line). Returns a **lazy** stream of `Image` frames. |
| `rectangle` | `rectangle({x, y, size}) → Rectangle` | A rect; `x`/`y` top-left (px), `size` a scalar (w=h) or `{w, h}`. |
| `path` | `path() → Path` | An empty path (logical px); painted by `fill`. |
| `text` | `text(str, {font, size, weight, x, y}) → Text` | Lay out a string; the ink box's top-left at `x`/`y` (default 0). `size`: px. `weight`: font weight (default 400). Painted by `fill`. |

### Image fields and methods

| op | signature | meaning |
|---|---|---|
| `clone` | `img:clone() → Image` | A copy sharing `img`'s buffer (cheap); the copy diverges from `img` (copy-on-write) on its first in-place write. The branch point. |
| `autocontrast` | `img:autocontrast()` | Histogram-stretch to full range, **tone-preserving** (no highlight/shadow clip). In place. |
| `invert` | `img:invert()` | Invert every pixel's color: the unpremultiplied channels become `1 − c`, the alpha is unchanged. In place. |
| `resize` | `img:resize({width, height, mode, filter})` | Scale to the target (logical px): exactly one of `width`/`height` — aspect preserved; both — the target box, `mode = "cover"` covers it and center-crops, no `mode` scales to it exactly; `filter = "lanczos"` or `"cubic"` (default). In place. |
| `blend` | `img:blend(other, {at, color_space})` | Alpha-composite `other` over `img`, `other`'s top-left at `at` (a `Rectangle`, default `(0, 0)`), clipped to the receiver. `color_space`: `"linear"` (default) / `"srgb"`. In place. |
| `fill` | `img:fill(region, color)` | Paint `region` (a `Rectangle`, `Text`, or `Path`) with `color` (a `Color`, an `Oklch`, or a hex string). The space is chosen by the region's kind: a `Text` region composites in sRGB, a `Rectangle` or `Path` region in linear. In place. |
| `luma_in` | `img:luma_in(region) → number` | Mean luma of `region` (a `Rectangle`), 0–255 (see *Luma*). |
| `dominant_in` | `img:dominant_in(region) → Color` | The dominant color of `region` (a `Rectangle`): the demultiplied linear channels, clamped to 0–1, quantized to 4 bits, the most-populated bucket weighted by alpha, returned as that bucket's alpha-weighted mean. An empty or fully transparent region is transparent. |
| `pixel` | `img:pixel(x, y) → Pixel` | The pixel at `(x, y)` (logical px) as a `Pixel` (sRGB, 0–255, § Pixel); nil outside the image. |
| `width` | `img.width → number` | The image's width in logical px. |
| `height` | `img.height → number` | The image's height in logical px. |
| `physical_width` | `img.physical_width → number` | The image's width in physical px (the run scale). |
| `physical_height` | `img.physical_height → number` | The image's height in physical px (the run scale). |
| `size` | `img.size → {w, h}` | The image's logical dims as a `{w, h}` pair — a convenience for `rectangle`'s `size`. |

### Text, Rectangle, and Path

| op | signature | meaning |
|---|---|---|
| `x` | `text.x → number` | The ink box's x in logical px. |
| `y` | `text.y → number` | The ink box's y in logical px. |
| `width` | `text.width → number` | The ink box's width in logical px. |
| `height` | `text.height → number` | The ink box's height in logical px. |
| `bounds` | `text.bounds → Rectangle` | The tight ink box at the laid-out position. |
| `translate` | `text:translate({x, y})` | Move the ink box by a logical offset. In place. |
| `move_to` | `text:move_to({x, y})` | Move the ink box to a logical point (`x`/`y` default 0). In place. |
| `padded` | `rectangle:padded(n) → Rectangle` | The rect grown by `n` px on all sides; `padded({vertical, horizontal})` grows top/bottom and left/right; `padded({left, top, right, bottom})` grows each side (a missing side is 0). |
| `move_to` | `path:move_to({x, y})` | Start a subpath at a logical point (`x`/`y` default 0). In place. |
| `line_to` | `path:line_to({x, y})` | Extend the current subpath with a straight line to a logical point (`x`/`y` default 0). In place. |
| `close` | `path:close()` | Close the current subpath. In place. |

### Color methods

| op | signature | meaning |
|---|---|---|
| `with_luma` | `color:with_luma(luma) → Color` | A new `Color` whose luma (see *Luma*) is `luma` (0–1): the linear channels scale proportionally (the hue is preserved) and clamp — a target the hue can't carry falls short, and black stays black. |
| `to_oklch` | `color:to_oklch() → Oklch` | The color in Oklch (see *Oklch*): lightness, chroma, hue in degrees, alpha. The inverse of the `Oklch` transform that `fill` applies. |

### Pixel

A `Pixel` is the image's 8-bit boundary form: the four sRGB channels
(0–255) — the stored linear value through the sRGB encode (the
boundary's single rounding).

| op | signature | meaning |
|---|---|---|
| `r`, `g`, `b`, `a` | `pixel.r → number` | A channel of the `Pixel` (0–255) — `img:pixel(x, y).r` is the `r` of that pixel. |

### Output

| op | signature | meaning |
|---|---|---|
| `yield` | `yield(frame)` | Emit one frame. The composition's frame sequence is the output: one frame is a still (a PNG), a sequence a video (an MP4); the first frame is always written as a PNG. All frames of a run share the first frame's dims — the engine asserts it. |

## Execution model

- **In-place ops; one explicit copy.** The pixel ops (`autocontrast`,
  `invert`, `resize`, `blend`, `fill`) mutate the receiver in place
  and return nothing. `clone()` is the model's only copy and the
  branch point: fork the base, paint on it, `clone()` to start a
  fresh one.
- **No hidden canvas.** The running composite is a value the script
  threads explicitly (`base` → `output`). There is no global state.
- **Streaming.** `stream` is lazy. The `for` loop over it is the
  sole driver: the engine produces exactly one frame per iteration,
  on demand. Nothing else advances the stream.
- **Watch.** `--watch` reruns on change: after the first run, the
  engine watches the script's directory recursively and reruns the
  whole script on a content change to any `.lua` file in it — the
  script itself or a module it `require`s. A rerun reuses the first
  run's scale, output, and arguments; a failure is reported and the
  watch continues.

## Color space

- **`blend` defaults to linear light; `fill` chooses by the region's
  kind** — a `Text` region composites in sRGB (text is sRGB, always),
  a `Rectangle` or `Path` region in linear. For proper color handling,
  artwork composites in linear light by default. The buffer holds
  **premultiplied** RGBA in linear light (0–1): the linear composite
  is `layer + base·(1 − layer.a)` — no division, and a 0 or 1 alpha
  on either side falls out of the formula exactly. A fully
  transparent pixel's color is discarded with the alpha — the ops
  defined on stored channels (the luma, the autocontrast histogram) read
  such a pixel as colorless; exact while the region is opaque.
- **`text` is sRGB, always.** Type is authored in sRGB; no opt-out.
- **Opt-in sRGB.** `color_space = "srgb"` switches a `blend` to sRGB
  compositing — for layers authored in sRGB (a logo over the linear
  artwork, for instance).

### The linear transfer function

Linear light uses the **IEC 61966-2-1 sRGB curve** on the 0–1 scale
(evaluated directly — no lookup tables):

- Decode (sRGB `x` → linear `L`): `x/12.92` if `x ≤ 0.04045`, else
  `((x + 0.055)/1.055)^2.4`.
- Encode (linear `L` → sRGB `x`): `12.92·L` if `L ≤ 0.0031308`, else
  `1.055·L^(1/2.4) − 0.055`.

Stored pixels are 8-bit: encoding rounds to the nearest 1/255 step,
ties away from zero (Rust's `round` — the first implementation's
native rounding) — the one rounding a color value undergoes. The
8-bit decode/encode round-trip is exact: where a layer's alpha is 0
or 255 the linear result is byte-identical to an sRGB composite; the
difference is confined to partially transparent pixels.

### Luma

`luma_in` returns the **mean**, over the region, of the Rec. 709
luma `0.2126·R + 0.7152·G + 0.0722·B` per pixel (0–255). Thresholds
on the mean are a *script* decision — the engine only supplies the
mean luma.

### Oklch

`Oklch` is a color in the cylindrical Oklab space:
`l` lightness (0–1), `c` chroma, `h` hue in degrees (0–360), `a`
alpha (0–1). All four fields are readable and settable; `to_oklch`
returns `h` canonicalized to 0–360.

`fill` transforms an `Oklch` to a linear `Color` before painting (the
inverse of `to_oklch`). Out-of-gamut chroma yields linear channels
outside 0–1 (linear light is unbounded); the encode and the
bounded-domain ops (`dominant_in`) clamp to the gamut.

## Units

- **Logical px.** Every value in the script — positions, sizes, font
  sizes, `rectangle` and `resize` targets, `padded` amounts — is in
  logical px at the run's scale. The engine holds **no canvas**: the
  script decides its dims (its own values — a top-level
  `width = 1640`, say), and the run's `--scale` (default 1) is the
  caller's only knob. The engine performs the logical → physical
  transform internally (each dim × the scale, rounded to the px);
  the script never scales. One script renders at every run scale.
- **Physical px.** An image's `physical_width`/`physical_height`
  fields report its dims in physical px at the run scale — the only
  physical-px interface. A frame producer renders in physical px and
  takes them as its viewport; every other op takes and reports
  logical px.
- **Fractions.** A position may be a fraction of a dim
  (`height * 0.72`).
- **Coordinates.** Origin top-left; +x right, +y down. The top edge
  is `0`, the right edge the image's width, the bottom edge its
  height.

## Invariants

The engine's contract is its own design — not a match against any
other implementation. A conforming engine is correct by
construction and deterministic; it is not byte-identical to another
conforming engine, because some ops are deliberately free.

- **The pinned math is exact.** The IEC 61966-2-1 sRGB transfer
  function, the Porter-Duff `over` composite, the Rec. 709 luma
  weights, the autocontrast intent (full-range, tone-preserving
  stretch), and the single encode rounding are mathematical: a
  conforming engine gets them right up to integer rounding. The
  pinned ops must not drift.
- **Deterministic.** The same inputs, parameters and run scale yield
  the same frame pixels, every run.
- **The free ops may differ.** The resampling/resize (any scaling),
  the font rasterization (hinting/antialiasing), and the encoders
  (PNG, MP4) are free: a conforming engine may use any
  implementation — that is where two implementations legitimately
  differ.

