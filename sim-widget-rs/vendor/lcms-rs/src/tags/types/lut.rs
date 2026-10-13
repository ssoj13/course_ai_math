//! The four ICC lookup-table tag types, all backed by a [`Pipeline`]:
//! `cmsSigLut8Type` (`mft1`), `cmsSigLut16Type` (`mft2`), `cmsSigLutAtoBType` (`mAB `) and
//! `cmsSigLutBtoAType` (`mBA `). Faithful port of the `Type_LUT8/16/A2B/B2A_*` handlers and
//! their `Read*`/`Write*` helpers in `cmstypes.c`.

use std::io::{Read, Seek, Write};

use crate::color::gamma::ToneCurve;
use crate::error::{Error, ErrorCode, Result};
use crate::io::endian::{
    read_s15f16, read_u16, read_u16_array, read_u8, write_s15f16, write_u16, write_u16_array,
    write_u8,
};
use crate::math::mat3::{Mat3, Vec3};
use crate::pipeline::stage::ClutData;
use crate::pipeline::{ClutTable, Pipeline, Stage, StageData, StageLoc};
use crate::MAX_CHANNELS;

use super::super::handler::{
    from_16_to_8, from_8_to_16, mpe_sig, read_bytes, rel_offset, seek_to, tell, wrong_variant,
    write_alignment, write_bytes,
};
use super::super::value::TagValue;
use super::curve::{read_embedded_curve_aligned, write_embedded_curve};

/// `uipow` — `n * a^b` with the C overflow sentinel (`u32::MAX` == `(cmsUInt32Number)-1`).
fn uipow(n: u32, a: u32, b: u32) -> u32 {
    if a == 0 {
        return 0;
    }
    if n == 0 {
        return 0;
    }
    let mut rv: u32 = 1;
    for _ in 0..b {
        rv = rv.wrapping_mul(a);
        if rv > u32::MAX / a {
            return u32::MAX;
        }
    }
    let rc = rv.wrapping_mul(n);
    if rv != rc / n {
        return u32::MAX;
    }
    rc
}

/// `_cmsMAT3isIdentity` over a row-major 3×3.
fn mat3_is_identity(m: &[f64; 9]) -> bool {
    Mat3([
        Vec3::new(m[0], m[1], m[2]),
        Vec3::new(m[3], m[4], m[5]),
        Vec3::new(m[6], m[7], m[8]),
    ])
    .is_identity()
}

/// Insert `stage` (from a fallible reader) at `loc`, erroring if the chain becomes inconsistent.
fn insert(pipe: &mut Pipeline, loc: StageLoc, stage: Stage) -> Result<()> {
    if !pipe.insert_stage(loc, stage) {
        return Err(Error::new(ErrorCode::CorruptionDetected, "LUT channel-count mismatch"));
    }
    Ok(())
}

// --- 8- and 16-bit prelinearization tables ----------------------------------------------

/// `Read8bitTables` — `nChannels` 256-entry curves, each byte widened via `FROM_8_TO_16`.
fn read_8bit_tables<R: Read + Seek>(io: &mut R, pipe: &mut Pipeline, n_channels: u32) -> Result<()> {
    if n_channels == 0 || n_channels as usize > MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "bad channel count for 8-bit tables"));
    }
    let mut curves = Vec::with_capacity(n_channels as usize);
    for _ in 0..n_channels {
        let raw = read_bytes(io, 256)?;
        let table: Vec<u16> = raw.iter().map(|&b| from_8_to_16(b)).collect();
        curves.push(ToneCurve::build_tabulated_16(256, &table)?);
    }
    insert(pipe, StageLoc::End, Stage::alloc_tone_curves(n_channels, Some(&curves))?)
}

/// `Write8bitTables` — identity curves become the `0..255` ramp; others must be 256-entry.
/// A `None` curve set writes nothing (the C `if (Tables)` guard).
fn write_8bit_tables<W: Write>(io: &mut W, n: u32, tables: Option<&[ToneCurve]>) -> Result<()> {
    let tables = match tables {
        Some(t) => t,
        None => return Ok(()),
    };
    for c in tables.iter().take(n as usize) {
        let entries = c.estimated_table_entries();
        let table = c.estimated_table();
        if entries == 2 && table[0] == 0 && table[1] == 65535 {
            for j in 0..256u32 {
                write_u8(io, j as u8)?;
            }
        } else if entries != 256 {
            return Err(Error::new(ErrorCode::Range, "LUT8 needs 256 entries on prelinearization"));
        } else {
            for &v in table {
                write_u8(io, from_16_to_8(v))?;
            }
        }
    }
    Ok(())
}

