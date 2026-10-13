//! Formatter dispatch tables and selection (`InputFormatters16`, `OutputFormatters16`,
//! `InputFormattersFloat`, `OutputFormattersFloat` and `_cmsGetStock*Formatter` in
//! `cmspack.c`).
//!
//! Selection walks the table in order and returns the first row whose masked `Type` matches:
//! `(dwFormat & !Mask) == Type` — bits set in `Mask` are "don't care". The row order and the
//! `Type`/`Mask` values are a 1:1 transcription of the C tables, so the same engine is
//! selected for every format word. Specialised rows that are bit-identical to a generic
//! engine point at that engine (see the [module docs](super)).
//!
//! IEEE-half rows are gated behind the `half-tables` feature. Because a half format
//! (`FLOAT|BYTES(2)`) is disjoint from every other row's `Type`, moving the half check to a
//! feature-gated fallback after the base table is behaviour-preserving.
//!
//! # Plugin factories
//!
//! The full C `_cmsGetFormatter` first consults a chain of plug-in `cmsFormatterFactory`s and
//! only falls back to these stock tables. Each `get_*_formatter_*` here consults the
//! process-global [`crate::plugin`] registry first (`find_*_formatter_*`) and then these stock
//! tables, so a registered factory wins exactly as in C; with no plugin registered the stock
//! path is taken unchanged.

use super::input;
use super::output;
use super::pt;
use super::{Formatter16In, Formatter16Out, FormatterFloatIn, FormatterFloatOut};
use crate::context::Context;
use crate::types::format::PixelFormat;

// --- `*_SH` field builders (mirroring the lcms2.h OR-together macros) ---
const fn float_sh(a: u32) -> u32 {
    a << 22
}
const fn optimized_sh(a: u32) -> u32 {
    a << 21
}
const fn colorspace_sh(a: u32) -> u32 {
    a << 16
}
const fn swapfirst_sh(a: u32) -> u32 {
    a << 14
}
const fn flavor_sh(a: u32) -> u32 {
    a << 13
}
const fn planar_sh(a: u32) -> u32 {
    a << 12
}
const fn endian16_sh(a: u32) -> u32 {
    a << 11
}
const fn doswap_sh(a: u32) -> u32 {
    a << 10
}
const fn extra_sh(a: u32) -> u32 {
    a << 7
}
const fn channels_sh(a: u32) -> u32 {
    a << 3
}
const fn bytes_sh(a: u32) -> u32 {
    a
}
const fn premul_sh(a: u32) -> u32 {
    a << 23
}

// --- ANY* "don't care" masks (cmspack.c L74-82) ---
const ANYSPACE: u32 = colorspace_sh(31);
const ANYCHANNELS: u32 = channels_sh(15);
const ANYEXTRA: u32 = extra_sh(7);
const ANYPLANAR: u32 = planar_sh(1);
const ANYENDIAN: u32 = endian16_sh(1);
const ANYSWAP: u32 = doswap_sh(1);
const ANYSWAPFIRST: u32 = swapfirst_sh(1);
const ANYFLAVOR: u32 = flavor_sh(1);
const ANYPREMUL: u32 = premul_sh(1);

// --- concrete TYPE_* words used as table keys (lcms2.h) ---
const TYPE_LAB_DBL: u32 = float_sh(1) | colorspace_sh(pt::LAB) | channels_sh(3) | bytes_sh(0);
const TYPE_XYZ_DBL: u32 = float_sh(1) | colorspace_sh(pt::XYZ) | channels_sh(3) | bytes_sh(0);
const TYPE_LAB_FLT: u32 = float_sh(1) | colorspace_sh(pt::LAB) | channels_sh(3) | bytes_sh(4);
const TYPE_XYZ_FLT: u32 = float_sh(1) | colorspace_sh(pt::XYZ) | channels_sh(3) | bytes_sh(4);
const TYPE_GRAY_DBL: u32 = float_sh(1) | colorspace_sh(pt::GRAY) | channels_sh(1) | bytes_sh(0);
const TYPE_LABV2_8: u32 = colorspace_sh(pt::LAB_V2) | channels_sh(3) | bytes_sh(1);
const TYPE_ALABV2_8: u32 =
    colorspace_sh(pt::LAB_V2) | channels_sh(3) | bytes_sh(1) | extra_sh(1) | swapfirst_sh(1);
const TYPE_LABV2_16: u32 = colorspace_sh(pt::LAB_V2) | channels_sh(3) | bytes_sh(2);

