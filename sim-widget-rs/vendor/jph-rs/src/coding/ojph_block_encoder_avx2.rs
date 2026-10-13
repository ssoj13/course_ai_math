//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.31.0/src/core/coding/ojph_block_encoder_avx2.cpp`
//!
//! **Round 10 (BUG1.md): ported the 0.31.0 MEL/VLC/UVLC/MS rewrite** (`git diff 0.26.3 0.31.0 --
//! src/core/coding/ojph_block_encoder_avx2.cpp`, 754 changed lines). Unlike `tx_to_cb`/
//! `find_max_val` (`ojph_codestream_gen.rs`), the MEL/VLC/MS bit-packing state machines here are
//! **not shared with the generic scalar encoder** (`ojph_block_encoder.rs`) — C++ 0.26.3 and
//! 0.31.0 both define their own local `mel_struct`/`vlc_struct`/`ms_struct` inside this file,
//! separate from the scalar ones, and 0.31.0 only rewrites the AVX2 file's copies (the scalar
//! encoder's `mel_emit_bit`/`ms_encode`/`ms_terminate` in `ojph_block_encoder.rs` are untouched
//! upstream and untouched here). What changed, all behavior-preserving (same final bitstream,
//! proven by `coding::codeblock_fuzz_tests`), all a batching/lookup-table win:
//! - `mel_emit_bit` (1 bit/call) -> `mel_emit_bits` (N bits/call, same byte-stuff rule
//!   branchlessly folded into `remaining_bits += 8 - (byte == 0xFF)`).
//! - `vlc_encode`/`ms_encode64`+`ms_terminate` (eager, drain-every-call) -> `vlc_encode`/
//!   `ms_encode_nodefer`+`ms_drain`+`ms_terminate` (deferred: accumulate up to a 64-bit
//!   `tmp`/`used_bits`, drain only when it would overflow, or explicitly at end of pass/stream).
//!   `ms_drain` additionally processes up to 8 bytes per iteration via the SWAR "find an 0xFF
//!   byte" trick (`(!word - 0x0101..) & word & 0x8080..`) instead of one byte at a time.
//! - New `uvlc_tbl_pair1`/`uvlc_tbl_pair2` (33x33 lookup, precomputed once): replaces four
//!   conditional `ULVC_CWD_PRE`/`ULVC_CWD_SUF` lookups per lane-pair (`proc_vlc_encode1`/`2`'s old
//!   branches) with one table read (`build_vlc_uvlc_pair`), and `proc_vlc_encode` now merges TWO
//!   lane-pairs (4 lanes) into one `vlc_encode` call instead of one lane-pair per call — half as
//!   many `vlc_encode` calls.
//! - `proc_mel_encode2` (line >= 1: `cq==0` never true when `rho!=0`... actually: skip MEL calls
//!   for insignificant quads) now finds the `cq==0` lanes via `_mm256_movemask_epi8` + a
//!   trailing-zero scan instead of a per-lane branch, matching the note in BUG1.md Round 9
//!   ("`proc_mel_encode2` movemask lane-skip").
//! - `ms_struct`'s bounds checks (jph-rs's own hardening, not in C++ — matches the `vlc_encode`
//!   buffer-full guard already added on the 0.26.3 port) are ported forward into the new
//!   `ms_drain`'s byte-store branches (C++'s `ms_drain` has none, relying on the ~17KB "more than
//!   enough" MS scratch buffer never actually filling for a single code-block).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use crate::openjph::ojph_defs::{ojph_max, ojph_min, Ui32, Ui64, Ui8};
use crate::openjph::ojph_mem::{mem_elastic_allocator, CodedLists};

use super::ojph_block_encoder::{mel_init, mel_struct, uvlc_tbl, vlc_tbl0, vlc_tbl1};

#[cfg(target_arch = "x86")]
use core::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

#[allow(non_upper_case_globals)]
static VLC_TBL0_AVX2: LazyLock<[Ui32; 2048]> = LazyLock::new(|| {
    let mut out = [0u32; 2048];
    for (dst, src) in out.iter_mut().zip(vlc_tbl0.iter()) {
        *dst = *src as Ui32;
    }
    out
});

#[allow(non_upper_case_globals)]
static VLC_TBL1_AVX2: LazyLock<[Ui32; 2048]> = LazyLock::new(|| {
    let mut out = [0u32; 2048];
    for (dst, src) in out.iter_mut().zip(vlc_tbl1.iter()) {
        *dst = *src as Ui32;
    }
    out
});

#[allow(non_upper_case_globals)]
static ULVC_CWD_PRE: LazyLock<[Ui32; 33]> = LazyLock::new(|| {
    let mut out = [0u32; 33];
    for i in 0..33 {
        out[i] = uvlc_tbl[i].pre as Ui32;
    }
    out
});

#[allow(non_upper_case_globals)]
static ULVC_CWD_PRE_LEN: LazyLock<[i32; 33]> = LazyLock::new(|| {
    let mut out = [0i32; 33];
    for i in 0..33 {
        out[i] = uvlc_tbl[i].pre_len as i32;
    }
    out
});

#[allow(non_upper_case_globals)]
static ULVC_CWD_SUF: LazyLock<[Ui32; 33]> = LazyLock::new(|| {
    let mut out = [0u32; 33];
    for i in 0..33 {
        out[i] = uvlc_tbl[i].suf as Ui32;
    }
    out
});

#[allow(non_upper_case_globals)]
static ULVC_CWD_SUF_LEN: LazyLock<[i32; 33]> = LazyLock::new(|| {
    let mut out = [0i32; 33];
    for i in 0..33 {
        out[i] = uvlc_tbl[i].suf_len as i32;
    }
    out
});

/// Builds one of the two 33x33 (uq0, uq1) -> `(cwd << 5) | len` pair tables (0.31.0
/// `uvlc_init_pair_tables`). `pair1` mirrors `uvlc_tbl_pair1` (line 1: the special
/// `uq0 > 2 && uq1 > 0` half-prefix case); otherwise mirrors `uvlc_tbl_pair2` (line 2+: always
/// full prefix+suffix for both). One lookup replaces the four conditional `ULVC_CWD_PRE`/
/// `ULVC_CWD_SUF` reads `proc_vlc_encode1`/`2` used to do per lane-pair.
fn build_uvlc_pair_table(pair1: bool) -> [Ui32; 33 * 33] {
    let mut out = [0u32; 33 * 33];
    for uq0 in 0..33usize {
        for uq1 in 0..33usize {
            let (cwd, len): (Ui32, i32) = if pair1 && uq0 > 2 && uq1 > 2 {
                let mut cwd = ULVC_CWD_PRE[uq0 - 2];
                let mut len = ULVC_CWD_PRE_LEN[uq0 - 2];
                cwd |= ULVC_CWD_PRE[uq1 - 2] << len;
                len += ULVC_CWD_PRE_LEN[uq1 - 2];
                cwd |= ULVC_CWD_SUF[uq0 - 2] << len;
                len += ULVC_CWD_SUF_LEN[uq0 - 2];
                cwd |= ULVC_CWD_SUF[uq1 - 2] << len;
                len += ULVC_CWD_SUF_LEN[uq1 - 2];
                (cwd, len)
            } else if pair1 && uq0 > 2 && uq1 > 0 {
                let mut cwd = ULVC_CWD_PRE[uq0];
                let mut len = ULVC_CWD_PRE_LEN[uq0];
                cwd |= (uq1 as Ui32 - 1) << len;
                len += 1;
                cwd |= ULVC_CWD_SUF[uq0] << len;
                len += ULVC_CWD_SUF_LEN[uq0];
                (cwd, len)
            } else {
                let mut cwd = ULVC_CWD_PRE[uq0];
                let mut len = ULVC_CWD_PRE_LEN[uq0];
                cwd |= ULVC_CWD_PRE[uq1] << len;
                len += ULVC_CWD_PRE_LEN[uq1];
                cwd |= ULVC_CWD_SUF[uq0] << len;
                len += ULVC_CWD_SUF_LEN[uq0];
                cwd |= ULVC_CWD_SUF[uq1] << len;
                len += ULVC_CWD_SUF_LEN[uq1];
                (cwd, len)
            };
            out[uq0 * 33 + uq1] = (cwd << 5) | (len as Ui32);
        }
    }
    out
}

#[allow(non_upper_case_globals)]
static UVLC_TBL_PAIR1: LazyLock<[Ui32; 33 * 33]> = LazyLock::new(|| build_uvlc_pair_table(true));

#[allow(non_upper_case_globals)]
static UVLC_TBL_PAIR2: LazyLock<[Ui32; 33 * 33]> = LazyLock::new(|| build_uvlc_pair_table(false));

