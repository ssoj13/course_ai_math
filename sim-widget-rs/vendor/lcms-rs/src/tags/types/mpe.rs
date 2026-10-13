//! `cmsSigMultiProcessElementType` (`mpet`) and its three built-in element handlers
//! (`MPEcurve` / `MPEmatrix` / `MPEclut`), plus the segmented-curve codec. Faithful port of
//! the MPE section of `cmstypes.c`, including `SupportedMPEtypes` (with the ignored
//! `bACS`/`eACS` elements).

use std::io::{Read, Seek, Write};

use crate::color::gamma::{CurveSegment, ToneCurve};
use crate::context::Context;
use crate::error::{Error, ErrorCode, Result};
use crate::io::endian::{
    read_f32, read_u16, read_u32, write_f32, write_u16, write_u32,
};
use crate::pipeline::{ClutTable, Pipeline, Stage, StageData, StageLoc};
use crate::types::sig::Signature;
use crate::MAX_CHANNELS;

use super::super::handler::{
    mpe_sig, read_bytes, read_position_table, seek_to, tell, write_alignment, write_bytes,
    write_position_table, wrong_variant,
};
use super::super::value::TagValue;
use super::curve::n_segments;

/// `MINUS_INF` (`-1E22F`) and `PLUS_INF` (`+1E22F`) — the outer segment breakpoints.
const MINUS_INF: f32 = -1e22;
const PLUS_INF: f32 = 1e22;

/// Parameter count per formula-segment type (0..=2) — `ParamsByType` in `ReadSegmentedCurve`.
const SEG_PARAMS_BY_TYPE: [usize; 3] = [4, 5, 5];

// --- Segmented curve codec --------------------------------------------------------------

/// `ReadSegmentedCurve`.
fn read_segmented_curve<R: Read + Seek>(io: &mut R) -> Result<ToneCurve> {
    let sig = read_u32(io)?;
    if Signature(sig) != mpe_sig::SEGMENTED_CURVE {
        return Err(Error::new(ErrorCode::CorruptionDetected, "expected segmented curve"));
    }
    let _reserved = read_u32(io)?;
    let n_segments = read_u16(io)?;
    let _reserved2 = read_u16(io)?;
    if n_segments < 1 {
        return Err(Error::new(ErrorCode::Range, "segmented curve needs >= 1 segment"));
    }
    let n = n_segments as usize;

    let mut segments: Vec<CurveSegment> = Vec::with_capacity(n);
    for _ in 0..n {
        segments.push(CurveSegment {
            x0: 0.0,
            x1: 0.0,
            seg_type: 0,
            params: [0.0; 10],
            n_grid_points: 0,
            sampled_points: Vec::new(),
        });
    }

    // Breakpoints: the first segment starts at -inf, the last ends at +inf.
    let mut prev_break = MINUS_INF;
    for seg in segments.iter_mut().take(n - 1) {
        seg.x0 = prev_break;
        seg.x1 = read_f32(io)?;
        prev_break = seg.x1;
    }
    segments[n - 1].x0 = prev_break;
    segments[n - 1].x1 = PLUS_INF;

    for seg in segments.iter_mut() {
        let elem_sig = Signature(read_u32(io)?);
        let _reserved = read_u32(io)?;

        if elem_sig == mpe_sig::FORMULA_CURVE_SEG {
            let seg_type = read_u16(io)?;
            let _reserved = read_u16(io)?;
            if seg_type > 2 {
                return Err(Error::new(ErrorCode::CorruptionDetected, "bad formula segment type"));
            }
            seg.seg_type = seg_type as i32 + 6;
            for p in seg.params.iter_mut().take(SEG_PARAMS_BY_TYPE[seg_type as usize]) {
                *p = read_f32(io)? as f64;
            }
        } else if elem_sig == mpe_sig::SAMPLED_CURVE_SEG {
            let count = read_u32(io)?;
            // The first point is implicit; allocate one extra to be populated later.
            // C's `Count++` (`cmstypes.c:4295`) wraps modulo 2^32 on an untrusted count, after
            // which `_cmsCalloc(ContextID, 0, ...)` fails and the read aborts cleanly. A plain
            // `+ 1` here would instead panic under `overflow-checks`, so fail the read directly.
            let count = count.checked_add(1).ok_or_else(|| {
                Error::new(ErrorCode::Range, "sampled curve segment point count overflow")
            })?;
            seg.seg_type = 0;
            seg.n_grid_points = count;
            let mut pts = vec![0.0f32; count as usize];
            for slot in pts.iter_mut().skip(1) {
                *slot = read_f32(io)?;
            }
            seg.sampled_points = pts;
        } else {
            return Err(Error::new(
                ErrorCode::UnknownExtension,
                format!("Unknown curve element type '{elem_sig}' found."),
            ));
        }
    }

    // The implicit first sampled point (`SampledPoints[0]`) is never serialized, so each sampled
    // segment still carries the 0.0 placeholder parked above. It is a live interpolation knot,
    // not just a serialization filler, so repair it exactly where C does - after the curve is
    // built, by evaluating the curve at the segment's x0 (`cmstypes.c:4326-4334`).
    //
    // Byte round-trip is unaffected: `write_segmented_curve` skips index 0.
    let mut curve = ToneCurve::build_segmented(&segments)?;
    curve.fix_implicit_sampled_points();
    Ok(curve)
}