// ============================================================================================
// Input 16-bit table (InputFormatters16, cmspack.c L3573)
// ============================================================================================

#[rustfmt::skip]
static INPUT_16: &[(u32, u32, Formatter16In)] = &[
    (TYPE_LAB_DBL,  ANYPLANAR | ANYEXTRA, input::unroll_lab_double_to_16),
    (TYPE_XYZ_DBL,  ANYPLANAR | ANYEXTRA, input::unroll_xyz_double_to_16),
    (TYPE_LAB_FLT,  ANYPLANAR | ANYEXTRA, input::unroll_lab_float_to_16),
    (TYPE_XYZ_FLT,  ANYPLANAR | ANYEXTRA, input::unroll_xyz_float_to_16),
    (TYPE_GRAY_DBL, 0, input::unroll_double_1chan),
    (float_sh(1) | bytes_sh(0),
        ANYCHANNELS | ANYPLANAR | ANYSWAPFIRST | ANYFLAVOR | ANYSWAP | ANYEXTRA | ANYSPACE,
        input::unroll_double_to_16),
    (float_sh(1) | bytes_sh(4),
        ANYCHANNELS | ANYPLANAR | ANYSWAPFIRST | ANYFLAVOR | ANYSWAP | ANYEXTRA | ANYSPACE,
        input::unroll_float_to_16),
    // half row -> feature-gated fallback below

    (channels_sh(1) | bytes_sh(1),               ANYSPACE, input::unroll_1_byte),
    (channels_sh(1) | bytes_sh(1) | extra_sh(1), ANYSPACE, input::unroll_1_byte_skip1),
    (channels_sh(1) | bytes_sh(1) | extra_sh(2), ANYSPACE, input::unroll_1_byte_skip2),
    (channels_sh(1) | bytes_sh(1) | flavor_sh(1), ANYSPACE, input::unroll_1_byte_reversed),
    (colorspace_sh(pt::MCH2) | channels_sh(2) | bytes_sh(1), 0, input::unroll_chunky_bytes),

    (TYPE_LABV2_8,  0, input::unroll_lab_v2_8),
    (TYPE_ALABV2_8, 0, input::unroll_a_lab_v2_8),
    (TYPE_LABV2_16, 0, input::unroll_lab_v2_16),

    (channels_sh(3) | bytes_sh(1),                                ANYSPACE, input::unroll_chunky_bytes),
    (channels_sh(3) | bytes_sh(1) | doswap_sh(1),                 ANYSPACE, input::unroll_chunky_bytes),
    (channels_sh(3) | extra_sh(1) | bytes_sh(1) | doswap_sh(1),   ANYSPACE, input::unroll_chunky_bytes),
    (channels_sh(3) | extra_sh(1) | bytes_sh(1) | swapfirst_sh(1), ANYSPACE, input::unroll_chunky_bytes),
    (channels_sh(3) | extra_sh(1) | bytes_sh(1) | doswap_sh(1) | swapfirst_sh(1),
        ANYSPACE, input::unroll_chunky_bytes),

    (channels_sh(4) | bytes_sh(1),                                ANYSPACE, input::unroll_chunky_bytes),
    (channels_sh(4) | bytes_sh(1) | flavor_sh(1),                 ANYSPACE, input::unroll_chunky_bytes),
    (channels_sh(4) | bytes_sh(1) | swapfirst_sh(1),              ANYSPACE, input::unroll_chunky_bytes),
    (channels_sh(4) | bytes_sh(1) | doswap_sh(1),                 ANYSPACE, input::unroll_chunky_bytes),
    (channels_sh(4) | bytes_sh(1) | doswap_sh(1) | swapfirst_sh(1), ANYSPACE, input::unroll_chunky_bytes),

    (bytes_sh(1) | planar_sh(1),
        ANYFLAVOR | ANYSWAPFIRST | ANYPREMUL | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        input::unroll_planar_bytes),
    (bytes_sh(1),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYPREMUL | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        input::unroll_chunky_bytes),

    (channels_sh(1) | bytes_sh(2),                ANYSPACE, input::unroll_1_word),
    (channels_sh(1) | bytes_sh(2) | flavor_sh(1), ANYSPACE, input::unroll_1_word_reversed),
    (channels_sh(1) | bytes_sh(2) | extra_sh(3),  ANYSPACE, input::unroll_1_word_skip3),

    (channels_sh(2) | bytes_sh(2),                ANYSPACE, input::unroll_any_words),
    (channels_sh(3) | bytes_sh(2),                ANYSPACE, input::unroll_any_words),
    (channels_sh(4) | bytes_sh(2),                ANYSPACE, input::unroll_any_words),

    (channels_sh(3) | bytes_sh(2) | doswap_sh(1),                 ANYSPACE, input::unroll_any_words),
    (channels_sh(3) | bytes_sh(2) | extra_sh(1) | swapfirst_sh(1), ANYSPACE, input::unroll_any_words),
    (channels_sh(3) | bytes_sh(2) | extra_sh(1) | doswap_sh(1),   ANYSPACE, input::unroll_any_words),
    (channels_sh(4) | bytes_sh(2) | flavor_sh(1),                 ANYSPACE, input::unroll_any_words),
    (channels_sh(4) | bytes_sh(2) | swapfirst_sh(1),              ANYSPACE, input::unroll_any_words),
    (channels_sh(4) | bytes_sh(2) | doswap_sh(1),                 ANYSPACE, input::unroll_any_words),
    (channels_sh(4) | bytes_sh(2) | doswap_sh(1) | swapfirst_sh(1), ANYSPACE, input::unroll_any_words),

    (bytes_sh(2) | planar_sh(1),
        ANYFLAVOR | ANYSWAP | ANYENDIAN | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        input::unroll_planar_words),
    (bytes_sh(2),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYENDIAN | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        input::unroll_any_words),

    (bytes_sh(2) | planar_sh(1),
        ANYFLAVOR | ANYSWAP | ANYENDIAN | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        input::unroll_planar_words_premul),
    (bytes_sh(2),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYENDIAN | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        input::unroll_any_words_premul),
];