/// `Read16bitTables` — `nChannels` curves of `nEntries` words each (empty tables are skipped).
fn read_16bit_tables<R: Read + Seek>(
    io: &mut R,
    pipe: &mut Pipeline,
    n_channels: u32,
    n_entries: u32,
) -> Result<()> {
    if n_entries == 0 {
        return Ok(()); // lcms extension: an empty table inserts no stage
    }
    if n_entries < 2 {
        return Err(Error::new(ErrorCode::Range, "16-bit table needs >= 2 entries"));
    }
    if n_channels as usize > MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "too many channels for 16-bit tables"));
    }
    let mut curves = Vec::with_capacity(n_channels as usize);
    for _ in 0..n_channels {
        let table = read_u16_array(io, n_entries as usize)?;
        curves.push(ToneCurve::build_tabulated_16(n_entries as usize, &table)?);
    }
    insert(pipe, StageLoc::End, Stage::alloc_tone_curves(n_channels, Some(&curves))?)
}

/// `Write16bitTables` — every curve's 16-bit table, in order.
fn write_16bit_tables<W: Write>(io: &mut W, tables: &[ToneCurve]) -> Result<()> {
    for c in tables {
        write_u16_array(io, c.estimated_table())?;
    }
    Ok(())
}

// --- LUT disassembly (matrix / pre / clut / post) ---------------------------------------

/// Borrowed view of a matrix stage: coefficients, optional offset, and I/O channel counts.
struct MatrixRef<'a> {
    m: &'a [f64],
    offset: Option<&'a [f64]>,
    input: u32,
    output: u32,
}

fn as_matrix(stage: &Stage) -> Option<MatrixRef<'_>> {
    match &stage.data {
        StageData::Matrix { m, offset, .. } => Some(MatrixRef {
            m,
            offset: offset.as_deref(),
            input: stage.input_channels,
            output: stage.output_channels,
        }),
        _ => None,
    }
}

fn as_curves(stage: &Stage) -> Option<&[ToneCurve]> {
    match &stage.data {
        StageData::ToneCurves(c) => Some(c),
        _ => None,
    }
}

fn as_clut(stage: &Stage) -> Option<&ClutData> {
    match &stage.data {
        StageData::Clut(c) => Some(c),
        _ => None,
    }
}

/// The `[Matrix?] [pre] [CLUT] [post]` decomposition used by `Type_LUT8/16_Write`.
struct LutParts<'a> {
    matrix: Option<MatrixRef<'a>>,
    pre: Option<&'a [ToneCurve]>,
    clut: Option<&'a ClutData>,
    post: Option<&'a [ToneCurve]>,
}

/// Disassemble a pipeline into optional matrix + pre-curves + CLUT + post-curves, exactly like
/// the linear walk in `Type_LUT8_Write` / `Type_LUT16_Write` (any leftover stage is fatal).
///
/// An empty pipeline yields an all-`None` decomposition, which is what `Type_LUT16_Write`
/// wants: its walk is guarded by `if (mpe != NULL && ...)` (`cmstypes.c:2414-2415`), so LUT16
/// legitimately tolerates an empty chain. `Type_LUT8_Write` does *not* — it rejects the empty
/// case up front (`cmstypes.c:2120-2123`), so that check lives in [`write_lut8`] rather than
/// here. Do not move it into this shared helper: that would wrongly tighten LUT16.
fn disassemble_lut(pipe: &Pipeline) -> Result<LutParts<'_>> {
    let stages = pipe.stages();
    let mut idx = 0;
    let mut parts = LutParts { matrix: None, pre: None, clut: None, post: None };

    if let Some(st) = stages.get(idx) {
        if let Some(m) = as_matrix(st) {
            if m.input != 3 || m.output != 3 {
                return Err(Error::new(ErrorCode::NotSuitable, "LUT matrix must be 3x3"));
            }
            parts.matrix = Some(m);
            idx += 1;
        }
    }
    if let Some(st) = stages.get(idx) {
        if let Some(c) = as_curves(st) {
            parts.pre = Some(c);
            idx += 1;
        }
    }
    if let Some(st) = stages.get(idx) {
        if let Some(c) = as_clut(st) {
            parts.clut = Some(c);
            idx += 1;
        }
    }
    if let Some(st) = stages.get(idx) {
        if let Some(c) = as_curves(st) {
            parts.post = Some(c);
            idx += 1;
        }
    }
    if idx != stages.len() {
        return Err(Error::new(ErrorCode::UnknownExtension, "LUT is not suitable to be saved"));
    }
    Ok(parts)
}

/// The single CLUT grid size (`clutPoints`), verifying every dimension is equal (LUT8/16 only
/// permit a uniform grid). Returns 0 when there is no CLUT.
fn uniform_clut_points(clut: Option<&ClutData>, input_channels: u32) -> Result<u32> {
    let clut = match clut {
        Some(c) => c,
        None => return Ok(0),
    };
    let points = clut.params.n_samples[0];
    for i in 1..input_channels as usize {
        if clut.params.n_samples[i] != points {
            return Err(Error::new(
                ErrorCode::UnknownExtension,
                "LUT with different samples per dimension not suitable",
            ));
        }
    }
    Ok(points)
}

/// Write a 3×3 matrix as nine `s15Fixed16`, or the identity when absent.
fn write_lut_matrix<W: Write>(io: &mut W, matrix: Option<&MatrixRef<'_>>) -> Result<()> {
    match matrix {
        Some(m) => {
            for i in 0..9 {
                write_s15f16(io, m.m[i])?;
            }
            Ok(())
        }
        None => {
            const ID: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
            for v in ID {
                write_s15f16(io, v)?;
            }
            Ok(())
        }
    }
}

