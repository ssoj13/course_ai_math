//! Per-code-block debug extraction — an oracle tap for the GPU port (`jph-wgpu-rs`).
//!
//! # Why this module exists
//! The GPU HTJ2K decoder (`jph-wgpu-rs`) validates its entropy kernels (K1 = MEL+VLC,
//! K2 = MagSgn) **bit-exactly, one code-block at a time**, against this reference
//! crate. But the public API ([`crate::mem::decode`]) only returns the final, whole
//! decoded image — it never exposes the per-code-block *(coded bytes in → decoded
//! coefficients out)* pairs a kernel author needs as ground truth. Without that, a GPU
//! entropy bug (where a single wrong bit silently corrupts an entire block) is
//! practically undebuggable: there is no `printf` on divergent GPU compute.
//!
//! This module taps the single per-block decode call site
//! ([`crate::codestream::ojph_codeblock`]) and records each block's exact inputs and
//! its reconstructed sign-magnitude output into a process-global sink.
//!
//! # Cost & safety
//! It is **purely additive** and lives behind the off-by-default `debug-extract` Cargo
//! feature. Normal builds compile *none* of it, so the 1:1 OpenJPH decode path is
//! byte-for-byte unchanged and downstream consumers (`exr-core`, `heif-rs`) are
//! unaffected. It is a debugging aid, not a production API.
//!
//! # Usage
//! ```ignore
//! // (build jph-rs with `--features debug-extract`)
//! let (image, blocks) = jph_rs::debug_extract::capture(|| jph_rs::mem::decode(jph_bytes));
//! for cb in blocks {
//!     // feed cb.coded + params through the GPU kernel, diff against cb.decoded
//! }
//! ```

use std::cell::{Cell, RefCell};

/// One captured code-block: the exact reference-decoder inputs and its output.
///
/// `coded` plus the four scalar fields are precisely the arguments the scalar
/// reference decoder (`ojph_decode_codeblock32`) received; `decoded` is what it
/// produced. A GPU kernel that consumes the same inputs must reproduce `decoded`
/// bit-for-bit.
#[derive(Clone, Debug, Default)]
pub struct CodeblockRecord {
    /// The block's coded byte-stream, `len_cup + len_ref` bytes (CUP then SPP/MRP).
    pub coded: Vec<u8>,
    /// Number of all-zero (missing) most-significant bit-planes for this block.
    pub missing_msbs: u32,
    /// Coding-pass count (1..=3 on the supported HT path).
    pub num_passes: u32,
    /// Cleanup-pass byte length (`pass_length[0]`).
    pub len_cup: u32,
    /// SigProp+MagRef byte length (`pass_length[1]`); 0 when CUP-only.
    pub len_ref: u32,
    /// Code-block width in samples.
    pub width: u32,
    /// Code-block height in samples.
    pub height: u32,
    /// `true` if the reference used the 64-bit magnitude path. The GPU M1 decodes
    /// only the 32-bit path, so for 64-bit blocks `decoded` is left empty and the
    /// harness is expected to skip (loudly), not silently treat it as data.
    pub is_64bit: bool,
    /// Reconstructed sign-magnitude coefficients, row-major `width * height`
    /// (the code-block stride is removed). Empty when [`Self::is_64bit`].
    pub decoded: Vec<u32>,
    /// Reconstructed sign-magnitude coefficients for the **64-bit magnitude path**, row-major
    /// `width * height` (stride removed). Populated only when [`Self::is_64bit`]; empty otherwise.
    /// Additive oracle tap for the GPU 64-bit cleanup kernel (`jph-wgpu-rs` `cleanup64`): the
    /// 32-bit `decoded` cannot hold magnitudes past 32 bits, so they are captured here as `u64`.
    pub decoded64: Vec<u64>,
    /// Whether the 64-bit path's `u_bias`/`u_ext` unsigned-VLC extension (`ojph_block_decoder64.rs
    /// decode_step1_vlc_mel_64`, the initial-row `u_bias`-adjusted `cond0`/`cond1` check or a
    /// subsequent-row raw `u_q > 32` check) fired for AT LEAST ONE quad while decoding this block.
    /// `false` for 32-bit blocks (the check does not exist on that path). Additive oracle-side
    /// coverage tap for the GPU `cleanup64` kernel's parity gate — lets the test corpus be
    /// selected/verified on the CPU (fast) instead of blind GPU trial-and-error.
    pub u_ext_fired: bool,
    /// Placement geometry (filled by [`set_geom`] from the subband decode loop): which
    /// subband this block belongs to and where it sits within it. Enables the GPU port to
    /// assemble subbands for the inverse DWT. Zeroed if geometry was not set.
    pub geom: Geom,
}

