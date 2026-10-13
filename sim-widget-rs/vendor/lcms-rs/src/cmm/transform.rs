//! The color transform (`cmsxform.c`).
//!
//! A [`Transform`] is the compiled, ready-to-run object produced from a chain of profiles: it
//! owns the optimized [`Pipeline`], the input/output pixel formatters, and the metadata needed
//! to convert raster buffers with [`Transform::do_transform`]. The construction path mirrors
//! lcms exactly — [`create_extended_transform`] links the profiles ([`crate::cmm::intents`]),
//! optimizes the pipeline ([`crate::cmm::optimize`]), binds formatters ([`crate::cmm::pack`]),
//! seeds the 1-pixel cache, and wires alpha handling ([`crate::cmm::alpha`]).
//!
//! ## The six evaluation kernels (`FloatXFORM`, `NullFloatXFORM`, `NullXFORM`,
//! `PrecalculatedXFORM`[`GamutCheck`], `CachedXFORM`[`GamutCheck`])
//!
//! Each is ported as a [`Transform`] method selected by [`XformKind`]. The 1-pixel cache
//! (`_cmsCACHE`) is kept as a **stack-local** in the cached kernels, seeded from the
//! transform's immutable `cache_in`/`cache_out`; this is what lets `do_transform` take `&self`
//! and keeps [`Transform`] `Sync` — a faithful reproduction of lcms's "copy of zero cache"
//! design (`memcpy(&Cache, &p->Cache, ...)`).
//!
//! Fully ported: gamut check (`cmsFLAGS_GAMUTCHECK`) builds
//! [`crate::cmm::gamut::create_gamut_check_pipeline`] and runs the gamut-check kernels; the
//! linear-RGB `NOOPTIMIZE` heuristic uses [`crate::cmm::gamut::detect_rgb_profile_gamma`]; and
//! `cmsFLAGS_KEEP_SEQUENCE` compiles the profile sequence (via [`Profile::clone`], which unblocks
//! the `&mut [Profile]` shape [`compile_profile_sequence`] requires).

use crate::cmm::alpha::handle_extra_channels;
use crate::cmm::gamut::{create_gamut_check_pipeline, detect_rgb_profile_gamma};
use crate::cmm::intents::link_profiles;
use crate::cmm::optimize::{optimize_pipeline_thr, FLAGS_NOCACHE, FLAGS_NOOPTIMIZE};
use crate::cmm::pack::{
    get_input_formatter_16_thr, get_input_formatter_float_thr, get_output_formatter_16_thr,
    get_output_formatter_float_thr, pixel_size, Formatter16In, Formatter16Out, FormatterFloatIn,
    FormatterFloatOut,
};
use crate::cmm::Stride;
use crate::color::pcs::{cms_channels_of, cms_channels_of_colorspace, lcms_colorspace, pt};
use crate::color::{D50X, D50Y, D50Z};
use crate::context::Context;
use crate::error::{ErrorCode, Result};
use crate::named::dict::Seq;
use crate::named::namedcolor::NamedColorList;
use crate::pipeline::Pipeline;
use crate::profile::read_hl::{
    compile_profile_sequence, INTENT_PERCEPTUAL, INTENT_RELATIVE_COLORIMETRIC,
};
use crate::profile::Profile;
use crate::tags::descriptor::tag_sig;
use crate::tags::value::TagValue;
use crate::types::color::CIEXYZ;
use crate::types::format::PixelFormat;
use crate::types::sig::{class, colorspace, Signature};
use crate::cms_err;
use crate::MAX_CHANNELS;

// -------------------------------------------------------------------------------------------
// Transform-level flags (lcms2.h) beyond those in cmm::optimize
// -------------------------------------------------------------------------------------------

/// `cmsFLAGS_GAMUTCHECK` — mark out-of-gamut pixels with the alarm codes.
pub const FLAGS_GAMUTCHECK: u32 = 0x1000;
/// `cmsFLAGS_NULLTRANSFORM` — apply formatters only, no color processing.
pub const FLAGS_NULLTRANSFORM: u32 = 0x0200;
/// `cmsFLAGS_BLACKPOINTCOMPENSATION`.
pub const FLAGS_BLACKPOINTCOMPENSATION: u32 = 0x2000;
/// `cmsFLAGS_COPY_ALPHA` — copy the extra (alpha) channels verbatim.
pub const FLAGS_COPY_ALPHA: u32 = 0x0400_0000;
/// `cmsFLAGS_CAN_CHANGE_FORMATTER` — the transform's formatters may be swapped afterwards.
pub const FLAGS_CAN_CHANGE_FORMATTER: u32 = 0x0200_0000;
/// `cmsFLAGS_KEEP_SEQUENCE` — record the profile sequence used to build the transform.
pub const FLAGS_KEEP_SEQUENCE: u32 = 0x0080;
/// `cmsFLAGS_SOFTPROOFING` (`lcms2.h:1753`) — simulate the proofing device's rendering.
pub const FLAGS_SOFTPROOFING: u32 = 0x4000;

// -------------------------------------------------------------------------------------------
// Transform
// -------------------------------------------------------------------------------------------

/// Which evaluation kernel a [`Transform`] runs (`p->xform`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum XformKind {
    /// `FloatXFORM` — full float conversion (also handles gamut check).
    Float,
    /// `NullFloatXFORM` — float formatters only.
    NullFloat,
    /// `NullXFORM` — 16-bit formatters only.
    Null,
    /// `PrecalculatedXFORM` — 16-bit, no cache, no gamut check.
    Precalc,
    /// `PrecalculatedXFORMGamutCheck` — 16-bit, no cache, gamut check.
    PrecalcGamut,
    /// `CachedXFORM` — 16-bit, 1-pixel cache, no gamut check.
    Cached,
    /// `CachedXFORMGamutCheck` — 16-bit, 1-pixel cache, gamut check.
    CachedGamut,
}

/// A compiled color transform (`_cmsTRANSFORM`).
///
/// `do_transform` takes `&self`; the mutable 1-pixel cache lives on the stack per call, so a
/// `Transform` is freely shareable across threads.
pub struct Transform {
    context: Context,
    input_format: PixelFormat,
    output_format: PixelFormat,
    dw_original_flags: u32,
    kind: XformKind,

    from_input: Option<Formatter16In>,
    to_output: Option<Formatter16Out>,
    from_input_float: Option<FormatterFloatIn>,
    to_output_float: Option<FormatterFloatOut>,

    /// The optimized pipeline (`p->Lut`); `None` only for a pure null transform with no pipeline.
    lut: Option<Pipeline>,
    /// The gamut-check pipeline (`p->GamutCheck`), built by [`create_extended_transform`] when
    /// [`FLAGS_GAMUTCHECK`] is set together with a gamut profile; `None` otherwise.
    gamut_check: Option<Pipeline>,

    cache_in: [u16; MAX_CHANNELS],
    cache_out: [u16; MAX_CHANNELS],

    entry_color_space: Signature,
    exit_color_space: Signature,
    rendering_intent: u32,
    entry_white_point: CIEXYZ,
    exit_white_point: CIEXYZ,

    input_colorant: Option<NamedColorList>,
    output_colorant: Option<NamedColorList>,

    /// The profile sequence recorded when `cmsFLAGS_KEEP_SEQUENCE` is set (`p->Sequence`).
    sequence: Option<Seq>,

    /// A transform plugin's replacement pixel loop (`p->xform` / `p->OldXform`), if a
    /// registered factory claimed this transform. When set it fully supersedes [`Self::kind`].
    xform: Option<crate::plugin::PluginXform>,
    /// Private state the claiming plugin attached (`p->UserData`).
    user_data: Option<crate::plugin::XformUserData>,

    /// The scheduler installed by `ParalellizeIfSuitable` (`p->xform` after the swap). When
    /// set, it runs instead of the pixel loop and is expected to call [`Self::run_worker`]
    /// once per slice — the demoted `p->Worker`.
    scheduler: Option<crate::plugin::TransformFn>,
    /// `p->MaxWorkers` — see [`Self::max_workers`].
    max_workers: i32,
    /// `p->WorkerFlags` — see [`Self::worker_flags`].
    worker_flags: u32,
}

impl Transform {
    /// `cmsGetTransformInputFormat`.
    pub fn input_format(&self) -> PixelFormat {
        self.input_format
    }
    /// `cmsGetTransformOutputFormat`.
    pub fn output_format(&self) -> PixelFormat {
        self.output_format
    }
    /// `cmsGetTransformPipeline` — the optimized pipeline (read-only).
    pub fn pipeline(&self) -> Option<&Pipeline> {
        self.lut.as_ref()
    }
    /// `cmsGetTransformGamutCheckPipeline` (`cmsxform.c:1453`) — the gamut-check pipeline
    /// (read-only). `None` unless the transform was built with [`FLAGS_GAMUTCHECK`] and a
    /// gamut profile.
    pub fn gamut_check_pipeline(&self) -> Option<&Pipeline> {
        self.gamut_check.as_ref()
    }