/// Borrow a CLUT stage's 16-bit sample table (LUT8/16 CLUTs are always 16-bit).
fn clut_u16(clut: &ClutData) -> Result<&[u16]> {
    match &clut.table {
        ClutTable::U16(t) => Ok(t),
        ClutTable::F32(_) => Err(Error::new(ErrorCode::NotSuitable, "LUT CLUT must be 16-bit")),
    }
}

// --- cmsSigLut8Type ---------------------------------------------------------------------

/// `Type_LUT8_Read`.
pub(crate) fn read_lut8<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let input_channels = read_u8(io)?;
    let output_channels = read_u8(io)?;
    let clut_points = read_u8(io)?;
    if clut_points == 1 {
        return Err(Error::new(ErrorCode::Range, "impossible CLUT grid of 1"));
    }
    let _pad = read_u8(io)?;

    if input_channels == 0 || input_channels as usize > MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "bad input channel count"));
    }
    if output_channels == 0 || output_channels as usize > MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "bad output channel count"));
    }

    let mut lut = Pipeline::alloc(input_channels as u32, output_channels as u32)?;

    let mut matrix = [0.0f64; 9];
    for m in matrix.iter_mut() {
        *m = read_s15f16(io)?;
    }
    if input_channels == 3 && !mat3_is_identity(&matrix) {
        insert(&mut lut, StageLoc::Begin, Stage::alloc_matrix(3, 3, &matrix, None)?)?;
    }

    read_8bit_tables(io, &mut lut, input_channels as u32)?;

    let n_tab_size = uipow(output_channels as u32, clut_points as u32, input_channels as u32);
    if n_tab_size == u32::MAX {
        return Err(Error::new(ErrorCode::Range, "LUT8 table size overflow"));
    }
    if n_tab_size > 0 {
        let raw = read_bytes(io, n_tab_size as usize)?;
        let table: Vec<u16> = raw.iter().map(|&b| from_8_to_16(b)).collect();
        insert(
            &mut lut,
            StageLoc::End,
            Stage::alloc_clut_16bit(clut_points as u32, input_channels as u32, output_channels as u32, Some(&table))?,
        )?;
    }

    read_8bit_tables(io, &mut lut, output_channels as u32)?;

    Ok(TagValue::Pipeline(lut))
}

/// `Type_LUT8_Write`.
pub(crate) fn write_lut8<W: Write + Seek>(io: &mut W, value: &TagValue) -> Result<()> {
    let lut = as_pipeline(value)?;
    // LUT8-only guard, checked before disassembly exactly as C does (`cmstypes.c:2120-2123`,
    // "Should never be empty. Corrupted?"). Without it a zero-stage pipeline disassembles into
    // an all-`None` `LutParts` and emits a structurally valid but semantically dead `mft1`.
    if lut.stages().is_empty() {
        return Err(Error::new(ErrorCode::UnknownExtension, "empty LUT8 is not supported"));
    }
    let parts = disassemble_lut(lut)?;
    let input_channels = lut.input_channels();
    let output_channels = lut.output_channels();
    let clut_points = uniform_clut_points(parts.clut, input_channels)?;

    write_u8(io, input_channels as u8)?;
    write_u8(io, output_channels as u8)?;
    write_u8(io, clut_points as u8)?;
    write_u8(io, 0)?; // padding

    write_lut_matrix(io, parts.matrix.as_ref())?;
    write_8bit_tables(io, input_channels, parts.pre)?;

    let n_tab_size = uipow(output_channels, clut_points, input_channels);
    if n_tab_size == u32::MAX {
        return Err(Error::new(ErrorCode::Range, "LUT8 table size overflow"));
    }
    if n_tab_size > 0 {
        if let Some(clut) = parts.clut {
            for &v in clut_u16(clut)? {
                write_u8(io, from_16_to_8(v))?;
            }
        }
    }

    write_8bit_tables(io, output_channels, parts.post)
}

// --- cmsSigLut16Type --------------------------------------------------------------------

/// `Type_LUT16_Read`.
pub(crate) fn read_lut16<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let input_channels = read_u8(io)?;
    let output_channels = read_u8(io)?;
    let clut_points = read_u8(io)?;
    let _pad = read_u8(io)?;

    if input_channels == 0 || input_channels as usize > MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "bad input channel count"));
    }
    if output_channels == 0 || output_channels as usize > MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "bad output channel count"));
    }

    let mut lut = Pipeline::alloc(input_channels as u32, output_channels as u32)?;

    let mut matrix = [0.0f64; 9];
    for m in matrix.iter_mut() {
        *m = read_s15f16(io)?;
    }
    if input_channels == 3 && !mat3_is_identity(&matrix) {
        insert(&mut lut, StageLoc::End, Stage::alloc_matrix(3, 3, &matrix, None)?)?;
    }

    let input_entries = read_u16(io)?;
    let output_entries = read_u16(io)?;
    if input_entries > 0x7FFF || output_entries > 0x7FFF {
        return Err(Error::new(ErrorCode::Range, "too many table entries"));
    }
    if clut_points == 1 {
        return Err(Error::new(ErrorCode::Range, "impossible CLUT grid of 1"));
    }

    read_16bit_tables(io, &mut lut, input_channels as u32, input_entries as u32)?;

    let n_tab_size = uipow(output_channels as u32, clut_points as u32, input_channels as u32);
    if n_tab_size == u32::MAX {
        return Err(Error::new(ErrorCode::Range, "LUT16 table size overflow"));
    }
    if n_tab_size > 0 {
        let table = read_u16_array(io, n_tab_size as usize)?;
        insert(
            &mut lut,
            StageLoc::End,
            Stage::alloc_clut_16bit(clut_points as u32, input_channels as u32, output_channels as u32, Some(&table))?,
        )?;
    }

    read_16bit_tables(io, &mut lut, output_channels as u32, output_entries as u32)?;

    Ok(TagValue::Pipeline(lut))
}

