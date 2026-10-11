# Visual language v3 — "a professional DCC viewport, and a hologram in it"

Subject: a math-for-AI textbook for a CG/VFX developer. Reference for UI/UX (user requirement): **Blender** — a
professional DCC look, not "edtech". The reader's home turf is a viewport: navigation gizmo, X red / Y green / Z blue,
an N-panel of draggable number fields, a status bar that tells you what the mouse does, an orange outline on what is
selected, and a light whose brightness *is* a dot product. The course's thesis starts there (N·L), so the identity is built
from that world. Signature illustrations: `signature-illustrations.md` (everything is light, shadows and areas).

**v3 (2026-10-10): the viewport's rendering is "holographic"** — the user's chosen look, reference image
`refs/holo-style-ref.png`, reproduced **as is**: a deep-space background (starfield, colourful nebulae and spiral galaxies as in the reference) with a vignette,
geometry as glowing light filaments, translucent glass faces with a fine inner grid and brighter edges, technical rulers
and dials as HUD instruments at the sides, measurement arrows and tick marks along measured sides, a framed glowing
formula plate, and in 3D a dark reflective floor the figure floats above. A secondary **clay** look (`refs/clay-style-ref.png`:
soft matte plastic tiles, pastel, studio light, contact shadows) is defined for countable solids (§Clay). The Blender
chrome, interaction and semantics (term ids, hot outline, hatch for negatives, overlays, N-panel, status bar) are unchanged.

Single job of every scene: make one relation visible so clearly that the formula next to it feels obvious.
Every glow is honest: a filament glows because it *is* the vector/side; a face glows at its edges because that is where
its boundary is; nothing glows to decorate. The background is the only purely atmospheric layer, and it sits far below
every data pixel in contrast (table below).

## Signature

**Every scene is a small, real DCC viewport; inside it, the math is a hologram.** Blender-grade chrome (header, gizmo,
N-panel, status bar) frames a near-black viewport with a quiet starfield; vectors and sides are glowing filaments
with a soft bloom; areas are glass plates with an inner grid; rulers and dials at the edges read like instruments; the
3D floor mirrors the figure; the lit sphere computes brightness as exactly `max(0, N·L)`. The prose around it stays quiet.

## Viewport anatomy (Blender-like) — unchanged from v2

```
┌ header ─────────────────────────────────────────────────────────────┐
│ ◧ 2D ◫ 3D │ Persp/Ortho │ Overlays ▾ │ Reset view        LC-1 · Areas│
├─────────────────────────────────────────────────────────┬───────────┤
│ ruler                                          (gizmo) │ N-panel   │
│ │      viewport (space bg, glow filaments)      X Y Z  │ a  [3.00 ]│
│ │      glass faces, floor mirror           dial ◎      │ θ  [60.0°]│
│ │      ┌ formula plate ───────────┐                    │ a·b 4.50  │
├─────────────────────────────────────────────────────────┴───────────┤
│ status: LMB drag tip · Shift 1.0 steps · MMB orbit · Ctrl+Wheel zoom │
└──────────────────────────────────────────────────────────────────────┘
```
- **Header** (28 px): view toggles, Overlays popover (grid, legs, labels, shadows, angles, **rulers**, **floor**,
  **instruments** — each scene declares which exist), Reset view, size buttons **Enlarge 2×** (Ctrl+Space, like Blender's
  Toggle Maximize Area) and **Full screen** (Ctrl+Alt+Space / F; Esc exits), scene name right-aligned in muted mono.