    /// `cmsChangeBuffersFormat` (`cmsxform.c:1469-1490`) — rebind the 16-bit formatters to a new
    /// input/output format pair.
    ///
    /// This is the recovery path for a transform built with both formats set to 0, whose
    /// formatters are the no-ops from `cmsxform.c:945-949` (see [`unroll_nothing`]). C guards it
    /// on `cmsFLAGS_CAN_CHANGE_FORMATTER`, i.e. it refuses transforms created with less than 16
    /// bits of precision, and rejects formats it has no formatter for.
    pub fn change_buffers_format(
        &mut self,
        input_format: PixelFormat,
        output_format: PixelFormat,
    ) -> Result<()> {
        // We only can afford to change formatters if previous transform is at least 16 bits.
        if self.dw_original_flags & FLAGS_CAN_CHANGE_FORMATTER == 0 {
            return Err(cms_err!(
                ErrorCode::NotSuitable,
                "cmsChangeBuffersFormat works only on transforms created originally with at \
                 least 16 bits of precision"
            ));
        }

        // C resolves through `p->ContextID` here (`cmsxform.c:1476-1477`).
        let from_input = get_input_formatter_16_thr(&self.context, input_format);
        let to_output = get_output_formatter_16_thr(&self.context, output_format);
        let (Some(from_input), Some(to_output)) = (from_input, to_output) else {
            return Err(cms_err!(ErrorCode::UnknownExtension, "Unsupported raster format"));
        };

        self.input_format = input_format;
        self.output_format = output_format;
        self.from_input = Some(from_input);
        self.to_output = Some(to_output);
        Ok(())
    }
    /// The entry (input) color space signature.
    pub fn entry_color_space(&self) -> Signature {
        self.entry_color_space
    }
    /// The exit (output) color space signature.
    pub fn exit_color_space(&self) -> Signature {
        self.exit_color_space
    }
    /// The rendering intent of the last profile.
    pub fn rendering_intent(&self) -> u32 {
        self.rendering_intent
    }
    /// The entry media white point.
    pub fn entry_white_point(&self) -> CIEXYZ {
        self.entry_white_point
    }
    /// The exit media white point.
    pub fn exit_white_point(&self) -> CIEXYZ {
        self.exit_white_point
    }
    /// `cmsGetTransformInputColorants`.
    pub fn input_colorant(&self) -> Option<&NamedColorList> {
        self.input_colorant.as_ref()
    }
    /// `cmsGetTransformOutputColorants`.
    pub fn output_colorant(&self) -> Option<&NamedColorList> {
        self.output_colorant.as_ref()
    }
    /// `cmsGetNamedColorList` — the named-colour list of a named-colour transform.
    ///
    /// Ports the C walk of `v->Lut->Elements`: the list is held by the pipeline's *first*
    /// stage when that stage is the named-colour lookup element (`cmsSigNamedColorElemType`).
    /// Returns `None` for any other transform (no pipeline, or a non-named-colour first stage).
    pub fn named_color_list(&self) -> Option<&NamedColorList> {
        self.lut.as_ref()?.first_stage()?.named_color_list()
    }

    /// `cmsGetTransformContextID` — the owning context.
    pub fn context(&self) -> &Context {
        &self.context
    }
    /// The recorded profile sequence (`p->Sequence`), set only when the transform was built with
    /// [`FLAGS_KEEP_SEQUENCE`].
    pub fn sequence(&self) -> Option<&Seq> {
        self.sequence.as_ref()
    }

    // -----------------------------------------------------------------------------------
    // Plugin-facing accessors (the `_cmsGetTransform*` family in cmsxform.c)
    // -----------------------------------------------------------------------------------

    /// `_cmsGetTransformUserData` — the private state a transform plugin attached.
    ///
    /// The C counterpart `_cmsSetTransformUserData` has no analogue: the state is handed over
    /// once, in [`crate::plugin::TransformFit`], and released when the transform drops (so the
    /// `_cmsFreeUserDataFn` half of the C API is unnecessary in Rust).
    pub fn user_data(&self) -> Option<&crate::plugin::XformUserData> {
        self.user_data.as_ref()
    }

    /// `_cmsGetTransformFlags` — the flags the transform was built with (`dwOriginalFlags`).
    pub fn flags(&self) -> u32 {
        self.dw_original_flags
    }

    /// `_cmsGetTransformFormatters16` — the 16-bit unpack/pack pair.
    pub fn formatters_16(&self) -> (Option<Formatter16In>, Option<Formatter16Out>) {
        (self.from_input, self.to_output)
    }

    /// `_cmsGetTransformFormattersFloat` — the float unpack/pack pair.
    pub fn formatters_float(&self) -> (Option<FormatterFloatIn>, Option<FormatterFloatOut>) {
        (self.from_input_float, self.to_output_float)
    }

    /// `_cmsGetTransformMaxWorkers` — the worker cap a parallelization plugin asked for, or
    /// [`crate::plugin::CMS_THREADED_GUESS_MAX_THREADS`] to auto-detect.
    pub fn max_workers(&self) -> i32 {
        self.max_workers
    }

    /// `_cmsGetTransformWorkerFlags` — reserved scheduler flags (unused, as in C).
    pub fn worker_flags(&self) -> u32 {
        self.worker_flags
    }

    // -----------------------------------------------------------------------------------
    // do_transform entry points
    // -----------------------------------------------------------------------------------

    /// `cmsDoTransform` — convert `pixels` pixels from `src` into `dst`.
    pub fn do_transform(&self, src: &[u8], dst: &mut [u8], pixels: usize) {
        let stride = Stride {
            bytes_per_line_in: 0,
            bytes_per_line_out: 0,
            bytes_per_plane_in: pixels * pixel_size(self.input_format),
            bytes_per_plane_out: pixels * pixel_size(self.output_format),
        };
        self.run(src, dst, pixels, 1, &stride);
    }

    /// `cmsDoTransformStride` — legacy planar stride variant.
    pub fn do_transform_stride(&self, src: &[u8], dst: &mut [u8], pixels: usize, stride: usize) {
        let stride = Stride {
            bytes_per_line_in: 0,
            bytes_per_line_out: 0,
            bytes_per_plane_in: stride,
            bytes_per_plane_out: stride,
        };
        self.run(src, dst, pixels, 1, &stride);
    }

    /// `cmsDoTransformLineStride` — the fully general line/plane stride entry.
    #[allow(clippy::too_many_arguments)]
    pub fn do_transform_line_stride(
        &self,
        src: &[u8],
        dst: &mut [u8],
        pixels_per_line: usize,
        line_count: usize,
        bytes_per_line_in: usize,
        bytes_per_line_out: usize,
        bytes_per_plane_in: usize,
        bytes_per_plane_out: usize,
    ) {
        let stride = Stride {
            bytes_per_line_in,
            bytes_per_line_out,
            bytes_per_plane_in,
            bytes_per_plane_out,
        };
        self.run(src, dst, pixels_per_line, line_count, &stride);
    }

    /// Dispatch to `p->xform`.
    ///
    /// After `ParalellizeIfSuitable` has run, `p->xform` **is** the plugin's scheduler and the
    /// real pixel loop has been demoted to `p->Worker`; that is exactly the two-level shape
    /// reproduced here.
    fn run(&self, src: &[u8], dst: &mut [u8], ppl: usize, lines: usize, stride: &Stride) {
        match self.scheduler {
            Some(schedule) => schedule(self, src, dst, ppl, lines, stride),
            None => self.run_worker(src, dst, ppl, lines, stride),
        }
    }

    /// `_cmsGetTransformWorker`'s callee — the pixel loop *without* the parallelization layer.
    ///
    /// A [`crate::plugin::ParallelizationConfig::scheduler`] calls this once per slice. With no
    /// parallelization plugin registered, [`Self::run`] calls it directly, so behaviour is
    /// identical to a build that never had the seam.
    pub fn run_worker(&self, src: &[u8], dst: &mut [u8], ppl: usize, lines: usize, stride: &Stride) {
        match self.xform {
            // A transform plugin claimed the conversion: its loop replaces every built-in.
            Some(crate::plugin::PluginXform::Full(f)) => f(self, src, dst, ppl, lines, stride),
            Some(crate::plugin::PluginXform::Legacy(f)) => {
                self.legacy_adaptor(f, src, dst, ppl, lines, stride)
            }
            None => match self.kind {
                XformKind::Float => self.xform_float(src, dst, ppl, lines, stride),
                XformKind::NullFloat => self.xform_null_float(src, dst, ppl, lines, stride),
                XformKind::Null => self.xform_null(src, dst, ppl, lines, stride),
                XformKind::Precalc => self.xform_precalc(src, dst, ppl, lines, stride, false),
                XformKind::PrecalcGamut => self.xform_precalc(src, dst, ppl, lines, stride, true),
                XformKind::Cached => self.xform_cached(src, dst, ppl, lines, stride, false),
                XformKind::CachedGamut => self.xform_cached(src, dst, ppl, lines, stride, true),
            },
        }
    }

    /// `_cmsTransform2toTransformAdaptor` — drive a pre-2.8 single-scanline plugin loop.
    ///
    /// Handles the extra channels once for the whole block (as C does), then calls the legacy
    /// function once per line with the plane stride.
    fn legacy_adaptor(
        &self,
        f: crate::plugin::LegacyTransformFn,
        src: &[u8],
        dst: &mut [u8],
        ppl: usize,
        lines: usize,
        stride: &Stride,
    ) {
        handle_extra_channels(
            self.input_format,
            self.output_format,
            self.copy_alpha(),
            src,
            dst,
            ppl,
            lines,
            *stride,
        );

        let mut stride_in = 0usize;
        let mut stride_out = 0usize;
        for _ in 0..lines {
            f(self, &src[stride_in..], &mut dst[stride_out..], ppl, stride.bytes_per_plane_in);
            stride_in += stride.bytes_per_line_in;
            stride_out += stride.bytes_per_line_out;
        }
    }