pub(crate) fn initialize_block_encoder_tables_avx2() -> bool {
    let _ = &*VLC_TBL0_AVX2;
    let _ = &*VLC_TBL1_AVX2;
    let _ = &*ULVC_CWD_PRE;
    let _ = &*ULVC_CWD_PRE_LEN;
    let _ = &*UVLC_TBL_PAIR1;
    let _ = &*UVLC_TBL_PAIR2;
    let _ = &*ULVC_CWD_SUF;
    let _ = &*ULVC_CWD_SUF_LEN;
    true
}

/// Batched MEL bit emission (0.31.0 `mel_emit_bits`), replacing the old bit-at-a-time
/// `mel_emit_bit` this file used to import from the generic scalar encoder. Same byte-stuffing
/// rule (a `0xFF` output byte reduces the next byte's available bits to 7), folded branchlessly
/// into `remaining_bits += 8 - (byte == 0xFF)` instead of the scalar path's `if` — see
/// `mel_encode` below for why the scalar `ojph_block_encoder.rs` copy is intentionally left
/// as-is (it is a different, unrewritten C++ file).
#[inline]
unsafe fn mel_emit_bits(
    melp: &mut mel_struct,
    bits: Ui32,
    num_bits: i32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    melp.tmp = (melp.tmp << num_bits) | (bits as i32);
    melp.remaining_bits -= num_bits;
    if melp.remaining_bits <= 0 {
        let excess = -melp.remaining_bits;
        let byte = (melp.tmp >> excess) as Ui8;
        if melp.pos >= melp.buf_size {
            return Err(crate::ojph_error!(
                0x00020001,
                "mel encoder's buffer is full"
            ));
        }
        *melp.buf.add(melp.pos as usize) = byte;
        melp.pos += 1;
        melp.tmp &= (1 << excess) - 1;
        melp.remaining_bits += 8 - (byte == 0xFF) as i32;
    }
    Ok(())
}

/// 0.31.0 rewrite of `mel_encode`: the "significant" branch emits the sign bit and the `t`
/// run-length bits in one `mel_emit_bits` call instead of `mel_emit_bit`'s per-bit loop.
#[inline]
unsafe fn mel_encode(
    melp: &mut mel_struct,
    bit: bool,
) -> crate::openjph::ojph_message::OjphResult<()> {
    static MEL_EXP: [i32; 13] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 5];

    if !bit {
        melp.run += 1;
        if melp.run >= melp.threshold {
            mel_emit_bits(melp, 1, 1)?;
            melp.run = 0;
            melp.k = ojph_min(12, melp.k + 1);
            melp.threshold = 1 << MEL_EXP[melp.k as usize];
        }
    } else {
        let t = MEL_EXP[melp.k as usize];
        mel_emit_bits(melp, (melp.run as Ui32) & ((1u32 << t) - 1), t + 1)?;
        melp.run = 0;
        melp.k = ojph_max(0, melp.k - 1);
        melp.threshold = 1 << MEL_EXP[melp.k as usize];
    }
    Ok(())
}

#[allow(non_camel_case_types)]
struct vlc_struct {
    buf: *mut Ui8,
    pos: Ui32,
    buf_size: Ui32,
    used_bits: i32,
    tmp: Ui64,
    last_greater_than_8f: bool,
}

impl Default for vlc_struct {
    fn default() -> Self {
        Self {
            buf: core::ptr::null_mut(),
            pos: 0,
            buf_size: 0,
            used_bits: 0,
            tmp: 0,
            last_greater_than_8f: false,
        }
    }
}

#[inline]
unsafe fn vlc_init(vlcp: &mut vlc_struct, buffer_size: Ui32, data: *mut Ui8) {
    vlcp.buf = data.add(buffer_size as usize - 1);
    vlcp.pos = 1;
    vlcp.buf_size = buffer_size;
    *vlcp.buf = 0xFF;
    vlcp.used_bits = 4;
    vlcp.tmp = 0xF;
    vlcp.last_greater_than_8f = true;
}

/// Drains whole bytes out of `vlcp.tmp` (0.31.0 `vlc_drain`, extracted from what used to be
/// inlined in `vlc_encode`). Behavior-identical byte-stuffing rule to the pre-rewrite version:
/// a byte > `0x8F` following the previous one forces the next byte to use only 7 bits (unless it
/// would itself be `0x7F`, which stays 8 bits and clears `last_greater_than_8f`). The memory-
/// safety guard (buffer-full check) is jph-rs's own hardening (see the 0.26.3 port note this
/// replaces): the C++ original has none, relying on the ~3KB mel+vlc scratch "more than enough".
#[inline]
unsafe fn vlc_drain(vlcp: &mut vlc_struct) -> crate::openjph::ojph_message::OjphResult<()> {
    while vlcp.used_bits >= 8 {
        if vlcp.pos >= vlcp.buf_size {
            return Err(crate::ojph_error!(
                0x00020002,
                "vlc encoder's buffer is full"
            ));
        }
        let escape = vlcp.last_greater_than_8f;
        let is_7f = (vlcp.tmp & 0x7F) == 0x7F;
        let need_stuff = escape && is_7f;
        let bits: i32 = if need_stuff { 7 } else { 8 };
        let byte = (vlcp.tmp & ((1u64 << bits) - 1)) as Ui8;
        *vlcp.buf.sub(vlcp.pos as usize) = byte;
        vlcp.pos += 1;
        vlcp.tmp >>= bits;
        vlcp.used_bits -= bits;
        vlcp.last_greater_than_8f = byte > 0x8F;
    }
    Ok(())
}

/// 0.31.0 `vlc_encode`: deferred draining — accumulates into the 64-bit `tmp`/`used_bits` and
/// only calls [`vlc_drain`] when the accumulator would overflow (or the caller drains explicitly,
/// as `ojph_encode_codeblock_avx2` now does before [`terminate_mel_vlc_avx2`]), instead of the
/// pre-rewrite version's drain-every-call. Same final byte stream (proven by
/// `coding::codeblock_fuzz_tests`), fewer byte-store operations per call since `proc_vlc_encode`
/// now merges 4 lanes (up to ~30 bits) per call.
#[inline]
unsafe fn vlc_encode(
    vlcp: &mut vlc_struct,
    mut cwd: Ui64,
    mut cwd_len: i32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    loop {
        let avail = 64 - vlcp.used_bits;
        if avail > 0 && cwd_len <= avail {
            vlcp.tmp |= cwd << vlcp.used_bits;
            vlcp.used_bits += cwd_len;
            return Ok(());
        }
        if avail > 0 {
            vlcp.tmp |= cwd << vlcp.used_bits;
        }
        vlcp.used_bits = 64;
        vlc_drain(vlcp)?;
        cwd >>= avail;
        cwd_len -= avail;
    }
}

#[inline]
unsafe fn terminate_mel_vlc_avx2(
    melp: &mut mel_struct,
    vlcp: &mut vlc_struct,
) -> crate::openjph::ojph_message::OjphResult<()> {
    if melp.run > 0 {
        mel_emit_bits(melp, 1, 1)?;
    }

    if vlcp.last_greater_than_8f && (vlcp.tmp & 0x7F) == 0x7F {
        *vlcp.buf.sub(vlcp.pos as usize) = 0x7F;
        vlcp.pos += 1;
        vlcp.tmp >>= 7;
        vlcp.used_bits -= 7;
    }

    melp.tmp <<= melp.remaining_bits;
    let mel_mask = (0xFF << melp.remaining_bits) & 0xFF;
    let vlc_mask = 0xFF >> (8 - vlcp.used_bits);
    if (mel_mask | vlc_mask) == 0 {
        return Ok(());
    }
    if melp.pos >= melp.buf_size {
        return Err(crate::ojph_error!(
            0x00020003,
            "mel encoder's buffer is full"
        ));
    }
    let vlcp_tmp = vlcp.tmp as Ui8;
    let fuse = melp.tmp | (vlcp_tmp as i32);
    if ((((fuse ^ melp.tmp) & mel_mask) | ((fuse ^ (vlcp_tmp as i32)) & vlc_mask)) == 0)
        && fuse != 0xFF
        && vlcp.pos > 1
    {
        *melp.buf.add(melp.pos as usize) = fuse as Ui8;
        melp.pos += 1;
    } else {
        if vlcp.pos >= vlcp.buf_size {
            return Err(crate::ojph_error!(
                0x00020004,
                "vlc encoder's buffer is full"
            ));
        }
        *melp.buf.add(melp.pos as usize) = melp.tmp as Ui8;
        melp.pos += 1;
        *vlcp.buf.sub(vlcp.pos as usize) = vlcp_tmp;
        vlcp.pos += 1;
    }
    Ok(())
}

/// How AVX2 block encode computes lane leading-zero counts in `proc_pixel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Avx2LzcntMode {
    /// OpenJPH C++ default: pure SIMD emulation (no `_lzcnt_u32`).
    Emulated,
    /// Scalar `_lzcnt_u32` per lane (requires CPU `lzcnt`).
    Hardware,
    /// Hardware when `lzcnt` is detected, otherwise emulated.
    Auto,
}