- **Navigation gizmo** top-right (Blender position): DOM/SVG gizmo (`Gizmo.tsx`, see AGENTS.md 6d for why not drei's),
  circles with X/Y/Z letters, negative axes as hollow circles; click to align (3D); hidden in 2D scenes (2D shows a tiny
  static XY corner instead).
- **N-panel** (right, 200 px, collapsible with `N` key and a chevron): Blender-style property rows — label left, value
  field right; fields are drag-to-scrub (horizontal drag, Ctrl = snap, Shift = fine), click to type, arrow keys; read-only
  values render as flat rows. These rows are the linked "numbers view" (term ids → hover lights scene + formula).
  At < 720 px the N-panel becomes a row of chips under the viewport.
- **Status bar** (22 px): context hints for what the pointer is over ("Drag tip of a", "MMB orbit"), like Blender's.
  Touch devices get touch hints (drag, two-finger pinch), never mouse buttons.
- **Wheel**: a plain wheel always scrolls the lesson page, never the scene (a card must not trap reading);
  Ctrl/Cmd+wheel and pinch zoom.
- **Selection/hot**: Blender's active outline — a 2 px light-orange outline + halo on the hovered/linked object;
  never changes the object's own color (color = meaning, outline = attention).
- **Tooltips**: Blender-like, delayed 500 ms, title + one-line explanation + the formula term.

## Tokens (v3)

The viewport and its chrome are **always dark**, in both site themes (like an embedded Blender viewport or a code
block): colors are calibrated once against one background and stay exact. Page prose, formulas and cards outside the
viewport follow the Starlight theme. Every value is a literal `#rrggbb` (WebGL needs concrete colors); alphas are
separate numbers in `STYLE` (palette.ts), never baked into the hex.

### Viewport, chrome, background

| role | value | use |
|---|---|---|
| `vp-bg-top` / `vp-bg-bottom` | `#0C1220` / `#05070F` | viewport vertical gradient (deep space navy → near black) |
| `vignette` | `#02030A` | radial darkening toward the corners (0 % at center, 55 % alpha at corners) |
| `floor` | `#070A15` | 3D floor plane base color (under the mirror layer) |
| `chrome` / `chrome-2` | `#2B2B2B` / `#232323` | header, N-panel, status bar — unchanged (Blender grey; the neutral grey frame makes the cool viewport read as "screen inside a tool") |
| `field` / `field-hover` | `#545454` / `#656565` | number fields (Blender widget grey) |
| `grid` / `gridMinor` | `#1E2D4D` / `#141C30` | drei shader Grid; cool blue-grey so it belongs to the hologram, not to the chrome (1.37:1 / 1.10:1 on `vp-bg-top`: visible, quiet, always below every data stroke) |
| `star` | `#C9D6F2` | starfield points (alpha 0.25..0.9 per star, see Background) |
| `nebula-a` / `nebula-b` | `#1B2A5A` / `#3A1E4E` | nebula tint colors (max alpha 0.18, blurred blobs) |
| `ink` / `muted` | `#E9EEF8` / `#93A0B8` | labels, tick numbers, chrome text |
| `plate` | `#0B1A33` | formula/HUD plate fill (alpha 0.55) with `vecA` edge at 60 % |

### Data colors (strokes, faces, points)

| role | value | use |
|---|---|---|
| `axisX` / `axisY` / `axisZ` | `#FF3352` / `#8BDC00` / `#2890FF` | **Blender's axis colors, kept.** Justification: the gizmo, the Blender muscle memory of the reader and the component legs (`a_x` red, `a_y` green) are the course's bridge to his DCC; recoloring them to holo-cyan would make the gizmo lie about the viewport. They are drawn as filaments like everything else, so they still belong to the look. Contrast 5.2 / 11.0 / 5.8 : 1 on `vp-bg-top`. |
| `vecA` | `#4FD8FF` (holo cyan) | vector a, sides, the "default" filament color of the reference |
| `vecB` | `#D85CC8` (magenta) | vector b; the reference's magenta rectangles |
| `vecC` | `#3DF0B4` (mint) | results: a−b, a+b, projections, c |
| `light` | `#FFE27A` | lamps, the N·L light |
| `hot` | `#FFAA40` | active outline/halo (Blender selection orange); distinct from every vector color |
| `neg` | 45° hatch of the owning color | negative signed quantities (sign never color-only) |

### Contrast and color-vision check (method and result)

Method: `scratch contrast.ts` (to become `tests/js/tokens.test.ts` extensions in 6h): WCAG 2.x relative luminance for
contrast; color-vision deficiency simulated with the Machado–Oliveira–Fernandes 2009 matrices at severity 1.0 in linear
RGB, then CIE76 ΔE in Lab between every pair of a group (a pair ≥ 15 ΔE is distinguishable side by side; ≥ 10 with a
label). Results (2026-10-10):

| check | result |
|---|---|
| every data stroke vs `vp-bg-top` / `vp-bg-bottom` | min **5.21:1** (`axisX`); vecA 11.2, vecB 5.6, vecC 12.8, light 14.6, hot 9.9, muted 7.1, ink 16.1 — all ≥ 3:1 |
| text vs chrome | `ink` 12.2:1, `muted` 5.4:1 (`chrome`), 6.0:1 (`chrome-2`) — all ≥ 4.5:1 |
| grid below every data stroke | `grid` 1.37:1, `gridMinor` 1.10:1 (gate: minor ≥ 1.08 and < major < 1.6) |
| {vecA, vecB, vecC, hot, light} min pair ΔE | normal 30.8 (hot/light), protan **18.4** (hot/light), deutan **17.0** (hot/light); vecA/vecB deutan 20+ after moving vecB from `#F07BE8` (7.9 ΔE, rejected) to `#D85CC8` |
| {axisX, axisY, axisZ, vecA} | normal 53, protan 40.6, deutan 35.3 |

Note: the weakest CVD pair is `hot` vs `light` (lamp color vs selection outline). They never compete for the same role
(light = a fill/lamp, hot = an outline + halo), and the hot halo pulses (§Motion), so it is also distinguishable by shape
and motion. Recorded as acceptable.

### Glow (filament) parameters

| layer | width | alpha | blending |
|---|---|---|---|
| core | 2 px | 1.0, color = role, **lightened 35 % toward white** in the center (`mix(role, #FFFFFF, 0.35)`), so the filament reads as "hot wire" like the reference | normal |
| inner glow | 7 px | 0.38 of role | additive |
| outer glow | 18 px | 0.10 of role | additive |
| hot outline | +2 px each side of the core, `hot` | 1.0 | normal (behind the core) |
| hot halo | 18 px | 0.30 `hot` (replaces the outer glow while hot) | additive |
| point | 6 px disc, core lightened as above, + 14 px additive halo at 0.3 | | |

Widths are pixel-stable (`SLine` resolution refresh per frame, as today). Additive blending on a near-black background is
exactly what a real emissive line does on a sensor: overlapping glows add, nothing is hidden. Over a glass face the inner
glow adds to the face fill, which is also physically what an emissive edge on glass does.

### Glass faces (areas)

| role | value |
|---|---|
| fill | role color at **0.12** alpha, plus a vertical gradient to 0.06 at the bottom of the face (shader uniform, keeps the "thin glass" feel) |
| inner grid | role color at **0.22** alpha, 1 px lines; **world units** aligned to the scene grid: 1 unit; subdivided to 0.5 while 1 unit ≥ 80 px; hidden while 1 unit < 14 px (then only the fill remains) — the grid *counts* the area (a 3×3 face shows 9 cells), that is why it is in world units, not px |
| edge | 1.5 px core (lightened as a filament) + 7 px inner glow at 0.30; the edge is brighter than the fill by construction |
| corner points | 5 px discs in the role color (reference: lit vertices) |
| negative | 45° hatch at 6 px / 1 px in the role color over the fill — unchanged; sign is never color-only |
| hot | hot outline on the edge; fill rises to 0.18 |

### Rulers, ticks, measurement arrows (HUD instruments)

- **Measured side ruler**: a side that carries a term in the current formula (a, b, c, b cos θ) gets ticks on its outside:
  major every 1 unit (8 px, `muted`), minor every 0.5 while 1 unit ≥ 40 px (4 px), labels at integers in **JetBrains Mono 11 px**
  `muted` while 1 unit ≥ 24 px. Declared per segment (`ruler` prop), never automatic for guide lines.
- **Measurement arrow** (dimension line): 14 px outside the side, thin 1 px `ink` line with 6 px end ticks and
  filled arrow heads, the value in the middle in mono (`3.00`), term id = the side's term (hover → hot in formula). It appears
  only for sides the lesson names ("width = b cos θ" in LC-0); it is the reference's double-arrow under the magenta strip.
- **Side instruments** (the reference's vertical rulers, dials, small panels at the left/right edges): DOM overlays in
  `.lab-stage-overlay`, SVG, `muted` strokes at 0.5 alpha, 1 px, so they sit below every data pixel. They are *real* readouts,
  not decoration: the vertical ruler is the y-axis scale of the current view (updates on zoom); the dial is the current angle
  (θ) or the orbit azimuth in 3D; the small panel is a 3-line mono readout of the hovered term (name, value, formula term).
  Declared in the Overlays popover as **instruments** (default on at ≥ 900 px, off below: at 360 px they would cover data).
- **Formula plate**: the lesson's linked `Formula` is rendered over the bottom of the viewport in a glass plate (`plate` fill
  0.55, 1 px `vecA` edge at 0.6 with 7 px inner glow, 10 px corner chamfers like the reference), KaTeX in `ink`. It is the
  same linked Formula (term ids, scrubbable numbers), only positioned inside the card; under 720 px it returns to the page flow.

### Reflective floor (3D)

- Floor plane at y = 0 in `floor` with a radial lightening toward the figure's footprint (`mix(floor, #0F1830, 0.5)` within
  1.5× extent) and the drei Grid on top (fading).