    // -----------------------------------------------------------------------------------
    // Kernels
    // -----------------------------------------------------------------------------------

    fn copy_alpha(&self) -> bool {
        self.dw_original_flags & FLAGS_COPY_ALPHA != 0
    }

    /// The 16-bit formatter pair used by the `Null`/`Precalc`/`Cached` kernels.
    ///
    /// The fields stay `Option` because two construction paths legitimately leave them unset,
    /// mirroring a NULL `Fmt16` in C: the float branch (`cmsxform.c:919-921`, which never
    /// selects a 16-bit kernel) and [`plugin_transform`] (`cmsxform.c:896-897`, where a missing
    /// formatter is explicitly not an error and the factory's own loop runs instead). Neither
    /// reaches this helper, and every path that *does* reach it assigns both — either real
    /// formatters, or the [`unroll_nothing`]/[`pack_nothing`] pair for the all-zero formats
    /// (`cmsxform.c:945-949`). Defaulting to that same pair keeps the kernels total without a
    /// panic and without diverging: no advance, no read, no write — precisely what C's
    /// `UnrollNothing`/`PackNothing` do.
    fn fmt16(&self) -> (Formatter16In, Formatter16Out) {
        (
            self.from_input.unwrap_or(unroll_nothing as Formatter16In),
            self.to_output.unwrap_or(pack_nothing as Formatter16Out),
        )
    }

    /// `TransformOnePixelWithGamutCheck`.
    fn one_pixel_gamut(&self, w_in: &[u16], w_out: &mut [u16]) {
        let lut = self.lut.as_ref().expect("precalc kernel requires a pipeline");
        if let Some(gc) = &self.gamut_check {
            let mut oog = [0u16; 1];
            gc.eval_16(w_in, &mut oog);
            if oog[0] >= 1 {
                let codes = self.context.alarm_codes();
                let n = lut.output_channels() as usize;
                w_out[..n].copy_from_slice(&codes[..n]);
                return;
            }
        }
        lut.eval_16(w_in, w_out);
    }

    /// `FloatXFORM`.
    fn xform_float(&self, in_: &[u8], out: &mut [u8], ppl: usize, lines: usize, stride: &Stride) {
        handle_extra_channels(
            self.input_format,
            self.output_format,
            self.copy_alpha(),
            in_,
            out,
            ppl,
            lines,
            *stride,
        );

        let lut = self.lut.as_ref().expect("float xform requires a pipeline");
        let from = self.from_input_float.expect("float input formatter");
        let to = self.to_output_float.expect("float output formatter");

        let mut f_in = [0f32; MAX_CHANNELS];
        let mut f_out = [0f32; MAX_CHANNELS];

        let mut stride_in = 0usize;
        let mut stride_out = 0usize;
        for _i in 0..lines {
            let mut accum = stride_in;
            let mut output = stride_out;
            for _j in 0..ppl {
                accum += from(self.input_format, &mut f_in, &in_[accum..], stride.bytes_per_plane_in);

                if let Some(gc) = &self.gamut_check {
                    let mut oog = [0f32; 1];
                    gc.eval_float(&f_in, &mut oog);
                    if oog[0] > 0.0 {
                        let codes = self.context.alarm_codes();
                        for c in 0..MAX_CHANNELS {
                            f_out[c] = codes[c] as f32 / 65535.0;
                        }
                    } else {
                        lut.eval_float(&f_in, &mut f_out);
                    }
                } else {
                    lut.eval_float(&f_in, &mut f_out);
                }

                output +=
                    to(self.output_format, &f_out, &mut out[output..], stride.bytes_per_plane_out);
            }
            stride_in += stride.bytes_per_line_in;
            stride_out += stride.bytes_per_line_out;
        }
    }

    /// `NullFloatXFORM`.
    fn xform_null_float(
        &self,
        in_: &[u8],
        out: &mut [u8],
        ppl: usize,
        lines: usize,
        stride: &Stride,
    ) {
        handle_extra_channels(
            self.input_format,
            self.output_format,
            self.copy_alpha(),
            in_,
            out,
            ppl,
            lines,
            *stride,
        );

        let from = self.from_input_float.expect("float input formatter");
        let to = self.to_output_float.expect("float output formatter");
        let mut f_in = [0f32; MAX_CHANNELS];

        let mut stride_in = 0usize;
        let mut stride_out = 0usize;
        for _i in 0..lines {
            let mut accum = stride_in;
            let mut output = stride_out;
            for _j in 0..ppl {
                accum += from(self.input_format, &mut f_in, &in_[accum..], stride.bytes_per_plane_in);
                output +=
                    to(self.output_format, &f_in, &mut out[output..], stride.bytes_per_plane_out);
            }
            stride_in += stride.bytes_per_line_in;
            stride_out += stride.bytes_per_line_out;
        }
    }

    /// `NullXFORM`.
    fn xform_null(&self, in_: &[u8], out: &mut [u8], ppl: usize, lines: usize, stride: &Stride) {
        handle_extra_channels(
            self.input_format,
            self.output_format,
            self.copy_alpha(),
            in_,
            out,
            ppl,
            lines,
            *stride,
        );

        let (from, to) = self.fmt16();
        let mut w_in = [0u16; MAX_CHANNELS];
        let mut stride_in = 0usize;
        let mut stride_out = 0usize;
        for _i in 0..lines {
            let mut accum = stride_in;
            let mut output = stride_out;
            for _j in 0..ppl {
                accum +=
                    from(self.input_format, &mut w_in, &in_[accum..], stride.bytes_per_plane_in);
                output +=
                    to(self.output_format, &w_in, &mut out[output..], stride.bytes_per_plane_out);
            }
            stride_in += stride.bytes_per_line_in;
            stride_out += stride.bytes_per_line_out;
        }
    }

    /// `PrecalculatedXFORM` / `PrecalculatedXFORMGamutCheck`.
    fn xform_precalc(
        &self,
        in_: &[u8],
        out: &mut [u8],
        ppl: usize,
        lines: usize,
        stride: &Stride,
        gamut: bool,
    ) {
        handle_extra_channels(
            self.input_format,
            self.output_format,
            self.copy_alpha(),
            in_,
            out,
            ppl,
            lines,
            *stride,
        );

        let lut = self.lut.as_ref().expect("precalc xform requires a pipeline");
        let (from, to) = self.fmt16();

        let mut w_in = [0u16; MAX_CHANNELS];
        let mut w_out = [0u16; MAX_CHANNELS];

        let mut stride_in = 0usize;
        let mut stride_out = 0usize;
        for _i in 0..lines {
            let mut accum = stride_in;
            let mut output = stride_out;
            for _j in 0..ppl {
                accum += from(self.input_format, &mut w_in, &in_[accum..], stride.bytes_per_plane_in);
                if gamut {
                    self.one_pixel_gamut(&w_in, &mut w_out);
                } else {
                    lut.eval_16(&w_in, &mut w_out);
                }
                output +=
                    to(self.output_format, &w_out, &mut out[output..], stride.bytes_per_plane_out);
            }
            stride_in += stride.bytes_per_line_in;
            stride_out += stride.bytes_per_line_out;
        }
    }

    /// `CachedXFORM` / `CachedXFORMGamutCheck`.
    fn xform_cached(
        &self,
        in_: &[u8],
        out: &mut [u8],
        ppl: usize,
        lines: usize,
        stride: &Stride,
        gamut: bool,
    ) {
        handle_extra_channels(
            self.input_format,
            self.output_format,
            self.copy_alpha(),
            in_,
            out,
            ppl,
            lines,
            *stride,
        );

        let lut = self.lut.as_ref().expect("cached xform requires a pipeline");
        let (from, to) = self.fmt16();

        let mut w_in = [0u16; MAX_CHANNELS];
        let mut w_out = [0u16; MAX_CHANNELS];

        // Stack-local copy of the seeded zero cache (memcpy(&Cache, &p->Cache, ...)).
        let mut cache_in = self.cache_in;
        let mut cache_out = self.cache_out;

        let mut stride_in = 0usize;
        let mut stride_out = 0usize;
        for _i in 0..lines {
            let mut accum = stride_in;
            let mut output = stride_out;
            for _j in 0..ppl {
                accum += from(self.input_format, &mut w_in, &in_[accum..], stride.bytes_per_plane_in);

                if w_in == cache_in {
                    w_out = cache_out;
                } else {
                    if gamut {
                        self.one_pixel_gamut(&w_in, &mut w_out);
                    } else {
                        lut.eval_16(&w_in, &mut w_out);
                    }
                    cache_in = w_in;
                    cache_out = w_out;
                }

                output +=
                    to(self.output_format, &w_out, &mut out[output..], stride.bytes_per_plane_out);
            }
            stride_in += stride.bytes_per_line_in;
            stride_out += stride.bytes_per_line_out;
        }
    }
}

// -------------------------------------------------------------------------------------------
// Construction
// -------------------------------------------------------------------------------------------

