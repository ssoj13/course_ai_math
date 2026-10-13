//! Cross-crate drift guard: `vfx-transfer` vs `heif-rs`'s `heif-transfer`.
//!
//! # Why this test exists
//!
//! `heif-rs` ships a `heif-transfer` crate whose six CICP curves — `pq`, `hlg`, `bt2020`,
//! `srgb`, `rec709`, `gamma` — are a **byte-for-byte copy** of the same-named modules in
//! `vfx-transfer` (see this crate's `AGENTS.md`, section "Trap: heif-transfer is a verbatim
//! copy"). The copy is deliberate: `heif-rs` is an upstream dependency of oiio-rs and cannot
//! depend on `vfx-transfer` without a dependency cycle, so it carries its own copy of the maths.
//!
//! Nothing in the source *enforces* that the two stay equal — the only thing linking them is a
//! comment on each side asking future editors to keep them in sync. That is exactly the failure
//! shape that produced six drifting copies of `VIDEO_OETF_POWER` elsewhere in this tree: edit one
//! side, forget the other, and they diverge silently with no test to catch it.
//!
//! # What it guards
//!
//! This is a **live** guard (guard shape #1): `heif-transfer` is a zero-dependency dev-dependency
//! already present in the tree transitively via `heif-color` (pinned rev), so we compare the two
//! implementations by *calling both* at a spread of inputs and asserting bit-for-bit agreement.
//! Because it compares live values rather than hardcoded goldens, it can never go stale: an edit
//! to *either* crate that changes any number reddens this test.
//!
//! Inputs are chosen where a divergence would first surface — the **segment breakpoints** of each
//! piecewise curve and the **negative-mirror** regions — not just midpoints (a midpoint-only test
//! is the vacuous-green trap this project has hit repeatedly). Each curve also carries an explicit
//! non-constancy guard so the equality assertions cannot be satisfied by both copies having been
//! reduced to a constant.

use heif_transfer as ht;
use vfx_transfer as vt;

// ---------------------------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------------------------

/// Value-equality check (NaN-aware) that names the curve, direction and input on failure.
///
/// Uses `==` rather than an epsilon on purpose: the two module bodies are byte-for-byte copies,
/// so on unchanged sources they agree to the last bit at every input, and any real drift (a
/// changed constant or a reordered formula) moves the result far beyond one ULP. So `==` is both
/// the tightest and the most legible guard. Signed zero (`-0.0 == 0.0`) is intentionally *not* a
/// failure — it is not a numeric divergence. The raw bit patterns are printed so a reviewer sees
/// exactly how far, and in which curve, the two desynced.
#[track_caller]
fn assert_same(label: &str, input: f32, vfx: f32, heif: f32) {
    let equal = vfx == heif || (vfx.is_nan() && heif.is_nan());
    assert!(
        equal,
        "DRIFT: {label} disagrees at input {input}: \
         vfx-transfer={vfx} (bits {:#010x}) vs heif-transfer={heif} (bits {:#010x}) \
         — the two verbatim copies have desynced; re-sync the module on both sides",
        vfx.to_bits(),
        heif.to_bits(),
    );
}

/// Compare a single-argument `vfx` vs `heif` function across every input in `inputs`.
#[track_caller]
fn compare_fn(label: &str, inputs: &[f32], vfx: impl Fn(f32) -> f32, heif: impl Fn(f32) -> f32) {
    for &x in inputs {
        assert_same(label, x, vfx(x), heif(x));
    }
}

/// Anti-vacuous-green guard: prove a curve is NON-CONSTANT on *both* crates, so the equality
/// assertions cannot be trivially satisfied by both implementations having been reduced to a
/// constant (the classic "both return 0.0, test still green" trap). Samples two well-separated
/// inputs and requires distinct outputs from each crate independently.
#[track_caller]
fn assert_varies(label: &str, a: f32, b: f32, vfx: impl Fn(f32) -> f32, heif: impl Fn(f32) -> f32) {
    assert_ne!(
        vfx(a),
        vfx(b),
        "{label}: vfx-transfer is constant across [{a}, {b}] — curve is degenerate, \
         the equality guard would be vacuously green"
    );
    assert_ne!(
        heif(a),
        heif(b),
        "{label}: heif-transfer is constant across [{a}, {b}] — curve is degenerate, \
         the equality guard would be vacuously green"
    );
}

