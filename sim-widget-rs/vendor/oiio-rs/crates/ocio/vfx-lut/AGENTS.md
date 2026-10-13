# vfx-lut agent notes

What the crate is and its public surface: `README.md`. Open work: `docs/ROADMAP.md` (see the
last section here). The reference for every format is OpenColorIO's `FileFormat*.cpp` in
`C:/projects/projects.rust.cg/vfx.ref/OpenColorIO/` (outside this repo).

## Conventions

- **3D LUT order.** `Lut3D::data` is Blue-fastest (`b + size*(g + size*r)`, OCIO's
  `Lut3DOpData`). vfx-ocio's `ProcessorOp::Lut3d` and GPU textures are Red-fastest. Convert only
  through `OrderedLut3dData::{blue_fastest, red_fastest}(..).into_order(want)`; it names the
  source order at the call site. On disk: `.cube`, Truelight `.cub`, CSP, `.itx`, `.look` and
  HDL are Red-fastest (the readers reorder, the writers reorder back); `.3dl`, `.spi3d`, `.clf`,
  Nuke `.vf` and Pandora are Blue-fastest. Check `FileFormat*.cpp` for
  `setArrayFromRedFastestOrder` / `LUT3DORDER_FAST_RED` before trusting a new reader: `.cub`
  and CSP were read blue-fastest until 2026-09-24 (`vfx-ocio/tests/lut_file_axis_order.rs`).
- **One interpolation kernel.** `interpolate_lattice` is the 3D-LUT kernel for the whole stack
  (port of OCIO's `Lut3DOpCPU.cpp` renderers). Do not add another; `Lut3D::apply` and vfx-ocio
  call it with a `fetch` for their own memory order.
- **CLF/CTF bit depths differ by op type.** LUT1D/LUT3D values scale by `1/outMax`. A Matrix
  scales its 3x3 by `inMax/outMax` and its offset column by `1/outMax` alone. Do not assume one
  rule for all nodes.
- **`.3dl` bit depths are per table.** The shaper and the 3D data each get their bit depth from
  their own maximum (`GetLikelyLutBitDepth`). `read_3dl`/`parse_3dl` error on a non-identity
  shaper because a `Lut3D` has nowhere to put it; `read_3dl_full` returns both.
- **SPI3D is addressed by its index triplet**, not by file order, and a duplicate index or
  incomplete coverage is an error, as in `FileFormatSpi3D.cpp`.
- **Size limits live in `limits.rs` only** (OCIO `LutLimits.h`: 1D 2..=300000 entries, 3D
  2..=129 per axis). Every parser passes a file-derived size through `check_1d_length`,
  `check_3d_size` / `check_3d_edge` or `cube_edge_from_count` before it allocates or divides;
  never write the numbers again in a parser (C2, `tests/size_limits.rs`).
- **Unsupported attributes are errors.** CLF `halfDomain`, `rawHalfs` and `hueAdjust` return a
  typed `LutError::Unsupported` naming the attribute. Never ignore an attribute silently.
- Malformed lines are errors. Do not absorb them with `lines().filter_map(Result::ok)`.

## Traps

- No direct rayon: the `Lut3D::*_par` helpers run bands on `vfx_core::runner::default_runner()`
  (feature `rayon` = `vfx-core/rayon`), so vfx-compute without `rayon` links none (OpenFX).

- **Parsing is not correctness.** Past format bugs all parsed and produced a valid `Lut1D`/`Lut3D`;
  nothing downstream could notice. A fix needs a numeric test (known input, known output from the
  reference), and the test should fail on the old code.
- **Identity LUTs hide axis swaps.** An identity cube's values are the same under either memory
  order. Use asymmetric data (`cube.rs` `roundtrip_3d_asymmetric_detects_axis_swap`).
- **Fixtures can be invalid.** In-file test data once contained `.3dl` lines the reference rejects
  (`FileFormat3DL.cpp` throws on lines with fewer than 3 values). Check the reference before
  treating an existing fixture as ground truth.
- The crate-level rustdoc in `lib.rs` says `.cube` parsing is in vfx-io; it is in `cube.rs` here.

## CLF Range follows OCIO's `RangeOpData`

`RangeParams` has four optional SCALAR bounds (a CLF bound with more than one value is OCIO's
"Range element: non-single value." error), normalised by the node's bit depths
(`RangeOpData::normalize`); `style="noClamp"` = no clamp. `RangeParams::affine` is
`RangeOpData::validate` + `fillScaleOffset` with OCIO's messages (one-sided = pure clamp,
in == out required) and is the one owner: vfx-ocio's RangeTransform and its CLF compile use it.
The writer emits scalars + style.