#[cfg(feature = "half-tables")]
const HALF_IN_16: (u32, u32) = (
    float_sh(1) | bytes_sh(2),
    ANYCHANNELS | ANYPLANAR | ANYSWAPFIRST | ANYFLAVOR | ANYEXTRA | ANYSWAP | ANYSPACE,
);

/// Stock 16-bit input formatter for `fmt` (`_cmsGetStockInputFormatter`, 16-bit path).
pub fn get_input_formatter_16(fmt: PixelFormat) -> Option<Formatter16In> {
    get_input_formatter_16_for(None, fmt)
}

/// [`get_input_formatter_16`] against the factories visible to `ctx`.
///
/// C's `_cmsGetFormatter` takes the `ContextID` as its first argument because the factory chain
/// it walks is a per-context chunk; this is that form, and the plain one is the `NULL` call.
pub fn get_input_formatter_16_thr(ctx: &Context, fmt: PixelFormat) -> Option<Formatter16In> {
    get_input_formatter_16_for(Some(ctx), fmt)
}

/// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
fn get_input_formatter_16_for(
    ctx: Option<&Context>,
    fmt: PixelFormat,
) -> Option<Formatter16In> {
    // Plugin formatter factories (`_cmsRegisterFormattersPlugin`) are consulted first, exactly
    // as `_cmsGetFormatter` walks the context factory chain before the stock tables.
    if let Some(f) = crate::plugin::find_input_formatter_16(ctx, fmt) {
        return Some(f);
    }
    let dw = fmt.raw();
    for &(ty, mask, f) in INPUT_16 {
        if (dw & !mask) == ty {
            return Some(f);
        }
    }
    #[cfg(feature = "half-tables")]
    {
        let (ty, mask) = HALF_IN_16;
        if (dw & !mask) == ty {
            return Some(input::unroll_half_to_16);
        }
    }
    None
}

// ============================================================================================
// Input float table (InputFormattersFloat, cmspack.c L3648)
// ============================================================================================

#[rustfmt::skip]
static INPUT_FLOAT: &[(u32, u32, FormatterFloatIn)] = &[
    (TYPE_LAB_DBL, ANYPLANAR | ANYEXTRA, input::unroll_lab_double_to_float),
    (TYPE_LAB_FLT, ANYPLANAR | ANYEXTRA, input::unroll_lab_float_to_float),
    (TYPE_XYZ_DBL, ANYPLANAR | ANYEXTRA, input::unroll_xyz_double_to_float),
    (TYPE_XYZ_FLT, ANYPLANAR | ANYEXTRA, input::unroll_xyz_float_to_float),
    (float_sh(1) | bytes_sh(4),
        ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYPREMUL | ANYCHANNELS | ANYSPACE,
        input::unroll_floats_to_float),
    (float_sh(1) | bytes_sh(0),
        ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        input::unroll_doubles_to_float),
    (TYPE_LABV2_8,  0, input::unroll_lab_v2_8_to_float),
    (TYPE_ALABV2_8, 0, input::unroll_a_lab_v2_8_to_float),
    (TYPE_LABV2_16, 0, input::unroll_lab_v2_16_to_float),
    (bytes_sh(1),
        ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        input::unroll_8_to_float),
    (bytes_sh(2),
        ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        input::unroll_16_to_float),
    // half row -> feature-gated fallback below
];