/// `Type_LUT16_Write`.
pub(crate) fn write_lut16<W: Write + Seek>(io: &mut W, value: &TagValue) -> Result<()> {
    let lut = as_pipeline(value)?;
    let parts = disassemble_lut(lut)?;
    let input_channels = lut.input_channels();
    let output_channels = lut.output_channels();
    let clut_points = uniform_clut_points(parts.clut, input_channels)?;

    write_u8(io, input_channels as u8)?;
    write_u8(io, output_channels as u8)?;
    write_u8(io, clut_points as u8)?;
    write_u8(io, 0)?; // padding

    write_lut_matrix(io, parts.matrix.as_ref())?;

    // Input/output prelinearization entry counts.
    let in_entries = parts.pre.map(|p| p[0].estimated_table_entries()).unwrap_or(2);
    let out_entries = parts.post.map(|p| p[0].estimated_table_entries()).unwrap_or(2);
    write_u16(io, in_entries as u16)?;
    write_u16(io, out_entries as u16)?;

    // Prelinearization tables (identity ramps when absent).
    match parts.pre {
        Some(pre) => write_16bit_tables(io, pre)?,
        None => {
            for _ in 0..input_channels {
                write_u16(io, 0)?;
                write_u16(io, 0xffff)?;
            }
        }
    }

    let n_tab_size = uipow(output_channels, clut_points, input_channels);
    if n_tab_size == u32::MAX {
        return Err(Error::new(ErrorCode::Range, "LUT16 table size overflow"));
    }
    if n_tab_size > 0 {
        if let Some(clut) = parts.clut {
            write_u16_array(io, clut_u16(clut)?)?;
        }
    }

    match parts.post {
        Some(post) => write_16bit_tables(io, post)?,
        None => {
            for _ in 0..output_channels {
                write_u16(io, 0)?;
                write_u16(io, 0xffff)?;
            }
        }
    }
    Ok(())
}

// --- V4 shared readers (ReadMatrix / ReadCLUT / ReadSetOfCurves) -------------------------

/// `ReadMatrix` — nine coefficients + three offsets at `offset`, as a 3×3 matrix stage.
fn read_matrix<R: Read + Seek>(io: &mut R, offset: u32) -> Result<Stage> {
    seek_to(io, offset)?;
    let mut m = [0.0f64; 9];
    for v in m.iter_mut() {
        *v = read_s15f16(io)?;
    }
    let mut off = [0.0f64; 3];
    for v in off.iter_mut() {
        *v = read_s15f16(io)?;
    }
    Stage::alloc_matrix(3, 3, &m, Some(&off))
}

/// `ReadCLUT` — a granular 16-bit CLUT at `offset` (1- or 2-byte precision on disk).
fn read_clut<R: Read + Seek>(io: &mut R, offset: u32, input: u32, output: u32) -> Result<Stage> {
    seek_to(io, offset)?;
    let grid8 = read_bytes(io, 16)?;
    let mut grid = [0u32; MAX_CHANNELS];
    for (i, &g) in grid8.iter().enumerate() {
        if g == 1 {
            return Err(Error::new(ErrorCode::Range, "impossible CLUT grid of 1"));
        }
        grid[i] = g as u32;
    }
    let precision = read_u8(io)?;
    let _ = read_u8(io)?;
    let _ = read_u8(io)?;
    let _ = read_u8(io)?;

    let mut stage =
        Stage::alloc_clut_16bit_granular(&grid[..input as usize], input, output, None)?;

    // Fill the sample table directly (fields are public), matching the C precision handling.
    if let StageData::Clut(cd) = &mut stage.data {
        if let ClutTable::U16(t) = &mut cd.table {
            match precision {
                1 => {
                    for slot in t.iter_mut() {
                        *slot = from_8_to_16(read_u8(io)?);
                    }
                }
                2 => {
                    for slot in t.iter_mut() {
                        *slot = read_u16(io)?;
                    }
                }
                _ => {
                    return Err(Error::new(
                        ErrorCode::UnknownExtension,
                        format!("Unknown precision of '{precision}'"),
                    ));
                }
            }
        }
    }
    Ok(stage)
}