- Mirror: see §Technique; reflection drawn at 0.28 alpha, fading to 0 over 0.8× extent of depth below the floor.

## Clay look (secondary mode) — "things you can count"

Reference `refs/clay-style-ref.png`. Where it honestly helps: solids whose **count or size is the meaning** — unit tiles and
cubes (0.G counting, 0.2 powers: area = tiles, volume = cubes), tensor cells on the tensor road (3.1–3.5), the matmul light
box cells (S3), stacked ledger blocks when they are shown inside a viewport. Where it does not: vectors, angles, projections,
anything that is a *line* or a *light* — those stay filaments. So clay is a **material for solids**, not a second product:

- `look` is per object class, not per scene: `ClayTile`/`ClayCube` primitives render matte pastel bodies **inside the same
  dark holo viewport** (same chrome, same background, same floor). No light studio background in the viewport: the pastel
  bodies have ≥ 8:1 contrast on the space background (table below) and the mirror floor reflects them like everything else.
  A whole-scene light studio (`look: 'clay'` on the card: `#D6DAE1`→`#C4C9D2` background, no stars, grid `#B9BFC9`) exists
  **only for covers and the gentle Part 0 scenes where every object is a solid** (0.G, 0.2), and is a card prop so one card
  never mixes both backgrounds.