/// Where a code-block sits in the wavelet decomposition — recorded so the GPU port can place
/// decoded coefficients into per-subband buffers for inverse-transform assembly.
#[derive(Clone, Copy, Debug, Default)]
pub struct Geom {
    /// Image component index (0-based) this block belongs to.
    pub comp_num: u32,
    /// Resolution level index (0 = lowest/LL resolution).
    pub res_num: u32,
    /// Subband index within the resolution (0=LL, 1=HL, 2=LH, 3=HH; res 0 has only LL).
    pub band_num: u32,
    /// X origin of the block within its subband (samples).
    pub bx0: u32,
    /// Y origin of the block within its subband (samples).
    pub by0: u32,
    /// Subband width (samples).
    pub band_w: u32,
    /// Subband height (samples).
    pub band_h: u32,
}

// Thread-local state: jph-rs decode is single-threaded and `capture` runs the decode on the
// calling thread, so per-thread storage keeps concurrent captures (e.g. parallel test threads)
// fully isolated — no cross-talk, no locking.
thread_local! {
    /// Capture sink for this thread. `None` = capture disarmed (records are dropped).
    static SINK: RefCell<Option<Vec<CodeblockRecord>>> = const { RefCell::new(None) };

    /// Geometry of the block about to be decoded, set by the subband loop just before
    /// `Codeblock::decode`; aligns 1:1 with this thread's records.
    static CUR_GEOM: Cell<Geom> = const {
        Cell::new(Geom { comp_num: 0, res_num: 0, band_num: 0, bx0: 0, by0: 0, band_w: 0, band_h: 0 })
    };

    /// Whether the 64-bit path's `u_bias`/`u_ext` extension fired for the block CURRENTLY being
    /// decoded. Reset by [`reset_uext_fired`] at the top of `decode_step1_vlc_mel_64` (before that
    /// block's quads are scanned), OR-ed in by [`mark_uext_fired`] at each `cond0`/`cond1` site,
    /// read by [`uext_fired`] from `capture_debug_record` right after decode.
    static CUR_UEXT_FIRED: Cell<bool> = const { Cell::new(false) };
}

/// Reset the current block's `u_ext_fired` coverage flag. Called at the top of
/// `decode_step1_vlc_mel_64` (64-bit path only), before that block's quads are scanned.
pub(crate) fn reset_uext_fired() {
    CUR_UEXT_FIRED.with(|c| c.set(false));
}

/// Mark that the `u_ext` extension fired for the block currently being decoded (OR-ed — any one
/// quad firing is enough). Called from `decode_step1_vlc_mel_64` at each `cond0`/`cond1` site.
pub(crate) fn mark_uext_fired() {
    CUR_UEXT_FIRED.with(|c| c.set(true));
}

fn uext_fired() -> bool {
    CUR_UEXT_FIRED.with(|c| c.get())
}

/// Record the placement of the next code-block to be decoded. Called from the subband decode
/// loop. No-op cost when capture is disarmed (the value is simply overwritten).
#[allow(clippy::too_many_arguments)]
pub fn set_geom(
    comp_num: u32,
    res_num: u32,
    band_num: u32,
    bx0: u32,
    by0: u32,
    band_w: u32,
    band_h: u32,
) {
    CUR_GEOM.with(|g| g.set(Geom { comp_num, res_num, band_num, bx0, by0, band_w, band_h }));
}

fn cur_geom() -> Geom {
    CUR_GEOM.with(|g| g.get())
}

/// Arm capture, run `f`, then return its result together with every code-block recorded during
/// the call (on this thread). Resets any previously buffered records first.
pub fn capture<R>(f: impl FnOnce() -> R) -> (R, Vec<CodeblockRecord>) {
    SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
    let out = f();
    let records = SINK.with(|s| s.borrow_mut().take().unwrap_or_default());
    (out, records)
}

/// Arm capture on this thread (low-level). Clears any prior buffer. Pair with [`take`].
pub fn begin() {
    SINK.with(|s| *s.borrow_mut() = Some(Vec::new()));
}

/// Take and clear everything captured so far on this thread, disarming capture (low-level).
pub fn take() -> Vec<CodeblockRecord> {
    SINK.with(|s| s.borrow_mut().take().unwrap_or_default())
}

/// Append one record iff capture is armed; a cheap no-op otherwise. Called from the
/// decode call site under the `debug-extract` feature.
pub(crate) fn record(mut rec: CodeblockRecord) {
    rec.geom = cur_geom();
    rec.u_ext_fired = uext_fired();
    SINK.with(|s| {
        if let Some(buf) = s.borrow_mut().as_mut() {
            buf.push(rec);
        }
    });
}
