# O0.1 implementation spec — `jph_rs::parse::enumerate` (parse-only, B-prime)

> Self-contained spec for implementing a parse-only per-codeblock enumeration in jph-rs.
> ALL design questions are resolved below (verified in-code). This is mechanical implementation +
> build/test. Strictly ADDITIVE: the existing 1:1 OpenJPH decode path must stay byte-identical.
> Use filesystem MCP tools for file ops; `cargo` via run_command.

## Goal
`pub fn jph_rs::parse::enumerate(codestream: &[u8]) -> Result<ParsedImage, ParseError>` returns, per
code-block, its decode plan (coded bytes + lengths + missing_msbs + num_passes + flags + geometry)
WITHOUT entropy decode or DWT.

## Verified facts (rely on these; re-read to confirm signatures, do not re-derive)
- Driver setup (mirror `src/mem.rs:116-124`): `MemInfile::default()` + `infile.open(ptr,len)`;
  `Codestream::new()`; `cs.read_headers(&mut infile)`; `cs.create()`. After these two calls the FULL
  tile tree is allocated AND every `coded_cbs` is populated (read_headers runs the tile-part loop →
  `Tile::parse_tile_header`→`parse_packets`, `ojph_codestream_local.rs:1033`/`ojph_tile.rs:1382`).
  NO entropy/DWT runs until `pull_ref`/`exchange` — which we DO NOT call.
- Tree + field visibility:
  - `Codestream` (`src/openjph/ojph_codestream.rs`) wraps `state: LocalCodestream`. Add a delegating
    `pub(crate) fn enumerate_blocks(&mut self, sink)`.
  - `LocalCodestream` (`ojph_codestream_local.rs:28`): private `num_tiles: Size`, `tiles: *mut Tile`,
    `num_comps`. Implement the walk method HERE (same module ⇒ can read private fields). Iterate
    `for t in 0..num_tiles.area() { (*tiles.add(t)).enumerate_blocks(t, sink) }`.
  - `Tile` (`ojph_tile.rs:25`): private `num_comps: Ui32`, `comps: *mut TileComp`. Walk:
    `for c in 0..num_comps { (*comps.add(c)).enumerate_blocks(tile_idx, sink) }`.
  - `TileComp` (`ojph_tile_comp.rs:16`): private `res: *mut Resolution` (TOP resolution),
    `num_decomps`, `comp_num`. Walk: `(*self.res).enumerate_blocks(self.comp_num, tile_idx, sink)`.
  - `Resolution` (`ojph_resolution.rs:73`): private `bands: *mut Subband` (4), `child_res`, `res_num`.
    Band set MUST match the decode pull (`Resolution::pull_line`, `:1005-1009,1035-1057`):
      * if `res_num == 0`  ⇒ enumerate `bands.add(0)` (LL) only; `child_res` is null.
      * else (res_num > 0) ⇒ recurse `(*child_res).enumerate_blocks(...)` AND enumerate
        `bands.add(1)` (HL), `bands.add(2)` (LH), `bands.add(3)` (HH). Do NOT touch band[0] at res>0.
    Skip a band whose `Subband.empty` is true.
  - `Subband` (`ojph_subband.rs:16`): fields are `pub(crate)` (readable from a new crate-root module
    too, but cleanest is to implement `enumerate_blocks` IN ojph_subband). Has `band_rect`,
    `blocks: *mut Codeblock`, `num_blocks: Size`, `xcb_prime`, `ycb_prime`, `coded_cbs`, `res_num`,
    `band_num`.
  - `Codeblock` (`ojph_codeblock.rs:92`): private `precision`, `is_ht`. Add TWO `pub(crate)` getters:
    `fn is_ht(&self) -> bool { self.is_ht }` and `fn is_64bit(&self) -> bool { self.precision != Self::BUF32 }`.
  - `CodedCbHeader` (`ojph_codeblock.rs:50`, all pub): `pass_length:[Ui32;2]`, `num_passes`,
    `missing_msbs`, `next_coded: *mut CodedLists`. `PREFIX_BUF_SIZE = 8`.