// Segment breakpoints, mirrored from the curve sources. If a source moves a breakpoint the live
// comparison still catches the resulting output drift; these constants only steer *where* we look.
const SRGB_EOTF_BREAK: f32 = 0.04045;
const SRGB_OETF_BREAK: f32 = 0.0031308;
const REC709_OETF_BREAK: f32 = 0.018;
const REC709_EOTF_BREAK: f32 = 0.081;
/// BT.2020 beta (oetf breakpoint); f32-trimmed on the heif side but bit-identical.
const BT2020_BETA: f32 = 0.018_053_97;
const HLG_OETF_BREAK: f32 = 1.0 / 12.0;
const HLG_EOTF_BREAK: f32 = 0.5;

/// A small step used to straddle a breakpoint from just-below and just-above.
const NUDGE: f32 = 1e-4;

// ---------------------------------------------------------------------------------------------
// sRGB (IEC 61966-2-1) — piecewise, negative-mirror both directions
// ---------------------------------------------------------------------------------------------

#[test]
fn srgb_parity() {
    // eotf break at v = 0.04045, mirrors negatives.
    let eotf_inputs = [
        -1.0,
        -SRGB_EOTF_BREAK,
        -0.02,
        0.0,
        0.01,
        SRGB_EOTF_BREAK - NUDGE,
        SRGB_EOTF_BREAK,
        SRGB_EOTF_BREAK + NUDGE,
        0.2,
        0.5,
        1.0,
    ];
    compare_fn("srgb::eotf", &eotf_inputs, vt::srgb::eotf, ht::srgb::eotf);

    // oetf break at l = 0.0031308, mirrors negatives.
    let oetf_inputs = [
        -1.0,
        -SRGB_OETF_BREAK,
        -0.001,
        0.0,
        0.001,
        SRGB_OETF_BREAK - NUDGE,
        SRGB_OETF_BREAK,
        SRGB_OETF_BREAK + NUDGE,
        0.2,
        0.5,
        1.0,
    ];
    compare_fn("srgb::oetf", &oetf_inputs, vt::srgb::oetf, ht::srgb::oetf);

    assert_varies("srgb::eotf", 0.2, 0.8, vt::srgb::eotf, ht::srgb::eotf);
    assert_varies("srgb::oetf", 0.2, 0.8, vt::srgb::oetf, ht::srgb::oetf);
}

// ---------------------------------------------------------------------------------------------
// Rec.709 (ITU-R BT.709) — piecewise, negative-mirror both directions
// ---------------------------------------------------------------------------------------------

#[test]
fn rec709_parity() {
    let oetf_inputs = [
        -0.5,
        -REC709_OETF_BREAK,
        0.0,
        0.01,
        REC709_OETF_BREAK - NUDGE,
        REC709_OETF_BREAK,
        REC709_OETF_BREAK + NUDGE,
        0.5,
        1.0,
    ];
    compare_fn(
        "rec709::oetf",
        &oetf_inputs,
        vt::rec709::oetf,
        ht::rec709::oetf,
    );

    let eotf_inputs = [
        -0.5,
        -REC709_EOTF_BREAK,
        0.0,
        0.05,
        REC709_EOTF_BREAK - NUDGE,
        REC709_EOTF_BREAK,
        REC709_EOTF_BREAK + NUDGE,
        0.5,
        1.0,
    ];
    compare_fn(
        "rec709::eotf",
        &eotf_inputs,
        vt::rec709::eotf,
        ht::rec709::eotf,
    );

    assert_varies("rec709::oetf", 0.1, 0.9, vt::rec709::oetf, ht::rec709::oetf);
    assert_varies("rec709::eotf", 0.1, 0.9, vt::rec709::eotf, ht::rec709::eotf);
}

// ---------------------------------------------------------------------------------------------
// BT.2020 (ITU-R BT.2020) — distinct alpha/beta from Rec.709, negative-mirror both directions
// ---------------------------------------------------------------------------------------------

#[test]
fn bt2020_parity() {
    // oetf break at l = beta.
    let oetf_inputs = [
        -0.5,
        -BT2020_BETA,
        0.0,
        0.01,
        BT2020_BETA - NUDGE,
        BT2020_BETA,
        BT2020_BETA + NUDGE,
        0.5,
        1.0,
    ];
    compare_fn(
        "bt2020::oetf",
        &oetf_inputs,
        vt::bt2020::oetf,
        ht::bt2020::oetf,
    );

    // eotf break in the code domain at v = 4.5 * beta.
    let eotf_break = 4.5 * BT2020_BETA;
    let eotf_inputs = [
        -0.5,
        -eotf_break,
        0.0,
        0.05,
        eotf_break - NUDGE,
        eotf_break,
        eotf_break + NUDGE,
        0.5,
        1.0,
    ];
    compare_fn(
        "bt2020::eotf",
        &eotf_inputs,
        vt::bt2020::eotf,
        ht::bt2020::eotf,
    );

    assert_varies("bt2020::oetf", 0.1, 0.9, vt::bt2020::oetf, ht::bt2020::oetf);
    assert_varies("bt2020::eotf", 0.1, 0.9, vt::bt2020::eotf, ht::bt2020::eotf);
}