/// `WriteSegmentedCurve`.
fn write_segmented_curve<W: Write>(io: &mut W, g: &ToneCurve) -> Result<()> {
    let n = n_segments(g);
    write_u32(io, mpe_sig::SEGMENTED_CURVE.as_u32())?;
    write_u32(io, 0)?;
    write_u16(io, n as u16)?;
    write_u16(io, 0)?;

    // Breakpoints (indices `< n` are guaranteed present by `n_segments`).
    for i in 0..n - 1 {
        let seg = g
            .segment(i as i32)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "missing curve segment"))?;
        write_f32(io, seg.x1)?;
    }

    for i in 0..n {
        let seg = g
            .segment(i as i32)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "missing curve segment"))?;
        if seg.seg_type == 0 {
            // Sampled. The first point is implicit in the ICC format, so the on-disk count is
            // one less than `nGridPoints` — which therefore must be at least 1. A zero here is
            // only reachable from an API-constructed curve (the reader's `checked_add` above
            // rules it out), and would underflow this subtraction.
            // STRICTER THAN C: C computes `nGridPoints - 1` in `cmsUInt32Number`, which wraps to
            // 0xFFFFFFFF and writes that count. Erroring is a deliberate divergence, not a port.
            if seg.n_grid_points == 0 {
                return Err(Error::new(
                    ErrorCode::Range,
                    "sampled curve segment has zero grid points",
                ));
            }
            write_u32(io, mpe_sig::SAMPLED_CURVE_SEG.as_u32())?;
            write_u32(io, 0)?;
            write_u32(io, seg.n_grid_points - 1)?;
            for j in 1..seg.n_grid_points as usize {
                write_f32(io, seg.sampled_points[j])?;
            }
        } else {
            write_u32(io, mpe_sig::FORMULA_CURVE_SEG.as_u32())?;
            write_u32(io, 0)?;
            let seg_type = seg.seg_type - 6;
            if !(0..=2).contains(&seg_type) {
                return Err(Error::new(ErrorCode::CorruptionDetected, "unwritable formula segment"));
            }
            write_u16(io, seg_type as u16)?;
            write_u16(io, 0)?;
            for j in 0..SEG_PARAMS_BY_TYPE[seg_type as usize] {
                write_f32(io, seg.params[j] as f32)?;
            }
        }
    }
    Ok(())
}

// --- MPE element: curve set (cvst) ------------------------------------------------------