/// `ReadSetOfCurves` — `n_curves` embedded curves at `offset`, each followed by alignment.
fn read_set_of_curves<R: Read + Seek>(io: &mut R, offset: u32, n_curves: u32) -> Result<Stage> {
    if n_curves as usize > MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "too many curves"));
    }
    seek_to(io, offset)?;
    let mut curves = Vec::with_capacity(n_curves as usize);
    for _ in 0..n_curves {
        curves.push(read_embedded_curve_aligned(io)?);
    }
    Stage::alloc_tone_curves(n_curves, Some(&curves))
}

// --- V4 shared writers (WriteMatrix / WriteCLUT / WriteSetOfCurves) ----------------------

/// `WriteMatrix` — `input*output` coefficients then `output` offsets (zeros when absent).
fn write_matrix<W: Write>(io: &mut W, m: &MatrixRef<'_>) -> Result<()> {
    let n = (m.input * m.output) as usize;
    for i in 0..n {
        write_s15f16(io, m.m[i])?;
    }
    match m.offset {
        Some(off) => {
            for &v in &off[..m.output as usize] {
                write_s15f16(io, v)?;
            }
        }
        None => {
            for _ in 0..m.output {
                write_s15f16(io, 0.0)?;
            }
        }
    }
    Ok(())
}

/// `WriteCLUT` — grid dimensions, precision, samples (1 or 2 bytes), then alignment.
fn write_clut<W: Write + Seek>(io: &mut W, precision: u8, clut: &ClutData) -> Result<()> {
    let table = match &clut.table {
        ClutTable::U16(t) => t,
        ClutTable::F32(_) => {
            return Err(Error::new(
                ErrorCode::NotSuitable,
                "Cannot save floating point data, CLUT are 8 or 16 bit only",
            ));
        }
    };

    let mut grid = [0u8; MAX_CHANNELS];
    for (g, &s) in grid.iter_mut().zip(clut.params.n_samples.iter()).take(clut.params.n_inputs) {
        *g = s as u8;
    }
    write_bytes(io, &grid)?;

    write_u8(io, precision)?;
    write_u8(io, 0)?;
    write_u8(io, 0)?;
    write_u8(io, 0)?;

    match precision {
        1 => {
            for &v in table {
                write_u8(io, from_16_to_8(v))?;
            }
        }
        2 => write_u16_array(io, table)?,
        _ => {
            return Err(Error::new(
                ErrorCode::UnknownExtension,
                format!("Unknown precision of '{precision}'"),
            ));
        }
    }
    write_alignment(io)
}

/// `WriteSetOfCurves` — every curve as an embedded (aligned) `curv`/`para`.
fn write_set_of_curves<W: Write + Seek>(io: &mut W, curves: &[ToneCurve]) -> Result<()> {
    for c in curves {
        write_embedded_curve(io, c)?;
    }
    Ok(())
}

// --- cmsSigLutAToBType / cmsSigLutBToAType ----------------------------------------------

/// The five processing elements of an A2B/B2A pipeline, bound to their write roles.
struct AbParts<'a> {
    a: Option<&'a [ToneCurve]>,
    clut: Option<&'a ClutData>,
    m: Option<&'a [ToneCurve]>,
    matrix: Option<MatrixRef<'a>>,
    b: Option<&'a [ToneCurve]>,
}

/// The stage type sequence, for pattern matching (`cmsPipelineCheckAndRetreiveStages`).
fn stage_types(pipe: &Pipeline) -> Vec<crate::types::sig::Signature> {
    pipe.stages().iter().map(|s| s.type_signature()).collect()
}

/// `Type_LUTA2B_Write` retrieval: match one of `B` / `M-Matrix-B` / `A-CLUT-B` /
/// `A-CLUT-M-Matrix-B`.
fn match_a2b(pipe: &Pipeline) -> Result<AbParts<'_>> {
    let s = pipe.stages();
    let t = stage_types(pipe);
    let (cvst, matf, clut) = (mpe_sig::CURVE_SET, mpe_sig::MATRIX, mpe_sig::CLUT);
    let mut p = AbParts { a: None, clut: None, m: None, matrix: None, b: None };
    match t.as_slice() {
        // Faithful: C wraps the whole retrieval cascade in `if (Lut->Elements != NULL)`
        // (`cmstypes.c:2966`), so an empty A2B pipeline skips every check and writes an
        // all-zero directory. `match_b2a` deliberately has no such arm — the asymmetry is
        // intentional in C, not a copy-paste slip.
        [] => {}
        [x] if *x == cvst => p.b = as_curves(&s[0]),
        [x, y, z] if *x == cvst && *y == matf && *z == cvst => {
            p.m = as_curves(&s[0]);
            p.matrix = as_matrix(&s[1]);
            p.b = as_curves(&s[2]);
        }
        [x, y, z] if *x == cvst && *y == clut && *z == cvst => {
            p.a = as_curves(&s[0]);
            p.clut = as_clut(&s[1]);
            p.b = as_curves(&s[2]);
        }
        [x, y, z, w, v]
            if *x == cvst && *y == clut && *z == cvst && *w == matf && *v == cvst =>
        {
            p.a = as_curves(&s[0]);
            p.clut = as_clut(&s[1]);
            p.m = as_curves(&s[2]);
            p.matrix = as_matrix(&s[3]);
            p.b = as_curves(&s[4]);
        }
        _ => return Err(Error::new(ErrorCode::NotSuitable, "LUT is not suitable to be saved as LutAToB")),
    }
    Ok(p)
}