/// `_cmsFormatterIsFloat` — does this format carry floating-point samples?
fn is_float_format(fmt: PixelFormat) -> bool {
    fmt.float()
}

/// `UnrollNothing` (`cmsxform.c:832`) — an empty unroll that consumes no input.
///
/// Installed, together with [`pack_nothing`], when both formats are 0 (`cmsxform.c:945-949`).
/// Its C doc comment states the purpose verbatim: *"An empty unroll to avoid a check with NULL
/// on cmsDoTransform()"* (`cmsxform.c:823-825`). C returns `accum` unchanged; the Rust
/// formatters return the *advance* rather than the new pointer, so that is `0`.
fn unroll_nothing(_fmt: PixelFormat, _values: &mut [u16], _src: &[u8], _stride: usize) -> usize {
    0
}

/// `PackNothing` (`cmsxform.c:845`) — the pack half of [`unroll_nothing`]; writes nothing and
/// leaves the output pointer where it was (advance `0`).
fn pack_nothing(_fmt: PixelFormat, _values: &[u16], _dst: &mut [u8], _stride: usize) -> usize {
    0
}

/// `AllocEmptyTransform` — pick the kernel + formatters for the (possibly optimized) pipeline.
///
/// `input_format`/`output_format`/`flags` are updated in place, exactly as the C out-params
/// (`optimize` may rewrite the formats/flags; the float path forces `NOCACHE`).
fn alloc_empty_transform(
    ctx: &Context,
    mut lut: Option<Pipeline>,
    intent: u32,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    flags: &mut u32,
) -> Result<Transform> {
    // Let's see if any plug-in wants to do the transform by itself, before the optimizer gets
    // a look in. C skips the whole plugin loop when NOOPTIMIZE is set.
    if let Some(pipe) = lut.as_mut() {
        if *flags & FLAGS_NOOPTIMIZE == 0 {
            if let Some(fit) = crate::plugin::find_transform_factory(
                Some(ctx),
                pipe,
                input_format,
                output_format,
                flags,
            ) {
                return Ok(plugin_transform(ctx, lut, intent, input_format, output_format, flags, fit));
            }
        }
        // C discards the optimizer's return value by design (`cmsxform.c:914`): optimization is
        // an accelerator, not a precondition. A failure leaves the pipeline untouched and the
        // transform is still perfectly usable, so propagating the error here would refuse
        // transforms that lcms builds happily. Log it instead of swallowing it silently.
        if let Err(e) = optimize_pipeline_thr(ctx, pipe, intent, input_format, output_format, flags)
        {
            ctx.signal(e.code, &format!("pipeline optimization skipped: {}", e.message));
        }
    }

    let mut from_input = None;
    let mut to_output = None;
    let mut from_input_float = None;
    let mut to_output_float = None;
    let kind;

    if is_float_format(*input_format) || is_float_format(*output_format) {
        from_input_float = get_input_formatter_float_thr(ctx, *input_format);
        to_output_float = get_output_formatter_float_thr(ctx, *output_format);
        *flags |= FLAGS_CAN_CHANGE_FORMATTER;

        if from_input_float.is_none() || to_output_float.is_none() {
            return Err(cms_err!(ErrorCode::UnknownExtension, "Unsupported raster format"));
        }

        kind = if *flags & FLAGS_NULLTRANSFORM != 0 {
            XformKind::NullFloat
        } else {
            XformKind::Float
        };
    } else {
        if input_format.raw() == 0 && output_format.raw() == 0 {
            // Formats are intended to be changed before use (`cmsxform.c:944-949`): install the
            // no-op formatters so the 16-bit kernels never face a missing one, and mark the
            // transform as re-formattable for `Transform::change_buffers_format`.
            from_input = Some(unroll_nothing as Formatter16In);
            to_output = Some(pack_nothing as Formatter16Out);
            *flags |= FLAGS_CAN_CHANGE_FORMATTER;
        } else {
            from_input = get_input_formatter_16_thr(ctx, *input_format);
            to_output = get_output_formatter_16_thr(ctx, *output_format);
            if from_input.is_none() || to_output.is_none() {
                return Err(cms_err!(ErrorCode::UnknownExtension, "Unsupported raster format"));
            }
            let bpp = input_format.bytes();
            if bpp == 0 || bpp >= 2 {
                *flags |= FLAGS_CAN_CHANGE_FORMATTER;
            }
        }

        kind = if *flags & FLAGS_NULLTRANSFORM != 0 {
            XformKind::Null
        } else if *flags & FLAGS_NOCACHE != 0 {
            if *flags & FLAGS_GAMUTCHECK != 0 {
                XformKind::PrecalcGamut
            } else {
                XformKind::Precalc
            }
        } else if *flags & FLAGS_GAMUTCHECK != 0 {
            XformKind::CachedGamut
        } else {
            XformKind::Cached
        };
    }

    // Alpha channel consistency.
    if *flags & FLAGS_COPY_ALPHA != 0 && input_format.extra() != output_format.extra() {
        return Err(cms_err!(ErrorCode::NotSuitable, "Mismatched alpha channels"));
    }

    Ok(Transform {
        context: ctx.clone(),
        input_format: *input_format,
        output_format: *output_format,
        dw_original_flags: *flags,
        kind,
        from_input,
        to_output,
        from_input_float,
        to_output_float,
        lut,
        gamut_check: None,
        cache_in: [0; MAX_CHANNELS],
        cache_out: [0; MAX_CHANNELS],
        entry_color_space: colorspace::XYZ,
        exit_color_space: colorspace::XYZ,
        rendering_intent: intent,
        entry_white_point: CIEXYZ::default(),
        exit_white_point: CIEXYZ::default(),
        input_colorant: None,
        output_colorant: None,
        sequence: None,
        xform: None,
        user_data: None,
        scheduler: None,
        max_workers: 0,
        worker_flags: 0,
    }
    .parallelize_if_suitable())
}

/// Build the transform a plugin factory claimed (`AllocEmptyTransform`, plugin branch).
///
/// The factory owns the conversion, so [`Transform::kind`] is never consulted — but it must
/// hold *some* value, and [`XformKind::Precalc`] is the inert choice. Formatters are filled in
/// "just in case the optimized routine is interested" and, exactly as in C, a missing formatter
/// is **not** an error here: it is up to the factory to decide what it needs.
#[allow(clippy::too_many_arguments)]
fn plugin_transform(
    ctx: &Context,
    lut: Option<Pipeline>,
    intent: u32,
    input_format: &mut PixelFormat,
    output_format: &mut PixelFormat,
    flags: &mut u32,
    fit: crate::plugin::TransformFit,
) -> Transform {
    Transform {
        context: ctx.clone(),
        input_format: *input_format,
        output_format: *output_format,
        dw_original_flags: *flags,
        kind: XformKind::Precalc,
        from_input: get_input_formatter_16_thr(ctx, *input_format),
        to_output: get_output_formatter_16_thr(ctx, *output_format),
        from_input_float: get_input_formatter_float_thr(ctx, *input_format),
        to_output_float: get_output_formatter_float_thr(ctx, *output_format),
        lut,
        gamut_check: None,
        cache_in: [0; MAX_CHANNELS],
        cache_out: [0; MAX_CHANNELS],
        entry_color_space: colorspace::XYZ,
        exit_color_space: colorspace::XYZ,
        rendering_intent: intent,
        entry_white_point: CIEXYZ::default(),
        exit_white_point: CIEXYZ::default(),
        input_colorant: None,
        output_colorant: None,
        sequence: None,
        xform: Some(fit.xform),
        user_data: fit.user_data,
        scheduler: None,
        max_workers: 0,
        worker_flags: 0,
    }
    .parallelize_if_suitable()
}

impl Transform {
    /// `ParalellizeIfSuitable` — if a parallelization plugin is registered, demote the pixel
    /// loop to the worker and put the plugin's scheduler in front of it.
    ///
    /// C performs the swap on the `xform`/`Worker` function pointers; here [`Self::run`] reads
    /// [`Self::scheduler`] to get the same two-level dispatch. Called at the end of **both**
    /// construction paths, matching the two `ParalellizeIfSuitable(p)` calls in C.
    fn parallelize_if_suitable(mut self) -> Self {
        // C reads the plugin off `p->ContextID`; the transform carries its context for exactly
        // this reason, so neither construction path has to pass one in.
        if let Some(cfg) = crate::plugin::parallelization(Some(&self.context)) {
            self.scheduler = Some(cfg.scheduler);
            self.max_workers = cfg.max_workers;
            self.worker_flags = cfg.worker_flags;
        }
        self
    }
}

/// `GetXFormColorSpaces` — the entry/exit color spaces of a profile chain.
fn get_xform_color_spaces(profiles: &[&mut Profile]) -> Option<(Signature, Signature)> {
    let n = profiles.len();
    if n == 0 {
        return None;
    }

    let mut input = profiles[0].color_space();
    let mut post = input;

    for (i, prof) in profiles.iter().enumerate() {
        let l_is_input = post != colorspace::XYZ && post != colorspace::LAB;
        let cls = prof.device_class();

        let (cs_in, cs_out) = if cls == class::NAMED_COLOR {
            (
                colorspace::CLR1,
                if n > 1 { prof.pcs() } else { prof.color_space() },
            )
        } else if l_is_input || cls == class::LINK {
            (prof.color_space(), prof.pcs())
        } else {
            (prof.pcs(), prof.color_space())
        };

        if i == 0 {
            input = cs_in;
        }
        post = cs_out;
    }

    Some((input, post))
}