/// `OJPH_AVX2_LZCNT=emulated|hw|hardware|auto` (default: `emulated`).
pub const ENV_AVX2_LZCNT: &str = "OJPH_AVX2_LZCNT";

/// When `1`, `ojph_encode_codeblock_avx2` prints a timing breakdown to stderr after each call.
pub const ENV_ENCODE_PROFILE: &str = "OJPH_ENCODE_PROFILE";

const LZCNT_MODE_USE_ENV: u8 = 0;
const LZCNT_MODE_EMULATED: u8 = 1;
const LZCNT_MODE_HARDWARE: u8 = 2;
const LZCNT_MODE_AUTO: u8 = 3;

static LZCNT_MODE_OVERRIDE: AtomicU8 = AtomicU8::new(LZCNT_MODE_USE_ENV);

static HW_LZCNT_AVAILABLE: LazyLock<bool> =
    LazyLock::new(|| std::arch::is_x86_feature_detected!("lzcnt"));

impl Avx2LzcntMode {
    fn from_stored(v: u8) -> Self {
        match v {
            LZCNT_MODE_HARDWARE => Self::Hardware,
            LZCNT_MODE_AUTO => Self::Auto,
            _ => Self::Emulated,
        }
    }

    fn to_stored(self) -> u8 {
        match self {
            Self::Emulated => LZCNT_MODE_EMULATED,
            Self::Hardware => LZCNT_MODE_HARDWARE,
            Self::Auto => LZCNT_MODE_AUTO,
        }
    }
}

/// Parse `emulated`, `hw` / `hardware`, or `auto`.
pub fn parse_avx2_lzcnt_mode(s: &str) -> Option<Avx2LzcntMode> {
    match s.trim().to_ascii_lowercase().as_str() {
        "emulated" | "emu" | "simd" => Some(Avx2LzcntMode::Emulated),
        "hw" | "hardware" | "lzcnt" => Some(Avx2LzcntMode::Hardware),
        "auto" => Some(Avx2LzcntMode::Auto),
        _ => None,
    }
}

pub fn avx2_lzcnt_mode_from_env() -> Avx2LzcntMode {
    let Ok(raw) = std::env::var(ENV_AVX2_LZCNT) else {
        return Avx2LzcntMode::Emulated;
    };
    parse_avx2_lzcnt_mode(&raw).unwrap_or(Avx2LzcntMode::Emulated)
}

/// Runtime override (`None` clears override → use env / default).
pub fn set_avx2_lzcnt_mode(mode: Option<Avx2LzcntMode>) {
    let v = mode
        .map(Avx2LzcntMode::to_stored)
        .unwrap_or(LZCNT_MODE_USE_ENV);
    LZCNT_MODE_OVERRIDE.store(v, Ordering::Relaxed);
}

pub fn get_avx2_lzcnt_mode() -> Avx2LzcntMode {
    // Read the env ONCE: this runs per code-block, and `std::env::var` allocates and takes the
    // process-wide environment lock (Windows: RtlQueryEnvironmentVariable) — 1 alloc per block and
    // a serialisation point for parallel encoders. Same latch as `encode_profile_enabled`.
    static ENV_MODE: LazyLock<Avx2LzcntMode> = LazyLock::new(avx2_lzcnt_mode_from_env);
    match LZCNT_MODE_OVERRIDE.load(Ordering::Relaxed) {
        LZCNT_MODE_USE_ENV => *ENV_MODE,
        v => Avx2LzcntMode::from_stored(v),
    }
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn avx2_lzcnt_epi32_emulated(mut v: __m256i) -> __m256i {
    v = _mm256_andnot_si256(_mm256_srli_epi32(v, 8), v);
    v = _mm256_castps_si256(_mm256_cvtepi32_ps(v));
    v = _mm256_srli_epi32(v, 23);
    v = _mm256_subs_epu16(_mm256_set1_epi32(158), v);
    _mm256_min_epi16(v, _mm256_set1_epi32(32))
}

/// Hardware LZCNT per lane. Input is already `val-1` from `proc_pixel` (see OpenJPH AVX2).
#[cfg(target_arch = "x86_64")]
#[inline]
#[target_feature(enable = "avx2")]
#[target_feature(enable = "lzcnt")]
unsafe fn avx2_lzcnt_epi32_hw(mut v: __m256i) -> __m256i {
    v = _mm256_andnot_si256(_mm256_srli_epi32(v, 8), v);
    let mut lanes = [0u32; 8];
    _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, v);
    for lane in &mut lanes {
        *lane = _lzcnt_u32(*lane);
    }
    _mm256_loadu_si256(lanes.as_ptr() as *const __m256i)
}

#[cfg(target_arch = "x86")]
#[inline]
#[target_feature(enable = "avx2")]
#[target_feature(enable = "lzcnt")]
unsafe fn avx2_lzcnt_epi32_hw(mut v: __m256i) -> __m256i {
    v = _mm256_andnot_si256(_mm256_srli_epi32(v, 8), v);
    let mut lanes = [0u32; 8];
    _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, v);
    for lane in &mut lanes {
        *lane = _lzcnt_u32(*lane);
    }
    _mm256_loadu_si256(lanes.as_ptr() as *const __m256i)
}