/// `Type_LUTB2A_Write` retrieval: match one of `B` / `B-Matrix-M` / `B-CLUT-A` /
/// `B-Matrix-M-CLUT-A`.
fn match_b2a(pipe: &Pipeline) -> Result<AbParts<'_>> {
    let s = pipe.stages();
    let t = stage_types(pipe);
    let (cvst, matf, clut) = (mpe_sig::CURVE_SET, mpe_sig::MATRIX, mpe_sig::CLUT);
    let mut p = AbParts { a: None, clut: None, m: None, matrix: None, b: None };
    // No empty arm here, unlike `match_a2b`: `Type_LUTB2A_Write` has no `Elements != NULL`
    // guard (`cmstypes.c:3154`), so on an empty pipeline all four retrieval attempts fail and
    // C falls through to `cmsSignalError(..., "LUT is not suitable to be saved as LutBToA")`.
    // An empty stage list therefore lands on the `_ =>` error arm below — that is deliberate.
    match t.as_slice() {
        [x] if *x == cvst => p.b = as_curves(&s[0]),
        [x, y, z] if *x == cvst && *y == matf && *z == cvst => {
            p.b = as_curves(&s[0]);
            p.matrix = as_matrix(&s[1]);
            p.m = as_curves(&s[2]);
        }
        [x, y, z] if *x == cvst && *y == clut && *z == cvst => {
            p.b = as_curves(&s[0]);
            p.clut = as_clut(&s[1]);
            p.a = as_curves(&s[2]);
        }
        [x, y, z, w, v]
            if *x == cvst && *y == matf && *z == cvst && *w == clut && *v == cvst =>
        {
            p.b = as_curves(&s[0]);
            p.matrix = as_matrix(&s[1]);
            p.m = as_curves(&s[2]);
            p.clut = as_clut(&s[3]);
            p.a = as_curves(&s[4]);
        }
        _ => return Err(Error::new(ErrorCode::NotSuitable, "LUT is not suitable to be saved as LutBToA")),
    }
    Ok(p)
}

/// Shared directory-based writer for both A2B and B2A (identical layout, different retrieval).
fn write_lut_ab<W: Write + Seek>(io: &mut W, pipe: &Pipeline, parts: &AbParts<'_>) -> Result<()> {
    let base_offset = tell(io)?.wrapping_sub(8);
    let input_chan = pipe.input_channels();
    let output_chan = pipe.output_channels();

    write_u8(io, input_chan as u8)?;
    write_u8(io, output_chan as u8)?;
    write_u16(io, 0)?;

    let directory_pos = tell(io)?;
    for _ in 0..5 {
        crate::io::endian::write_u32(io, 0)?;
    }

    let mut offset_a = 0u32;
    let mut offset_c = 0u32;
    let mut offset_m = 0u32;
    let mut offset_mat = 0u32;
    let mut offset_b = 0u32;

    if let Some(a) = parts.a {
        offset_a = tell(io)? - base_offset;
        write_set_of_curves(io, a)?;
    }
    if let Some(clut) = parts.clut {
        offset_c = tell(io)? - base_offset;
        let precision = if pipe.save_as_8bits() { 1 } else { 2 };
        write_clut(io, precision, clut)?;
    }
    if let Some(m) = parts.m {
        offset_m = tell(io)? - base_offset;
        write_set_of_curves(io, m)?;
    }
    if let Some(matrix) = &parts.matrix {
        offset_mat = tell(io)? - base_offset;
        write_matrix(io, matrix)?;
    }
    if let Some(b) = parts.b {
        offset_b = tell(io)? - base_offset;
        write_set_of_curves(io, b)?;
    }

    let current = tell(io)?;
    seek_to(io, directory_pos)?;
    crate::io::endian::write_u32(io, offset_b)?;
    crate::io::endian::write_u32(io, offset_mat)?;
    crate::io::endian::write_u32(io, offset_m)?;
    crate::io::endian::write_u32(io, offset_c)?;
    crate::io::endian::write_u32(io, offset_a)?;
    seek_to(io, current)
}