/// `Type_MPEcurve_Read`.
fn read_mpe_curve<R: Read + Seek>(io: &mut R) -> Result<Stage> {
    let base_offset = tell(io)?.wrapping_sub(8);
    let input_chans = read_u16(io)?;
    let output_chans = read_u16(io)?;
    if input_chans != output_chans {
        return Err(Error::new(ErrorCode::CorruptionDetected, "MPE curve in != out"));
    }

    let mut curves: Vec<Option<ToneCurve>> = (0..input_chans).map(|_| None).collect();
    read_position_table(io, input_chans as u32, base_offset, |io, n, _size| {
        curves[n as usize] = Some(read_segmented_curve(io)?);
        Ok(())
    })?;

    let curves: Vec<ToneCurve> = curves
        .into_iter()
        .map(|c| c.ok_or_else(|| Error::new(ErrorCode::CorruptionDetected, "missing MPE curve")))
        .collect::<Result<_>>()?;
    Stage::alloc_tone_curves(input_chans as u32, Some(&curves))
}

/// `Type_MPEcurve_Write`.
fn write_mpe_curve<W: Write + Seek>(io: &mut W, stage: &Stage) -> Result<()> {
    let base_offset = tell(io)?.wrapping_sub(8);
    let curves = match &stage.data {
        StageData::ToneCurves(c) => c,
        _ => return Err(Error::new(ErrorCode::NotSuitable, "MPE curve stage expected")),
    };
    // Input and output channels are identical for a curve set.
    write_u16(io, stage.input_channels as u16)?;
    write_u16(io, stage.input_channels as u16)?;
    write_position_table(io, 0, stage.input_channels, base_offset, |io, n, _size| {
        write_segmented_curve(io, &curves[n as usize])
    })
}

// --- MPE element: matrix (matf) ---------------------------------------------------------

/// `Type_MPEmatrix_Read`.
fn read_mpe_matrix<R: Read + Seek>(io: &mut R) -> Result<Stage> {
    let input_chans = read_u16(io)?;
    let output_chans = read_u16(io)?;
    if input_chans as usize >= MAX_CHANNELS || output_chans as usize >= MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "MPE matrix channels out of range"));
    }
    let n_elems = input_chans as usize * output_chans as usize;
    let mut matrix = Vec::with_capacity(n_elems);
    for _ in 0..n_elems {
        matrix.push(read_f32(io)? as f64);
    }
    let mut offsets = Vec::with_capacity(output_chans as usize);
    for _ in 0..output_chans {
        offsets.push(read_f32(io)? as f64);
    }
    // rows = OutputChans, cols = InputChans.
    Stage::alloc_matrix(output_chans as usize, input_chans as usize, &matrix, Some(&offsets))
}

/// `Type_MPEmatrix_Write`.
fn write_mpe_matrix<W: Write>(io: &mut W, stage: &Stage) -> Result<()> {
    let (m, offset) = match &stage.data {
        StageData::Matrix { m, offset, .. } => (m, offset),
        _ => return Err(Error::new(ErrorCode::NotSuitable, "MPE matrix stage expected")),
    };
    write_u16(io, stage.input_channels as u16)?;
    write_u16(io, stage.output_channels as u16)?;

    let n_elems = stage.input_channels as usize * stage.output_channels as usize;
    for &v in &m[..n_elems] {
        write_f32(io, v as f32)?;
    }
    for i in 0..stage.output_channels as usize {
        match offset {
            None => write_f32(io, 0.0)?,
            Some(off) => write_f32(io, off[i] as f32)?,
        }
    }
    Ok(())
}

// --- MPE element: CLUT (clut) -----------------------------------------------------------