// ---------------------------------------------------------------------------------------------
// HLG (ITU-R BT.2100) — piecewise, clamps negatives to 0 (no mirror)
// ---------------------------------------------------------------------------------------------

#[test]
fn hlg_parity() {
    // oetf break at e = 1/12.
    let oetf_inputs = [
        -0.5,
        0.0,
        0.05,
        HLG_OETF_BREAK - NUDGE,
        HLG_OETF_BREAK,
        HLG_OETF_BREAK + NUDGE,
        0.3,
        0.5,
        1.0,
    ];
    compare_fn("hlg::oetf", &oetf_inputs, vt::hlg::oetf, ht::hlg::oetf);

    // eotf break at ep = 0.5.
    let eotf_inputs = [
        -0.5,
        0.0,
        0.3,
        HLG_EOTF_BREAK - NUDGE,
        HLG_EOTF_BREAK,
        HLG_EOTF_BREAK + NUDGE,
        0.75,
        0.9,
        1.0,
    ];
    compare_fn("hlg::eotf", &eotf_inputs, vt::hlg::eotf, ht::hlg::eotf);

    // ootf was deleted from both copies (5c7b28f) - wrong shape (scalar where BT.2100 needs
    // triplet luminance) and the wrong layer regardless (a view-time op, not a decode-time one).
    // No comparison to make until it exists again, correctly, beside the display transform.

    assert_varies("hlg::oetf", 0.1, 0.9, vt::hlg::oetf, ht::hlg::oetf);
    assert_varies("hlg::eotf", 0.1, 0.9, vt::hlg::eotf, ht::hlg::eotf);
}

// ---------------------------------------------------------------------------------------------
// PQ (SMPTE ST 2084) — absolute-nits domain, clamps <= 0 to 0
// ---------------------------------------------------------------------------------------------

#[test]
fn pq_parity() {
    // eotf: encoded [0,1] -> absolute nits [0, 10000].
    // 1.5 and 3.0: out of range, both clamp the signal to 1 (3.0 is past the NaN pole).
    let eotf_inputs = [-0.5, 0.0, 0.1, 0.25, 0.508, 0.5, 0.75, 1.0, 1.5, 3.0];
    compare_fn("pq::eotf", &eotf_inputs, vt::pq::eotf, ht::pq::eotf);

    // oetf: absolute nits -> encoded. Spread across the full HDR range including the clamp.
    let oetf_inputs = [-10.0, 0.0, 1.0, 10.0, 100.0, 1000.0, 10000.0, 12000.0];
    compare_fn("pq::oetf", &oetf_inputs, vt::pq::oetf, ht::pq::oetf);

    // Normalized wrappers (÷/×100) are part of the shared surface.
    compare_fn(
        "pq::eotf_normalized",
        &eotf_inputs,
        vt::pq::eotf_normalized,
        ht::pq::eotf_normalized,
    );
    let norm_lin_inputs = [-0.5, 0.0, 0.01, 0.18, 1.0, 5.0, 100.0];
    compare_fn(
        "pq::oetf_normalized",
        &norm_lin_inputs,
        vt::pq::oetf_normalized,
        ht::pq::oetf_normalized,
    );

    assert_varies("pq::eotf", 0.2, 0.8, vt::pq::eotf, ht::pq::eotf);
    assert_varies("pq::oetf", 10.0, 1000.0, vt::pq::oetf, ht::pq::oetf);
}

// ---------------------------------------------------------------------------------------------
// Gamma: pure power + OCIO moncurve (incl. negative-mirror variants)
// ---------------------------------------------------------------------------------------------