/// `Type_LUTA2B_Read`.
pub(crate) fn read_lut_a2b<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let base_offset = tell(io)?.wrapping_sub(8);
    let input_chan = read_u8(io)?;
    let output_chan = read_u8(io)?;
    let _pad = read_u16(io)?;

    let offset_b = crate::io::endian::read_u32(io)?;
    let offset_mat = crate::io::endian::read_u32(io)?;
    let offset_m = crate::io::endian::read_u32(io)?;
    let offset_c = crate::io::endian::read_u32(io)?;
    let offset_a = crate::io::endian::read_u32(io)?;

    if input_chan == 0 || input_chan as usize >= MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "bad input channels"));
    }
    if output_chan == 0 || output_chan as usize >= MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "bad output channels"));
    }

    let mut lut = Pipeline::alloc(input_chan as u32, output_chan as u32)?;

    if offset_a != 0 {
        let s = read_set_of_curves(io, rel_offset(base_offset, offset_a)?, input_chan as u32)?;
        insert(&mut lut, StageLoc::End, s)?;
    }
    if offset_c != 0 {
        let at = rel_offset(base_offset, offset_c)?;
        let s = read_clut(io, at, input_chan as u32, output_chan as u32)?;
        insert(&mut lut, StageLoc::End, s)?;
    }
    if offset_m != 0 {
        let s = read_set_of_curves(io, rel_offset(base_offset, offset_m)?, output_chan as u32)?;
        insert(&mut lut, StageLoc::End, s)?;
    }
    if offset_mat != 0 {
        let s = read_matrix(io, rel_offset(base_offset, offset_mat)?)?;
        insert(&mut lut, StageLoc::End, s)?;
    }
    if offset_b != 0 {
        let s = read_set_of_curves(io, rel_offset(base_offset, offset_b)?, output_chan as u32)?;
        insert(&mut lut, StageLoc::End, s)?;
    }

    Ok(TagValue::Pipeline(lut))
}

/// `Type_LUTA2B_Write`.
pub(crate) fn write_lut_a2b<W: Write + Seek>(io: &mut W, value: &TagValue) -> Result<()> {
    let lut = as_pipeline(value)?;
    let parts = match_a2b(lut)?;
    write_lut_ab(io, lut, &parts)
}

/// `Type_LUTB2A_Read`.
pub(crate) fn read_lut_b2a<R: Read + Seek>(io: &mut R) -> Result<TagValue> {
    let base_offset = tell(io)?.wrapping_sub(8);
    let input_chan = read_u8(io)?;
    let output_chan = read_u8(io)?;

    if input_chan == 0 || input_chan as usize >= MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "bad input channels"));
    }
    if output_chan == 0 || output_chan as usize >= MAX_CHANNELS {
        return Err(Error::new(ErrorCode::Range, "bad output channels"));
    }

    let _pad = read_u16(io)?;
    let offset_b = crate::io::endian::read_u32(io)?;
    let offset_mat = crate::io::endian::read_u32(io)?;
    let offset_m = crate::io::endian::read_u32(io)?;
    let offset_c = crate::io::endian::read_u32(io)?;
    let offset_a = crate::io::endian::read_u32(io)?;

    let mut lut = Pipeline::alloc(input_chan as u32, output_chan as u32)?;

    if offset_b != 0 {
        let s = read_set_of_curves(io, rel_offset(base_offset, offset_b)?, input_chan as u32)?;
        insert(&mut lut, StageLoc::End, s)?;
    }
    if offset_mat != 0 {
        let s = read_matrix(io, rel_offset(base_offset, offset_mat)?)?;
        insert(&mut lut, StageLoc::End, s)?;
    }
    if offset_m != 0 {
        let s = read_set_of_curves(io, rel_offset(base_offset, offset_m)?, input_chan as u32)?;
        insert(&mut lut, StageLoc::End, s)?;
    }
    if offset_c != 0 {
        let at = rel_offset(base_offset, offset_c)?;
        let s = read_clut(io, at, input_chan as u32, output_chan as u32)?;
        insert(&mut lut, StageLoc::End, s)?;
    }
    if offset_a != 0 {
        let s = read_set_of_curves(io, rel_offset(base_offset, offset_a)?, output_chan as u32)?;
        insert(&mut lut, StageLoc::End, s)?;
    }

    Ok(TagValue::Pipeline(lut))
}

/// `Type_LUTB2A_Write`.
pub(crate) fn write_lut_b2a<W: Write + Seek>(io: &mut W, value: &TagValue) -> Result<()> {
    let lut = as_pipeline(value)?;
    let parts = match_b2a(lut)?;
    write_lut_ab(io, lut, &parts)
}