/// `Type_MPEclut_Read` — a float granular CLUT.
fn read_mpe_clut<R: Read + Seek>(io: &mut R) -> Result<Stage> {
    let input_chans = read_u16(io)?;
    let output_chans = read_u16(io)?;
    if input_chans == 0 || input_chans as usize >= MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "MPE CLUT input channels out of range"));
    }
    if output_chans == 0 || output_chans as usize >= MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "MPE CLUT output channels out of range"));
    }
    let dims8 = read_bytes(io, 16)?;

    // Copy at most MAX_INPUT_DIMENSIONS grid sizes (15).
    let max_grids = (input_chans as usize).min(15);
    let mut grid = [0u32; 16];
    for i in 0..max_grids {
        if dims8[i] == 1 {
            return Err(Error::new(ErrorCode::Range, "impossible CLUT grid of 1"));
        }
        grid[i] = dims8[i] as u32;
    }

    let mut stage =
        Stage::alloc_clut_float_granular(&grid[..input_chans as usize], input_chans as u32, output_chans as u32, None)?;

    if let StageData::Clut(cd) = &mut stage.data {
        if let ClutTable::F32(t) = &mut cd.table {
            for slot in t.iter_mut() {
                *slot = read_f32(io)?;
            }
        }
    }
    Ok(stage)
}

/// `Type_MPEclut_Write`.
fn write_mpe_clut<W: Write>(io: &mut W, stage: &Stage) -> Result<()> {
    let cd = match &stage.data {
        StageData::Clut(c) => c,
        _ => return Err(Error::new(ErrorCode::NotSuitable, "MPE CLUT stage expected")),
    };
    if stage.input_channels as usize > 15 {
        return Err(Error::new(ErrorCode::Range, "MPE CLUT exceeds MAX_INPUT_DIMENSIONS"));
    }
    let table = match &cd.table {
        ClutTable::F32(t) => t,
        ClutTable::U16(_) => {
            return Err(Error::new(ErrorCode::NotSuitable, "Only floats are supported in MPE"));
        }
    };

    write_u16(io, stage.input_channels as u16)?;
    write_u16(io, stage.output_channels as u16)?;

    let mut dims8 = [0u8; 16];
    for (d, &s) in dims8.iter_mut().zip(cd.params.n_samples.iter()).take(stage.input_channels as usize) {
        *d = s as u8;
    }
    write_bytes(io, &dims8)?;

    for &v in table {
        write_f32(io, v)?;
    }
    Ok(())
}

// --- MPE main dispatcher (mpet) ---------------------------------------------------------

/// `ReadMPEElem` + `SupportedMPEtypes` — read one element and (if it has a reader) append it.
fn read_mpe_elem<R: Read + Seek>(
    ctx: Option<&Context>,
    io: &mut R,
    pipe: &mut Pipeline,
) -> Result<()> {
    let elem_sig = Signature(read_u32(io)?);
    let _reserved = read_u32(io)?;

    let stage = if elem_sig == mpe_sig::CURVE_SET {
        Some(read_mpe_curve(io)?)
    } else if elem_sig == mpe_sig::MATRIX {
        Some(read_mpe_matrix(io)?)
    } else if elem_sig == mpe_sig::CLUT {
        Some(read_mpe_clut(io)?)
    } else if elem_sig == mpe_sig::B_ACS || elem_sig == mpe_sig::E_ACS {
        None // ignored per spec (no read method)
    } else if let Some(reader) = crate::plugin::find_mpe_reader(ctx, elem_sig) {
        // A plugin-registered MPE type (`_cmsRegisterMultiProcessElementPlugin`): materialise
        // the stage through its reader, positioned after the 8-byte element base.
        Some(reader(&mut *io)?)
    } else {
        return Err(Error::new(ErrorCode::UnknownExtension, format!("Unknown MPE type '{elem_sig}' found.")));
    };

    if let Some(stage) = stage {
        if !pipe.insert_stage(StageLoc::End, stage) {
            return Err(Error::new(ErrorCode::CorruptionDetected, "MPE channel mismatch"));
        }
    }
    Ok(())
}