/// `IsProperColorSpace` — is `check` compatible with format `fmt`?
fn is_proper_color_space(check: Signature, fmt: PixelFormat) -> bool {
    if fmt.raw() == 0 {
        return true; // Bypass used by linkicc.
    }
    let space1 = fmt.colorspace() as i32;
    let space2 = lcms_colorspace(check);

    if space1 == pt::ANY {
        return fmt.channels() == cms_channels_of(check);
    }
    if space1 == space2 {
        return true;
    }
    if space1 == pt::LAB_V2 && space2 == pt::LAB {
        return true;
    }
    if space1 == pt::LAB && space2 == pt::LAB_V2 {
        return true;
    }
    false
}

/// `NormalizeXYZ` — the W2K "media white × 100/×10" sanity clamp.
fn normalize_xyz(dest: &mut CIEXYZ) {
    while dest.x > 2.0 && dest.y > 2.0 && dest.z > 2.0 {
        dest.x /= 10.0;
        dest.y /= 10.0;
        dest.z /= 10.0;
    }
}

/// `SetWhitePoint` — read a media white point, defaulting to D50 and normalizing.
fn set_white_point(src: Option<CIEXYZ>) -> CIEXYZ {
    match src {
        None => CIEXYZ { x: D50X, y: D50Y, z: D50Z },
        Some(mut wp) => {
            normalize_xyz(&mut wp);
            wp
        }
    }
}

/// Read the media white point tag as a plain XYZ, if present.
fn read_white_point(prof: &mut Profile) -> Option<CIEXYZ> {
    match prof.read_tag(tag_sig::MEDIA_WHITE_POINT).ok()? {
        TagValue::Xyz(xyz) => Some(xyz),
        _ => None,
    }
}

/// Read a colorant table tag as a named-color list, if present.
fn read_colorant_table(prof: &mut Profile, sig: Signature) -> Option<NamedColorList> {
    match prof.read_tag(sig).ok()? {
        TagValue::ColorantTable(list) => Some(list),
        _ => None,
    }
}

/// `cmsCreateExtendedTransform` — the full-parameter transform constructor.
///
/// When `gamut_profile` is supplied with [`FLAGS_GAMUTCHECK`], a gamut-check pipeline is built
/// (`gamut_pcs_position` selects the PCS slot) and out-of-gamut pixels are flagged with the
/// context alarm codes. A missing gamut profile drops the flag (matching lcms). The linear-RGB
/// `NOOPTIMIZE` heuristic and `cmsFLAGS_KEEP_SEQUENCE` are honoured.
#[allow(clippy::too_many_arguments)]
pub fn create_extended_transform(
    ctx: &Context,
    profiles: &mut [&mut Profile],
    bpc: &mut [bool],
    intents: &[u32],
    adaptation: &[f64],
    gamut_profile: Option<&mut Profile>,
    gamut_pcs_position: u32,
    input_format: PixelFormat,
    output_format: PixelFormat,
    flags: u32,
) -> Result<Transform> {
    let n_profiles = profiles.len();
    if n_profiles == 0 || n_profiles > 255 {
        return Err(cms_err!(ErrorCode::Range, "Wrong number of profiles (1..255), {n_profiles}"));
    }

    let mut input_format = input_format;
    let mut output_format = output_format;
    let mut flags = flags;

    let last_intent = intents[n_profiles - 1];

    // Fake (null) transform: no profiles processed.
    if flags & FLAGS_NULLTRANSFORM != 0 {
        return alloc_empty_transform(
            ctx,
            None,
            INTENT_PERCEPTUAL,
            &mut input_format,
            &mut output_format,
            &mut flags,
        );
    }

    // Gamut check requested but no gamut profile — drop the flag (as lcms does).
    if flags & FLAGS_GAMUTCHECK != 0 && gamut_profile.is_none() {
        flags &= !FLAGS_GAMUTCHECK;
    }
    // A gamut check needs a valid PCS position (1..=nProfiles-1).
    if flags & FLAGS_GAMUTCHECK != 0
        && (gamut_pcs_position < 1 || gamut_pcs_position as usize > n_profiles - 1)
    {
        return Err(cms_err!(ErrorCode::Range, "Wrong gamut PCS position '{gamut_pcs_position}'"));
    }

    // On floating point transforms, inhibit cache.
    if is_float_format(input_format) || is_float_format(output_format) {
        flags |= FLAGS_NOCACHE;
    }

    let (entry_color_space, exit_color_space) =
        get_xform_color_spaces(profiles).ok_or_else(|| cms_err!(ErrorCode::Null, "NULL input profiles"))?;

    if !is_proper_color_space(entry_color_space, input_format) {
        return Err(cms_err!(ErrorCode::ColorspaceCheck, "Wrong input color space"));
    }
    if !is_proper_color_space(exit_color_space, output_format) {
        return Err(cms_err!(ErrorCode::ColorspaceCheck, "Wrong output color space"));
    }

    // 16-bit transforms that begin with (near-)linear RGB defeat the matrix-shaper optimizer, so
    // disable optimizations for them (`cmsDetectRGBProfileGamma`).
    if entry_color_space == colorspace::RGB
        && input_format.bytes() == 2
        && flags & FLAGS_NOOPTIMIZE == 0
    {
        let gamma = detect_rgb_profile_gamma(&mut *profiles[0], 0.1);
        if gamma > 0.0 && gamma < 1.6 {
            flags |= FLAGS_NOOPTIMIZE;
        }
    }

    // Link the profiles into a pipeline.
    let lut = link_profiles(ctx, profiles, intents, bpc, adaptation, flags)?;

    // Channel-count sanity.
    if cms_channels_of_colorspace(entry_color_space) != lut.input_channels() as i32
        || cms_channels_of_colorspace(exit_color_space) != lut.output_channels() as i32
    {
        return Err(cms_err!(ErrorCode::NotSuitable, "Channel count doesn't match"));
    }

    let mut xform = alloc_empty_transform(
        ctx,
        Some(lut),
        last_intent,
        &mut input_format,
        &mut output_format,
        &mut flags,
    )?;

    xform.entry_color_space = entry_color_space;
    xform.exit_color_space = exit_color_space;
    xform.rendering_intent = last_intent;

    let entry_wp = read_white_point(profiles[0]);
    xform.entry_white_point = set_white_point(entry_wp);
    let exit_wp = read_white_point(profiles[n_profiles - 1]);
    xform.exit_white_point = set_white_point(exit_wp);

    // Input colorant table (only from the first profile).
    if profiles[0].is_tag(tag_sig::COLORANT_TABLE) {
        xform.input_colorant = read_colorant_table(profiles[0], tag_sig::COLORANT_TABLE);
    }

    // Output colorant table (devicelink uses ColorantTableOut).
    let last = n_profiles - 1;
    if profiles[last].device_class() == class::LINK {
        if profiles[last].is_tag(tag_sig::COLORANT_TABLE_OUT) {
            xform.output_colorant = read_colorant_table(profiles[last], tag_sig::COLORANT_TABLE_OUT);
        }
    } else if profiles[last].is_tag(tag_sig::COLORANT_TABLE) {
        xform.output_colorant = read_colorant_table(profiles[last], tag_sig::COLORANT_TABLE);
    }

    // Build the gamut-check pipeline when a gamut profile was supplied with the flag. `bpc` is the
    // post-link array, matching the C (which passes the same, mutated, BPC[] on).
    if flags & FLAGS_GAMUTCHECK != 0 {
        if let Some(gamut) = gamut_profile {
            let gc = create_gamut_check_pipeline(
                ctx,
                profiles,
                bpc,
                intents,
                adaptation,
                gamut_pcs_position,
                gamut,
            )?;
            xform.gamut_check = Some(gc);
        }
    }

    // Store the sequence of profiles (`cmsFLAGS_KEEP_SEQUENCE`). `_cmsCompileProfileSequence`
    // wants owned profiles; `Profile::clone` supplies them from the `&mut [&mut Profile]` chain.
    if flags & FLAGS_KEEP_SEQUENCE != 0 {
        let mut owned: Vec<Profile> = profiles.iter().map(|p| (**p).clone()).collect();
        xform.sequence = compile_profile_sequence(&mut owned).ok();
    }

    // Seed the 1-pixel cache for cached kernels (16-bit, non-NOCACHE).
    if matches!(xform.kind, XformKind::Cached | XformKind::CachedGamut) {
        xform.cache_in = [0; MAX_CHANNELS];
        let mut cache_out = [0u16; MAX_CHANNELS];
        if xform.gamut_check.is_some() {
            xform.one_pixel_gamut(&xform.cache_in.clone(), &mut cache_out);
        } else if let Some(lut) = xform.lut.as_ref() {
            lut.eval_16(&xform.cache_in, &mut cache_out);
        }
        xform.cache_out = cache_out;
    }

    Ok(xform)
}

/// The current context adaptation state (`cmsSetAdaptationStateTHR(ctx, -1)` — the query side),
/// The context's observer adaptation state.
///
/// No negative-sentinel fallback: `Context::set_adaptation_state` reproduces C's `if (d >= 0.0)`
/// guard (`cmsxform.c:66`), so a negative value can never be stored in the first place. The
/// previous fallback here existed only because that guard was missing.
fn adaptation_state(ctx: &Context) -> f64 {
    ctx.adaptation_state()
}

