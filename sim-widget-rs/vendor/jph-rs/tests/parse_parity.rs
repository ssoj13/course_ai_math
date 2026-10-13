//! O0.2 verification gate — geometry-keyed parity of [`jph_rs::enumerate`] vs the
//! `debug-extract` oracle, **without** entropy decode on the enumerate side.
//!
//! # What this proves
//! The parse-only front-end [`jph_rs::parse::enumerate`] (O0.1) must reproduce, per code-block,
//! exactly the same decode plan that the full reference decoder ([`jph_rs::mem::decode`], tapped
//! by [`jph_rs::debug_extract`]) sees — same block set, same coded bytes, same
//! `len_cup`/`len_ref`/`num_passes`/`missing_msbs`/`is_64bit`/`width`/`height` — but reach it by
//! only parsing markers/packets/tag-trees (no entropy decode, no inverse DWT). If they ever
//! diverge, the GPU port (`jph-wgpu-rs`) would feed its kernels the wrong plan, so this gate
//! must hold byte-for-byte.
//!
//! Blocks are keyed by their placement `(comp, res, band, bx0, by0)`. Both producers compute
//! that geometry from the *same* parsed subband tree (see `Subband::enumerate_blocks` and the
//! decode-path `set_geom` in `ojph_subband.rs`), so the keys are directly comparable. We compare
//! only the **plan**, never `decoded` — `enumerate` deliberately carries no coefficients.
//!
//! Gated to `--features debug-extract` (the oracle tap is compiled only then).
//!
//! Run: `cargo test -p jph-rs --features debug-extract --test parse_parity -- --nocapture`
#![cfg(feature = "debug-extract")]

use std::collections::HashMap;

use jph_rs::{CodeblockPlan, EncodeParams, Image, ParsedImage, Plane, debug_extract, enumerate, mem};

/// Placement key shared by both producers: `(comp, res, band, bx0, by0)`. Unique within a single
/// tile (the subband + origin fully identify a code-block), which is all `mem::encode` ever emits.
type Key = (u32, u32, u32, u32, u32);

/// Tiny deterministic LCG (Numerical Recipes constants). Self-contained on purpose — the gate must
/// be reproducible with **no** external entropy (`rand`, clock, thread-rng all forbidden), so a
/// given image always encodes to the same bytes and the parity result is stable run-to-run.
struct Lcg(u32);

impl Lcg {
    fn new(seed: u32) -> Self {
        // Mix the seed once so adjacent seeds (component indices) don't yield correlated streams.
        Lcg(seed.wrapping_mul(2_654_435_761).wrapping_add(0x9E37_79B9))
    }

    /// Next sample in `1..=250` — strictly non-zero so even a 1×1 image yields a real (non-empty)
    /// code-block (an all-zero block is filtered by both producers and would defeat the parity).
    fn next_sample(&mut self) -> i32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let v = (self.0 >> 24) & 0xff; // high byte — better mixed than the low bits
        1 + (v % 250) as i32
    }
}

/// Build an `n`-component 8-bit unsigned image of `w×h`, each plane filled by an independent LCG so
/// every code-block is non-empty and components differ.
fn make_image(num_comps: u32, w: u32, h: u32) -> Image {
    let planes: Vec<Plane> = (0..num_comps)
        .map(|c| {
            let mut rng = Lcg::new(c.wrapping_add(1).wrapping_mul(0x85EB_CA77) ^ (w << 13) ^ h);
            let samples = (0..w * h).map(|_| rng.next_sample()).collect::<Vec<i32>>();
            Plane { width: w, height: h, bit_depth: 8, signed: false, samples }
        })
        .collect();
    Image { width: w, height: h, planes }
}