/// Select emulated SIMD vs hardware `_lzcnt_u32` (see [`Avx2LzcntMode`]).
#[inline]
#[target_feature(enable = "avx2")]
pub unsafe fn avx2_lzcnt_epi32(v: __m256i, mode: Avx2LzcntMode) -> __m256i {
    match mode {
        Avx2LzcntMode::Emulated => avx2_lzcnt_epi32_emulated(v),
        Avx2LzcntMode::Hardware => {
            if *HW_LZCNT_AVAILABLE {
                avx2_lzcnt_epi32_hw(v)
            } else {
                avx2_lzcnt_epi32_emulated(v)
            }
        }
        Avx2LzcntMode::Auto => {
            if *HW_LZCNT_AVAILABLE {
                avx2_lzcnt_epi32_hw(v)
            } else {
                avx2_lzcnt_epi32_emulated(v)
            }
        }
    }
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn avx2_cmpneq_epi32(v: __m256i, v2: __m256i) -> __m256i {
    _mm256_xor_si256(
        _mm256_cmpeq_epi32(v, v2),
        _mm256_set1_epi32(0xFFFF_FFFFu32 as i32),
    )
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn proc_pixel(
    src_vec: *const __m256i,
    p: Ui32,
    lzcnt_mode: Avx2LzcntMode,
    eq_vec: *mut __m256i,
    s_vec: *mut __m256i,
    rho_vec: &mut __m256i,
    e_qmax_vec: &mut __m256i,
) {
    let zero = _mm256_setzero_si256();
    let one = _mm256_set1_epi32(1);
    let p_shift = _mm_cvtsi32_si128(p as i32);
    let mut val_vec = [_mm256_setzero_si256(); 4];
    let mut eq_tmp = [_mm256_setzero_si256(); 4];
    let mut s_tmp = [_mm256_setzero_si256(); 4];
    let mut rho_tmp = [_mm256_setzero_si256(); 4];

    for i in 0..4usize {
        val_vec[i] = _mm256_add_epi32(*src_vec.add(i), *src_vec.add(i));
        val_vec[i] = _mm256_srl_epi32(val_vec[i], p_shift);
        val_vec[i] = _mm256_and_si256(val_vec[i], _mm256_set1_epi32(!1u32 as i32));

        let val_notmask = avx2_cmpneq_epi32(val_vec[i], zero);
        val_vec[i] = _mm256_sub_epi32(val_vec[i], one);
        eq_tmp[i] = _mm256_sub_epi32(
            _mm256_set1_epi32(32),
            avx2_lzcnt_epi32(val_vec[i], lzcnt_mode),
        );
        val_vec[i] = _mm256_sub_epi32(val_vec[i], one);
        s_tmp[i] = _mm256_add_epi32(_mm256_srli_epi32(*src_vec.add(i), 31), val_vec[i]);

        eq_tmp[i] = _mm256_and_si256(eq_tmp[i], val_notmask);
        s_tmp[i] = _mm256_and_si256(s_tmp[i], val_notmask);
        val_vec[i] = _mm256_srli_epi32(val_notmask, 31);
    }

    let idx = _mm256_set_epi32(7, 5, 3, 1, 6, 4, 2, 0);
    for i in 0..2usize {
        let tmp1 = _mm256_permutevar8x32_epi32(eq_tmp[i], idx);
        let tmp2 = _mm256_permutevar8x32_epi32(eq_tmp[2 + i], idx);
        *eq_vec.add(i) = _mm256_permute2x128_si256(tmp1, tmp2, (0 << 0) + (2 << 4));
        *eq_vec.add(2 + i) = _mm256_permute2x128_si256(tmp1, tmp2, (1 << 0) + (3 << 4));

        let tmp1 = _mm256_permutevar8x32_epi32(s_tmp[i], idx);
        let tmp2 = _mm256_permutevar8x32_epi32(s_tmp[2 + i], idx);
        *s_vec.add(i) = _mm256_permute2x128_si256(tmp1, tmp2, (0 << 0) + (2 << 4));
        *s_vec.add(2 + i) = _mm256_permute2x128_si256(tmp1, tmp2, (1 << 0) + (3 << 4));

        let tmp1 = _mm256_permutevar8x32_epi32(val_vec[i], idx);
        let tmp2 = _mm256_permutevar8x32_epi32(val_vec[2 + i], idx);
        rho_tmp[i] = _mm256_permute2x128_si256(tmp1, tmp2, (0 << 0) + (2 << 4));
        rho_tmp[2 + i] = _mm256_permute2x128_si256(tmp1, tmp2, (1 << 0) + (3 << 4));
    }

    *e_qmax_vec = _mm256_max_epi32(*eq_vec.add(0), *eq_vec.add(1));
    *e_qmax_vec = _mm256_max_epi32(*e_qmax_vec, *eq_vec.add(2));
    *e_qmax_vec = _mm256_max_epi32(*e_qmax_vec, *eq_vec.add(3));

    rho_tmp[1] = _mm256_slli_epi32(rho_tmp[1], 1);
    rho_tmp[2] = _mm256_slli_epi32(rho_tmp[2], 2);
    rho_tmp[3] = _mm256_slli_epi32(rho_tmp[3], 3);
    *rho_vec = _mm256_or_si256(rho_tmp[0], rho_tmp[1]);
    *rho_vec = _mm256_or_si256(*rho_vec, rho_tmp[2]);
    *rho_vec = _mm256_or_si256(*rho_vec, rho_tmp[3]);
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn rotate_matrix(matrix: *mut __m256i) {
    let mut tmp1 = _mm256_unpacklo_epi32(*matrix.add(0), *matrix.add(1));
    let mut tmp2 = _mm256_unpacklo_epi32(*matrix.add(2), *matrix.add(3));
    let tmp3 = _mm256_unpackhi_epi32(*matrix.add(0), *matrix.add(1));
    let tmp4 = _mm256_unpackhi_epi32(*matrix.add(2), *matrix.add(3));

    *matrix.add(0) = _mm256_unpacklo_epi64(tmp1, tmp2);
    *matrix.add(1) = _mm256_unpacklo_epi64(tmp3, tmp4);
    *matrix.add(2) = _mm256_unpackhi_epi64(tmp1, tmp2);
    *matrix.add(3) = _mm256_unpackhi_epi64(tmp3, tmp4);

    tmp1 = _mm256_permute2x128_si256(*matrix.add(0), *matrix.add(2), 0x20);
    *matrix.add(2) = _mm256_permute2x128_si256(*matrix.add(0), *matrix.add(2), 0x31);
    *matrix.add(0) = tmp1;

    tmp2 = _mm256_permute2x128_si256(*matrix.add(1), *matrix.add(3), 0x20);
    *matrix.add(3) = _mm256_permute2x128_si256(*matrix.add(1), *matrix.add(3), 0x31);
    *matrix.add(1) = tmp2;
}

/// 0.31.0's own local `ms_struct` for the AVX2 encoder: a 64-bit deferred accumulator
/// (`tmp`/`used_bits`) plus `last_was_ff` for the byte-stuffing rule, replacing the generic
/// scalar encoder's `ms_struct` (`max_bits`/`used_bits`/`tmp: Ui32`, eager one-byte-at-a-time)
/// this file used to import and share. Not shared with `ojph_block_encoder.rs` upstream either —
/// see the module doc.
#[allow(non_camel_case_types)]
struct ms_struct {
    buf: *mut Ui8,
    pos: Ui32,
    buf_size: Ui32,
    used_bits: i32,
    tmp: Ui64,
    last_was_ff: bool,
}

impl Default for ms_struct {
    fn default() -> Self {
        Self {
            buf: core::ptr::null_mut(),
            pos: 0,
            buf_size: 0,
            used_bits: 0,
            tmp: 0,
            last_was_ff: false,
        }
    }
}

#[inline]
unsafe fn ms_init(msp: &mut ms_struct, buffer_size: Ui32, data: *mut Ui8) {
    msp.buf = data;
    msp.pos = 0;
    msp.buf_size = buffer_size;
    msp.used_bits = 0;
    msp.tmp = 0;
    msp.last_was_ff = false;
}

/// 0.31.0 `ms_drain`: processes up to 8 bytes per iteration (vs the scalar encoder's one byte at
/// a time) via a SWAR "does any of these bytes equal 0xFF" test on `!tmp`
/// (`(w - 0x0101..) & !w & 0x8080..` finds a zero byte in `w = !tmp`, i.e. an `0xFF` byte in
/// `tmp`). When none of the ready bytes is `0xFF`, all of them drain in one `memcpy`; otherwise
/// only the bytes up to and including the first `0xFF` drain (then, per the byte-stuffing rule,
/// at most 7 more bits from the following byte if a full 7 bits are already available — else
/// `last_was_ff` defers the rest to the next call). The bounds checks are jph-rs's own hardening
/// (C++'s `ms_drain` has none, relying on the ~17KB MS scratch "more than enough" for one
/// code-block) — new error codes since this is new-in-0.31.0 code, not a pre-existing guard.
#[inline]
unsafe fn ms_drain(msp: &mut ms_struct) -> crate::openjph::ojph_message::OjphResult<()> {
    if msp.last_was_ff {
        if msp.used_bits < 7 {
            return Ok(());
        }
        if msp.pos >= msp.buf_size {
            return Err(crate::ojph_error!(
                0x00020007,
                "magnitude sign encoder's buffer is full"
            ));
        }
        *msp.buf.add(msp.pos as usize) = (msp.tmp & 0x7F) as Ui8;
        msp.pos += 1;
        msp.tmp >>= 7;
        msp.used_bits -= 7;
        msp.last_was_ff = false;
    }

    while msp.used_bits >= 8 {
        let mut n_bytes = msp.used_bits >> 3;
        if n_bytes > 8 {
            n_bytes = 8;
        }

        let word = msp.tmp;
        let valid_mask: Ui64 = if n_bytes < 8 {
            (1u64 << (n_bytes * 8)) - 1
        } else {
            u64::MAX
        };
        let w = !word;
        let mut ff_detect = w.wrapping_sub(0x0101_0101_0101_0101) & !w & 0x8080_8080_8080_8080;
        ff_detect &= valid_mask;

        if ff_detect == 0 {
            if msp.pos as u64 + n_bytes as u64 > msp.buf_size as u64 {
                return Err(crate::ojph_error!(
                    0x00020007,
                    "magnitude sign encoder's buffer is full"
                ));
            }
            let bytes = word.to_le_bytes();
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                msp.buf.add(msp.pos as usize),
                n_bytes as usize,
            );
            msp.pos += n_bytes as Ui32;
            if n_bytes < 8 {
                msp.tmp >>= n_bytes * 8;
            } else {
                msp.tmp = 0;
            }
            msp.used_bits -= n_bytes * 8;
        } else {
            let ff_pos = (ff_detect.trailing_zeros() >> 3) as i32;
            let safe = ff_pos + 1;
            if msp.pos as u64 + safe as u64 > msp.buf_size as u64 {
                return Err(crate::ojph_error!(
                    0x00020008,
                    "magnitude sign encoder's buffer is full"
                ));
            }
            let bytes = word.to_le_bytes();
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                msp.buf.add(msp.pos as usize),
                safe as usize,
            );
            msp.pos += safe as Ui32;
            let bits = safe * 8;
            if bits < 64 {
                msp.tmp >>= bits;
            } else {
                msp.tmp = 0;
            }
            msp.used_bits -= bits;

            if msp.used_bits >= 7 {
                if msp.pos >= msp.buf_size {
                    return Err(crate::ojph_error!(
                        0x00020008,
                        "magnitude sign encoder's buffer is full"
                    ));
                }
                *msp.buf.add(msp.pos as usize) = (msp.tmp & 0x7F) as Ui8;
                msp.pos += 1;
                msp.tmp >>= 7;
                msp.used_bits -= 7;
                msp.last_was_ff = false;
            } else {
                msp.last_was_ff = true;
                return Ok(());
            }
        }
    }
    Ok(())
}

/// 0.31.0 `ms_encode_nodefer`: deferred accumulation into the 64-bit `tmp`/`used_bits`, draining
/// only when it would overflow — the MS-side counterpart of the new `vlc_encode`. "nodefer"
/// (upstream's name) refers to the *drain*, not the accumulation: unlike the commented-out
/// `ms_encode` alternative in the C++ source (which defers `ms_drain` to the caller), this
/// version drains immediately whenever the 64-bit accumulator fills, so callers never need to
/// drain mid-sequence themselves (only [`ms_terminate`]/explicit end-of-pass draining is needed).
#[inline]
unsafe fn ms_encode_nodefer(
    msp: &mut ms_struct,
    mut cwd: Ui64,
    mut cwd_len: i32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    loop {
        let avail = 64 - msp.used_bits;
        if avail > 0 && cwd_len <= avail {
            msp.tmp |= cwd << msp.used_bits;
            msp.used_bits += cwd_len;
            return Ok(());
        }
        if avail > 0 {
            msp.tmp |= cwd << msp.used_bits;
        }
        msp.used_bits = 64;
        ms_drain(msp)?;
        cwd >>= avail;
        cwd_len -= avail;
    }
}

/// 0.31.0 `ms_terminate`: drains any remaining bits, then applies the same "don't emit a literal
/// 0xFF" end-of-stream padding rule as the scalar encoder's `ms_terminate`
/// (`ojph_block_encoder.rs`), keyed on `last_was_ff` instead of `max_bits == 7`.
#[inline]
unsafe fn ms_terminate(msp: &mut ms_struct) -> crate::openjph::ojph_message::OjphResult<()> {
    ms_drain(msp)?;
    if msp.used_bits != 0 {
        let max_bits: i32 = if msp.last_was_ff { 7 } else { 8 };
        let t = max_bits - msp.used_bits;
        let mut byte = (msp.tmp & ((1u64 << msp.used_bits) - 1)) as Ui32;
        byte |= (0xFFu32 & ((1u32 << t) - 1)) << msp.used_bits;
        if byte != 0xFF {
            if msp.pos >= msp.buf_size {
                return Err(crate::ojph_error!(
                    0x00020006,
                    "magnitude sign encoder's buffer is full"
                ));
            }
            *msp.buf.add(msp.pos as usize) = byte as Ui8;
            msp.pos += 1;
        }
    } else if msp.last_was_ff {
        msp.pos -= 1;
    }
    Ok(())
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn proc_ms_encode(
    msp: &mut ms_struct,
    tuple_vec: __m256i,
    uq_vec: __m256i,
    rho_vec: __m256i,
    s_vec: *mut __m256i,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let one = _mm256_set1_epi32(1);
    let zero = _mm256_setzero_si256();
    let mut m_vec = [_mm256_setzero_si256(); 4];

    let mut tmp = _mm256_sub_epi32(uq_vec, _mm256_and_si256(tuple_vec, one));
    let mut tmp1 = _mm256_and_si256(rho_vec, one);
    let mut mask = avx2_cmpneq_epi32(tmp1, zero);
    m_vec[0] = _mm256_and_si256(mask, tmp);

    tmp = _mm256_sub_epi32(
        uq_vec,
        _mm256_srli_epi32(_mm256_and_si256(tuple_vec, _mm256_set1_epi32(2)), 1),
    );
    tmp1 = _mm256_and_si256(rho_vec, _mm256_set1_epi32(2));
    mask = avx2_cmpneq_epi32(tmp1, zero);
    m_vec[1] = _mm256_and_si256(mask, tmp);

    tmp = _mm256_sub_epi32(
        uq_vec,
        _mm256_srli_epi32(_mm256_and_si256(tuple_vec, _mm256_set1_epi32(4)), 2),
    );
    tmp1 = _mm256_and_si256(rho_vec, _mm256_set1_epi32(4));
    mask = avx2_cmpneq_epi32(tmp1, zero);
    m_vec[2] = _mm256_and_si256(mask, tmp);

    tmp = _mm256_sub_epi32(
        uq_vec,
        _mm256_srli_epi32(_mm256_and_si256(tuple_vec, _mm256_set1_epi32(8)), 3),
    );
    tmp1 = _mm256_and_si256(rho_vec, _mm256_set1_epi32(8));
    mask = avx2_cmpneq_epi32(tmp1, zero);
    m_vec[3] = _mm256_and_si256(mask, tmp);

    rotate_matrix(m_vec.as_mut_ptr());
    rotate_matrix(s_vec);

    let mut cwd = [0u32; 8];
    let mut cwd_len = [0i32; 8];
    for i in 0..4usize {
        _mm256_storeu_si256(cwd_len.as_mut_ptr() as *mut __m256i, m_vec[i]);
        tmp = _mm256_sub_epi32(_mm256_sllv_epi32(one, m_vec[i]), one);
        tmp = _mm256_and_si256(tmp, *s_vec.add(i));
        _mm256_storeu_si256(cwd.as_mut_ptr() as *mut __m256i, tmp);

        // 0.31.0 merges TWO lane-pairs (4 lanes) per `ms_encode_nodefer` call instead of the
        // pre-rewrite one-pair-per-`ms_encode64` call (half as many calls): combine
        // (idx0, idx0+1, idx1, idx1+1) into one up-to-64-bit codeword when it fits, else emit
        // the first pair immediately and the second as its own call (matches C++'s
        // `_cwd_len + len1 <= 64` fast-path check).
        let mut j = 0usize;
        while j < 4 {
            let idx0 = j * 2;
            let mut packed_cwd = cwd[idx0] as Ui64;
            let mut packed_len = cwd_len[idx0];
            packed_cwd |= (cwd[idx0 + 1] as Ui64) << packed_len;
            packed_len += cwd_len[idx0 + 1];

            let idx1 = (j + 1) * 2;
            let len1 = cwd_len[idx1] + cwd_len[idx1 + 1];
            if packed_len + len1 <= 64 {
                packed_cwd |= (cwd[idx1] as Ui64) << packed_len;
                packed_len += cwd_len[idx1];
                packed_cwd |= (cwd[idx1 + 1] as Ui64) << packed_len;
                packed_len += cwd_len[idx1 + 1];
                ms_encode_nodefer(msp, packed_cwd, packed_len)?;
            } else {
                ms_encode_nodefer(msp, packed_cwd, packed_len)?;
                let mut packed_cwd2 = cwd[idx1] as Ui64;
                let mut packed_len2 = cwd_len[idx1];
                packed_cwd2 |= (cwd[idx1 + 1] as Ui64) << packed_len2;
                packed_len2 += cwd_len[idx1 + 1];
                ms_encode_nodefer(msp, packed_cwd2, packed_len2)?;
            }
            j += 2;
        }
    }
    ms_drain(msp)?;
    Ok(())
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn cal_eps_vec(eq_vec: *const __m256i, u_q_vec: __m256i, e_qmax_vec: __m256i) -> __m256i {
    let zero = _mm256_setzero_si256();
    let u_q_mask = _mm256_cmpgt_epi32(u_q_vec, zero);

    let mut mask = _mm256_cmpeq_epi32(*eq_vec.add(0), e_qmax_vec);
    let mut eps_vec = _mm256_srli_epi32(mask, 31);

    mask = _mm256_cmpeq_epi32(*eq_vec.add(1), e_qmax_vec);
    let mut tmp = _mm256_slli_epi32(_mm256_srli_epi32(mask, 31), 1);
    eps_vec = _mm256_or_si256(eps_vec, tmp);

    mask = _mm256_cmpeq_epi32(*eq_vec.add(2), e_qmax_vec);
    tmp = _mm256_slli_epi32(_mm256_srli_epi32(mask, 31), 2);
    eps_vec = _mm256_or_si256(eps_vec, tmp);

    mask = _mm256_cmpeq_epi32(*eq_vec.add(3), e_qmax_vec);
    tmp = _mm256_slli_epi32(_mm256_srli_epi32(mask, 31), 3);
    _mm256_and_si256(u_q_mask, _mm256_or_si256(eps_vec, tmp))
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn update_lep(
    x: Ui32,
    prev_e_val_vec: &mut __m256i,
    eq_vec: *const __m256i,
    e_val_vec: *mut __m256i,
    left_shift: __m256i,
) {
    let mut tmp = _mm256_permutevar8x32_epi32(*eq_vec.add(3), left_shift);
    tmp = _mm256_insert_epi32(
        tmp,
        _mm_cvtsi128_si32(_mm256_castsi256_si128(*prev_e_val_vec)),
        0,
    );
    *prev_e_val_vec = _mm256_insert_epi32(
        _mm256_setzero_si256(),
        _mm256_extract_epi32(*eq_vec.add(3), 7),
        0,
    );
    *e_val_vec.add(x as usize) = _mm256_max_epi32(*eq_vec.add(1), tmp);
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn update_lcxp(
    x: Ui32,
    prev_cx_val_vec: &mut __m256i,
    rho_vec: __m256i,
    cx_val_vec: *mut __m256i,
    left_shift: __m256i,
) {
    let mut tmp = _mm256_permutevar8x32_epi32(rho_vec, left_shift);
    tmp = _mm256_insert_epi32(
        tmp,
        _mm_cvtsi128_si32(_mm256_castsi256_si128(*prev_cx_val_vec)),
        0,
    );
    *prev_cx_val_vec =
        _mm256_insert_epi32(_mm256_setzero_si256(), _mm256_extract_epi32(rho_vec, 7), 0);

    tmp = _mm256_srli_epi32(_mm256_and_si256(tmp, _mm256_set1_epi32(8)), 3);
    let tmp1 = _mm256_srli_epi32(_mm256_and_si256(rho_vec, _mm256_set1_epi32(2)), 1);
    *cx_val_vec.add(x as usize) = _mm256_or_si256(tmp, tmp1);
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn cal_tuple(
    cq_vec: __m256i,
    rho_vec: __m256i,
    eps_vec: __m256i,
    vlc_tbl: *const Ui32,
) -> __m256i {
    let mut tmp = _mm256_slli_epi32(cq_vec, 8);
    tmp = _mm256_add_epi32(tmp, _mm256_slli_epi32(rho_vec, 4));
    tmp = _mm256_add_epi32(tmp, eps_vec);
    _mm256_i32gather_epi32(vlc_tbl as *const i32, tmp, 4)
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn proc_cq1(
    _x: Ui32,
    _cx_val_vec: *const __m256i,
    rho_vec: __m256i,
    _right_shift: __m256i,
) -> __m256i {
    let tmp = _mm256_srli_epi32(rho_vec, 1);
    let tmp1 = _mm256_and_si256(rho_vec, _mm256_set1_epi32(1));
    _mm256_or_si256(tmp, tmp1)
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn proc_cq2(
    x: Ui32,
    cx_val_vec: *const __m256i,
    rho_vec: __m256i,
    right_shift: __m256i,
) -> __m256i {
    let lcxp1_vec = _mm256_permutevar8x32_epi32(*cx_val_vec.add(x as usize), right_shift);
    let mut tmp = _mm256_permutevar8x32_epi32(lcxp1_vec, right_shift);
    tmp = _mm256_insert_epi64(
        tmp,
        _mm_cvtsi128_si64(_mm256_castsi256_si128(*cx_val_vec.add(x as usize + 1))),
        3,
    );
    tmp = _mm256_slli_epi32(tmp, 2);
    let tmp1 = _mm256_insert_epi32(
        lcxp1_vec,
        _mm_cvtsi128_si32(_mm256_castsi256_si128(*cx_val_vec.add(x as usize + 1))),
        7,
    );
    tmp = _mm256_add_epi32(tmp1, tmp);

    let tmp1 = _mm256_srli_epi32(_mm256_and_si256(rho_vec, _mm256_set1_epi32(4)), 1);
    let tmp = _mm256_or_si256(tmp, tmp1);
    let tmp1 = _mm256_srli_epi32(_mm256_and_si256(rho_vec, _mm256_set1_epi32(8)), 2);
    _mm256_or_si256(tmp, tmp1)
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn proc_mel_encode1(
    melp: &mut mel_struct,
    cq_vec: __m256i,
    rho_vec: __m256i,
    u_q_vec: __m256i,
    ignore: Ui32,
    right_shift: __m256i,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let zero = _mm256_setzero_si256();
    let mut mel_need_encode = [0i32; 8];
    let mut mel_need_encode2 = [0i32; 8];
    let mut mel_bit = [0i32; 8];
    let mut mel_bit2 = [0i32; 8];

    _mm256_storeu_si256(
        mel_need_encode.as_mut_ptr() as *mut __m256i,
        _mm256_cmpeq_epi32(cq_vec, zero),
    );
    _mm256_storeu_si256(
        mel_bit.as_mut_ptr() as *mut __m256i,
        _mm256_srli_epi32(avx2_cmpneq_epi32(rho_vec, zero), 31),
    );

    let tmp = _mm256_permutevar8x32_epi32(u_q_vec, right_shift);
    let tmp1 = _mm256_min_epi32(u_q_vec, tmp);
    _mm256_storeu_si256(
        mel_bit2.as_mut_ptr() as *mut __m256i,
        _mm256_srli_epi32(_mm256_cmpgt_epi32(tmp1, _mm256_set1_epi32(2)), 31),
    );

    let need_encode2 = _mm256_cmpgt_epi32(u_q_vec, zero);
    _mm256_storeu_si256(
        mel_need_encode2.as_mut_ptr() as *mut __m256i,
        _mm256_and_si256(need_encode2, _mm256_cmpgt_epi32(tmp, zero)),
    );

    let i_max = 8 - (ignore / 2);
    let mut i = 0;
    while i < i_max {
        if mel_need_encode[i as usize] != 0 {
            mel_encode(melp, mel_bit[i as usize] != 0)?;
        }
        if i + 1 < i_max && mel_need_encode[(i + 1) as usize] != 0 {
            mel_encode(melp, mel_bit[(i + 1) as usize] != 0)?;
        }
        if mel_need_encode2[i as usize] != 0 {
            mel_encode(melp, mel_bit2[i as usize] != 0)?;
        }
        i += 2;
    }
    Ok(())
}

#[inline]
#[target_feature(enable = "avx2")]
unsafe fn proc_mel_encode2(
    melp: &mut mel_struct,
    cq_vec: __m256i,
    rho_vec: __m256i,
    _u_q_vec: __m256i,
    ignore: Ui32,
    _right_shift: __m256i,
) -> crate::openjph::ojph_message::OjphResult<()> {
    // 0.31.0 rewrite: find the lanes needing a MEL call (`cq == 0`) via `movemask` + a
    // trailing-zero scan instead of testing each of the 8 lanes with a branch (BUG1.md Round 9's
    // "proc_mel_encode2 movemask lane-skip"). `movemask_epi8` packs one bit per BYTE (32 bits for
    // 8 i32 lanes); `& 0x88888888` keeps only each lane's top (sign) byte bit, so `bit_pos / 4`
    // recovers the lane index from a set bit's position.
    let zero = _mm256_setzero_si256();
    let need = _mm256_cmpeq_epi32(cq_vec, zero);
    let mut mask = _mm256_movemask_epi8(need) as u32;
    mask &= 0x8888_8888;

    let i_max = 8 - (ignore / 2);
    if i_max < 8 {
        mask &= (1u32 << (i_max * 4)) - 1;
    }
    if mask == 0 {
        return Ok(());
    }

    let mut mel_bit = [0i32; 8];
    _mm256_storeu_si256(
        mel_bit.as_mut_ptr() as *mut __m256i,
        _mm256_srli_epi32(avx2_cmpneq_epi32(rho_vec, zero), 31),
    );
    while mask != 0 {
        let bit_pos = mask.trailing_zeros();
        let i = (bit_pos / 4) as usize;
        mel_encode(melp, mel_bit[i] != 0)?;
        mask &= mask - 1;
    }
    Ok(())
}

/// 0.31.0 `build_vlc_uvlc_pair`: combines one lane-pair's VLC tuple bits with its UVLC
/// prefix+suffix codeword via a SINGLE `uvlc_tbl` lookup (`uvlc_tbl[uq0*33+uq1]`, `(cwd<<5)|len`
/// packed by [`build_uvlc_pair_table`]) instead of the pre-rewrite `proc_vlc_encode1`/`2`'s four
/// separate conditional `ULVC_CWD_PRE`/`ULVC_CWD_SUF` array reads per pair.
#[inline]
unsafe fn build_vlc_uvlc_pair(
    tuple: *const Ui32,
    u_q: *const Ui32,
    i: usize,
    uvlc_pair_tbl: &[Ui32; 33 * 33],
) -> (Ui64, i32) {
    let mut val = (*tuple.add(i) >> 4) as Ui64;
    let mut size = (*tuple.add(i) & 7) as i32;

    val |= ((*tuple.add(i + 1) >> 4) as Ui64) << size;
    size += (*tuple.add(i + 1) & 7) as i32;

    let uq0 = *u_q.add(i) as usize;
    let uq1 = *u_q.add(i + 1) as usize;
    let entry = uvlc_pair_tbl[uq0 * 33 + uq1];
    val |= ((entry >> 5) as Ui64) << size;
    size += (entry & 0x1F) as i32;

    (val, size)
}

/// 0.31.0 `proc_vlc_encode`: merges TWO lane-pairs (4 lanes) into one `vlc_encode` call when both
/// pairs are present (the `i + 2 < i_max` bulk loop), falling back to a single lane-pair call for
/// the odd one out — half as many `vlc_encode` calls as the pre-rewrite `proc_vlc_encode1`/`2`
/// (which called once per lane-pair). Every index this function or [`build_vlc_uvlc_pair`] reads
/// (`0..8`, since a bulk-loop iteration never reaches index 8 — the maximum `i_max` is 8) is a
/// real SIMD lane; the caller is responsible for zeroing `tuple[i_max]`/`u_q[i_max]` first when
/// `i_max` is odd, so this function never needs to know about padding — see the call site's
/// comment for why that specific zeroing (not a blanket zero-fill) is required.
#[inline]
unsafe fn proc_vlc_encode(
    vlcp: &mut vlc_struct,
    tuple: *const Ui32,
    u_q: *const Ui32,
    ignore: Ui32,
    uvlc_pair_tbl: &[Ui32; 33 * 33],
) -> crate::openjph::ojph_message::OjphResult<()> {
    let i_max = 8 - (ignore / 2);

    let mut i = 0u32;
    while i + 2 < i_max {
        let (val1, size1) = build_vlc_uvlc_pair(tuple, u_q, i as usize, uvlc_pair_tbl);
        let (val2, size2) = build_vlc_uvlc_pair(tuple, u_q, (i + 2) as usize, uvlc_pair_tbl);
        vlc_encode(vlcp, val1 | (val2 << size1), size1 + size2)?;
        i += 4;
    }
    if i < i_max {
        let (val, size) = build_vlc_uvlc_pair(tuple, u_q, i as usize, uvlc_pair_tbl);
        vlc_encode(vlcp, val, size)?;
    }
    Ok(())
}

#[derive(Default, Clone, Copy)]
struct Avx2EncodeProfile {
    loads: Duration,
    proc_pixel: Duration,
    cal_tuple: Duration,
    cq_pipeline: Duration,
    proc_mel: Duration,
    proc_ms: Duration,
    proc_vlc: Duration,
}

impl Avx2EncodeProfile {
    fn total(&self) -> Duration {
        self.loads
            + self.proc_pixel
            + self.cal_tuple
            + self.cq_pipeline
            + self.proc_mel
            + self.proc_ms
            + self.proc_vlc
    }

    fn print(&self, width: Ui32, height: Ui32) {
        let total = self.total();
        if total.is_zero() {
            return;
        }
        let pct = |d: Duration| (d.as_nanos() as f64 * 100.0) / total.as_nanos() as f64;
        eprintln!(
            "OJPH_ENCODE_PROFILE cb={}x{} total={:.3}ms loads={:.1}% proc_pixel={:.1}% cal_tuple(gather)={:.1}% cq_pipeline={:.1}% mel={:.1}% ms={:.1}% vlc={:.1}%",
            width,
            height,
            total.as_secs_f64() * 1000.0,
            pct(self.loads),
            pct(self.proc_pixel),
            pct(self.cal_tuple),
            pct(self.cq_pipeline),
            pct(self.proc_mel),
            pct(self.proc_ms),
            pct(self.proc_vlc),
        );
    }
}

#[inline]
fn encode_profile_enabled() -> bool {
    static ENABLED: LazyLock<bool> = LazyLock::new(|| {
        std::env::var(ENV_ENCODE_PROFILE)
            .ok()
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    });
    *ENABLED
}

#[target_feature(enable = "avx2")]
pub(crate) unsafe fn ojph_encode_codeblock_avx2(
    buf: *mut Ui32,
    missing_msbs: Ui32,
    num_passes: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    lengths: &mut [Ui32; 2],
    elastic: *mut mem_elastic_allocator,
    coded: &mut *mut CodedLists,
) -> crate::openjph::ojph_message::OjphResult<()> {
    let _ = initialize_block_encoder_tables_avx2();
    debug_assert!(num_passes == 1);

    let width_padded = (width + 15) & !15u32;
    let ignore = width_padded - width;
    let ms_size = (16384 * 16 + 14) / 15;
    let mel_vlc_size = 3072usize;
    let mel_size = 192usize;
    let vlc_size = mel_vlc_size - mel_size;

    let mut ms_buf = [0u8; (16384 * 16 + 14) / 15];
    let mut mel_vlc_buf = [0u8; 3072];
    let mel_buf = mel_vlc_buf.as_mut_ptr();
    let vlc_buf = mel_buf.add(mel_size);

    let mut mel = mel_struct::default();
    mel_init(&mut mel, mel_size as Ui32, mel_buf);
    let mut vlc = vlc_struct::default();
    vlc_init(&mut vlc, vlc_size as Ui32, vlc_buf);
    let mut ms = ms_struct::default();
    ms_init(&mut ms, ms_size as Ui32, ms_buf.as_mut_ptr());

    let p = 30 - missing_msbs;
    let lzcnt_mode = get_avx2_lzcnt_mode();
    let right_shift = _mm256_set_epi32(0, 7, 6, 5, 4, 3, 2, 1);
    let left_shift = _mm256_set_epi32(6, 5, 4, 3, 2, 1, 0, 7);
    let one = _mm256_set1_epi32(1);
    let zero = _mm256_setzero_si256();
    let n_loop = (width_padded + 15) / 16;

    let mut e_val_vec = [zero; 65];
    let mut cx_val_vec = [zero; 65];
    let mut prev_e_val_vec = zero;
    let mut prev_cx_val_vec = zero;
    let mut prev_cq = 0u32;
    let mut eq_vec = [zero; 4];
    let mut s_vec = [zero; 4];
    let mut src_vec = [zero; 4];

    let profile_on = encode_profile_enabled();
    let mut prof = Avx2EncodeProfile::default();

    let mut y = 0;
    while y < height {
        let line2 = y > 0;
        let vlc_tbl = if line2 {
            VLC_TBL1_AVX2.as_ptr()
        } else {
            VLC_TBL0_AVX2.as_ptr()
        };

        e_val_vec[n_loop as usize] = prev_e_val_vec;
        let mut tmp = _mm256_and_si256(prev_cx_val_vec, _mm256_set1_epi32(8));
        cx_val_vec[n_loop as usize] = _mm256_srli_epi32(tmp, 3);

        prev_e_val_vec = zero;
        prev_cx_val_vec = zero;

        let mut sp = buf.add((y * stride) as usize);
        for x in 0..n_loop as usize {
            let t_load = profile_on.then(Instant::now);
            if x == (n_loop as usize - 1) && (width & 15) != 0 {
                let mut tmp_buf = [0u32; 16];
                core::ptr::copy_nonoverlapping(sp, tmp_buf.as_mut_ptr(), (width & 15) as usize);
                src_vec[0] = _mm256_loadu_si256(tmp_buf.as_ptr() as *const __m256i);
                src_vec[2] = _mm256_loadu_si256(tmp_buf.as_ptr().add(8) as *const __m256i);
                if y + 1 < height {
                    tmp_buf = [0u32; 16];
                    core::ptr::copy_nonoverlapping(
                        sp.add(stride as usize),
                        tmp_buf.as_mut_ptr(),
                        (width & 15) as usize,
                    );
                    src_vec[1] = _mm256_loadu_si256(tmp_buf.as_ptr() as *const __m256i);
                    src_vec[3] = _mm256_loadu_si256(tmp_buf.as_ptr().add(8) as *const __m256i);
                } else {
                    src_vec[1] = zero;
                    src_vec[3] = zero;
                }
            } else {
                src_vec[0] = _mm256_loadu_si256(sp as *const __m256i);
                src_vec[2] = _mm256_loadu_si256(sp.add(8) as *const __m256i);
                if y + 1 < height {
                    src_vec[1] = _mm256_loadu_si256(sp.add(stride as usize) as *const __m256i);
                    src_vec[3] =
                        _mm256_loadu_si256(sp.add((stride + 8) as usize) as *const __m256i);
                } else {
                    src_vec[1] = zero;
                    src_vec[3] = zero;
                }
                sp = sp.add(16);
            }
            if let Some(t0) = t_load {
                prof.loads += t0.elapsed();
            }

            let mut rho_vec = zero;
            let mut e_qmax_vec = zero;

            let t_pixel = profile_on.then(Instant::now);
            proc_pixel(
                src_vec.as_ptr(),
                p,
                lzcnt_mode,
                eq_vec.as_mut_ptr(),
                s_vec.as_mut_ptr(),
                &mut rho_vec,
                &mut e_qmax_vec,
            );
            if let Some(t0) = t_pixel {
                prof.proc_pixel += t0.elapsed();
            }

            let t_cq = profile_on.then(Instant::now);
            tmp = _mm256_permutevar8x32_epi32(e_val_vec[x], right_shift);
            tmp = _mm256_insert_epi32(
                tmp,
                _mm_cvtsi128_si32(_mm256_castsi256_si128(e_val_vec[x + 1])),
                7,
            );
            let max_e_vec = _mm256_sub_epi32(_mm256_max_epi32(tmp, e_val_vec[x]), one);

            tmp = _mm256_max_epi32(max_e_vec, one);
            let tmp1 = _mm256_and_si256(rho_vec, _mm256_sub_epi32(rho_vec, one));
            let cmp = _mm256_cmpeq_epi32(tmp1, zero);
            let kappa_vec1_ = _mm256_and_si256(cmp, one);
            let kappa_vec2_ = _mm256_and_si256(
                _mm256_xor_si256(cmp, _mm256_set1_epi32(0xFFFF_FFFFu32 as i32)),
                tmp,
            );
            let kappa_vec = _mm256_max_epi32(kappa_vec1_, kappa_vec2_);

            tmp = if line2 {
                proc_cq2(x as Ui32, cx_val_vec.as_ptr(), rho_vec, right_shift)
            } else {
                proc_cq1(x as Ui32, cx_val_vec.as_ptr(), rho_vec, right_shift)
            };
            let mut cq_vec = _mm256_permutevar8x32_epi32(tmp, left_shift);
            cq_vec = _mm256_insert_epi32(cq_vec, prev_cq as i32, 0);
            prev_cq = _mm256_extract_epi32(tmp, 7) as Ui32;

            update_lep(
                x as Ui32,
                &mut prev_e_val_vec,
                eq_vec.as_ptr(),
                e_val_vec.as_mut_ptr(),
                left_shift,
            );
            update_lcxp(
                x as Ui32,
                &mut prev_cx_val_vec,
                rho_vec,
                cx_val_vec.as_mut_ptr(),
                left_shift,
            );

            let uq_vec = _mm256_max_epi32(kappa_vec, e_qmax_vec);
            let u_q_vec = _mm256_sub_epi32(uq_vec, kappa_vec);
            let eps_vec = cal_eps_vec(eq_vec.as_ptr(), u_q_vec, e_qmax_vec);
            if let Some(t0) = t_cq {
                prof.cq_pipeline += t0.elapsed();
            }

            let t_tuple = profile_on.then(Instant::now);
            let mut tuple_vec = cal_tuple(cq_vec, rho_vec, eps_vec, vlc_tbl);
            if let Some(t0) = t_tuple {
                prof.cal_tuple += t0.elapsed();
            }
            let local_ignore = if x == n_loop as usize - 1 { ignore } else { 0 };

            let t_mel = profile_on.then(Instant::now);
            if line2 {
                proc_mel_encode2(
                    &mut mel,
                    cq_vec,
                    rho_vec,
                    u_q_vec,
                    local_ignore,
                    right_shift,
                )?;
            } else {
                proc_mel_encode1(
                    &mut mel,
                    cq_vec,
                    rho_vec,
                    u_q_vec,
                    local_ignore,
                    right_shift,
                )?;
            }
            if let Some(t0) = t_mel {
                prof.proc_mel += t0.elapsed();
            }

            let t_ms = profile_on.then(Instant::now);
            proc_ms_encode(&mut ms, tuple_vec, uq_vec, rho_vec, s_vec.as_mut_ptr())?;
            if let Some(t0) = t_ms {
                prof.proc_ms += t0.elapsed();
            }

            let mut u_q = [0u32; 8];
            let mut tuple = [0u32; 8];
            tuple_vec = _mm256_srli_epi32(tuple_vec, 4);
            _mm256_storeu_si256(tuple.as_mut_ptr() as *mut __m256i, tuple_vec);
            _mm256_storeu_si256(u_q.as_mut_ptr() as *mut __m256i, u_q_vec);
            // `build_vlc_uvlc_pair(i)` always merges lane `i+1`'s u_q into the SAME UVLC
            // codeword as lane `i` (the HT spec's U-values are always coded in pairs, real or
            // not), but must NOT do the same for lane `i+1`'s VLC tuple bits when that lane is
            // past `i_max` (padding, not a real quad) — the tuple side has no such pairing
            // requirement. `tuple`/`u_q` hold real (possibly nonzero, context-propagated from
            // neighboring quads) SIMD results for every one of the 8 lanes regardless of
            // `i_max`, so when `i_max` is odd, the first invalid index's *tuple* entry must be
            // explicitly zeroed before the merge (0.31.0 `if (i_max & 1) { tuple[i_max] = 0;
            // u_q[i_max] = 0; }` — `u_q[i_max]` is zeroed too since a zero-magnitude quad has no
            // U-value to contribute, keeping the forced pairing correct). Missing this exact
            // step first produced a real divergence from the generic encoder on every
            // odd-`i_max` (i.e. non-16-aligned width) code-block — caught by
            // `coding::codeblock_fuzz_tests::simd_encoders_byte_identical_to_generic`.
            let i_max = 8 - (local_ignore / 2);
            if i_max % 2 == 1 {
                tuple[i_max as usize] = 0;
                u_q[i_max as usize] = 0;
            }
            let t_vlc = profile_on.then(Instant::now);
            let uvlc_pair_tbl = if line2 {
                &*UVLC_TBL_PAIR2
            } else {
                &*UVLC_TBL_PAIR1
            };
            proc_vlc_encode(
                &mut vlc,
                tuple.as_ptr(),
                u_q.as_ptr(),
                local_ignore,
                uvlc_pair_tbl,
            )?;
            if let Some(t0) = t_vlc {
                prof.proc_vlc += t0.elapsed();
            }
        }

        tmp = _mm256_permutevar8x32_epi32(cx_val_vec[0], right_shift);
        tmp = _mm256_slli_epi32(tmp, 2);
        tmp = _mm256_add_epi32(tmp, cx_val_vec[0]);
        prev_cq = _mm_cvtsi128_si32(_mm256_castsi256_si128(tmp)) as Ui32;

        y += 2;
    }

    ms_terminate(&mut ms)?;
    // 0.31.0 order: explicit `vlc_drain` before `terminate_mel_vlc` — required now that
    // `vlc_encode` defers draining, unlike the pre-rewrite eager-drain version where `vlc.used_bits`
    // was always already < 8 by the time termination ran.
    vlc_drain(&mut vlc)?;
    terminate_mel_vlc_avx2(&mut mel, &mut vlc)?;
    let total = mel.pos + vlc.pos + ms.pos;
    (*elastic).get_buffer(total, coded);
    core::ptr::copy_nonoverlapping(ms.buf, (**coded).buf, ms.pos as usize);
    core::ptr::copy_nonoverlapping(
        mel.buf,
        (**coded).buf.add(ms.pos as usize),
        mel.pos as usize,
    );
    core::ptr::copy_nonoverlapping(
        vlc.buf.sub(vlc.pos as usize - 1),
        (**coded).buf.add((ms.pos + mel.pos) as usize),
        vlc.pos as usize,
    );
    lengths[0] = total;
    let num_bytes = mel.pos + vlc.pos;
    *((**coded).buf.add(total as usize - 1)) = (num_bytes >> 4) as Ui8;
    let last2 = (**coded).buf.add(total as usize - 2);
    *last2 &= 0xF0;
    *last2 |= (num_bytes & 0xF) as Ui8;
    (**coded).avail_size -= total;

    if profile_on {
        prof.print(width, height);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        avx2_lzcnt_epi32, avx2_lzcnt_epi32_emulated, avx2_lzcnt_epi32_hw, parse_avx2_lzcnt_mode,
        Avx2LzcntMode,
    };

    #[test]
    fn parse_avx2_lzcnt_mode_values() {
        assert_eq!(
            parse_avx2_lzcnt_mode("emulated"),
            Some(Avx2LzcntMode::Emulated)
        );
        assert_eq!(parse_avx2_lzcnt_mode("hw"), Some(Avx2LzcntMode::Hardware));
        assert_eq!(parse_avx2_lzcnt_mode("auto"), Some(Avx2LzcntMode::Auto));
        assert_eq!(parse_avx2_lzcnt_mode("nope"), None);
    }

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn lane_u32(v: core::arch::x86_64::__m256i, i: usize) -> u32 {
        let mut lanes = [0u32; 8];
        unsafe {
            core::arch::x86_64::_mm256_storeu_si256(
                lanes.as_mut_ptr() as *mut core::arch::x86_64::__m256i,
                v,
            );
        }
        lanes[i]
    }

    #[test]
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn avx2_lzcnt_hw_matches_emulated() {
        if !std::arch::is_x86_feature_detected!("lzcnt") {
            return;
        }
        for bits in 0u32..=20 {
            for v in [0u32, 1, 2, 3, 7, 8, 15, 16, 0x7FFF_FFFF, 0x8000_0000] {
                let x = v | (bits << 10);
                let vec = unsafe { core::arch::x86_64::_mm256_set1_epi32(x as i32) };
                let emu = unsafe { avx2_lzcnt_epi32_emulated(vec) };
                let hw = unsafe { avx2_lzcnt_epi32_hw(vec) };
                assert_eq!(lane_u32(emu, 0), lane_u32(hw, 0), "mismatch for x={x}");
                let via_fn = unsafe { avx2_lzcnt_epi32(vec, Avx2LzcntMode::Hardware) };
                assert_eq!(lane_u32(emu, 0), lane_u32(via_fn, 0));
            }
        }
        let vec = unsafe { core::arch::x86_64::_mm256_setzero_si256() };
        let _ = unsafe { avx2_lzcnt_epi32(vec, Avx2LzcntMode::Emulated) };
    }
}