/// Borrow the [`Pipeline`] out of a [`TagValue`].
fn as_pipeline(value: &TagValue) -> Result<&Pipeline> {
    match value {
        TagValue::Pipeline(p) => Ok(p),
        _ => Err(wrong_variant("pipeline")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::handler::TypeHandler;
    use crate::tags::testutil::round_trip_stable;

    /// Build a 3->3 LUT8-shaped pipeline: matrix + input curves + CLUT + output curves.
    fn lut8_pipeline() -> Pipeline {
        let mut p = Pipeline::alloc(3, 3).unwrap();
        let m = [2.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        p.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &m, None).unwrap());
        let ramp: Vec<u16> = (0..256).map(|i| from_8_to_16(i as u8)).collect();
        let curves: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_tabulated_16(256, &ramp).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&curves)).unwrap());
        // 2x2x2 identity-ish CLUT.
        let n = 3 * 2 * 2 * 2;
        let clut: Vec<u16> = (0..n).map(|i| (i * 100) as u16).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_clut_16bit(2, 3, 3, Some(&clut)).unwrap());
        let curves2: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_tabulated_16(256, &ramp).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&curves2)).unwrap());
        p
    }

    #[test]
    fn lut8_round_trip() {
        round_trip_stable(TypeHandler::Lut8, &TagValue::Pipeline(lut8_pipeline()));
    }

    #[test]
    fn lut16_round_trip() {
        // Reuse the same structure but with non-256 tables to exercise the 16-bit path.
        let mut p = Pipeline::alloc(3, 3).unwrap();
        let tbl: Vec<u16> = (0..1024).map(|i| (i * 64) as u16).collect();
        let curves: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_tabulated_16(1024, &tbl).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&curves)).unwrap());
        let n = 3 * 3 * 3 * 3;
        let clut: Vec<u16> = (0..n).map(|i| (i * 7) as u16).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_clut_16bit(3, 3, 3, Some(&clut)).unwrap());
        let curves2: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_tabulated_16(1024, &tbl).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&curves2)).unwrap());
        round_trip_stable(TypeHandler::Lut16, &TagValue::Pipeline(p));
    }

    /// A-CLUT-M-Matrix-B, the full A2B shape.
    fn a2b_pipeline() -> Pipeline {
        let mut p = Pipeline::alloc(3, 3).unwrap();
        let a: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_gamma(2.2).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&a)).unwrap());
        let n = 3 * 2 * 2 * 2;
        let clut: Vec<u16> = (0..n).map(|i| (i * 111) as u16).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_clut_16bit(2, 3, 3, Some(&clut)).unwrap());
        let m: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_gamma(1.0).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&m)).unwrap());
        let mat = [1.1, 0.0, 0.0, 0.0, 1.2, 0.0, 0.0, 0.0, 1.3];
        let off = [0.01, 0.02, 0.03];
        p.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &mat, Some(&off)).unwrap());
        let b: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_parametric(4, &[2.4, 0.947, 0.052, 0.077, 0.04, 0.0, 0.0]).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&b)).unwrap());
        p
    }

    #[test]
    fn lut_a2b_round_trip() {
        round_trip_stable(TypeHandler::LutA2B, &TagValue::Pipeline(a2b_pipeline()));
    }

    /// B-Matrix-M-CLUT-A, the full B2A shape.
    fn b2a_pipeline() -> Pipeline {
        let mut p = Pipeline::alloc(3, 3).unwrap();
        let b: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_gamma(2.2).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&b)).unwrap());
        let mat = [1.1, 0.0, 0.0, 0.0, 1.2, 0.0, 0.0, 0.0, 1.3];
        let off = [0.01, 0.02, 0.03];
        p.insert_stage(StageLoc::End, Stage::alloc_matrix(3, 3, &mat, Some(&off)).unwrap());
        let m: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_gamma(1.0).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&m)).unwrap());
        let n = 3 * 2 * 2 * 2;
        let clut: Vec<u16> = (0..n).map(|i| (i * 111) as u16).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_clut_16bit(2, 3, 3, Some(&clut)).unwrap());
        let a: Vec<ToneCurve> = (0..3).map(|_| ToneCurve::build_gamma(1.8).unwrap()).collect();
        p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, Some(&a)).unwrap());
        p
    }

    #[test]
    fn lut_b2a_round_trip() {
        round_trip_stable(TypeHandler::LutB2A, &TagValue::Pipeline(b2a_pipeline()));
    }

    /// C treats the empty pipeline differently per writer, on purpose. `Type_LUT8_Write`
    /// rejects it outright (`cmstypes.c:2120-2123`) while `Type_LUT16_Write` guards its walk
    /// with `mpe != NULL` (`cmstypes.c:2414-2415`) and writes a degenerate-but-valid `mft2`.
    /// Both halves are asserted so a future "unify the writers" pass cannot silently collapse
    /// them — sharing `disassemble_lut` is exactly what dropped the LUT8 check originally.
    #[test]
    fn empty_pipeline_rejected_by_lut8_only() {
        let empty = TagValue::Pipeline(Pipeline::alloc(3, 3).unwrap());

        let mut io = std::io::Cursor::new(Vec::new());
        assert!(write_lut8(&mut io, &empty).is_err(), "LUT8 must reject an empty pipeline");

        let mut io = std::io::Cursor::new(Vec::new());
        assert!(write_lut16(&mut io, &empty).is_ok(), "LUT16 must still accept an empty pipeline");
    }

    /// The same deliberate asymmetry on the V4 side: `Type_LUTA2B_Write` wraps its retrieval
    /// cascade in `if (Lut->Elements != NULL)` (`cmstypes.c:2966`), `Type_LUTB2A_Write` does not
    /// (`cmstypes.c:3154`) and so falls through to its `cmsERROR_NOT_SUITABLE` signal.
    #[test]
    fn empty_pipeline_rejected_by_b2a_only() {
        let empty = Pipeline::alloc(3, 3).unwrap();
        assert!(match_b2a(&empty).is_err(), "B2A must reject an empty pipeline");
        assert!(match_a2b(&empty).is_ok(), "A2B must still accept an empty pipeline");
    }
}