/// Core parity check for one codestream: build geometry-keyed maps from the oracle records and from
/// `enumerate`'s blocks (tile 0), assert identical key sets, and assert every per-block plan field
/// and the coded bytes match. Returns the `ParsedImage` so callers can run extra flag assertions.
///
/// `expect_decomp` is asserted against `config.num_decomp` when `Some` (the encode matrix knows it;
/// an opaque external stream would pass `None`).
fn run_parity(label: &str, bytes: &[u8], expect_decomp: Option<u32>) -> ParsedImage {
    // Oracle: full reference decode, tapping every successfully-decoded code-block.
    let (_img, recs) = debug_extract::capture(|| mem::decode(bytes));
    // Subject: parse-only enumeration (no entropy decode).
    let parsed = enumerate(bytes).unwrap_or_else(|e| panic!("[{label}] enumerate failed: {e}"));

    if let Some(d) = expect_decomp {
        assert_eq!(
            parsed.config.num_decomp, d,
            "[{label}] num_decomp must come authoritatively from COD"
        );
    }

    // Oracle map, keyed by placement. Duplicate keys would mean a broken oracle, not a parity miss.
    let mut oracle: HashMap<Key, &debug_extract::CodeblockRecord> = HashMap::new();
    for r in &recs {
        let key = (r.geom.comp_num, r.geom.res_num, r.geom.band_num, r.geom.bx0, r.geom.by0);
        assert!(
            oracle.insert(key, r).is_none(),
            "[{label}] duplicate oracle key {key:?} — single-tile assumption violated"
        );
    }

    // Subject map, same key, restricted to tile 0 (the only tile `mem::encode` emits, and the only
    // tile the oracle's tile-less `Geom` can represent unambiguously).
    let mut plan: HashMap<Key, &CodeblockPlan> = HashMap::new();
    for b in parsed.blocks.iter().filter(|b| b.geom.tile == 0) {
        let key = (b.geom.comp, b.geom.res, b.geom.band, b.geom.bx0, b.geom.by0);
        assert!(
            plan.insert(key, b).is_none(),
            "[{label}] duplicate enumerate key {key:?} — single-tile assumption violated"
        );
    }

    // Identical key sets: same count, and mutual containment (so neither side has an extra block).
    assert_eq!(
        oracle.len(),
        plan.len(),
        "[{label}] block-count mismatch: oracle {} vs enumerate {}",
        oracle.len(),
        plan.len()
    );
    for key in plan.keys() {
        assert!(oracle.contains_key(key), "[{label}] enumerate key {key:?} absent from oracle");
    }

    // Per-block field + coded-byte equality (decoded is intentionally NOT compared).
    for (key, r) in &oracle {
        let b = plan
            .get(key)
            .unwrap_or_else(|| panic!("[{label}] oracle key {key:?} missing from enumerate"));
        assert_eq!(b.len_cup, r.len_cup, "[{label}] {key:?} len_cup");
        assert_eq!(b.len_ref, r.len_ref, "[{label}] {key:?} len_ref");
        assert_eq!(b.num_passes, r.num_passes, "[{label}] {key:?} num_passes");
        assert_eq!(b.missing_msbs, r.missing_msbs, "[{label}] {key:?} missing_msbs");
        assert_eq!(b.is_64bit, r.is_64bit, "[{label}] {key:?} is_64bit");
        assert_eq!(b.width, r.width, "[{label}] {key:?} width");
        assert_eq!(b.height, r.height, "[{label}] {key:?} height");
        assert_eq!(
            b.coded, r.coded,
            "[{label}] {key:?} coded bytes diverge ({} vs {} bytes)",
            b.coded.len(),
            r.coded.len()
        );
    }

    parsed
}

/// The O0.2 gate proper: a matrix of reversible HTJ2K streams (the supported path) where every
/// block must parity-match the oracle, and additionally satisfy the HT-reversible flag invariants.
///
/// Matrix axes:
/// - components: 1 and 3
/// - num_decompositions: 1, 2, 3, 5
/// - sizes: 1×1, 7×5 (odd, forces partial edge code-blocks), 64×64, 97×53, 200×130 (multi-block
///   subbands with partial edge blocks)
#[test]
fn enumerate_matches_oracle_reversible_matrix() {
    // (num_comps, num_decompositions, width, height)
    let matrix: &[(u32, u8, u32, u32)] = &[
        (1, 1, 1, 1),
        (1, 1, 7, 5),
        (1, 3, 7, 5),
        (1, 5, 64, 64),
        (1, 3, 64, 64),
        (1, 2, 97, 53),
        (1, 3, 200, 130),
        (3, 1, 7, 5),
        (3, 3, 64, 64),
        (3, 2, 97, 53),
        (3, 5, 200, 130),
        (3, 3, 97, 53),
    ];

    for &(num_comps, decomp, w, h) in matrix {
        let label = format!("c{num_comps}_d{decomp}_{w}x{h}");
        let image = make_image(num_comps, w, h);
        let params = EncodeParams { num_decompositions: decomp, ..EncodeParams::default() };
        let bytes = mem::encode(&image, &params);

        let parsed = run_parity(&label, &bytes, Some(u32::from(decomp)));

        // Config sanity for the encode matrix.
        assert_eq!(parsed.config.num_comps, num_comps, "[{label}] num_comps");
        assert!(parsed.config.reversible, "[{label}] default encode is reversible");
        assert!(!parsed.blocks.is_empty(), "[{label}] a non-empty image must yield blocks");

        // HT-reversible invariants: cleanup-only HT blocks on the 32-bit magnitude path.
        for b in &parsed.blocks {
            let k = (b.geom.comp, b.geom.res, b.geom.band, b.geom.bx0, b.geom.by0);
            assert!(b.is_ht, "[{label}] {k:?} must be HT (HTJ2K) coded");
            assert!(!b.is_64bit, "[{label}] {k:?} must use the 32-bit magnitude path");
            assert_eq!(b.num_passes, 1, "[{label}] {k:?} HT cleanup is a single pass");
            assert_eq!(b.len_ref, 0, "[{label}] {k:?} cleanup-only block has no SPP/MRP bytes");
            assert_eq!(
                b.coded.len() as u32,
                b.len_cup + b.len_ref,
                "[{label}] {k:?} coded length must equal len_cup + len_ref"
            );
        }

        println!("[{label}] parity OK: {} blocks", parsed.blocks.len());
    }
}

// TODO(O0.2, external fixture): add a parity case for a multi-tile stream and for a Part-1
// (legacy, `is_ht == false`) `.j2c`/`.jph` stream. Neither exists in this repo today — `tests/`,
// `tests/data/ict` (only single-tile HT-lossy `ict_*.j2c`) and `tests/data/corpus` (raw `.pgm`/`.ppm`)
// have none, and `mem::encode` only ever emits a single-tile reversible HT stream. Such fixtures
// need an external encoder (e.g. Kakadu / a Part-1 OpenJPEG stream). Note also that the
// `debug-extract` oracle `Geom` carries no tile index, so multi-tile parity additionally requires
// keying by tile — fabricating a stream here would not exercise the real path, so it is left open.