/// `Type_MPE_Read`.
pub(crate) fn read_mpe<R: Read + Seek>(
    ctx: Option<&Context>,
    io: &mut R,
) -> Result<TagValue> {
    let base_offset = tell(io)?.wrapping_sub(8);
    let input_chans = read_u16(io)?;
    let output_chans = read_u16(io)?;
    if input_chans == 0 || input_chans as usize >= MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "MPE input channels out of range"));
    }
    if output_chans == 0 || output_chans as usize >= MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "MPE output channels out of range"));
    }

    let mut lut = Pipeline::alloc(input_chans as u32, output_chans as u32)?;
    let element_count = read_u32(io)?;

    read_position_table(io, element_count, base_offset, |io, _n, _size| {
        read_mpe_elem(ctx, io, &mut lut)
    })?;

    if input_chans as u32 != lut.input_channels() || output_chans as u32 != lut.output_channels() {
        return Err(Error::new(ErrorCode::CorruptionDetected, "MPE channel count mismatch"));
    }

    Ok(TagValue::Pipeline(lut))
}

/// `Type_MPE_Write` — inlined position table (element sizes include the 8-byte element base).
pub(crate) fn write_mpe<W: Write + Seek>(io: &mut W, value: &TagValue) -> Result<()> {
    let lut = match value {
        TagValue::Pipeline(p) => p,
        _ => return Err(wrong_variant("pipeline")),
    };
    let base_offset = tell(io)?.wrapping_sub(8);
    let input_chan = lut.input_channels();
    let output_chan = lut.output_channels();
    let elem_count = lut.stage_count();

    write_u16(io, input_chan as u16)?;
    write_u16(io, output_chan as u16)?;
    write_u32(io, elem_count)?;

    let directory_pos = tell(io)?;
    for _ in 0..elem_count {
        write_u32(io, 0)?; // Offset
        write_u32(io, 0)?; // size
    }

    let mut offsets = vec![0u32; elem_count as usize];
    let mut sizes = vec![0u32; elem_count as usize];

    for (i, stage) in lut.stages().iter().enumerate() {
        let before = tell(io)?;
        offsets[i] = before - base_offset;

        let elem_sig = stage.type_signature();
        write_u32(io, elem_sig.as_u32())?;
        write_u32(io, 0)?;

        if elem_sig == mpe_sig::CURVE_SET {
            write_mpe_curve(io, stage)?;
        } else if elem_sig == mpe_sig::MATRIX {
            write_mpe_matrix(io, stage)?;
        } else if elem_sig == mpe_sig::CLUT {
            write_mpe_clut(io, stage)?;
        } else {
            return Err(Error::new(ErrorCode::UnknownExtension, format!("Found unknown MPE type '{elem_sig}'")));
        }
        write_alignment(io)?;

        sizes[i] = tell(io)? - before;
    }

    let current = tell(io)?;
    seek_to(io, directory_pos)?;
    for i in 0..elem_count as usize {
        write_u32(io, offsets[i])?;
        write_u32(io, sizes[i])?;
    }
    seek_to(io, current)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::handler::TypeHandler;
    use crate::tags::testutil::round_trip_stable;

    fn mpe_pipeline() -> Pipeline {
        let mut p = Pipeline::alloc(3, 3).unwrap();

        // Curve set: MPE tone curves are always *segmented* (sampled/formula), never plain
        // parametric — build them the way `ReadSegmentedCurve` would produce them.
        let samples: Vec<f32> = (0..16).map(|i| (i as f32 / 15.0).powf(2.2)).collect();
        let curves: Vec<ToneCurve> =
            (0..3).map(|_| ToneCurve::build_tabulated_float(&samples).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&curves)).unwrap());

        // Matrix 3x3 + offset.
        let m = [1.0, 0.1, 0.0, 0.0, 1.0, 0.0, 0.0, 0.2, 1.0];
        let off = [0.01, 0.02, 0.03];
        p.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, Some(&off)).unwrap());

        // Float CLUT 2x2x2.
        let n = 3 * 2 * 2 * 2;
        let table: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_clut_float(2, 3, 3, Some(&table)).unwrap());

        p
    }

    #[test]
    fn mpe_round_trip() {
        round_trip_stable(TypeHandler::Mpe, &TagValue::Pipeline(mpe_pipeline()));
    }

    #[test]
    fn mpe_sampled_curve_round_trip() {
        // A float-tabulated curve produces a sampled middle segment.
        let samples: Vec<f32> = (0..32).map(|i| (i as f32 / 31.0).powf(2.2)).collect();
        let curves: Vec<ToneCurve> =
            (0..3).map(|_| ToneCurve::build_tabulated_float(&samples).unwrap()).collect();
        let mut p = Pipeline::alloc(3, 3).unwrap();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&curves)).unwrap());
        round_trip_stable(TypeHandler::Mpe, &TagValue::Pipeline(p));
    }

    /// H2 regression: a sampled segment's implicit first point must be repaired on read.
    ///
    /// `SampledPoints[0]` is never serialized (the writer skips it), so a byte-stability test -
    /// like `mpe_sampled_curve_round_trip` above - cannot see whether it was restored. C repairs
    /// it in `ReadSegmentedCurve` (`cmstypes.c:4326-4334`) because it is a live interpolation
    /// knot; leaving the 0.0 placeholder drags the first sub-interval of the segment towards
    /// zero (a black-end crush), and only on the float evaluation path.
    ///
    /// So this asserts evaluation equivalence, not bytes.
    #[test]
    fn mpe_sampled_curve_survives_round_trip_by_value() {
        use crate::tags::testutil::{read_back, round_trip_bytes};

        // Deliberately non-zero at x=0 so an unrepaired placeholder is visible.
        let samples: Vec<f32> = (0..32).map(|i| 0.25 + 0.5 * (i as f32 / 31.0)).collect();
        let curve = ToneCurve::build_tabulated_float(&samples).unwrap();
        let mut p = Pipeline::alloc(1, 1).unwrap();
        p.insert_stage(
            StageLoc::End,
            Stage::alloc_tone_curves(1, Some(std::slice::from_ref(&curve))).unwrap(),
        );

        let bytes = round_trip_bytes(TypeHandler::Mpe, &TagValue::Pipeline(p), 4.3);
        let TagValue::Pipeline(back) = read_back(TypeHandler::Mpe, &bytes) else {
            panic!("expected a pipeline");
        };

        // Probe inside the first sub-interval of the sampled segment, where the placeholder
        // would still be acting as a knot: x0 = 0, node spacing = 1/(32-1).
        for k in 0..4 {
            let x = k as f32 * (1.0 / 31.0) / 4.0;
            let mut got = [0f32; 1];
            back.eval_float(&[x], &mut got);
            let want = curve.eval_f32(x);
            assert!(
                (got[0] - want).abs() < 1e-3,
                "x={x}: got {got:?}, want {want} - implicit SampledPoints[0] was not repaired"
            );
        }
    }

    /// A crafted `samf` segment declaring `Count == 0xFFFFFFFF`. C's `Count++`
    /// (`cmstypes.c:4295`) wraps to 0, the following `_cmsCalloc` fails and the read aborts
    /// cleanly; a plain `+ 1` panics under `overflow-checks` instead. The read must return an
    /// error, not unwind.
    #[test]
    fn sampled_segment_count_overflow_is_an_error() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&mpe_sig::SEGMENTED_CURVE.as_u32().to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes()); // one segment
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes.extend_from_slice(&mpe_sig::SAMPLED_CURVE_SEG.as_u32().to_be_bytes());
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.extend_from_slice(&u32::MAX.to_be_bytes());

        let mut io = std::io::Cursor::new(bytes);
        assert!(read_segmented_curve(&mut io).is_err());
    }
}