- Tokens: `clayA` `#9EDCB6` (mint), `clayB` `#8FC4F2` (sky), `clayC` `#A98CDF` (lilac), `clayAccent` `#F2994A` (orange,
  the "cut out"/correction tiles of the reference), `clayPlate` `#ECE9E3` (cream label plates), `clayInk` `#2A2E36` (text on
  plates, 11.2:1). On `vp-bg-top`: 11.9 / 10.1 / 6.7 / 8.4 / 15.4 : 1. CVD: min pair ΔE normal 25, protan 21.3, deutan 14.1
  (clayB/clayC after moving lilac from `#C3AEEA` (3.6 ΔE, rejected) to `#A98CDF`). Negative clay tiles = hatched top face +
  `clayAccent` rim, never color-only. Hot = the same orange outline around the body's silhouette.
- Lighting on a flat canvas (no scene lights, no tone mapping): **matcap** — one 256×256 PNG (≈ 6 kB, self-hosted, soft
  studio: key top-left, fill, slight rim) through `MeshMatcapMaterial` with the pastel as `color`; the body gets rounded
  bevels from geometry (`RoundedBoxGeometry` from three/addons, radius 6 % of the smallest side) so the matcap has edges
  to shade. Matcap is view-dependent (the light rides with the camera), which is fine for tiles and wrong for the N·L
  lamp — the lamp keeps its own shader. Rejected: a directional + hemisphere light (needs lights in every scene, flat canvas
  has no tone mapping so highlights clip) and drei `MeshReflectorMaterial`-style FBO tricks for clay (cost without meaning).
- Contact shadow: one **footprint sprite** per body — a radial-gradient quad (`#000000`, alpha 0.45 → 0, 1.4× the footprint)
  laid on the floor under it, moved with the body. For a flat tile on a plane this is what a blurred contact shadow is; no
  FBO, no blur passes, no per-frame cost. drei `ContactShadows` (depth render + 2 blur passes per invalidate) is rejected for
  the same reason as bloom (per-View FBO on a shared canvas).

