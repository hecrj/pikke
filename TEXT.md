# Phase 7 plan — real text on `parley`

Status: all decisions settled. Rasterization is **deferred to `fill`** —
`text` only lays out (plus a pixel-free ink probe); `fill` rasterizes and
composites.

## 1. The stack swap

`parley 0.12` replaces cosmic-text for **layout/shaping**; rasterization falls to
the crate the engine already runs — **vello_cpu** — fed by **skrifa** outlines.
All pure Rust, no fontconfig (the PLAN §2 ruling holds; doc names update).

| role | crate | what we use |
|---|---|---|
| layout, shaping, line/bidi, variation resolution | `parley 0.12` (`default-features = false, features = ["std"]` — default pulls `system`/fontconfig, off) | `FontContext`, `LayoutContext`, `RangedBuilder`, `Layout`, glyph runs |
| font file loading | `fontique 0.12` (same feature cut; re-exported by parley) | `Collection::register_fonts(Blob<u8>)` → family |
| outlines at (size, variation) | `skrifa 0.44` (same cut; parley's own outline source) | `FontRef::from_index`, `OutlineGlyphCollection`, `OutlineGlyph::draw(pen)` |
| coverage | `vello_cpu 0.3` (already in the tree, the 6d rasterizer) | `fill_path(&BezPath)` with the pinned `AlphaColor::<Srgb>::BLACK` + `Pixmap` alpha — the 6d `fill_rect` mechanics, paths instead of rects |

No swash (its `Rasterizer` is gone in 0.2.x; cosmic-text 0.19 dropped it too).
Shaping is harfrust inside parley_engine — **default mode** (full GSUB/GPOS on
the single loaded font): kerning on, no reordering for our LTR Latin, no
fallback possible (the collection holds only our fonts). That satisfies the
settled "basic shaping, swash-kerns-by-default" ruling.

## 2. Verified API facts (read from source, not docs)

- **Registration:** `FontContext::default()` (empty collection, no system
  scan). `collection.register_fonts(Blob<u8>, None) -> Vec<(FamilyId, Vec<FontInfo>)>`
  — we read the bytes ourselves (`fs::read`, io error → user-facing),
  `Blob: From<Vec<u8>>`; empty result ⇒ not a font. Family name:
  `collection.family_name(FamilyId)`. Font bytes stay owned by the collection
  (reference-counted `Blob`).
- **Scale:** `layout_cx.ranged_builder(&mut fcx, text, scale, quantize: bool)` —
  the builder's `scale` **multiplies the font size at style resolution**
  (`FontSize(v * scale)`), so all layout units are device px at the run scale.
  `quantize = true` (decision 1): layout coordinates align to pixel
  boundaries — parley rounds the **block axis** (line height,
  ascent/descent/leading, line y, baseline and vertical offsets) to whole
  device px; glyph x advances stay subpixel (the inline axis is not
  quantized). Probed at the copyright size (20 px): baseline exactly 24.0,
  glyph y's 24.0, x's fractional (17.24, 26.9, …) — the small text's baseline
  phase is pixel-aligned regardless of the anchor, so its vertical AA does
  not depend on placement.
- **Style:** `push_default(StyleProperty::{FontFamily(FontFamily::named(&family)), FontSize(size), FontWeight(FontWeight::new(weight))})`.
- **Layout:** `builder.build(&text) -> Layout<()>` — the `Layout` **owns** its
  data (font blobs copied in; the context borrows are short — the shaping LRU
  lives in the `LayoutContext`, so sharing it across ops caches repeated
  per-frame layouts). `break_all_lines(None)` (no wrap — one line),
  `align(Alignment::Start, AlignmentOptions::default())`.
- **Glyphs:** `line.items() → PositionedLayoutItem::GlyphRun` →
  `glyph_run.positioned_glyphs() → Glyph { id: u32, x, y, advance }` — `x` along the
  baseline, `y` from the baseline (y-down), layout origin = line-box top-left,
  device px. `run.font_size()` (device px), `run.font() → FontData { data: Blob, index }`.
- **Variation:** `run.normalized_coords() → &[parlance::NormalizedCoord]`
  (16.16 bits) — populated when the file is variable, empty when static.
  Conversion: `skrifa` prelude `NormalizedCoord::from_bits(c.to_bits())` →
  `LocationRef::new(&coords)`.
- **Outlines:** `skrifa::FontRef::from_index(data, index)` →
  `OutlineGlyphCollection::new(&font)` → `.get(GlyphId::new(id))` →
  `.draw((Size::new(run.font_size()), location), &mut pen)` —
  `pen: OutlinePen { move_to, line_to, quad_to, curve_to, close }` (f32,
  font-size px, **y-up**, origin at the glyph origin). The same `draw` call
  serves both phases with different pens: at `text` time a min/max pen
  (the ink probe — outline traversal only, no pixels, no path storage); at
  `fill` time a path pen building a `BezPath` — per-glyph device transform
  `Affine::new([1, 0, 0, −1, g.x, g.y])` (y-flip + translate) →
  `BezPath::apply_affine` (kurbo 0.13 via `vello_cpu::kurbo`) →
  `context.fill_path`. `AdjustedMetrics` (draw's result) carries only
  `lsb`/`advance_width` — no bbox, hence the probe pen.
- **Probe (run this session):** "Hello" @100 Oswald wght 300 → baseline 119.3,
  upright, kerned, `normalized = [NormalizedCoord(−10240)]`; "Copyright ©
  PIKKE" @60 AireExterior → static (`normalized = []`), the © glyph
  rasterizes correctly; parley↔skrifa advances agree to <0.01 px. ASCII dumps
  of both PPMs looked right.

## 3. Op design

**`font(path)`** — `font.rs` becomes real.
`install(&lua, &env, &script_dir)` (new param: `script_dir` for path
resolution). The font database is a process-lifetime static in `font.rs` —
the home of the `Font` noun:

```rust
static FONTS: LazyLock<Mutex<FontContext>> =
    LazyLock::new(|| Mutex::new(FontContext::default()));
```

(`LazyLock` because the `Default`s are not const.) Closure: join +
`fs::read` (error: `failed to load {path}: {error}`, the image op's shape) →
`FONTS.lock()` → `register_fonts` (empty ⇒ `anywho!` "not a valid font") →
return `Font { family: String }` (UserData `__name "Font"`).

**`text(string, {font, size, weight, x, y})`** — `text.rs` becomes real.
`install(&lua, &env, viewport)`. `font` and `size` are required; `weight`,
`x` and `y` are optional, defaulting to `400`, `0` and `0`. Binding
validates (RuntimeError, fill-binding style): `text: 'font' is missing or
not a `Font``; `text: '{name}' is missing or not a number`;
`text: 'size' must be positive`; a supplied non-finite `x`/`y` ⇒ error
(decision 7). Then the pure constructor
`Text::new(family, size, weight, x, y, scale, string) -> Text` (the
"constructor returns the built value" shape; the shared contexts are the
statics, so nothing engine-level enters the signature). **Layout only — no
rasterization:**

1. empty string ⇒ degenerate Text early (decision 6);
2. lock scope — `FONTS.lock()` then `LAYOUTS.lock()` (the only place both
   are held; fixed order, no deadlock), build/align (styles as §2) — the
   `Layout` owns its data (the font blobs are refcounted inside it), so
   the guards drop before the probe and the layout itself moves into the
   value;
3. **Ink probe (pixel-free):** over the layout's glyphs — the one walk in
   `text.rs` that the `runs`/`Run::glyphs` iterators also expose — per
   glyph
   `draw((Size, location), &mut BboxPen)` — a min/max pen over the outline
   (the §2 outline flow with a tracking pen — outline traversal only):
   every pen point (on- and off-curve — a curve stays inside its control
   hull, so the hull's bbox bounds the ink within a subpixel);
   layout-space corner `(g.x + pen_min_x, g.y − pen_max_y)` (the y-flip);
   the union over all glyphs ⇒ ink `(min_x, min_y, max_x, max_y)` (f32,
   layout space);
4. **Anchor (pinned):** `A = (round(x·scale), round(y·scale))` (the
   `to_physical_signed` math); the ink box (device px, canvas origin) =
   `(A.x, A.y, round(max_x − min_x), round(max_y − min_y))`; store
   `ink_min = (min_x, min_y)` — `fill` places the layout at
   `A − ink_min`, so the ink's min corner lands **exactly** at the anchor;
5. no glyphs (an unresolvable family) ⇒ degenerate: 0×0 box at `A`, the
   layout as-is (the `glyphs` walk yields nothing).

**`Text` value:** `{ x: i32, y: i32, width: u32, height: u32, scale: f32,
ink_min: (f32, f32), layout: Layout<()> }` — parley's owned `Layout` stored
directly (owned data, `Clone`, no lifetimes — the statics' locks are long
gone; the layout carries its own font blobs, refcounted). The degenerate
cases store `Layout::new()` ("creates an empty layout" — zero lines, zero
runs) — no `Option`. No pixels, no color (decision 2 — the paint arrives
via `fill`).

**`Text::runs(&self) -> impl Iterator<Item = Run>`** — the walk over the
stored layout (`lines() → items() → the GlyphRun items`), each `Run`
holding parley's `GlyphRun` handle; **`Run::glyphs(&self) ->
impl Iterator<Item = Glyph>`** — the run's `positioned_glyphs()`,
yielding
`Glyph { font: &FontData, size: f32 (device px),
location: &[parlance::NormalizedCoord], id: u32, x: f32, y: f32 }` — all
borrowed from the stored layout (zero allocations — no clones, no
per-run `collect`; the `Run` handle is the stable referent the
borrows lean on). Parley slices runs where the
font/size/weight changes (one run for our single-font op) and the fields
are exactly what the §2 outline flow consumes, `location` among them being
the weight the layout shaped with. One private walk, shared by the ink
probe (step 3) and the method. `bounds() → Rectangle` =
`(x/scale, y/scale, w/scale, h/scale)` in logical px (replaces the stub).


**`fill` upgrade** — `image.rs`'s Text branch (the 6d no-op) calls
`Image::fill_text(&text, paint)`. Zero-area box ⇒ no-op. Otherwise
**rasterization happens here** (the 6d vello mechanics):

- scratch `width + 2 × height + 2` (a 1 px AA margin all round; decision 5),
  its origin at canvas `(box.x − 1, box.y − 1)`;
- the layout origin in canvas space: `O = (box.x − ink_min.x,
  box.y − ink_min.y)` (f32 — `A` is the box's top-left);
  `set_transform(Affine::translate(O − scratch_origin))`;
- walk `text.runs()` → `run.glyphs()`; the rasterizer rebuilds
  `FontRef::from_index` +
  `OutlineGlyphCollection` + the skrifa `LocationRef` (from the run's
  parlance bits — a small per-glyph `Vec`, variable fonts only) for each
  glyph (the rebuild is a table lookup, negligible against the outline's
  `draw`); then the path pen + `apply_affine` + `fill_path` (§2);
- render, then composite: each scratch pixel with `cover = a/255 > 0` maps
  to canvas `(box.x − 1 + sx, box.y − 1 + sy)`, **clipped to the box** (the
  ≤1 px AA fringe beyond the geometric ink box is dropped — the box is
  exactly the painted region and what `bounds()` reports) and to the
  canvas; the layer is built premultiplied in the linear space
  (`paint·cover`) and composited with
  `base.linear_to_srgb().over(layer.linear_to_srgb()).srgb_to_linear()`
  — the `over` is evaluated in the pinned sRGB space, and the symmetric
  conversion round-trips exactly (`srgb_to_linear ∘ linear_to_srgb` is the
  identity), so the stored value is exactly the premultiplied linear of
  `paint·cover` (at full coverage the hex's own linear bytes;
  `paint.linear_to_srgb()` is exactly the
  hex bytes).

Cost: `text` pays layout + outline traversal (no pixel work); `fill` pays
the pixels. The script calls each once per frame — the same total work as
rasterizing at `text` time; the split keeps `Text` a pure geometry value
(like `Rectangle`) with `fill` as the only rasterizing op.


## 4. Engine wiring

No per-run state: the shared contexts are process-lifetime statics, each in
the home of its noun —

- `font.rs`: `static FONTS: LazyLock<Mutex<FontContext>>` (the font
  database; loaded by the `font` op, shaped from by the `text` op);
- `text.rs`: `static LAYOUTS: LazyLock<Mutex<LayoutContext>>` (the layout
  scratch + shaping LRU; the `Layout` it produces is owned, so the guard is
  held only for build/align).

`LazyLock` because the `Default`s are not const. `std::sync::Mutex` (not
`RefCell`) — the process-lifetime instances outlive any per-run `Rc`
scheme and are safe if frames ever run in parallel. Contention is a
non-issue today (single-threaded run) and stays one: text layout is
relatively fast next to rasterization and the other image ops, so even
parallel frames would serialize on it briefly. The font database
accumulates across `render` calls; that is harmless (one `render` per
process invocation; the script registers each file once).

`engine.rs` gains nothing — `font::install` and `text::install` just get
their new parameters. `Cargo.toml`: + `parley`, `fontique`, `skrifa` (the
feature cuts above).

## 5. Tests (pinned)

Unit tests (6d style; the real fixture is the vendored font, decision 8 —
`tests/fonts/NotoSans-Regular.ttf`, read via
`concat!(env!("CARGO_MANIFEST_DIR"), …)`, no personal/ dependency):

- `font`: missing file ⇒ error; non-font bytes ⇒ "not a valid font" error;
  the vendored Noto Sans ⇒ a `Font` whose family is `Noto Sans`;
- `text` binding: missing/mistyped options ⇒ the exact errors (a
  `Font { family }` built in-crate); omitted `weight`/`x`/`y` ⇒ the defaults
  (400, 0, 0) flow through — with an unregistered family the degenerate box
  lands at the (0, 0) anchor;
- `text` layout (the vendored font, e.g. "Noto" @100): a non-degenerate ink
  box, its top-left exactly the anchor (pinned); an unregistered family ⇒
  the degenerate box (probed: parley lays out zero width, no glyphs, no
  panic);
- `fill_text` (a real `Text` from the vendored font, e.g. "N" @100): the
  exact hex bytes at solid-ink pixels (the pinned sRGB round-trip), correct
  AA blends at edge pixels, the painted region ⊆ the ink box, outside the
  box untouched; a zero-area box is a no-op; a box crossing the canvas
  corner clips.
- `bounds`: synthetic `Text` at scale 0.5 ⇒ logical px half the device box.

The personal/ fonts stay the gate (the variable Oswald, the static
AireExterior); the vendored Noto Sans carries the unit tests.

## 6. Gate (PLAN §7, reworded for parley)

The script's two texts (Oswald variable 300, AireExterior static 700):

- the ink box's top-left exactly `(x, y)` (pinned at `text` time —
  copyright at (50, height()−50), title at (50, height()·0.72), in logical
  px: `bounds()` reports the anchor, and the rendered ink reaches the box
  corner with nothing painted outside the box);
- the rasterized ink matches the box (the outline ink probe) in size within
  ~1 px (a one-off probe mirroring `fill`: same stored layout, the painted
  support measured independently);
- the rasterized text clean at its size (free — PIL/eye: the 20 px Light
  caption legible, the 150 px title crisp);
- `fill(copyright, #FFF/#000)` paints the real ink (exact hex, correct AA) and
  the `title:bounds():padded(30)` plate is intact.

`cargo fmt --check` + `cargo lint` clean.

## 7. Docs

PLAN updates:

- §3 file map — `font.rs`: the face registered in the fontique collection
  (the file's bytes, the path relative to the script); `text.rs`: the parley
  layout at the run scale, the pixel-free ink probe, the ink box, `bounds`
  — `fill` does the skrifa outlines → vello rasterization;
- the dependencies line — parley/fontique/skrifa (+ the existing vello role);
- the Phase 7 text;
- the "free ops" bullet — skrifa-outlines→vello;
- the unhinted-small-text risk note — skrifa draws unhinted by default; the
  4×-supersampling fallback ruling stands;
- the pure-Rust bullet — `parley/fontique/skrifa`.

DESIGN.md / AGENTS.md: no text-stack mentions, untouched.

## 8. Decision points

1. **`quantize = true`** — settled (enabled). Parley rounds the block axis to
   whole device px (line metrics, baseline, vertical offsets); glyph x
   advances stay subpixel. The small text (the copyright line) gets a
   pixel-aligned baseline phase regardless of the anchor — probed: 20 px →
   baseline 24.0, glyph y's 24.0, x's fractional.
2. **`Text` stores no color or pixels** — settled (confirmed): the layout +
   ink box only; rasterization and paint both come from `fill` (the script
   confirms: no color in the `text` opts).
3. **Required vs optional `text` opts** — settled: `font` and `size`
   required; `weight` optional, default `400` (regular); `x` and `y`
   optional, default `0`. Missing ⇒ default, wrong type ⇒ error.
4. **Shared contexts are one process-lifetime `static` `Mutex` each** —
   settled: `FONTS` in `font.rs`, `LAYOUTS` in `text.rs` (`LazyLock`);
   layout ≪ rasterization, so contention stays unlikely if frames are ever
   parallelized.
5. **The `fill` scratch = the ink box + a 1 px AA margin** — settled (the
   rasterization moved to `fill`, so the scratch is sized to the box, not
   the layout).
6. **Empty string ⇒ degenerate Text early** (no layout call) — settled —
   deterministic edge.
7. **Supplied non-finite `x`/`y` ⇒ error** (a script bug at the op's core) —
   settled; degenerate *regions* in `fill` stay no-ops (the 6d ruling).
8. **Vendor Noto Sans as the test fixture** — settled:
   `tests/fonts/NotoSans-Regular.ttf` (the static 607 KB face, SIL OFL 1.1 —
   the license text is vendored alongside) + the OFL notice. Unit tests
   shape against it; the variable-font path (wght → normalized coords) stays
   gated on personal/ (Oswald).
9. **`Text` stores parley's `Layout<()>` directly and exposes `runs`
   (each a `Run` holding parley's `GlyphRun` handle) whose `glyphs`
   yields borrowed `Glyph` headers** — settled: no flat `Vec` of runs
   (a re-allocation to move data around); the walk over the stored
   layout is allocation-free (no clones, no per-run `collect`);
   `Layout::new()` (empty) covers the degenerate cases — no `Option`;
   the layout's memory is negligible against megabyte canvases.