/// `cmsCreateMultiprofileTransform` — one intent + one BPC flag for every profile.
pub fn create_multiprofile_transform(
    ctx: &Context,
    profiles: &mut [&mut Profile],
    input_format: PixelFormat,
    output_format: PixelFormat,
    intent: u32,
    flags: u32,
) -> Result<Transform> {
    let n = profiles.len();
    if n == 0 || n > 255 {
        return Err(cms_err!(ErrorCode::Range, "Wrong number of profiles (1..255), {n}"));
    }

    let do_bpc = flags & FLAGS_BLACKPOINTCOMPENSATION != 0;
    let mut bpc = vec![do_bpc; n];
    let intents = vec![intent; n];
    let adaptation = vec![adaptation_state(ctx); n];

    create_extended_transform(
        ctx,
        profiles,
        &mut bpc,
        &intents,
        &adaptation,
        None,
        0,
        input_format,
        output_format,
        flags,
    )
}

/// `cmsCreateTransform` — the common two-profile (input → output) constructor.
///
/// # Why the profiles are `&mut`
///
/// Building a transform reads tags out of both profiles, and [`Profile::read_tag`] is `&mut`
/// because it **caches**: an `OnDisk` tag is decoded once and the cooked value stored back into
/// the tag dictionary, exactly as `cmsReadTag` populates `TagPtrs[]`. So the mutation is
/// load-bearing, not incidental - it is the lazy tag cache, and it is why C can take an opaque
/// `cmsHPROFILE` while this signature cannot take `&Profile`.
///
/// A consumer holding profiles behind a shared wrapper therefore has to propagate `&mut` or
/// clone. [`Profile`] is [`Clone`] and clones share one backing store through the same
/// `Arc<Mutex<..>>`, so cloning is cheap - it duplicates the tag dictionary, not the bytes.
///
/// Moving to `&Profile` would mean putting the tag payloads behind that same lock. That is a
/// deliberate design change, not an oversight; see `BUG.md` item 3.
#[allow(clippy::too_many_arguments)]
pub fn create_transform(
    ctx: &Context,
    input: &mut Profile,
    input_format: PixelFormat,
    output: &mut Profile,
    output_format: PixelFormat,
    intent: u32,
    flags: u32,
) -> Result<Transform> {
    let mut arr = [input, output];
    create_multiprofile_transform(ctx, &mut arr, input_format, output_format, intent, flags)
}