#[test]
fn gamma_pure_parity() {
    let inputs = [-1.0, -0.5, -0.18, 0.0, 0.18, 0.5, 1.0];
    for &g in &[2.2_f32, 2.4, 2.6] {
        compare_fn(
            "gamma::gamma_eotf",
            &inputs,
            |v| vt::gamma::gamma_eotf(v, g),
            |v| ht::gamma::gamma_eotf(v, g),
        );
        compare_fn(
            "gamma::gamma_oetf",
            &inputs,
            |l| vt::gamma::gamma_oetf(l, g),
            |l| ht::gamma::gamma_oetf(l, g),
        );
    }

    // The 2.2/2.4/2.6 convenience wrappers.
    compare_fn(
        "gamma::eotf_22",
        &inputs,
        vt::gamma::eotf_22,
        ht::gamma::eotf_22,
    );
    compare_fn(
        "gamma::oetf_22",
        &inputs,
        vt::gamma::oetf_22,
        ht::gamma::oetf_22,
    );
    compare_fn(
        "gamma::eotf_24",
        &inputs,
        vt::gamma::eotf_24,
        ht::gamma::eotf_24,
    );
    compare_fn(
        "gamma::oetf_24",
        &inputs,
        vt::gamma::oetf_24,
        ht::gamma::oetf_24,
    );
    compare_fn(
        "gamma::eotf_26",
        &inputs,
        vt::gamma::eotf_26,
        ht::gamma::eotf_26,
    );
    compare_fn(
        "gamma::oetf_26",
        &inputs,
        vt::gamma::oetf_26,
        ht::gamma::oetf_26,
    );

    assert_varies(
        "gamma::gamma_eotf(_, 2.2)",
        0.2,
        0.8,
        |v| vt::gamma::gamma_eotf(v, 2.2),
        |v| ht::gamma::gamma_eotf(v, 2.2),
    );
}

/// Compare `MoncurveParams` field-by-field (the two crates define distinct-but-identical structs,
/// so they cannot be compared directly).
#[track_caller]
fn same_params(label: &str, v: vt::gamma::MoncurveParams, h: ht::gamma::MoncurveParams) {
    assert_same(&format!("{label}.gamma"), 0.0, v.gamma, h.gamma);
    assert_same(&format!("{label}.offset"), 0.0, v.offset, h.offset);
    assert_same(&format!("{label}.break_pnt"), 0.0, v.break_pnt, h.break_pnt);
    assert_same(&format!("{label}.slope"), 0.0, v.slope, h.slope);
    assert_same(&format!("{label}.scale"), 0.0, v.scale, h.scale);
}

#[test]
fn gamma_moncurve_parity() {
    // Param derivation across offset=0 (cached fast path) and offset>0 (uncached).
    for &(g, off) in &[(2.2_f32, 0.0_f32), (2.4, 0.1), (2.6, 0.05)] {
        same_params(
            "moncurve_params_fwd",
            vt::gamma::moncurve_params_fwd(g, off),
            ht::gamma::moncurve_params_fwd(g, off),
        );
        same_params(
            "moncurve_params_rev",
            vt::gamma::moncurve_params_rev(g, off),
            ht::gamma::moncurve_params_rev(g, off),
        );
    }

    // Curve values around the linear/power breakpoint, both directions, both offset regimes.
    for &(g, off) in &[(2.2_f32, 0.0_f32), (2.2, 0.1), (2.4, 0.1)] {
        let brk = vt::gamma::moncurve_params_fwd(g, off).break_pnt;
        let fwd_inputs = [
            -0.5,
            0.0,
            (brk - NUDGE).max(0.0),
            brk,
            brk + NUDGE,
            0.2,
            0.5,
            1.0,
        ];
        compare_fn(
            "gamma::moncurve_fwd",
            &fwd_inputs,
            |x| vt::gamma::moncurve_fwd(x, g, off),
            |x| ht::gamma::moncurve_fwd(x, g, off),
        );

        let rev_brk = vt::gamma::moncurve_params_rev(g, off).break_pnt;
        let rev_inputs = [
            -0.5,
            0.0,
            (rev_brk - NUDGE).max(0.0),
            rev_brk,
            rev_brk + NUDGE,
            0.2,
            0.5,
            1.0,
        ];
        compare_fn(
            "gamma::moncurve_rev",
            &rev_inputs,
            |y| vt::gamma::moncurve_rev(y, g, off),
            |y| ht::gamma::moncurve_rev(y, g, off),
        );

        // Explicit mirror variants over the negative half.
        let mirror_inputs = [-1.0, -0.5, -0.18, -NUDGE, 0.0, 0.18, 0.5, 1.0];
        compare_fn(
            "gamma::moncurve_mirror_fwd",
            &mirror_inputs,
            |x| vt::gamma::moncurve_mirror_fwd(x, g, off),
            |x| ht::gamma::moncurve_mirror_fwd(x, g, off),
        );
        compare_fn(
            "gamma::moncurve_mirror_rev",
            &mirror_inputs,
            |y| vt::gamma::moncurve_mirror_rev(y, g, off),
            |y| ht::gamma::moncurve_mirror_rev(y, g, off),
        );
    }

    assert_varies(
        "gamma::moncurve_fwd(_, 2.2, 0.1)",
        0.2,
        0.8,
        |x| vt::gamma::moncurve_fwd(x, 2.2, 0.1),
        |x| ht::gamma::moncurve_fwd(x, 2.2, 0.1),
    );
}