## Subband::enumerate_blocks geometry + plan read (port of pull_line `ojph_subband.rs:464-499` +
## capture_debug_record `ojph_codeblock.rs:485-526`, MINUS recreate/decode):
```
let tbx0 = band_rect.org.x; let tby0 = band_rect.org.y;
let tbx1 = tbx0 + band_rect.siz.w; let tby1 = tby0 + band_rect.siz.h;
let nom_w = 1u32 << xcb_prime; let nom_h = 1u32 << ycb_prime;
let x_lower = (tbx0 >> xcb_prime) << xcb_prime;
let y_lower = (tby0 >> ycb_prime) << ycb_prime;
for row in 0..num_blocks.h {
  let cby0 = ojph_max(tby0, y_lower + row*nom_h);
  let cby1 = ojph_min(tby1, y_lower + (row+1)*nom_h);
  for col in 0..num_blocks.w {
    let cbx0 = ojph_max(tbx0, x_lower + col*nom_w);
    let cbx1 = ojph_min(tbx1, x_lower + (col+1)*nom_w);
    let idx = col + row*num_blocks.w;       // coded_cbs index (full grid, row-major)
    let cb = *coded_cbs.add(idx as usize);  // CodedCbHeader (Copy)
    // INCLUSION PREDICATE (mirror ojph_codeblock.rs:530-532): skip empty blocks.
    if cb.pass_length[0] == 0 || cb.num_passes == 0 || cb.next_coded.is_null() { continue; }
    let len = (cb.pass_length[0] + cb.pass_length[1]) as usize;
    let src = (*cb.next_coded).buf.add(CodedCbHeader::PREFIX_BUF_SIZE as usize);
    let mut coded = vec![0u8; len]; ptr::copy_nonoverlapping(src, coded.as_mut_ptr(), len);
    let blk = &*blocks.add(col as usize);   // blocks array is num_blocks.w wide (reused per row);
                                            // is_ht/precision are uniform per subband, set at alloc.
    sink(CodeblockPlan {
      coded, len_cup: cb.pass_length[0], len_ref: cb.pass_length[1],
      num_passes: cb.num_passes, missing_msbs: cb.missing_msbs,
      width: cbx1 - cbx0, height: cby1 - cby0,
      is_64bit: blk.is_64bit(), is_ht: blk.is_ht(),
      geom: Geom { comp: comp_num, res: self.res_num, band: self.band_num, tile: tile_idx,
                   bx0: cbx0 - tbx0, by0: cby0 - tby0,
                   band_w: band_rect.siz.w, band_h: band_rect.siz.h },
    });
  }
}
```
`CodedLists` is the type of `next_coded`; `.buf` is read the same way `capture_debug_record` does
(`ojph_codeblock.rs:488-490`) — import from wherever that file imports it.

## src/parse.rs (new, crate-root, idiomatic — NOT 1:1 port style)
- Public types: `CodecConfig`, `Geom`, `CodeblockPlan`, `ParsedImage`, `ParseError` (impl Display +
  std::error::Error). Field lists per `../../jph-wgpu-rs/docs/O0_PARSE_FRONTEND_PLAN.md` §"Required
  output types".
- `pub fn enumerate(codestream: &[u8]) -> Result<ParsedImage, ParseError>`:
  wrap the whole drive in `std::panic::catch_unwind(AssertUnwindSafe(|| { ... }))`. Inside: set up
  MemInfile+Codestream, `read_headers`, `create`, build `CodecConfig` from `cs.access_siz()` +
  `cs.access_cod()` (width/height = `get_image_extent()`; num_comps = `get_num_components()`;
  bit_depth/signed = `get_bit_depth(0)`/`is_signed(0)`; num_decomp = `cod.get_num_decompositions()`;
  cbw/cbh = `cod.get_block_dims()`; prog = `cod.get_progression_order()`; color_transform/reversible
  from the available cod/atk accessors — grep for the getter names, e.g. `is_reversible`/
  `is_employing_color_transform`; if a tile-grid getter is missing add a minimal `pub` one on ParamSiz
  or fill 0/whole-image). Collect blocks via `cs.enumerate_blocks(&mut |p| blocks.push(p))`.
  On `Err(payload)`: if `payload.downcast_ref::<OjphErrorPanic>().is_some()` ⇒
  `Err(ParseError::ParseFailed(...))`, else `std::panic::resume_unwind(payload)`. (OjphErrorPanic is
  in `src/openjph/ojph_message.rs` — grep for the exact name/path.)
- Use `?`/checked indexing in YOUR code; the only panics are the 1:1 code inside catch_unwind.

## src/lib.rs
`pub mod parse;` and `pub use parse::{enumerate, ParsedImage, CodeblockPlan, CodecConfig};`
(re-export `parse::Geom` as `ParseGeom` to avoid clashing with `debug_extract::Geom`).

## Verify (REQUIRED, show command tails)
1. `cargo build -p jph-rs` OK.  2. `cargo build -p jph-rs --features debug-extract` OK.
3. `cargo clippy -p jph-rs --all-targets -- -D warnings` clean.
4. Smoke test (`#[cfg(test)]` in parse.rs): build a tiny multi-comp image, `mem::encode` it,
   `enumerate` it, assert `blocks.len() > 0`, every `coded` non-empty, `num_passes>=1`,
   `config.num_decomp` equals the encode param. (Full geom-keyed parity vs debug_extract = task O0.2,
   NOT here.)
Report changed files, the new accessors added (each + why), and command-output tails. Do not claim
success without outputs.
```