/// `cmsCreateProofingTransform` (`cmsxform.c:1365-1394`) — soft proofing / gamut check.
///
/// Builds the four-slot chain `[input, proofing, proofing, output]` with intents
/// `[intent, intent, INTENT_RELATIVE_COLORIMETRIC, proofing_intent]` and BPC
/// `[do_bpc, do_bpc, false, false]`, using the proofing profile as the gamut profile at PCS
/// position 1. When neither [`FLAGS_SOFTPROOFING`] nor [`FLAGS_GAMUTCHECK`] is requested there
/// is nothing to proof, so C degrades to a plain [`create_transform`] — reproduced here.
///
/// C aliases the single `ProofingProfile` handle across three slots; `&mut Profile` forbids
/// that, so the extra slots take [`Profile::clone`]s. Cloning shares the backing store and
/// only duplicates the tag dictionary, so the reads are equivalent (see the [`Profile`] docs,
/// which call out this exact aliasing case).
#[allow(clippy::too_many_arguments)]
pub fn create_proofing_transform(
    ctx: &Context,
    input: &mut Profile,
    input_format: PixelFormat,
    output: &mut Profile,
    output_format: PixelFormat,
    proofing: &mut Profile,
    intent: u32,
    proofing_intent: u32,
    flags: u32,
) -> Result<Transform> {
    if flags & (FLAGS_SOFTPROOFING | FLAGS_GAMUTCHECK) == 0 {
        return create_transform(ctx, input, input_format, output, output_format, intent, flags);
    }

    let do_bpc = flags & FLAGS_BLACKPOINTCOMPENSATION != 0;
    let mut bpc = [do_bpc, do_bpc, false, false];
    let intents = [intent, intent, INTENT_RELATIVE_COLORIMETRIC, proofing_intent];
    let adaptation = [adaptation_state(ctx); 4];

    let mut proof_mid = proofing.clone();
    let mut proof_gamut = proofing.clone();

    create_extended_transform(
        ctx,
        &mut [input, proofing, &mut proof_mid, output],
        &mut bpc,
        &intents,
        &adaptation,
        Some(&mut proof_gamut),
        1,
        input_format,
        output_format,
        flags,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::pcs::pt;
    use crate::profile::read_hl::INTENT_SATURATION;
    use crate::types::format::FormatBuilder;

    fn rgb8() -> PixelFormat {
        FormatBuilder::new().colorspace(pt::RGB as u32).channels(3).bytes(1).build()
    }
    fn rgb16() -> PixelFormat {
        FormatBuilder::new().colorspace(pt::RGB as u32).channels(3).bytes(2).build()
    }
    fn rgb_flt() -> PixelFormat {
        FormatBuilder::new().float(true).colorspace(pt::RGB as u32).channels(3).bytes(4).build()
    }
    fn cmyk16() -> PixelFormat {
        FormatBuilder::new().colorspace(pt::CMYK as u32).channels(4).bytes(2).build()
    }

    use crate::test_profile as load;

    fn read3(dst: &[u8]) -> [u16; 3] {
        [
            u16::from_ne_bytes([dst[0], dst[1]]),
            u16::from_ne_bytes([dst[2], dst[3]]),
            u16::from_ne_bytes([dst[4], dst[5]]),
        ]
    }

    /// A synthetic CMYK proof profile whose **perceptual and colorimetric tables differ**.
    ///
    /// No Little-CMS testbed profile carries both, and `read_input_lut` falls back to the
    /// perceptual tag when the requested one is missing (`DEVICE_TO_PCS_16[intent]` then
    /// "revert to perceptual", `read_hl.rs`). With a single-table fixture every intent therefore
    /// resolves to the same pipeline, and an intent-selection bug is invisible.
    ///
    /// `A2B0` and `A2B1` here map CMYK to deliberately different Lab, so which one a chain slot
    /// picks shows up in the output bytes. Built in memory rather than shipped as a binary blob:
    /// the difference is then by construction and reviewable in the diff.
    fn cmyk_proof_with_distinct_intent_tables() -> Profile {
        use crate::pipeline::{Stage, StageLoc};
        use crate::tags::descriptor::tag_sig;
        use crate::tags::TagValue;
        use crate::types::sig::{class, colorspace};

        // 2 nodes per axis: CMYK -> Lab is 2^4 = 16 nodes x 3 outputs.
        fn a2b(scale: u32) -> Pipeline {
            let mut table = [0u16; 16 * 3];
            for (n, slot) in table.chunks_exact_mut(3).enumerate() {
                let n = n as u32;
                // Node index drives L*; `scale` pulls a/b apart between the two tables.
                slot[0] = (n * 65535 / 15) as u16;
                slot[1] = ((n * scale) % 65536) as u16;
                slot[2] = ((n * scale * 3) % 65536) as u16;
            }
            let mut p = Pipeline::alloc(4, 3).expect("alloc 4->3");
            p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(4, None).expect("A curves"));
            let clut = Stage::alloc_clut_16bit(2, 4, 3, Some(&table)).expect("A2B clut");
            p.insert_stage(StageLoc::End, clut);
            p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, None).expect("B curves"));
            p
        }

        // Lab -> CMYK for the forward leg (slot 1 reads B2A with the requested intent).
        fn b2a() -> Pipeline {
            let mut table = [0u16; 8 * 4];
            for (n, slot) in table.chunks_exact_mut(4).enumerate() {
                let v = (n as u32 * 65535 / 7) as u16;
                slot[0] = v;
                slot[1] = v / 2;
                slot[2] = v / 3;
                slot[3] = v / 4;
            }
            let mut p = Pipeline::alloc(3, 4).expect("alloc 3->4");
            p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(3, None).expect("A curves"));
            let clut = Stage::alloc_clut_16bit(2, 3, 4, Some(&table)).expect("B2A clut");
            p.insert_stage(StageLoc::End, clut);
            p.insert_stage(StageLoc::End, Stage::alloc_tone_curves(4, None).expect("B curves"));
            p
        }

        let mut p = Profile::create_placeholder(Context::new());
        p.set_profile_version(4.3);
        p.set_device_class(class::OUTPUT);
        p.set_color_space(colorspace::CMYK);
        p.set_pcs(colorspace::LAB);

        // The whole point of the fixture: perceptual != relative colorimetric.
        p.write_tag(tag_sig::A_TO_B0, TagValue::Pipeline(a2b(1024))).expect("A2B0");
        p.write_tag(tag_sig::A_TO_B1, TagValue::Pipeline(a2b(4096))).expect("A2B1");
        p.write_tag(tag_sig::B_TO_A0, TagValue::Pipeline(b2a())).expect("B2A0");
        p.write_tag(tag_sig::B_TO_A1, TagValue::Pipeline(b2a())).expect("B2A1");
        p
    }

    /// Black-point compensation now runs the real round-trip black-point detection, so a BPC
    /// transform to a raised-black CMYK printer differs from the non-BPC one in the shadows.
    #[test]
    fn bpc_changes_shadow_mapping() {
        let ctx = Context::new();
        let run = |flags: u32| -> [u16; 4] {
            let mut a = load("ibm-t61.icc"); // RGB display (black ≈ 0)
            let mut b = load("test1.icc"); // CMYK printer (raised black)
            let xf = create_transform(
                &ctx,
                &mut a,
                rgb16(),
                &mut b,
                cmyk16(),
                INTENT_RELATIVE_COLORIMETRIC,
                flags,
            )
            .expect("xform");
            let src: [u16; 3] = [0x1000, 0x1000, 0x1000]; // deep shadow grey
            let mut src_b = [0u8; 6];
            for (i, &v) in src.iter().enumerate() {
                src_b[i * 2..i * 2 + 2].copy_from_slice(&v.to_ne_bytes());
            }
            let mut dst = [0u8; 8];
            xf.do_transform(&src_b, &mut dst, 1);
            let mut out = [0u16; 4];
            for i in 0..4 {
                out[i] = u16::from_ne_bytes([dst[i * 2], dst[i * 2 + 1]]);
            }
            out
        };
        let no_bpc = run(0);
        let with_bpc = run(FLAGS_BLACKPOINTCOMPENSATION);
        assert_ne!(no_bpc, with_bpc, "BPC must change the shadow mapping ({no_bpc:?} vs {with_bpc:?})");
    }

    /// A proofing transform built with `cmsFLAGS_GAMUTCHECK` and a (smaller) CMYK gamut profile
    /// now marks out-of-gamut colours with the context alarm codes.
    #[test]
    fn gamut_check_marks_out_of_gamut_with_alarm_codes() {
        let mut ctx = Context::new();
        let mut alarm = [0u16; MAX_CHANNELS];
        alarm[0] = 0xDEAD;
        alarm[1] = 0xBEEF;
        alarm[2] = 0xCAFE;
        ctx.set_alarm_codes(alarm);

        let mut a = load("crayons.icc"); // wide RGB display
        let mut b = load("ibm-t61.icc"); // RGB display
        let mut gam = load("test1.icc"); // CMYK printer — a smaller gamut
        let mut bpc = [false, false];
        let xf = create_extended_transform(
            &ctx,
            &mut [&mut a, &mut b],
            &mut bpc,
            &[INTENT_RELATIVE_COLORIMETRIC; 2],
            &[1.0; 2],
            Some(&mut gam),
            1,
            rgb16(),
            rgb16(),
            FLAGS_GAMUTCHECK,
        )
        .expect("gamut-check transform");

        // Saturated primaries/secondaries — several land outside the CMYK gamut.
        let palette: [[u16; 3]; 6] = [
            [0, 0xFFFF, 0],
            [0xFFFF, 0, 0],
            [0, 0, 0xFFFF],
            [0, 0xFFFF, 0xFFFF],
            [0xFFFF, 0, 0xFFFF],
            [0xFFFF, 0xFFFF, 0],
        ];
        let mut alarmed = 0;
        for rgb in palette {
            let mut src_b = [0u8; 6];
            for (i, &v) in rgb.iter().enumerate() {
                src_b[i * 2..i * 2 + 2].copy_from_slice(&v.to_ne_bytes());
            }
            let mut dst = [0u8; 6];
            xf.do_transform(&src_b, &mut dst, 1);
            if read3(&dst) == [0xDEAD, 0xBEEF, 0xCAFE] {
                alarmed += 1;
            }
        }
        assert!(alarmed >= 1, "expected at least one out-of-gamut colour to hit the alarm codes");

        // A neutral mid grey is in gamut and must NOT be alarm-coded.
        let mut src_b = [0u8; 6];
        for i in 0..3 {
            src_b[i * 2..i * 2 + 2].copy_from_slice(&0x8000u16.to_ne_bytes());
        }
        let mut dst = [0u8; 6];
        xf.do_transform(&src_b, &mut dst, 1);
        assert_ne!(read3(&dst), [0xDEAD, 0xBEEF, 0xCAFE], "neutral grey should be in gamut");
    }

    /// End-to-end 8-bit RGB→RGB self transform on a real profile: black stays near black,
    /// white stays near white, and a mid grey round-trips near itself (near-identity).
    #[test]
    fn end_to_end_rgb8_self_transform() {
        let ctx = Context::new();
        let mut a = load("ibm-t61.icc");
        let mut b = load("ibm-t61.icc");
        let xform = create_transform(
            &ctx,
            &mut a,
            rgb8(),
            &mut b,
            rgb8(),
            INTENT_RELATIVE_COLORIMETRIC,
            0,
        )
        .expect("create transform");

        let src: [u8; 9] = [0, 0, 0, 128, 128, 128, 255, 255, 255];
        let mut dst = [0u8; 9];
        xform.do_transform(&src, &mut dst, 3);

        // Black near black, white near white (matrix-shaper self transform ≈ identity).
        assert!(dst[0] <= 4 && dst[1] <= 4 && dst[2] <= 4, "black -> {:?}", &dst[0..3]);
        assert!(dst[6] >= 251 && dst[7] >= 251 && dst[8] >= 251, "white -> {:?}", &dst[6..9]);
        for c in 0..3 {
            assert!((dst[3 + c] as i32 - 128).abs() <= 3, "grey -> {:?}", &dst[3..6]);
        }
    }

    /// The 16-bit cached kernel must agree with the same conversion and honour black/white.
    #[test]
    fn end_to_end_rgb16_self_transform() {
        let ctx = Context::new();
        let mut a = load("ibm-t61.icc");
        let mut b = load("ibm-t61.icc");
        let xform = create_transform(
            &ctx,
            &mut a,
            rgb16(),
            &mut b,
            rgb16(),
            INTENT_RELATIVE_COLORIMETRIC,
            0,
        )
        .expect("create transform");

        let src: [u16; 6] = [0, 0, 0, 65535, 65535, 65535];
        let mut src_bytes = [0u8; 12];
        for (i, &v) in src.iter().enumerate() {
            src_bytes[i * 2..i * 2 + 2].copy_from_slice(&v.to_ne_bytes());
        }
        let mut dst = [0u8; 12];
        xform.do_transform(&src_bytes, &mut dst, 2);

        let read = |o: usize| u16::from_ne_bytes([dst[o], dst[o + 1]]);
        // Black stays near black: the matrix-shaper self-transform is not exactly zero (the TRCs
        // and channel mixing leave a small residue), matching the 8-bit `<= 4` bound
        // (4/255 ≈ 1028/65535).
        assert!(read(0) <= 1030 && read(2) <= 1030 && read(4) <= 1030, "black");
        assert!(read(6) >= 64900 && read(8) >= 64900 && read(10) >= 64900, "white");
    }

    /// The float kernel path must run and keep black/white near the extremes.
    #[test]
    fn end_to_end_rgb_float_self_transform() {
        let ctx = Context::new();
        let mut a = load("ibm-t61.icc");
        let mut b = load("ibm-t61.icc");
        let xform = create_transform(
            &ctx,
            &mut a,
            rgb_flt(),
            &mut b,
            rgb_flt(),
            INTENT_RELATIVE_COLORIMETRIC,
            0,
        )
        .expect("create transform");

        let src: [f32; 6] = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let mut src_bytes = [0u8; 24];
        for (i, &v) in src.iter().enumerate() {
            src_bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        let mut dst = [0u8; 24];
        xform.do_transform(&src_bytes, &mut dst, 2);

        let read = |o: usize| f32::from_ne_bytes([dst[o], dst[o + 1], dst[o + 2], dst[o + 3]]);
        assert!(read(0) < 0.02 && read(4) < 0.02 && read(8) < 0.02, "black");
        assert!(read(12) > 0.98 && read(20) > 0.98, "white");
    }

    /// H5 — both formats zero is the "formats are intended to be changed before use" case
    /// (`cmsxform.c:944-949`). C installs `UnrollNothing`/`PackNothing` and flags the transform
    /// `cmsFLAGS_CAN_CHANGE_FORMATTER`; running it must be a silent no-op, not a panic.
    #[test]
    fn zero_formats_install_noop_formatters_and_do_not_panic() {
        let ctx = Context::new();
        let zero = PixelFormat::from_raw(0);
        let mut a = load("ibm-t61.icc");
        let mut b = load("ibm-t61.icc");
        let xf = create_transform(&ctx, &mut a, zero, &mut b, zero, INTENT_PERCEPTUAL, 0)
            .expect("xform");

        assert!(
            xf.flags() & FLAGS_CAN_CHANGE_FORMATTER != 0,
            "C sets cmsFLAGS_CAN_CHANGE_FORMATTER on this branch"
        );
        let (from, to) = xf.formatters_16();
        assert!(from.is_some() && to.is_some(), "both no-op formatters must be installed");

        // The no-ops read nothing and write nothing, so `dst` stays untouched.
        let src = [0u8; 16];
        let mut dst = [0xABu8; 16];
        xf.do_transform(&src, &mut dst, 1);
        assert_eq!(dst, [0xABu8; 16], "UnrollNothing/PackNothing must not touch the buffers");
    }

    /// `cmsChangeBuffersFormat` (`cmsxform.c:1469-1490`) makes the zero-format transform usable:
    /// after rebinding to real 16-bit formats it converts like a normally-built one. It also
    /// refuses unknown formats, and refuses transforms without `CAN_CHANGE_FORMATTER` at all.
    #[test]
    fn change_buffers_format_rebinds_and_guards() {
        let ctx = Context::new();
        let zero = PixelFormat::from_raw(0);
        let mut a = load("ibm-t61.icc");
        let mut b = load("ibm-t61.icc");
        let mut xf = create_transform(&ctx, &mut a, zero, &mut b, zero, INTENT_PERCEPTUAL, 0)
            .expect("xform");

        xf.change_buffers_format(rgb16(), rgb16()).expect("rebind to 16-bit RGB");
        assert_eq!(xf.input_format().raw(), rgb16().raw());
        assert_eq!(xf.output_format().raw(), rgb16().raw());

        // Now it really converts: white in, near-white out (matrix-shaper self transform).
        let mut src = [0u8; 6];
        for i in 0..3 {
            src[i * 2..i * 2 + 2].copy_from_slice(&0xFFFFu16.to_ne_bytes());
        }
        let mut dst = [0u8; 6];
        xf.do_transform(&src, &mut dst, 1);
        let out = read3(&dst);
        assert!(out.iter().all(|&v| v >= 64900), "white -> {out:?}");

        // An unsupported raster format is rejected, and nothing is mutated.
        let bogus = PixelFormat::from_raw(0xFFFF_FFFF);
        assert!(xf.change_buffers_format(bogus, rgb16()).is_err(), "unknown format must fail");
        assert_eq!(xf.input_format().raw(), rgb16().raw(), "a failed rebind must not mutate");

        // A float transform never gets CAN_CHANGE_FORMATTER cleared, so use a plain 8-bit one:
        // C sets the flag only when T_BYTES is 0 or >= 2, so an 8-bit transform is refused.
        let mut c = load("ibm-t61.icc");
        let mut d = load("ibm-t61.icc");
        let mut narrow = create_transform(&ctx, &mut c, rgb8(), &mut d, rgb8(), INTENT_PERCEPTUAL, 0)
            .expect("8-bit xform");
        assert_eq!(narrow.flags() & FLAGS_CAN_CHANGE_FORMATTER, 0, "8-bit is below the bar");
        assert!(
            narrow.change_buffers_format(rgb16(), rgb16()).is_err(),
            "less than 16 bits of precision must be refused (cmsERROR_NOT_SUITABLE)"
        );
    }

    /// `cmsCreateProofingTransform` (`cmsxform.c:1365-1394`): with SOFTPROOFING|GAMUTCHECK it
    /// builds the four-slot `[In, Proof, Proof, Out]` chain with the proofing profile as the
    /// gamut profile at PCS position 1.
    ///
    /// The load-bearing assertion is the last one: the result must be byte-identical to a
    /// `create_extended_transform` fed C's arrays literally
    /// (`hArray = [In, Proof, Proof, Out]`, `Intents = [n, n, RELCOL, proofIntent]`,
    /// `BPC = [d, d, 0, 0]`, `nGamutPCSposition = 1`). Comparing against the *direct* transform
    /// alone would only prove the chain is not a straight conversion.
    ///
    /// Verified by mutation to pin the slot ORDER, `nGamutPCSposition` (changing 1 to 2 reddens
    /// it) AND the hardcoded RELCOL in slot 2 - the last of those only because the proof profile
    /// is [`cmyk_proof_with_distinct_intent_tables`], whose A2B0 and A2B1 differ. With a
    /// testbed fixture that carries one table, `read_input_lut` reverts to perceptual and every
    /// intent resolves to the same pipeline, so slot 2 could be given any intent and the test
    /// would still pass.
    #[test]
    fn proofing_transform_builds_four_slot_chain() {
        let mut ctx = Context::new();
        let mut alarm = [0u16; MAX_CHANNELS];
        alarm[0] = 0xDEAD;
        alarm[1] = 0xBEEF;
        alarm[2] = 0xCAFE;
        ctx.set_alarm_codes(alarm);

        let mut a = load("crayons.icc"); // wide RGB source
        let mut b = load("ibm-t61.icc"); // RGB display
        // Distinct perceptual/colorimetric tables, so slot 2's intent is observable.
        let mut proof = cmyk_proof_with_distinct_intent_tables();
        let xf = create_proofing_transform(
            &ctx,
            &mut a,
            rgb16(),
            &mut b,
            rgb16(),
            &mut proof,
            // Deliberately DISTINCT from the proofing intent, and both distinct from the
            // hardcoded slot-2 RELCOL - otherwise the arrays below cannot tell the slots apart
            // and the shape assertion is satisfiable by a wrong chain.
            INTENT_PERCEPTUAL,
            INTENT_SATURATION,
            FLAGS_SOFTPROOFING | FLAGS_GAMUTCHECK,
        )
        .expect("proofing transform");

        assert!(
            xf.gamut_check_pipeline().is_some(),
            "nGamutPCSposition=1 + the proofing profile must yield a gamut-check pipeline"
        );

        // Going through the CMYK proof is not the same conversion as going straight across.
        let mut a2 = load("crayons.icc");
        let mut b2 = load("ibm-t61.icc");
        let direct = create_transform(
            &ctx,
            &mut a2,
            rgb16(),
            &mut b2,
            rgb16(),
            INTENT_RELATIVE_COLORIMETRIC,
            0,
        )
        .expect("direct transform");

        let mut src = [0u8; 6];
        for (i, &v) in [0u16, 0xFFFF, 0].iter().enumerate() {
            src[i * 2..i * 2 + 2].copy_from_slice(&v.to_ne_bytes());
        }
        let (mut proofed, mut plain) = ([0u8; 6], [0u8; 6]);
        xf.do_transform(&src, &mut proofed, 1);
        direct.do_transform(&src, &mut plain, 1);
        assert_ne!(
            read3(&proofed),
            read3(&plain),
            "the proofing chain must not collapse to the direct conversion"
        );

        // Pin the SHAPE: hand-assemble C's four arrays and require identical output.
        let mut h0 = load("crayons.icc");
        let mut h1 = cmyk_proof_with_distinct_intent_tables();
        let mut h2 = cmyk_proof_with_distinct_intent_tables();
        let mut h3 = load("ibm-t61.icc");
        let mut gamut = cmyk_proof_with_distinct_intent_tables();
        let mut bpc = [false, false, false, false];
        // C: Intents[0] = Intents[1] = nIntent, Intents[2] = RELCOL, Intents[3] = ProofingIntent.
        let intents = [
            INTENT_PERCEPTUAL,
            INTENT_PERCEPTUAL,
            INTENT_RELATIVE_COLORIMETRIC,
            INTENT_SATURATION,
        ];
        let adaptation = [adaptation_state(&ctx); 4];
        let expected = create_extended_transform(
            &ctx,
            &mut [&mut h0, &mut h1, &mut h2, &mut h3],
            &mut bpc,
            &intents,
            &adaptation,
            Some(&mut gamut),
            1,
            rgb16(),
            rgb16(),
            FLAGS_SOFTPROOFING | FLAGS_GAMUTCHECK,
        )
        .expect("hand-built four-slot chain");

        let mut want = [0u8; 6];
        expected.do_transform(&src, &mut want, 1);
        assert_eq!(
            read3(&proofed),
            read3(&want),
            "proofing must equal the literal C chain [In, Proof, Proof, Out] @ PCS position 1"
        );
    }

    /// Without either SOFTPROOFING or GAMUTCHECK there is nothing to proof, so C degrades to a
    /// plain `cmsCreateTransform` (`cmsxform.c:1388-1389`): no gamut-check pipeline, and the
    /// result is identical to the two-profile transform.
    #[test]
    fn proofing_transform_degrades_without_proofing_flags() {
        let ctx = Context::new();
        let mut a = load("crayons.icc");
        let mut b = load("ibm-t61.icc");
        let mut proof = load("test1.icc");
        let xf = create_proofing_transform(
            &ctx,
            &mut a,
            rgb16(),
            &mut b,
            rgb16(),
            &mut proof,
            INTENT_RELATIVE_COLORIMETRIC,
            INTENT_RELATIVE_COLORIMETRIC,
            0,
        )
        .expect("degraded proofing transform");
        assert!(xf.gamut_check_pipeline().is_none(), "no proofing flags -> no gamut check");

        let mut a2 = load("crayons.icc");
        let mut b2 = load("ibm-t61.icc");
        let direct = create_transform(
            &ctx,
            &mut a2,
            rgb16(),
            &mut b2,
            rgb16(),
            INTENT_RELATIVE_COLORIMETRIC,
            0,
        )
        .expect("direct transform");

        let mut src = [0u8; 6];
        for (i, &v) in [0u16, 0xFFFF, 0].iter().enumerate() {
            src[i * 2..i * 2 + 2].copy_from_slice(&v.to_ne_bytes());
        }
        let (mut degraded, mut plain) = ([0u8; 6], [0u8; 6]);
        xf.do_transform(&src, &mut degraded, 1);
        direct.do_transform(&src, &mut plain, 1);
        assert_eq!(read3(&degraded), read3(&plain), "must be exactly cmsCreateTransform");
    }
}