## CLF/CTF: this crate reads and writes, vfx-ocio evaluates (2026-09-23)

`<ASC_CDL>`, `<Exponent>` and `<Log>` are read with OCIO's model (`CdlParams` + `CdlStyle`
Fwd/Rev/FwdNoClamp/RevNoClamp and the CTF spellings; `ExponentParams` with the ten `GammaOpData`
styles and per-channel R/G/B/A `ExponentParams`; `LogParams` with the eight `LogUtils` styles and
per-channel `LogParams`; bit depths are parsed and do not scale these nodes, as in OCIO). The
writer emits valid CLF for them (bit depths, one `*Params` per channel). OCIO process nodes this
reader does not implement (ExposureContrast, FixedFunction, Grading*, InverseLUT1D/3D, Gamma,
Reference, Function, ACES) are `LutError::Unsupported`; elements OCIO does not know either are
skipped, as OCIO does. **There is no evaluator here any more**: `ProcessList::apply`,
`ProcessNode::apply` and the `*Params::apply` twins (a "simplified" moncurve, a CDL copy, a Log
copy with a 1e-10 floor) are deleted; vfx-ocio `compile_clf` turns each node into the OCIO
transform it is and compiles it, so the inverse file inverts every node.

`write_clf`/`write_ctf` are the stack's only CLF writer (vfx-ocio's baker uses them). A LUT1D is
written with all three channels, LUT values at shortest round-trip `f32` precision in the out
bit depth's code range, a LUT3D with its interpolation, and a LUT with a non-unit domain gets a
leading `Range` (uniform) or 3x4 `Matrix` (per channel), since a CLF LUT has no domain.
Gate `clf::tests::writer_round_trips_rgb_lut1d_depths_domain_and_interpolation`.

`Matrix` nodes are 3x3, 3x4, 4x4 (alpha) or 4x5 (alpha and offsets), as OCIO
`CTFReaderMatrixElt_1_3`; the writer undoes the reader's bit-depth scaling
(`writer_round_trips_matrix_shapes_and_depths`). Only the ProcessList's own `<Description>`,
`<InputDescriptor>`, `<OutputDescriptor>` fill `ProcessList`; a node's `<Description>` is the
node's.

## ICC Matrix/TRC profiles (2026-09-23)

`icc.rs` reads `.icc`/`.icm`/`.pf` the way OCIO's `FileFormatICC.cpp` does (Matrix/TRC only,
OCIO's error messages); vfx-ocio `compile_icc` builds the ops. This is not general ICC colour
management (that is vfx-icc / lcms).

## Open items

Tracked in `docs/ROADMAP.md` §1 P1 "vfx-ocio / vfx-lut": COLOR-05 (`halfDomain`/`rawHalfs` in the CLF reader); The 3D-LUT order as a compile-time type is in the vfx-ocio
row of §1 "Open parity passes".

## The CDL is here, once (2026-09-24, ROADMAP G7)

`cdl.rs` owns the CDL math (`apply_cdl_*`), `validate_cdl_params` and the `.cc`/`.ccc`/`.cdl`
I/O; vfx-ocio's processor and vfx-color's `Cdl` call it. It is bit-exact with OpenColorIO's CPU
(`tests/cdl_pyocio.rs`, 48 PyOpenColorIO 2.5.1 cases). Three OCIO rules that are easy to "fix"
wrongly: `fast_pow` (OCIO's SSE polynomial) is what OCIO computes, not `powf`; when any channel's
power is not 1 OCIO raises EVERY channel (`x^1` is then not exactly `x`) and always applies
saturation, while power (1,1,1) skips both (`renderer_path`); reverse parameters are
`1 / max(x, 0.01)` (`reciprocal`). One deliberate divergence: the clamped inverse clamps its
result even with power 1, which OCIO's matrix rewrite forgets.

## XML text goes through `xml_text.rs`

quick-xml reports `&lt;`, `&#x41;` and CDATA as separate events. Every XML reader here (CDL,
CLF/CTF) appends them with `xml_text::push_{text,ref,cdata}`, runs with `trim_text(false)` and
trims the whole value where it uses it. Keeping only `Event::Text` lost the references and the
spaces next to them.