#[cfg(feature = "half-tables")]
const HALF_IN_FLOAT: (u32, u32) = (
    float_sh(1) | bytes_sh(2),
    ANYPLANAR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
);

/// Stock float input formatter for `fmt` (`_cmsGetStockInputFormatter`, float path).
pub fn get_input_formatter_float(fmt: PixelFormat) -> Option<FormatterFloatIn> {
    get_input_formatter_float_for(None, fmt)
}

/// [`get_input_formatter_float`] against the factories visible to `ctx` (`_cmsGetFormatter`).
pub fn get_input_formatter_float_thr(
    ctx: &Context,
    fmt: PixelFormat,
) -> Option<FormatterFloatIn> {
    get_input_formatter_float_for(Some(ctx), fmt)
}

/// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
fn get_input_formatter_float_for(
    ctx: Option<&Context>,
    fmt: PixelFormat,
) -> Option<FormatterFloatIn> {
    if let Some(f) = crate::plugin::find_input_formatter_float(ctx, fmt) {
        return Some(f);
    }
    let dw = fmt.raw();
    for &(ty, mask, f) in INPUT_FLOAT {
        if (dw & !mask) == ty {
            return Some(f);
        }
    }
    #[cfg(feature = "half-tables")]
    {
        let (ty, mask) = HALF_IN_FLOAT;
        if (dw & !mask) == ty {
            return Some(input::unroll_half_to_float);
        }
    }
    None
}

// ============================================================================================
// Output 16-bit table (OutputFormatters16, cmspack.c L3721)
// ============================================================================================