## Type (self-hosted via Astro Fonts; no runtime font CDN) — unchanged

- UI chrome, headings: **Inter** (Blender 4.x ships Inter as its UI font) — headings Inter Display 650–700, tight tracking;
  chrome 12–13 px.
- Prose: **Atkinson Hyperlegible Next** — maximal legibility for long explanations.
- Numbers, fields, HUD, rulers, instrument readouts: **JetBrains Mono** (tabular figures, clear 0/O, 1/l).
- Math: KaTeX fonts.

## Technique: how the look is rendered on a shared canvas with Views

The constraints: ONE `<Canvas flat frameloop="demand">` per page, every Stage a drei `<View>` (scissored region, own
scene and camera), ~272 kB gz viz-core today with a 300 kB gate, mobile GPUs, exact positions (every pixel of a vector
is where the number says).

### Glow: multi-layer additive halo strokes (chosen) vs selective bloom postprocessing (rejected)

| | additive halos | postprocessing bloom (`postprocessing` + `@react-three/postprocessing`) |
|---|---|---|
| bundle | 0 kB (Line2 layers already exist: today's `Stroke` = halo + outline + core) | +~95 kB gz (postprocessing ~70, r3p ~25) → breaks the 300 kB gate |
| Views | each stroke draws inside its own View; nothing else to coordinate | EffectComposer owns the whole canvas; with N Views you need N composers each rendering to a canvas-sized FBO, then scissored copies — drei View + composer is a known-broken combination (the View's `size` is stale, the composer clears the canvas) |
| exactness | the core line is the geometry; glow is symmetric around it; nothing moves | bloom blurs across the threshold: thin lines bleed, positions read as "fuzzy", bright labels bloom too |
| GPU | 3 thin draws per stroke; fill rate = glow width × length | 2 full-canvas FBOs + 5–6 blur passes per frame, per View; on a demand loop each invalidate pays all of it; mobile tiles choke |
| honesty | glow alpha is fixed per role: a glow is a wire | threshold bloom lights whatever is bright (hatch, labels) |

Chosen: **additive halos** — `Stroke` gains a second glow layer and `AdditiveBlending` on the glow layers (`LineMaterial
.blending`), core lightened in the material color. The hot halo swaps for the outer glow (no 5th draw while hot).

### Background: starfield + nebula + vignette, per View, static

- One `Background` object per Stage, drawn first (`renderOrder` −10, `depthWrite` off): a fullscreen quad in the View with a
  tiny shader: vertical gradient `vp-bg-top`→`vp-bg-bottom`, radial vignette to `vignette`, nebulae and 1–2 spiral galaxies
  (`nebula-a/b` plus magenta/blue highlights, as bright and colourful as in the reference image; procedural spiral arms
  in the shader or a small compressed texture atlas, positions/radii from a per-scene seed; match the reference by side-by-side
  screenshot, not by a fixed alpha cap) and stars = `star` with a hash
  per cell (3 density layers, ~0.3 % of pixels, alpha 0.25..0.9, 1–2 px). No texture, no noise octaves, no time uniform:
  it is **static** (a twinkle would be idle animation; the reference is a still). Cost: one quad, ≈ 20 ALU per pixel.
  The quad follows the camera (fixed to the View's NDC), so zoom/orbit never moves the stars (they are "infinitely far").
- 2D scenes keep the same background (space is the identity, not a 3D effect); the drei Grid draws on top.
- Readability note for the user: stars at up to 0.9 alpha of `#C9D6F2` are brighter than `gridMinor` and can sit under a
  1 px hatch or a tick label; we keep them as the reference does. Mitigation without touching the field: labels get a 2 px
  `vp-bg-bottom` text-shadow (already how axis letters read on the grid), and stars are 1 px at ≤ 0.5 alpha within the
  central 60 % of the View, 1–2 px up to 0.9 toward the edges (a fixed radial law, so the field never changes with the
  data). The concern is recorded, not solved by removing stars.

### Reflective floor: mirrored geometry (chosen) vs `MeshReflectorMaterial`

| | mirrored geometry | drei `MeshReflectorMaterial` |
|---|---|---|
| what it is | the filament group drawn a second time under `scale(1,−1,1)` about y = 0 at 0.28 alpha, with a fade quad | a planar reflector: renders the scene again from the mirrored camera into an FBO (+ optional blur/depth passes), samples it on the floor |
| correctness | exact for a planar mirror (a mirror image *is* the reflected geometry); no resolution loss; lines stay pixel-stable | exact too, but at FBO resolution; blur is approximate |
| cost | second pass of ~20 thin lines; no FBO | 1 extra scene render + blur passes per invalidate, per View; FBO ≥ 1024² per floor on a shared canvas; multiple 3D Views = multiple FBOs |
| bundle | 0 | +~12 kB gz (material + BlurPass) |
| Views | nothing special | the reflector reads `gl.getSize` = whole canvas; needs per-View resolution work |
| what is reflected | only filaments, glass faces and clay bodies (what the lesson draws); the grid and background are not reflected — correct, they are "behind" the mirror plane | everything |

Chosen: **mirrored geometry**. Implementation: Stage wraps the scene children in a `Mirror` group when `floor` is on:
`<group>{children}</group><group scale={[1,-1,1]} userData.mirror>{children}</group>`; primitives read a `MirrorCtx` and
render at `STYLE.mirrorA` (0.28) with glows at half alpha and no labels, no handles, no hit testing (the registry ignores
mirror instances). A `FloorFade` quad (y just above 0, `vp-bg-bottom`, alpha 0 at the figure center → 0.9 at 0.8× extent)
dims the reflection with distance. Low-power / reduced effects: the Overlays flag `floor` is off when `html.lab-no-webgl`
is set or `navigator.hardwareConcurrency ≤ 4` on a touch device (recorded in AGENTS.md as the single "low-power" predicate;
reduced-motion does not touch it — the mirror is static).

### Glass faces

`Quad` (Parallelogram.tsx) grows a small `ShaderMaterial`: fill gradient + inner grid from the fragment's world-plane
coordinates (`fract(uv / step)` lines with `fwidth` AA; `step` from the world-unit rule above, uniform updated by
`useWorldPx`), hatch stays the existing line hatch. Edges are `Stroke`s (so they glow and take the hot outline).

## Motion

- Direct manipulation is 1:1 (never animated). Indirect changes (timeline, keyboard, snap, reset view) use the critically
  damped spring (~180 ms). Camera "Reset view"/gizmo align animate like Blender's smooth view (~250 ms).
- One orchestrated moment per lesson: the first scene draws itself once on first view (600 ms). Nothing idles: no star
  twinkle, no floor shimmer, no nebula drift.
- **Hot pulse**: while an object is hot, its hot halo alpha breathes 0.30 → 0.42 → 0.30 over 1.2 s (sine); a `useFrame` that
  calls `invalidate()` only while something is hot, so the demand loop sleeps otherwise. `prefers-reduced-motion`: no pulse
  (static 0.36), hard cuts everywhere.

## Covers (gpt-image-2.5-sunburst)

Cover = still image for a module/lesson card; it must show the lesson's actual geometry, not a mood. Two templates.

### Holo cover prompt template

```
Holographic technical diagram on a deep-space background: near-black navy (#05070F) with a faint vertical gradient to
#0C1220, a starfield of small white-blue stars, colourful spiral galaxies and nebulae (blue, purple, magenta) in the
corners and background exactly like a deep-space photograph, soft vignette.
Geometry drawn as glowing cyan (#4FD8FF) light filaments with a soft bloom; key faces as translucent glass plates with a
fine inner grid and brighter glowing edges; highlighted parts in magenta (#D85CC8); results in mint (#3DF0B4).
HUD instruments along the left and right edges: vertical rulers with tick marks, circular dials and target rings, small
framed panels showing a galaxy, a small planet in a ring instrument at the lower right.
A dark reflective floor below; the figure floats above it and mirrors softly in it.
At the bottom, a framed glass plate with chamfered corners containing exactly this text and nothing else: "<FORMULA>".
Exact geometry: <GEOMETRY — e.g. "a triangle ABC with C at bottom-left, B at bottom-right, A top-center; squares built
outward on each of its three sides; a dashed perpendicular from A onto side CB; a magenta rectangle inside the bottom
square whose width equals the length from C to the foot of that perpendicular; labels a, b, c on the sides, theta at C">.
No other text, no logos, no people, no extra geometric shapes. Clean, sharp, 3:2.
```

### Clay cover prompt template

```
Soft matte clay / plastic 3D illustration, isometric three-quarter view, light grey studio background (#D6DAE1),
one soft key light from the top-left, gentle contact shadows, slightly rounded edges, pastel colors:
mint (#9EDCB6), sky blue (#8FC4F2), lilac (#A98CDF), orange accent (#F2994A), cream label plates (#ECE9E3) with
dark embossed text (#2A2E36). Exact geometry: <GEOMETRY>. Labels exactly: <LABELS>. A cream plate at the bottom with
exactly this text: "<FORMULA>". No other text, no logos, no people, no background objects. Clean, sharp, 3:2.
```

### Checklist before a cover is used

1. Count every object named in `<GEOMETRY>`: present, none extra (no fourth square, no second triangle).
2. Topology: vertices/labels on the right sides (A/B/C order, which square touches which side), right-angle marks only at
   right angles, dashed lines are perpendicular where the text says so.
3. Signs/colors: magenta/orange only on the parts the prompt names as highlighted; nothing hatched that is positive.
4. Text: exactly the formula/labels we specified, correct glyphs (θ, superscripts), no invented words or numbers.
5. Holo: background, galaxies, HUD instruments and the formula plate as in the reference (docs/design/refs/holo-style-ref.png); no spaceship, no extra geometry; floor reflection matches the figure.
6. Clay: one light direction, shadows consistent, no text on the tiles except the given labels.
7. Reject and regenerate on any failure; never retouch geometry by hand (it must be regenerable).

## Layout of a lesson page — unchanged

```
eyebrow (mono caps): LESSON 4 · VECTORS
H1 (Inter Display)
hook paragraph (Atkinson)
┌ viewport card (header · viewport+N-panel · status) ┐   full column width, 16:10 viewport
└────────────────────────────────────────────────────┘
formula (linked)   |  timeline ◀ step 2/4 ▶            side by side ≥ 900 px, stacked below
```

## Self-critique (applied)

- v1 used an indigo "night" palette and a casual display face → replaced by Blender-grade neutral chrome and Inter, per the
  user's "professional, like a DCC" requirement; the viewport is always dark so the vector colors are calibrated once.
- vecB amber clashed with Blender's orange selection → orchid; vecA sky blue stays clear of the Z-axis blue by lightness.
- Bloom rejected (bundle, GPU variance, blurs exact values); halo strokes give the light feel without lying about geometry.
- Numbers appear only where order is real (lesson numbers, timeline steps).

### v2 → v3 changes

- Viewport background: Blender grey gradient `#3A3A3A/#282828` → deep space `#0C1220/#05070F` with vignette, static
  starfield and nebula (the user's chosen reference, reproduced as is). Chrome greys unchanged.
- Grid `#4A4A4A/#444444` → cool `#1E2D4D/#141C30` (re-tested: still below every data stroke, still visible).
- Strokes: 2.5 px core + one 9 px @ 14 % halo → 2 px lightened core + 7 px @ 0.38 + 18 px @ 0.10 **additive** glows.
- vecA `#56B4E9` → `#4FD8FF` (holo cyan), vecB `#E07BD0` → `#D85CC8` (CVD-checked against the new vecA), vecC `#3DD6B0` →
  `#3DF0B4`. Axis colors, `light`, `hot` kept.
- Areas: flat 18 % fill → glass: 0.12 gradient fill + world-unit inner grid + glowing edges + corner points. Hatch kept.
- New: rulers/ticks and measurement arrows on measured sides, side HUD instruments (real readouts), framed formula plate
  inside the card, reflective floor (mirrored geometry), hot pulse (the only animation), clay secondary look for solids
  with matcap + footprint shadows.
- Measured/recorded concerns, not removed: stars under labels (text-shadow + central star dimming), instruments at
  narrow widths (off below 900 px), the `hot`/`light` CVD pair (17 ΔE, different roles).