#[rustfmt::skip]
static OUTPUT_16: &[(u32, u32, Formatter16Out)] = &[
    (TYPE_LAB_DBL, ANYPLANAR | ANYEXTRA, output::pack_lab_double_from_16),
    (TYPE_XYZ_DBL, ANYPLANAR | ANYEXTRA, output::pack_xyz_double_from_16),
    (TYPE_LAB_FLT, ANYPLANAR | ANYEXTRA, output::pack_lab_float_from_16),
    (TYPE_XYZ_FLT, ANYPLANAR | ANYEXTRA, output::pack_xyz_float_from_16),
    (float_sh(1) | bytes_sh(0),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYCHANNELS | ANYPLANAR | ANYEXTRA | ANYSPACE,
        output::pack_double_from_16),
    (float_sh(1) | bytes_sh(4),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYCHANNELS | ANYPLANAR | ANYEXTRA | ANYSPACE,
        output::pack_float_from_16),
    // half row -> feature-gated fallback below

    (channels_sh(1) | bytes_sh(1),                                ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(1) | bytes_sh(1) | extra_sh(1),                  ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(1) | bytes_sh(1) | extra_sh(1) | swapfirst_sh(1), ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(1) | bytes_sh(1) | flavor_sh(1),                 ANYSPACE, output::pack_chunky_bytes),

    (TYPE_LABV2_8,  0, output::pack_lab_v2_8),
    (TYPE_ALABV2_8, 0, output::pack_a_lab_v2_8),
    (TYPE_LABV2_16, 0, output::pack_lab_v2_16),

    // "Optimized" rows: never matched in the stock path (OPTIMIZED cleared before matching),
    // retained for 1:1 table fidelity.
    (channels_sh(3) | bytes_sh(1) | optimized_sh(1),              ANYSPACE, output::pack_3_bytes_optimized),
    (channels_sh(3) | bytes_sh(1) | extra_sh(1) | optimized_sh(1), ANYSPACE, output::pack_3_bytes_and_skip1_optimized),
    (channels_sh(3) | bytes_sh(1) | extra_sh(1) | swapfirst_sh(1) | optimized_sh(1),
        ANYSPACE, output::pack_3_bytes_and_skip1_swap_first_optimized),
    (channels_sh(3) | bytes_sh(1) | extra_sh(1) | doswap_sh(1) | swapfirst_sh(1) | optimized_sh(1),
        ANYSPACE, output::pack_3_bytes_and_skip1_swap_swap_first_optimized),
    (channels_sh(3) | bytes_sh(1) | doswap_sh(1) | extra_sh(1) | optimized_sh(1),
        ANYSPACE, output::pack_3_bytes_and_skip1_swap_optimized),
    (channels_sh(3) | bytes_sh(1) | doswap_sh(1) | optimized_sh(1),
        ANYSPACE, output::pack_3_bytes_swap_optimized),

    (channels_sh(3) | bytes_sh(1),                                ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(3) | bytes_sh(1) | extra_sh(1),                  ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(3) | bytes_sh(1) | extra_sh(1) | swapfirst_sh(1), ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(3) | bytes_sh(1) | extra_sh(1) | doswap_sh(1) | swapfirst_sh(1),
        ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(3) | bytes_sh(1) | doswap_sh(1) | extra_sh(1),   ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(3) | bytes_sh(1) | doswap_sh(1),                 ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(4) | bytes_sh(1),                                ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(4) | bytes_sh(1) | flavor_sh(1),                 ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(4) | bytes_sh(1) | swapfirst_sh(1),              ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(4) | bytes_sh(1) | doswap_sh(1),                 ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(4) | bytes_sh(1) | doswap_sh(1) | swapfirst_sh(1), ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(6) | bytes_sh(1),                                ANYSPACE, output::pack_chunky_bytes),
    (channels_sh(6) | bytes_sh(1) | doswap_sh(1),                 ANYSPACE, output::pack_chunky_bytes),

    (bytes_sh(1),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        output::pack_chunky_bytes),
    (bytes_sh(1) | planar_sh(1),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        output::pack_planar_bytes),

    (channels_sh(1) | bytes_sh(2),                                ANYSPACE, output::pack_chunky_words),
    (channels_sh(1) | bytes_sh(2) | extra_sh(1),                  ANYSPACE, output::pack_chunky_words),
    (channels_sh(1) | bytes_sh(2) | extra_sh(1) | swapfirst_sh(1), ANYSPACE, output::pack_chunky_words),
    (channels_sh(1) | bytes_sh(2) | flavor_sh(1),                 ANYSPACE, output::pack_chunky_words),
    (channels_sh(1) | bytes_sh(2) | endian16_sh(1),              ANYSPACE, output::pack_chunky_words),
    (channels_sh(3) | bytes_sh(2),                                ANYSPACE, output::pack_chunky_words),
    (channels_sh(3) | bytes_sh(2) | doswap_sh(1),                 ANYSPACE, output::pack_chunky_words),
    (channels_sh(3) | bytes_sh(2) | endian16_sh(1),              ANYSPACE, output::pack_chunky_words),
    (channels_sh(3) | bytes_sh(2) | extra_sh(1),                  ANYSPACE, output::pack_chunky_words),
    (channels_sh(3) | bytes_sh(2) | extra_sh(1) | doswap_sh(1),   ANYSPACE, output::pack_chunky_words),
    (channels_sh(3) | bytes_sh(2) | extra_sh(1) | swapfirst_sh(1), ANYSPACE, output::pack_chunky_words),
    (channels_sh(3) | bytes_sh(2) | extra_sh(1) | doswap_sh(1) | swapfirst_sh(1),
        ANYSPACE, output::pack_chunky_words),
    (channels_sh(4) | bytes_sh(2),                                ANYSPACE, output::pack_chunky_words),
    (channels_sh(4) | bytes_sh(2) | flavor_sh(1),                 ANYSPACE, output::pack_chunky_words),
    (channels_sh(4) | bytes_sh(2) | doswap_sh(1),                 ANYSPACE, output::pack_chunky_words),
    (channels_sh(4) | bytes_sh(2) | endian16_sh(1),              ANYSPACE, output::pack_chunky_words),
    (channels_sh(6) | bytes_sh(2),                                ANYSPACE, output::pack_chunky_words),
    (channels_sh(6) | bytes_sh(2) | doswap_sh(1),                 ANYSPACE, output::pack_chunky_words),

    (bytes_sh(2),
        ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYENDIAN | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        output::pack_chunky_words),
    (bytes_sh(2) | planar_sh(1),
        ANYFLAVOR | ANYENDIAN | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE | ANYPREMUL,
        output::pack_planar_words),
];

#[cfg(feature = "half-tables")]
const HALF_OUT_16: (u32, u32) = (
    float_sh(1) | bytes_sh(2),
    ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYCHANNELS | ANYPLANAR | ANYEXTRA | ANYSPACE,
);

/// Stock 16-bit output formatter for `fmt` (`_cmsGetStockOutputFormatter`, 16-bit path).
///
/// As in C, the `OPTIMIZED` hint is cleared before matching, so the "optimized" rows are
/// never selected here.
pub fn get_output_formatter_16(fmt: PixelFormat) -> Option<Formatter16Out> {
    get_output_formatter_16_for(None, fmt)
}

/// [`get_output_formatter_16`] against the factories visible to `ctx` (`_cmsGetFormatter`).
pub fn get_output_formatter_16_thr(ctx: &Context, fmt: PixelFormat) -> Option<Formatter16Out> {
    get_output_formatter_16_for(Some(ctx), fmt)
}

/// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
fn get_output_formatter_16_for(
    ctx: Option<&Context>,
    fmt: PixelFormat,
) -> Option<Formatter16Out> {
    if let Some(f) = crate::plugin::find_output_formatter_16(ctx, fmt) {
        return Some(f);
    }
    let dw = fmt.raw() & !optimized_sh(1);
    for &(ty, mask, f) in OUTPUT_16 {
        if (dw & !mask) == ty {
            return Some(f);
        }
    }
    #[cfg(feature = "half-tables")]
    {
        let (ty, mask) = HALF_OUT_16;
        if (dw & !mask) == ty {
            return Some(output::pack_half_from_16);
        }
    }
    None
}

// ============================================================================================
// Output float table (OutputFormattersFloat, cmspack.c L3815)
// ============================================================================================

#[rustfmt::skip]
static OUTPUT_FLOAT: &[(u32, u32, FormatterFloatOut)] = &[
    (TYPE_LAB_FLT, ANYPLANAR | ANYEXTRA, output::pack_lab_float_from_float),
    (TYPE_XYZ_FLT, ANYPLANAR | ANYEXTRA, output::pack_xyz_float_from_float),
    (TYPE_LAB_DBL, ANYPLANAR | ANYEXTRA, output::pack_lab_double_from_float),
    (TYPE_XYZ_DBL, ANYPLANAR | ANYEXTRA, output::pack_xyz_double_from_float),
    (TYPE_LABV2_8,  ANYPLANAR | ANYEXTRA, output::pack_encoded_bytes_lab_v2_from_float),
    (TYPE_LABV2_16, ANYPLANAR | ANYEXTRA, output::pack_encoded_words_lab_v2_from_float),
    (float_sh(1) | bytes_sh(4),
        ANYPLANAR | ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        output::pack_floats_from_float),
    (float_sh(1) | bytes_sh(0),
        ANYPLANAR | ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        output::pack_doubles_from_float),
    (bytes_sh(2),
        ANYPLANAR | ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        output::pack_words_from_float),
    (bytes_sh(1),
        ANYPLANAR | ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
        output::pack_bytes_from_float),
    // half row -> feature-gated fallback below
];

#[cfg(feature = "half-tables")]
const HALF_OUT_FLOAT: (u32, u32) = (
    float_sh(1) | bytes_sh(2),
    ANYFLAVOR | ANYSWAPFIRST | ANYSWAP | ANYEXTRA | ANYCHANNELS | ANYSPACE,
);

/// Stock float output formatter for `fmt` (`_cmsGetStockOutputFormatter`, float path).
pub fn get_output_formatter_float(fmt: PixelFormat) -> Option<FormatterFloatOut> {
    get_output_formatter_float_for(None, fmt)
}

/// [`get_output_formatter_float`] against the factories visible to `ctx` (`_cmsGetFormatter`).
pub fn get_output_formatter_float_thr(
    ctx: &Context,
    fmt: PixelFormat,
) -> Option<FormatterFloatOut> {
    get_output_formatter_float_for(Some(ctx), fmt)
}

/// Shared body of the plain and `*THR` forms, so the two cannot drift apart.
fn get_output_formatter_float_for(
    ctx: Option<&Context>,
    fmt: PixelFormat,
) -> Option<FormatterFloatOut> {
    if let Some(f) = crate::plugin::find_output_formatter_float(ctx, fmt) {
        return Some(f);
    }
    let dw = fmt.raw() & !optimized_sh(1);
    for &(ty, mask, f) in OUTPUT_FLOAT {
        if (dw & !mask) == ty {
            return Some(f);
        }
    }
    #[cfg(feature = "half-tables")]
    {
        let (ty, mask) = HALF_OUT_FLOAT;
        if (dw & !mask) == ty {
            return Some(output::pack_half_from_float);
        }
    }
    None
}
