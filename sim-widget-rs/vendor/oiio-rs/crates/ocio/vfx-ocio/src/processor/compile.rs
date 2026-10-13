//! Compilation: `Transform` AST → `ProcessorOp` ops list.
//!
//! Extracted from `processor.rs` during STRUCT.2.next. The methods here
//! mutate `self.ops` (push compiled ops) and never read pixel data — that
//! keeps them naturally decoupled from `processor/apply.rs` and
//! `processor/accessors.rs`. The `from_transform_*` constructors in the
//! parent `processor.rs` are the only public entry points; everything
//! here is private (`fn`, not `pub fn`).
//!
//! Includes:
//! - `compile_transform` (~1000 LOC) — the master AST→ops walker, with
//!   one match arm per `Transform` variant
//! - `compile_lut1d`, `compile_lut3d`, `compile_clf` — file-based LUTs
//! - `compile_cdl_correction`, `compile_spi_matrix`, `compile_4x4_matrix`
//!   — small structured-format compilers

use super::*;

impl super::Processor {
    /// Compiles a transform into operations.
    pub(super) fn compile_transform(
        &mut self,
        transform: &Transform,
        direction: TransformDirection,
    ) -> OcioResult<()> {
        match transform {
            Transform::Matrix(m) => {
                let dir = if direction == TransformDirection::Inverse {
                    m.direction.inverse()
                } else {
                    m.direction
                };

                let (matrix, offset) = if dir == TransformDirection::Inverse {
                    let (inv, inv_offset) = super::op::invert_matrix_offset(&m.matrix, &m.offset)
                        .ok_or_else(|| {
                        OcioError::Transform("Singular Matrix can't be inverted.".into())
                    })?;
                    (inv.map(|v| v as f32), inv_offset.map(|v| v as f32))
                } else {
                    (m.matrix.map(|v| v as f32), m.offset.map(|v| v as f32))
                };

                self.ops.push(ProcessorOp::Matrix { matrix, offset });
            }

            Transform::Cdl(cdl) => {
                let dir = if direction == TransformDirection::Inverse {
                    cdl.direction.inverse()
                } else {
                    cdl.direction
                };

                let (slope, power) = (cdl.slope.map(|v| v as f32), cdl.power.map(|v| v as f32));
                // OCIO `CDLOpData::validate`: slope >= 0, power > 0, saturation >= 0.
                vfx_lut::cdl::validate_cdl_params(slope, power, cdl.saturation as f32)?;
                self.ops.push(ProcessorOp::cdl(
                    slope,
                    cdl.offset.map(|v| v as f32),
                    power,
                    cdl.saturation as f32,
                    cdl.style,
                    dir == TransformDirection::Forward,
                ));
            }

            Transform::Exponent(exp) => {
                let dir = if direction == TransformDirection::Inverse {
                    exp.direction.inverse()
                } else {
                    exp.direction
                };

                let value = if dir == TransformDirection::Inverse {
                    [
                        1.0 / exp.value[0] as f32,
                        1.0 / exp.value[1] as f32,
                        1.0 / exp.value[2] as f32,
                        1.0 / exp.value[3] as f32,
                    ]
                } else {
                    exp.value.map(|v| v as f32)
                };

                self.ops.push(ProcessorOp::Exponent {
                    value,
                    negative_style: exp.negative_style,
                });
            }

            Transform::Log(log) => {
                let dir = if direction == TransformDirection::Inverse {
                    log.direction.inverse()
                } else {
                    log.direction
                };

                self.ops.push(ProcessorOp::Log {
                    base: log.base as f32,
                    forward: dir == TransformDirection::Forward,
                });
            }

            Transform::Range(r) => {
                let dir = if direction == TransformDirection::Inverse {
                    r.direction.inverse()
                } else {
                    r.direction
                };

                self.ops.push(range_op(
                    &vfx_lut::clf::RangeParams {
                        min_in: r.min_in,
                        max_in: r.max_in,
                        min_out: r.min_out,
                        max_out: r.max_out,
                        clamp: r.style != RangeStyle::NoClamp,
                    },
                    dir == TransformDirection::Forward,
                )?);
            }

            Transform::Group(g) => {
                let dir = if direction == TransformDirection::Inverse {
                    g.direction.inverse()
                } else {
                    g.direction
                };

                if dir == TransformDirection::Forward {
                    for t in &g.transforms {
                        self.compile_transform(t, TransformDirection::Forward)?;
                    }
                } else {
                    for t in g.transforms.iter().rev() {
                        self.compile_transform(t, TransformDirection::Inverse)?;
                    }
                }
            }

            Transform::FileTransform(ft) => {
                let dir = if direction == TransformDirection::Inverse {
                    ft.direction.inverse()
                } else {
                    ft.direction
                };
                let forward = dir == TransformDirection::Forward;

                // Resolve $VAR references in path using context
                let resolved_path = if let Some(ref ctx) = self.context {
                    std::path::PathBuf::from(ctx.resolve(ft.src.to_string_lossy().as_ref()))
                } else {
                    ft.src.clone()
                };
                // Record the file for `Processor::metadata()` BEFORE the io-proxy branch below
                // may redirect `path` at a temporary file. The reference records the resolved
                // path too (`FileTransform.cpp:945` hands `CreateFileNoOp` the same `filepath`
                // it gave `GetCachedFileAndFormat`), and a caller asking which files a processor
                // depends on wants the logical one, not our scratch copy.
                self.files
                    .push(resolved_path.to_string_lossy().into_owned());

                let mut path = resolved_path.clone();
                // Deleted when dropped, after the LUT is read.
                let mut _staged: Option<tempfile::TempPath> = None;

                if !path.exists()
                    && let Some(ref proxy) = self.io_proxy
                    && let Some(data) = proxy.get_lut_data(path.to_string_lossy().as_ref())
                {
                    // The LUT readers take paths: stage the bytes in a file created atomically
                    // under a unique name (a predictable name in the shared temp dir could be
                    // pre-created or raced by another process).
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("lut");
                    let mut file = tempfile::Builder::new()
                        .prefix("vfx_ocio_lut_")
                        .suffix(&format!(".{ext}"))
                        .tempfile()?;
                    std::io::Write::write_all(&mut file, &data)?;
                    let staged = file.into_temp_path();
                    path = staged.to_path_buf();
                    _staged = Some(staged);
                }

                // Load LUT based on file extension
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                    .unwrap_or_default();

                let result = match ext.as_str() {
                    "cube" => {
                        // Iridas (1D or 3D) and Resolve (a 1D shaper and a 3D LUT in one file)
                        // `.cube`, as `FileFormatResolveCube.cpp` buildFileOps: the shaper then
                        // the 3D LUT; the inverse runs the inverse 3D LUT, then the inverse
                        // shaper. Each LUT carries its own domain (OCIO's MinMax op).
                        let f = vfx_lut::cube::read_cube(&path)?;
                        let hue = crate::transform::Lut1DHueAdjust::None;
                        if forward {
                            if let Some(s) = &f.lut1d {
                                self.compile_lut1d(s, forward, hue)?;
                            }
                            if let Some(l) = &f.lut3d {
                                self.compile_lut3d(l, ft.interpolation, forward);
                            }
                        } else {
                            if let Some(l) = &f.lut3d {
                                self.compile_lut3d(l, ft.interpolation, forward);
                            }
                            if let Some(s) = &f.lut1d {
                                self.compile_lut1d(s, forward, hue)?;
                            }
                        }
                        Ok(())
                    }
                    "spi1d" => {
                        let lut = vfx_lut::read_spi1d(&path)?;
                        self.compile_lut1d(&lut, forward, crate::transform::Lut1DHueAdjust::None)?;
                        Ok(())
                    }
                    "spi3d" => {
                        let lut = vfx_lut::read_spi3d(&path)?;
                        self.compile_lut3d(&lut, ft.interpolation, forward);
                        Ok(())
                    }
                    "clf" => {
                        let pl = vfx_lut::read_clf(&path)?;
                        self.compile_clf(&pl, forward)?;
                        Ok(())
                    }
                    "ctf" => {
                        let pl = vfx_lut::read_ctf(&path)?;
                        self.compile_clf(&pl, forward)?;
                        Ok(())
                    }
                    "3dl" => {
                        // `FileFormat3DL.cpp` buildFileOps: shaper (1D) then 3D LUT; the inverse
                        // runs the inverse 3D LUT, then the inverse shaper.
                        let f = vfx_lut::threedl::read_3dl_full(path)?;
                        let hue = crate::transform::Lut1DHueAdjust::None;
                        if forward {
                            if let Some(s) = &f.shaper {
                                self.compile_lut1d(s, forward, hue)?;
                            }
                            if let Some(l) = &f.lut3d {
                                self.compile_lut3d(l, ft.interpolation, forward);
                            }
                        } else {
                            if let Some(l) = &f.lut3d {
                                self.compile_lut3d(l, ft.interpolation, forward);
                            }
                            if let Some(s) = &f.shaper {
                                self.compile_lut1d(s, forward, hue)?;
                            }
                        }
                        Ok(())
                    }
                    "cc" => {
                        let cc = vfx_lut::read_cc(&path)?;
                        self.compile_cdl_correction(&cc, ft.cdl_style, forward)?;
                        Ok(())
                    }
                    // `.ccc` and `.cdl` pick one correction with `ccc_id` (`select_cdl`).
                    "ccc" => {
                        let ccc = vfx_lut::read_ccc(&path)?;
                        let cc = select_cdl(&ccc.corrections, ft.ccc_id.as_deref())?;
                        self.compile_cdl_correction(cc, ft.cdl_style, forward)?;
                        Ok(())
                    }
                    "cdl" => {
                        let cdl = vfx_lut::read_cdl(&path)?;
                        let corrections = cdl.to_collection().corrections;
                        let cc = select_cdl(&corrections, ft.ccc_id.as_deref())?;
                        self.compile_cdl_correction(cc, ft.cdl_style, forward)?;
                        Ok(())
                    }
                    "csp" => {
                        let csp = vfx_lut::read_csp(&path)?;
                        // Apply prelut as 1D if non-identity
                        if !csp.prelut.is_identity()
                            && let Some(lut1d) = csp.prelut.to_lut1d()
                        {
                            self.compile_lut1d(
                                &lut1d,
                                forward,
                                crate::transform::Lut1DHueAdjust::None,
                            )?;
                        }
                        // Apply 1D LUT if present
                        if let Some(ref lut) = csp.lut1d {
                            self.compile_lut1d(
                                lut,
                                forward,
                                crate::transform::Lut1DHueAdjust::None,
                            )?;
                        }
                        // Apply 3D LUT if present
                        if let Some(ref lut) = csp.lut3d {
                            self.compile_lut3d(lut, ft.interpolation, forward);
                        }
                        Ok(())
                    }
                    "1dl" => {
                        // Discreet 1DL format
                        let lut = vfx_lut::read_1dl(path)?;
                        self.compile_lut1d(&lut, forward, crate::transform::Lut1DHueAdjust::None)?;
                        Ok(())
                    }
                    "hdl" => {
                        let hdl = vfx_lut::read_hdl(path)?;
                        // Apply 1D shaper if present
                        if let Some(ref lut) = hdl.lut1d {
                            self.compile_lut1d(
                                lut,
                                forward,
                                crate::transform::Lut1DHueAdjust::None,
                            )?;
                        }
                        // Apply 3D LUT if present
                        if let Some(ref lut) = hdl.lut3d {
                            self.compile_lut3d(lut, ft.interpolation, forward);
                        }
                        Ok(())
                    }
                    "itx" => {
                        // Iridas ITX 3D LUT
                        let lut = vfx_lut::read_itx(path)?;
                        self.compile_lut3d(&lut, ft.interpolation, forward);
                        Ok(())
                    }
                    "look" => {
                        // Iridas Look 3D LUT
                        let lut = vfx_lut::read_look(path)?;
                        self.compile_lut3d(&lut, ft.interpolation, forward);
                        Ok(())
                    }
                    "mga" | "m3d" => {
                        // Pandora 3D LUT
                        let lut = vfx_lut::read_mga(path)?;
                        self.compile_lut3d(&lut, ft.interpolation, forward);
                        Ok(())
                    }
                    "icc" | "icm" | "pf" => {
                        let icc = vfx_lut::read_icc(&path)?;
                        self.compile_icc(&icc, forward)?;
                        Ok(())
                    }
                    "spimtx" => {
                        // SPI matrix format
                        let mtx = vfx_lut::read_spimtx(path)?;
                        self.compile_spi_matrix(&mtx, forward);
                        Ok(())
                    }
                    "cub" => {
                        // Truelight format
                        let tl = vfx_lut::read_cub(path)?;
                        // Apply 1D shaper if present
                        if let Some(ref shaper) = tl.shaper {
                            self.compile_lut1d(
                                shaper,
                                forward,
                                crate::transform::Lut1DHueAdjust::None,
                            )?;
                        }
                        // Apply 3D cube
                        self.compile_lut3d(&tl.cube, ft.interpolation, forward);
                        Ok(())
                    }
                    "vf" => {
                        // Nuke VF (`FileFormatVF.cpp` buildFileOps): matrix then LUT forward,
                        // inverse LUT then inverse matrix.
                        let vf = vfx_lut::read_vf(path)?;
                        if forward {
                            if let Some(ref m) = vf.matrix {
                                self.compile_4x4_matrix(m, forward)?;
                            }
                            self.compile_lut3d(&vf.lut, ft.interpolation, forward);
                        } else {
                            self.compile_lut3d(&vf.lut, ft.interpolation, forward);
                            if let Some(ref m) = vf.matrix {
                                self.compile_4x4_matrix(m, forward)?;
                            }
                        }
                        Ok(())
                    }
                    _ => {
                        // Unsupported format - return error
                        Err(OcioError::InvalidTransform {
                            reason: format!("Unsupported FileTransform format: .{}", ext),
                        })
                    }
                };

                result?;
            }

            Transform::BuiltinTransfer(bt) => {
                let dir = if direction == TransformDirection::Inverse {
                    bt.direction.inverse()
                } else {
                    bt.direction
                };

                let style = match bt.style.to_lowercase().as_str() {
                    "srgb" | "srgb_texture" => TransferStyle::Srgb,
                    "rec709" | "bt709" | "rec.709" => TransferStyle::Rec709,
                    "rec1886" | "bt1886" | "rec.1886" => TransferStyle::Rec1886,
                    "rec2020" | "bt2020" | "rec.2020" => TransferStyle::Rec2020,
                    "gamma22" | "gamma_2.2" => TransferStyle::Gamma22,
                    "gamma24" | "gamma_2.4" => TransferStyle::Gamma24,
                    "gamma26" | "gamma_2.6" | "dci" => TransferStyle::Gamma26,
                    "linear" => TransferStyle::Linear,
                    "pq" | "st2084" | "smpte2084" => TransferStyle::Pq,
                    "hlg" | "arib_std_b67" => TransferStyle::Hlg,
                    "acescct" => TransferStyle::AcesCct,
                    "acescc" => TransferStyle::AcesCc,
                    "log3g10" | "redlog3g10" => TransferStyle::Log3G10,
                    "logc" | "logc3" | "arri_logc3" => TransferStyle::LogC3,
                    "logc4" | "arri_logc4" => TransferStyle::LogC4,
                    "slog3" | "sony_slog3" => TransferStyle::SLog3,
                    "vlog" | "panasonic_vlog" => TransferStyle::VLog,
                    "bmdfilmgen5" | "blackmagic" => TransferStyle::BmdFilmGen5,
                    // These three were MISSING from this table while being fully implemented
                    // (`TransferStyle::AppleLog`/`CanonCLog2`/`CanonCLog3`, emitted by
                    // `gpu/transfer.rs:152-154`). Asking for them by name therefore fell into the
                    // catch-all below and silently produced a LINEAR identity instead of the
                    // camera log curve — a wrong picture with no error. Names follow the
                    // reference's style strings: OCIO spells them `APPLE_LOG`, `CANON_CLOG2` and
                    // `CANON_CLOG3` (e.g. "CURVE - CANON_CLOG2_to_LINEAR", Config.cpp:5615-5632).
                    "applelog" | "apple_log" => TransferStyle::AppleLog,
                    "clog2" | "canon_clog2" | "canonclog2" => TransferStyle::CanonCLog2,
                    "clog3" | "canon_clog3" | "canonclog3" => TransferStyle::CanonCLog3,
                    // No silent fallback. This used to be `_ => TransferStyle::Linear`, which
                    // turned every unrecognised or misspelled style into an identity transfer:
                    // the pixels come back subtly wrong and nothing reports why. An unknown style
                    // is a caller error, so it is now loud.
                    other => {
                        return Err(OcioError::InvalidTransform {
                            reason: format!(
                                "unknown BuiltinTransfer style {other:?} — it would previously \
                                 have been silently treated as linear"
                            ),
                        });
                    }
                };

                self.ops.push(ProcessorOp::Transfer {
                    style,
                    forward: dir == TransformDirection::Forward,
                });
            }

            Transform::ExposureContrast(ec) => {
                let dir = if direction == TransformDirection::Inverse {
                    ec.direction.inverse()
                } else {
                    ec.direction
                };

                // Forward parameters in both directions: the op's renderer and shader pick
                // OCIO's reverse form from `forward` (`ExposureContrastOpData::invert` only
                // flips the style too), so a dynamic value never needs re-inverting.
                let (exposure, contrast, gamma) =
                    (ec.exposure as f32, ec.contrast as f32, ec.gamma as f32);
                self.ops.push(ProcessorOp::ExposureContrast {
                    exposure,
                    contrast,
                    gamma,
                    pivot: ec.pivot as f32,
                    log_exposure_step: ec.log_exposure_step as f32,
                    log_mid_gray: ec.log_mid_gray as f32,
                    style: ec.style,
                    dynamic: ec.dynamic,
                    forward: dir == TransformDirection::Forward,
                });
            }

            Transform::FixedFunction(ff) => {
                let dir = if direction == TransformDirection::Inverse {
                    ff.direction.inverse()
                } else {
                    ff.direction
                };

                let fwd = dir == TransformDirection::Forward;
                let fp: Vec<f32> = ff.params.iter().map(|&v| v as f32).collect();

                if let Some(aces2_op) = fixed_function_to_aces2_op(ff.style, &fp, fwd)? {
                    self.ops.push(aces2_op);
                } else {
                    let (style, forward) =
                        super::op::resolve_fixed_function_direction(ff.style, fwd);
                    self.ops.push(ProcessorOp::FixedFunction {
                        style,
                        params: fp,
                        forward,
                    });
                }
            }

            Transform::Allocation(alloc) => {
                if alloc.allocation == crate::transform::AllocationType::Unknown {
                    return Err(OcioError::InvalidTransform {
                        reason: "Unsupported Allocation Type.".into(),
                    });
                }
                let dir = if direction == TransformDirection::Inverse {
                    alloc.direction.inverse()
                } else {
                    alloc.direction
                };

                self.ops.push(ProcessorOp::Allocation {
                    allocation: alloc.allocation,
                    vars: alloc.vars.iter().map(|&v| v as f32).collect(),
                    forward: dir == TransformDirection::Forward,
                });
            }

            Transform::GradingPrimary(gp) => {
                let dir = if direction == TransformDirection::Inverse {
                    gp.direction.inverse()
                } else {
                    gp.direction
                };
                let forward = dir == TransformDirection::Forward;

                // Single conversion point: `op::grading_primary_params` ports
                // OCIO's `GradingPrimaryPreRender::update` exactly (master
                // folded into each channel, direction-resolved
                // inversion/reciprocals, style-specific pivot remap) — CPU
                // apply and all four GPU emitters consume the result
                // directly, so this one call fixes all five places at once.
                let p = super::op::grading_primary_params(gp.style, forward, &gp.value);

                self.ops.push(ProcessorOp::GradingPrimary {
                    style: gp.style,
                    brightness: p.brightness,
                    contrast: p.contrast,
                    gamma: p.gamma,
                    exposure: p.exposure,
                    offset: p.offset,
                    slope: p.slope,
                    saturation: gp.value.saturation as f32,
                    pivot: p.pivot,
                    pivot_black: gp.value.pivot_black as f32,
                    pivot_white: gp.value.pivot_white as f32,
                    clamp_black: gp.value.clamp_black as f32,
                    clamp_white: gp.value.clamp_white as f32,
                    forward,
                    raw: gp.value.clone(),
                });
            }

            Transform::GradingRgbCurve(gc) => {
                // Combine transform direction with outer direction.
                let dir = if direction == TransformDirection::Inverse {
                    gc.direction.inverse()
                } else {
                    gc.direction
                };

                let forward = dir == TransformDirection::Forward;

                // `GradingBSplineCurveImpl::validate` (GradingBSplineCurve.cpp:659-663) throws
                // for fewer than two points. The fitter below skips such a curve as identity,
                // so accepting one would turn e.g. a one-point constant into a pass-through.
                for (name, pts) in [
                    ("red", &gc.red),
                    ("green", &gc.green),
                    ("blue", &gc.blue),
                    ("master", &gc.master),
                ] {
                    if pts.len() < 2 {
                        return Err(OcioError::InvalidTransform {
                            reason: format!(
                                "GradingRGBCurve {name} curve: there must be at least 2 control \
                                 points, got {}",
                                pts.len()
                            ),
                        });
                    }
                }

                let to_points = |pts: &[[f64; 2]]| -> Vec<GradingControlPoint> {
                    pts.iter()
                        .map(|p| GradingControlPoint {
                            x: p[0] as f32,
                            y: p[1] as f32,
                        })
                        .collect()
                };
                let to_slopes =
                    |vals: &[f64]| -> Vec<f32> { vals.iter().map(|v| *v as f32).collect() };

                let curve_points = [
                    to_points(&gc.red),
                    to_points(&gc.green),
                    to_points(&gc.blue),
                    to_points(&gc.master),
                ];
                let curve_slopes = [
                    to_slopes(&gc.red_slopes),
                    to_slopes(&gc.green_slopes),
                    to_slopes(&gc.blue_slopes),
                    to_slopes(&gc.master_slopes),
                ];

                let curve_data = build_rgb_uniforms(
                    [
                        &curve_points[0],
                        &curve_points[1],
                        &curve_points[2],
                        &curve_points[3],
                    ],
                    [
                        &curve_slopes[0],
                        &curve_slopes[1],
                        &curve_slopes[2],
                        &curve_slopes[3],
                    ],
                );

                // No bake: OCIO evaluates the fitted spline itself, unbounded, in both
                // directions (`GradingRGBCurveOpCPU.cpp:30-53`). This used to sample the curves
                // onto a 1024-entry LUT over [0, 1] and clamp there, which is a different
                // function for every input outside [0, 1] — i.e. for the LOG style's whole
                // working range (ACES 1.x tone curves run over log10 values in [-5, 5]).
                self.ops.push(ProcessorOp::GradingRgbCurve {
                    style: gc.style,
                    raw: Box::new(gc.clone()),
                    curve_data,
                    bypass_lin_to_log: gc.bypass_lin_to_log,
                    forward,
                });
            }

            Transform::GradingTone(gt) => {
                let dir = if direction == TransformDirection::Inverse {
                    gt.direction.inverse()
                } else {
                    gt.direction
                };
                let forward = dir == TransformDirection::Forward;

                // Resolve the GradingTone value and precompute its curve tables once.
                let tone = crate::grading_tone::GradingTone {
                    blacks: gt.blacks,
                    shadows: gt.shadows,
                    midtones: gt.midtones,
                    highlights: gt.highlights,
                    whites: gt.whites,
                    s_contrast: gt.scontrast,
                };
                tone.validate()?;
                let mut vpr = crate::grading_tone::GradingTonePreRender::new(gt.style);
                vpr.update(&tone);

                self.ops.push(ProcessorOp::GradingTone {
                    style: gt.style,
                    value: tone,
                    pre_render: Box::new(vpr),
                    forward,
                });
            }

            Transform::GradingHueCurve(ghc) => {
                let dir = if direction == TransformDirection::Inverse {
                    ghc.direction.inverse()
                } else {
                    ghc.direction
                };

                // Convert to vfx-ops types
                use crate::grading_hue_curve::{
                    GradingHueCurves, GradingStyle, HueControlPoint, HueCurve,
                };

                let to_curve = |pts: &[[f64; 2]]| {
                    HueCurve::new(
                        pts.iter()
                            .map(|p| HueControlPoint::new(p[0] as f32, p[1] as f32))
                            .collect(),
                    )
                };

                let style = match ghc.style {
                    crate::GradingHueCurveStyle::Log => GradingStyle::Log,
                    crate::GradingHueCurveStyle::Linear => GradingStyle::Linear,
                    crate::GradingHueCurveStyle::Video => GradingStyle::Video,
                };

                let to_points = |pts: &[[f64; 2]]| -> Vec<GradingControlPoint> {
                    pts.iter()
                        .map(|p| GradingControlPoint {
                            x: p[0] as f32,
                            y: p[1] as f32,
                        })
                        .collect()
                };
                let to_slopes =
                    |vals: &[f64]| -> Vec<f32> { vals.iter().map(|v| *v as f32).collect() };

                let curve_points = [
                    to_points(&ghc.hue_hue),
                    to_points(&ghc.hue_sat),
                    to_points(&ghc.hue_lum),
                    to_points(&ghc.lum_sat),
                    to_points(&ghc.sat_sat),
                    to_points(&ghc.lum_lum),
                    to_points(&ghc.sat_lum),
                    to_points(&ghc.hue_fx),
                ];
                let curve_slopes = [
                    to_slopes(&ghc.hue_hue_slopes),
                    to_slopes(&ghc.hue_sat_slopes),
                    to_slopes(&ghc.hue_lum_slopes),
                    to_slopes(&ghc.lum_sat_slopes),
                    to_slopes(&ghc.sat_sat_slopes),
                    to_slopes(&ghc.lum_lum_slopes),
                    to_slopes(&ghc.sat_lum_slopes),
                    to_slopes(&ghc.hue_fx_slopes),
                ];

                let curve_data = build_hue_uniforms(
                    [
                        &curve_points[0],
                        &curve_points[1],
                        &curve_points[2],
                        &curve_points[3],
                        &curve_points[4],
                        &curve_points[5],
                        &curve_points[6],
                        &curve_points[7],
                    ],
                    [
                        &curve_slopes[0],
                        &curve_slopes[1],
                        &curve_slopes[2],
                        &curve_slopes[3],
                        &curve_slopes[4],
                        &curve_slopes[5],
                        &curve_slopes[6],
                        &curve_slopes[7],
                    ],
                    ghc.draw_curve_only,
                );

                let curves = GradingHueCurves {
                    style,
                    hue_hue: to_curve(&ghc.hue_hue),
                    hue_sat: to_curve(&ghc.hue_sat),
                    hue_lum: to_curve(&ghc.hue_lum),
                    lum_sat: to_curve(&ghc.lum_sat),
                    sat_sat: to_curve(&ghc.sat_sat),
                    lum_lum: to_curve(&ghc.lum_lum),
                    sat_lum: to_curve(&ghc.sat_lum),
                    hue_fx: to_curve(&ghc.hue_fx),
                };

                self.ops.push(ProcessorOp::GradingHueCurve {
                    curves: Box::new(curves),
                    slopes: curve_slopes,
                    rgb_to_hsy: ghc.rgb_to_hsy,
                    curve_data,
                    draw_curve_only: ghc.draw_curve_only,
                    forward: dir == TransformDirection::Forward,
                });
            }

            Transform::LogAffine(la) => {
                let dir = if direction == TransformDirection::Inverse {
                    la.direction.inverse()
                } else {
                    la.direction
                };

                self.ops.push(ProcessorOp::LogAffine {
                    base: la.base as f32,
                    log_side_slope: la.log_side_slope.map(|v| v as f32),
                    log_side_offset: la.log_side_offset.map(|v| v as f32),
                    lin_side_slope: la.lin_side_slope.map(|v| v as f32),
                    lin_side_offset: la.lin_side_offset.map(|v| v as f32),
                    forward: dir == TransformDirection::Forward,
                });
            }

            Transform::LogCamera(lc) => {
                let dir = if direction == TransformDirection::Inverse {
                    lc.direction.inverse()
                } else {
                    lc.direction
                };

                let base = lc.base as f32;
                let log_base = base.ln();
                let log_side_slope: [f32; 3] = lc.log_side_slope.map(|v| v as f32);
                let log_side_offset: [f32; 3] = lc.log_side_offset.map(|v| v as f32);
                let lin_side_slope: [f32; 3] = lc.lin_side_slope.map(|v| v as f32);
                let lin_side_offset: [f32; 3] = lc.lin_side_offset.map(|v| v as f32);
                let lin_side_break: [f32; 3] = lc.lin_side_break.map(|v| v as f32);

                // Calculate linear slope for continuity at break point (OCIO: GetLinearSlope)
                // linear_slope = log_side_slope * lin_side_slope / (ln(base) * (lin_side_break * lin_side_slope + lin_side_offset))
                let linear_slope: [f32; 3] = lc
                    .linear_slope
                    .map(|arr| arr.map(|v| v as f32))
                    .unwrap_or_else(|| {
                        const EPSILON: f32 = 1e-10;
                        let calc_slope = |i: usize| -> f32 {
                            let denom = log_base
                                * (lin_side_break[i] * lin_side_slope[i] + lin_side_offset[i]);
                            if denom.abs() < EPSILON {
                                (log_side_slope[i] * lin_side_slope[i]).signum() * 1e6
                            } else {
                                (log_side_slope[i] * lin_side_slope[i]) / denom
                            }
                        };
                        [calc_slope(0), calc_slope(1), calc_slope(2)]
                    });

                // Precompute log_side_break and linear_offset (OCIO: computed in updateData)
                let mut log_side_break_arr = [0.0f32; 3];
                let mut linear_offset = [0.0f32; 3];
                for ch in 0..3 {
                    let break_lin = (lin_side_slope[ch] * lin_side_break[ch] + lin_side_offset[ch])
                        .max(f32::MIN_POSITIVE);
                    log_side_break_arr[ch] =
                        log_side_slope[ch] * break_lin.ln() / log_base + log_side_offset[ch];
                    linear_offset[ch] =
                        log_side_break_arr[ch] - linear_slope[ch] * lin_side_break[ch];
                }

                self.ops.push(ProcessorOp::LogCamera {
                    base,
                    log_base,
                    log_side_slope,
                    log_side_offset,
                    lin_side_slope,
                    lin_side_offset,
                    lin_side_break,
                    linear_slope,
                    log_side_break: log_side_break_arr,
                    linear_offset,
                    forward: dir == TransformDirection::Forward,
                });
            }

            Transform::ExponentWithLinear(ewl) => {
                // OCIO rejects CLAMP / PASS_THRU for a moncurve (B24).
                ewl.validate()?;
                let dir = if direction == TransformDirection::Inverse {
                    ewl.direction.inverse()
                } else {
                    ewl.direction
                };

                self.ops.push(ProcessorOp::ExponentWithLinear {
                    gamma: ewl.gamma.map(|v| v as f32),
                    offset: ewl.offset.map(|v| v as f32),
                    mirror: ewl.negative_style == NegativeStyle::Mirror,
                    forward: dir == TransformDirection::Forward,
                });
            }

            Transform::Lut1D(lut) => {
                let dir = if direction == TransformDirection::Inverse {
                    lut.direction.inverse()
                } else {
                    lut.direction
                };
                let forward = dir == TransformDirection::Forward;

                // Convert inline LUT to vfx_lut format (replicate scalar domain to per-channel)
                let vfx_lut = vfx_lut::Lut1D {
                    r: lut.red.clone(),
                    g: lut.green.clone(),
                    b: lut.blue.clone(),
                    domain_min: [lut.input_min; 3],
                    domain_max: [lut.input_max; 3],
                };
                self.compile_lut1d_domain(&vfx_lut, forward, lut.hue_adjust, lut.half_domain)?;
            }

            Transform::Lut3D(lut) => {
                let dir = if direction == TransformDirection::Inverse {
                    lut.direction.inverse()
                } else {
                    lut.direction
                };
                let forward = dir == TransformDirection::Forward;

                // Convert inline LUT to vfx_lut format. `Lut3DTransform::data`
                // is Red-fastest (its own documented contract — the `.cube`
                // file order); `vfx_lut::Lut3D::data` is Blue-fastest (OCIO's
                // internal `Lut3DOpData` order) — reorder to honor that
                // contract, rather than mislabeling Red-fastest data as
                // Blue-fastest by copying it straight across. See
                // `compile_lut3d`'s doc comment / `docs/ROADMAP.md` P0 #1.
                // Tagged explicitly (P1 hardening, `OrderedLut3dData`) rather
                // than calling `swap_red_blue_fastest_order` directly, so the
                // source order this call site asserts is visible at the call
                // site itself, not just in a comment.
                let vfx_lut = vfx_lut::Lut3D {
                    size: lut.size,
                    data: vfx_lut::OrderedLut3dData::red_fastest(lut.data.clone(), lut.size)
                        .into_order(vfx_lut::Lut3dOrder::BlueFastest),
                    domain_min: lut.domain_min,
                    domain_max: lut.domain_max,
                    interpolation: match lut.interpolation {
                        Interpolation::Nearest => vfx_lut::Interpolation::Nearest,
                        Interpolation::Default | Interpolation::Linear => {
                            vfx_lut::Interpolation::Linear
                        }
                        Interpolation::Tetrahedral | Interpolation::Best => {
                            vfx_lut::Interpolation::Tetrahedral
                        }
                    },
                };
                self.compile_lut3d(&vfx_lut, lut.interpolation, forward);
            }

            Transform::Builtin(bt) => {
                let dir = if direction == TransformDirection::Inverse {
                    bt.direction.inverse()
                } else {
                    bt.direction
                };
                // OCIO parity: an unknown style is a HARD ERROR with OCIO's own message
                // (`BuiltinTransformImpl::setStyle`, BuiltinTransform.cpp:63-77). Silently
                // emitting no ops would turn it into an identity pass-through.
                let ops = crate::builtin_transforms::get_builtin(&bt.style).ok_or_else(|| {
                    OcioError::InvalidTransform {
                        reason: crate::builtin_transforms::invalid_style_message(&bt.style),
                    }
                })?;
                // `CreateBuiltinTransformOps` (BuiltinTransformRegistry.cpp:125-152): the
                // registered ops, or their inversion. They are ordinary transforms, so they
                // compile through exactly the arms above, as a group.
                let group = Transform::Group(GroupTransform {
                    name: String::new(),
                    transforms: ops,
                    direction: TransformDirection::Forward,
                });
                self.compile_transform(&group, dir)?;
            }

            _ => {
                // Other transforms (ColorSpace, Look, DisplayView) handled at config level
            }
        }

        Ok(())
    }

    /// Compiles a 1D LUT (uniform domain) into ProcessorOp::Lut1d.
    fn compile_lut1d(
        &mut self,
        lut: &vfx_lut::Lut1D,
        forward: bool,
        hue_adjust: crate::transform::Lut1DHueAdjust,
    ) -> OcioResult<()> {
        self.compile_lut1d_domain(lut, forward, hue_adjust, false)
    }

    /// Compiles a 1D LUT into ProcessorOp::Lut1d; `half_domain` selects OCIO's
    /// `LUT_INPUT_HALF_CODE` indexing (one entry per 16-bit half code, 65536 entries).
    ///
    /// The flag used to be parsed (`halfDomain` in CLF/CTF) and then dropped here, so a
    /// half-domain LUT was indexed as if its 65536 entries spanned `[input_min, input_max]`.
    pub(super) fn compile_lut1d_domain(
        &mut self,
        lut: &vfx_lut::Lut1D,
        forward: bool,
        hue_adjust: crate::transform::Lut1DHueAdjust,
        half_domain: bool,
    ) -> OcioResult<()> {
        let size = lut.r.len();
        let channels = if lut.g.is_some() { 3 } else { 1 };

        // Lut1DOpData::validate (`Lut1DOpData.cpp:395`): OCIO defines the WYPN hue adjust but
        // does not implement it and refuses it; treating it as no adjustment would be a silent
        // substitution.
        if hue_adjust == crate::transform::Lut1DHueAdjust::Wypn {
            return Err(OcioError::InvalidTransform {
                reason: "1D LUT HUE_WYPN hue adjust style is not implemented.".into(),
            });
        }

        if half_domain {
            // Lut1DOpData::validate: a half-domain LUT must have one entry per half code.
            if size != 65536 {
                return Err(OcioError::InvalidTransform {
                    reason: format!(
                        "half-domain Lut1D must have 65536 entries (one per half code), got {size}"
                    ),
                });
            }
        }

        // Flatten LUT data
        let mut data = Vec::with_capacity(size * channels);
        if channels == 3 {
            let g = lut.g.as_ref().unwrap();
            let b = lut.b.as_ref().unwrap();
            for i in 0..size {
                data.push(lut.r[i]);
                data.push(g[i]);
                data.push(b[i]);
            }
        } else {
            data.extend_from_slice(&lut.r);
        }

        // Inverting swaps domain and range: the inverse is sampled over the forward
        // LUT's output range, and its samples live in the forward LUT's input domain.
        // Carrying the forward domain over to the inverse (as this did) offsets every
        // lookup whenever the two differ.
        let pair = if half_domain {
            LutPair::lut1d_half(&data, channels)
        } else {
            LutPair::lut1d(&data, size, channels, lut.domain_min, lut.domain_max)
        };
        let pair = Some(LutPair {
            inverse: !forward,
            ..pair
        });
        // The inverse keeps the forward table and is evaluated exactly per pixel (OCIO
        // `InvLut1DRenderer`, `InvLut1DRendererHalfCode` for a half-domain table); OCIO inverts
        // the hue-adjusted form separately (`InvLut1DRendererHueAdjust`), which is not ported.
        let inverse = if forward {
            None
        } else {
            if hue_adjust != crate::transform::Lut1DHueAdjust::None {
                return Err(OcioError::InvalidTransform {
                    reason: "inverse of a hue-adjusted Lut1D is not implemented \
                             (OCIO InvLut1DRendererHueAdjust)"
                        .into(),
                });
            }
            Some(std::sync::Arc::new(if half_domain {
                super::inv_lut::InvLut1d::new_half(&data, channels)
            } else {
                super::inv_lut::InvLut1d::new(&data, size, channels, lut.domain_min, lut.domain_max)
            }))
        };

        self.ops.push(ProcessorOp::Lut1d {
            lut: data,
            size,
            channels,
            domain_min: lut.domain_min,
            domain_max: lut.domain_max,
            half_domain,
            hue_adjust,
            pair,
            inverse,
        });
        Ok(())
    }

    /// Compiles a 3D LUT into ProcessorOp::Lut3d.
    fn compile_lut3d(&mut self, lut: &vfx_lut::Lut3D, interp: Interpolation, forward: bool) {
        // `vfx_lut::Lut3D::data` is Blue-fastest (its own documented contract,
        // matching OCIO's internal `Lut3DOpData`), but `ProcessorOp::Lut3d`'s
        // `lut` buffer is Red-fastest — the CPU apply (`apply.rs`'s
        // `idx(r,g,b) = b*size²+g*size+r`), `invert_lut3d`/
        // `eval_lut3d_tetrahedral` (`math.rs`, same formula) and the GPU
        // texture upload (`gpu.rs`, packed straight into a `width=height=
        // depth=size` texture with no swap, matching wgpu's native X-fastest
        // layout) all agree on Red-fastest. Reorder ONCE here — the single
        // conversion point for the R/B axis mismatch documented in
        // `docs/ROADMAP.md` P0 #1 — rather than at each of those readers.
        // Tagged (P1 hardening, `OrderedLut3dData`): this call site asserts
        // `lut.data` is Blue-fastest and asks for Red-fastest out.
        let reordered = vfx_lut::OrderedLut3dData::blue_fastest(lut.data.clone(), lut.size)
            .into_order(vfx_lut::Lut3dOrder::RedFastest);
        let flat_data: Vec<f32> = reordered
            .iter()
            .flat_map(|rgb| rgb.iter().copied())
            .collect();

        let pair =
            LutPair::lut3d(&flat_data, lut.size, lut.domain_min, lut.domain_max).map(|p| LutPair {
                inverse: !forward,
                ..p
            });
        // The inverse keeps the forward table and is evaluated exactly per pixel (OCIO
        // `InvLut3DRenderer`, which works on the Blue-fastest forward data).
        let inverse = (!forward).then(|| {
            std::sync::Arc::new(super::inv_lut::InvLut3d::new(
                &lut.data,
                lut.size,
                lut.domain_min,
                lut.domain_max,
            ))
        });
        let lut_data = flat_data;

        self.ops.push(ProcessorOp::Lut3d {
            lut: lut_data,
            size: lut.size,
            interp,
            domain_min: lut.domain_min,
            domain_max: lut.domain_max,
            pair,
            inverse,
        });
    }

    /// Compiles a CLF/CTF ProcessList into ops, as OCIO's `FileFormatCTF` builds them: each node
    /// becomes the transform OCIO's op data is (Matrix, CDL, Exponent / ExponentWithLinear, Log /
    /// LogAffine / LogCamera, Range, LUTs) and is compiled like any other transform, so the
    /// inverse file is the reversed node list with every node inverted.
    fn compile_clf(&mut self, pl: &vfx_lut::ProcessList, forward: bool) -> OcioResult<()> {
        use crate::transform::{
            CdlTransform, ExponentTransform, ExponentWithLinearTransform, LogAffineTransform,
            LogCameraTransform, LogTransform, MatrixTransform,
        };
        use vfx_lut::clf::{CdlStyle as ClfCdl, ExponentStyle as E, LogStyle as L};
        let dir = |node_forward: bool| {
            if node_forward == forward {
                TransformDirection::Forward
            } else {
                TransformDirection::Inverse
            }
        };
        let nodes: Vec<_> = if forward {
            pl.nodes.iter().collect()
        } else {
            pl.nodes.iter().rev().collect()
        };

        for node in nodes {
            let t = match node {
                vfx_lut::ProcessNode::Lut1D { lut, .. } => {
                    self.compile_lut1d(lut, forward, crate::transform::Lut1DHueAdjust::None)?;
                    continue;
                }
                vfx_lut::ProcessNode::Lut3D { lut, .. } => {
                    let interp = match lut.interpolation {
                        vfx_lut::Interpolation::Nearest => Interpolation::Nearest,
                        vfx_lut::Interpolation::Linear => Interpolation::Linear,
                        vfx_lut::Interpolation::Tetrahedral => Interpolation::Tetrahedral,
                    };
                    self.compile_lut3d(lut, interp, forward);
                    continue;
                }
                vfx_lut::ProcessNode::Range(rp) => {
                    self.ops.push(range_op(rp, forward)?);
                    continue;
                }
                // Row-major 3x3, or 3x4 with the offset in the fourth column (already scaled by
                // the node's bit depths in the reader).
                vfx_lut::ProcessNode::Matrix { values, .. } => {
                    // 3x3, 3x4, 4x4 (alpha) or 4x5 (alpha and offsets); the offsets are the
                    // last column of the 3x4 and 4x5 forms.
                    let (rows, cols) = match values.len() {
                        12 => (3, 4),
                        16 => (4, 4),
                        20 => (4, 5),
                        _ => (3, 3),
                    };
                    let mut matrix = MatrixTransform::IDENTITY;
                    let mut offset = [0.0; 4];
                    for r in 0..rows {
                        for c in 0..rows {
                            matrix[r * 4 + c] = values[r * cols + c] as f64;
                        }
                        if cols > rows {
                            offset[r] = values[r * cols + rows] as f64;
                        }
                    }
                    Transform::Matrix(MatrixTransform {
                        name: String::new(),
                        matrix,
                        offset,
                        direction: dir(true),
                    })
                }
                vfx_lut::ProcessNode::Cdl(c) => Transform::Cdl(CdlTransform {
                    name: String::new(),
                    slope: c.slope,
                    offset: c.offset,
                    power: c.power,
                    saturation: c.saturation,
                    style: match c.style {
                        ClfCdl::Fwd | ClfCdl::Rev => CdlStyle::AscCdl,
                        ClfCdl::FwdNoClamp | ClfCdl::RevNoClamp => CdlStyle::NoClamp,
                    },
                    direction: dir(matches!(c.style, ClfCdl::Fwd | ClfCdl::FwdNoClamp)),
                }),
                vfx_lut::ProcessNode::Exponent(x) => {
                    let fwd = matches!(
                        x.style,
                        E::BasicFwd
                            | E::BasicMirrorFwd
                            | E::BasicPassThruFwd
                            | E::MonCurveFwd
                            | E::MonCurveMirrorFwd
                    );
                    match x.style {
                        E::MonCurveFwd
                        | E::MonCurveRev
                        | E::MonCurveMirrorFwd
                        | E::MonCurveMirrorRev => {
                            let mut t = ExponentWithLinearTransform::new(x.exponent, x.offset);
                            if matches!(x.style, E::MonCurveMirrorFwd | E::MonCurveMirrorRev) {
                                t.set_negative_style(NegativeStyle::Mirror)?;
                            }
                            t.direction = dir(fwd);
                            Transform::ExponentWithLinear(t)
                        }
                        _ => Transform::Exponent(ExponentTransform {
                            name: String::new(),
                            value: x.exponent,
                            negative_style: match x.style {
                                E::BasicMirrorFwd | E::BasicMirrorRev => NegativeStyle::Mirror,
                                E::BasicPassThruFwd | E::BasicPassThruRev => {
                                    NegativeStyle::PassThru
                                }
                                _ => NegativeStyle::Clamp,
                            },
                            direction: dir(fwd),
                        }),
                    }
                }
                vfx_lut::ProcessNode::Log(l) => match l.style {
                    L::Log10 | L::Log2 | L::AntiLog10 | L::AntiLog2 => {
                        Transform::Log(LogTransform {
                            name: String::new(),
                            base: if matches!(l.style, L::Log10 | L::AntiLog10) {
                                10.0
                            } else {
                                2.0
                            },
                            direction: dir(matches!(l.style, L::Log10 | L::Log2)),
                        })
                    }
                    L::LinToLog | L::LogToLin => Transform::LogAffine(LogAffineTransform {
                        name: String::new(),
                        base: l.base,
                        log_side_slope: l.log_side_slope,
                        log_side_offset: l.log_side_offset,
                        lin_side_slope: l.lin_side_slope,
                        lin_side_offset: l.lin_side_offset,
                        direction: dir(l.style == L::LinToLog),
                    }),
                    L::CameraLinToLog | L::CameraLogToLin => {
                        Transform::LogCamera(LogCameraTransform {
                            name: String::new(),
                            base: l.base,
                            log_side_slope: l.log_side_slope,
                            log_side_offset: l.log_side_offset,
                            lin_side_slope: l.lin_side_slope,
                            lin_side_offset: l.lin_side_offset,
                            lin_side_break: l.lin_side_break.ok_or_else(|| {
                                OcioError::InvalidTransform {
                                    reason: "CLF camera Log needs linSideBreak".into(),
                                }
                            })?,
                            linear_slope: l.linear_slope,
                            direction: dir(l.style == L::CameraLinToLog),
                        })
                    }
                },
            };
            self.compile_transform(&t, TransformDirection::Forward)?;
        }
        Ok(())
    }

    /// Compiles a CDL ColorCorrection to processor ops.
    ///
    /// `style` is the `FileTransform`'s `cdl_style`. A parsed `.cc`/`.ccc`/`.cdl` correction has
    /// OCIO's default CDL style, `CDL_NO_CLAMP`, and a `FileTransform` only overrides it when its
    /// own style is not the default (`FileFormatCC.cpp` `buildFileOps`), so the unclamped default
    /// here is OCIO's, not an omission. (It used to be hard-wired to the clamping ASC style, which
    /// clipped every scene-referred grade loaded from a file and ignored `cdl_style`.)
    fn compile_cdl_correction(
        &mut self,
        cc: &vfx_lut::ColorCorrection,
        style: CdlStyle,
        forward: bool,
    ) -> OcioResult<()> {
        vfx_lut::cdl::validate_cdl_params(cc.slope, cc.power, cc.saturation)?;
        self.ops.push(ProcessorOp::cdl(
            cc.slope,
            cc.offset,
            cc.power,
            cc.saturation,
            style,
            forward,
        ));
        Ok(())
    }

    /// Compiles a SPI matrix to processor ops.
    fn compile_spi_matrix(&mut self, mtx: &vfx_lut::SpiMatrix, forward: bool) {
        // Convert 3x3 + offset to 4x4 matrix format
        let m = &mtx.matrix;
        let o = &mtx.offset;

        if forward {
            let matrix = [
                m[0][0] as f32,
                m[0][1] as f32,
                m[0][2] as f32,
                0.0,
                m[1][0] as f32,
                m[1][1] as f32,
                m[1][2] as f32,
                0.0,
                m[2][0] as f32,
                m[2][1] as f32,
                m[2][2] as f32,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
            ];
            let offset = [o[0] as f32, o[1] as f32, o[2] as f32, 0.0];
            self.ops.push(ProcessorOp::Matrix { matrix, offset });
        } else {
            // Inverse: invert 3x3 matrix and adjust offset
            if let Some(inv) = invert_3x3(m) {
                let matrix = [
                    inv[0][0] as f32,
                    inv[0][1] as f32,
                    inv[0][2] as f32,
                    0.0,
                    inv[1][0] as f32,
                    inv[1][1] as f32,
                    inv[1][2] as f32,
                    0.0,
                    inv[2][0] as f32,
                    inv[2][1] as f32,
                    inv[2][2] as f32,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                ];
                // Inverse offset: -inv(M) * offset
                let inv_off = [
                    -(inv[0][0] * o[0] + inv[0][1] * o[1] + inv[0][2] * o[2]) as f32,
                    -(inv[1][0] * o[0] + inv[1][1] * o[1] + inv[1][2] * o[2]) as f32,
                    -(inv[2][0] * o[0] + inv[2][1] * o[1] + inv[2][2] * o[2]) as f32,
                    0.0,
                ];
                self.ops.push(ProcessorOp::Matrix {
                    matrix,
                    offset: inv_off,
                });
            }
        }
    }

    /// Compiles a 4x4 matrix (row-major f64, no offset): OCIO `CreateMatrixOp(ops, m44, dir)`,
    /// i.e. a MatrixTransform. It is applied to RGBA as is, so the fourth column multiplies
    /// alpha.
    /// An ICC Matrix/TRC profile, as OCIO `FileFormatICC::buildFileOps`. The forward direction is
    /// PCS XYZ -> device code (the display usage): the D50 -> D65 Bradford adaptation inverted,
    /// the colorant matrix inverted, then the tone curve inverted. The inverse direction is the
    /// profile's own device -> XYZ order. A gamma curve is an unclamped, mirrored exponent; a
    /// table curve is a 1D LUT.
    fn compile_icc(&mut self, icc: &vfx_lut::IccMatrixTrc, forward: bool) -> OcioResult<()> {
        const D50_TO_D65: [f64; 16] = [
            0.955509474537,
            -0.023074829492,
            0.063312392987,
            0.0,
            -0.028327238868,
            1.00994465504,
            0.021055592145,
            0.0,
            0.012329273379,
            -0.020536209966,
            1.33072998567,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ];
        let m = &icc.matrix;
        let matrix = [
            m[0], m[1], m[2], 0.0, m[3], m[4], m[5], 0.0, m[6], m[7], m[8], 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let curve = |this: &mut Self, curve_forward: bool| -> OcioResult<()> {
            match &icc.trc {
                vfx_lut::IccTrc::Lut(lut) => {
                    this.compile_lut1d(lut, curve_forward, crate::transform::Lut1DHueAdjust::None)
                }
                vfx_lut::IccTrc::Gamma(g) => this.compile_transform(
                    &Transform::Exponent(crate::transform::ExponentTransform {
                        name: String::new(),
                        value: [g[0] as f64, g[1] as f64, g[2] as f64, 1.0],
                        negative_style: crate::transform::NegativeStyle::Mirror,
                        direction: TransformDirection::Forward,
                    }),
                    if curve_forward {
                        TransformDirection::Forward
                    } else {
                        TransformDirection::Inverse
                    },
                ),
            }
        };
        if forward {
            self.compile_4x4_matrix(&D50_TO_D65, false)?;
            self.compile_4x4_matrix(&matrix, false)?;
            curve(self, false)
        } else {
            curve(self, true)?;
            self.compile_4x4_matrix(&matrix, true)?;
            self.compile_4x4_matrix(&D50_TO_D65, true)
        }
    }

    fn compile_4x4_matrix(&mut self, m: &[f64; 16], forward: bool) -> OcioResult<()> {
        let matrix = Transform::Matrix(crate::transform::MatrixTransform {
            name: String::new(),
            matrix: *m,
            offset: [0.0; 4],
            direction: TransformDirection::Forward,
        });
        self.compile_transform(
            &matrix,
            if forward {
                TransformDirection::Forward
            } else {
                TransformDirection::Inverse
            },
        )
    }
}

/// The op for a RangeTransform or a CLF `<Range>` node: OCIO `RangeOpData` (validation, scale
/// and offset in `RangeParams::affine`), inverted through `op::invert_range_params`, the same
/// function `ProcessorOp::inverse()` uses.
fn range_op(params: &vfx_lut::clf::RangeParams, forward: bool) -> OcioResult<ProcessorOp> {
    let a = params
        .affine()
        .map_err(|e| crate::OcioError::InvalidTransform {
            reason: format!("RangeTransform validation failed: {e}"),
        })?;
    let fwd = (
        a.scale as f32,
        a.offset as f32,
        a.clamp_min.map(|v| v as f32),
        a.clamp_max.map(|v| v as f32),
    );
    let (scale, offset, clamp_min, clamp_max) = if forward {
        fwd
    } else {
        super::op::invert_range_params(fwd.0, fwd.1, fwd.2, fwd.3)
    };
    Ok(ProcessorOp::Range {
        scale,
        offset,
        clamp_min,
        clamp_max,
    })
}

/// Picks the correction a `FileTransform` names in a `.ccc` or `.cdl` file, as OCIO's
/// `FileFormatCCC.cpp` / `FileFormatCDL.cpp` `buildFileOps` do: `ccc_id` is first matched against
/// the correction ids (case-sensitive), then parsed strictly as an integer index; an empty or
/// absent id is index 0. A missing id or an index out of range is an error. (This used to fall
/// back to the first correction for an unknown id, panic on an empty file with an id, and ignore
/// `ccc_id` for `.cdl` files.)
fn select_cdl<'a>(
    corrections: &'a [vfx_lut::ColorCorrection],
    ccc_id: Option<&str>,
) -> OcioResult<&'a vfx_lut::ColorCorrection> {
    let id = ccc_id.unwrap_or("");
    if let Some(cc) = corrections
        .iter()
        .find(|c| !id.is_empty() && c.id.as_deref() == Some(id))
    {
        return Ok(cc);
    }
    let index = if id.is_empty() {
        Some(0)
    } else {
        id.parse::<i64>().ok()
    };
    let Some(index) = index else {
        return Err(OcioError::InvalidTransform {
            reason: format!(
                "You must specify a valid cccid to load from the ccc file (either by name or \
                 index). id='{id}' is not found in the file, and is not parsable as an integer index."
            ),
        });
    };
    usize::try_from(index)
        .ok()
        .and_then(|i| corrections.get(i))
        .ok_or_else(|| OcioError::InvalidTransform {
            reason: format!(
                "The specified cccindex {index} is outside the valid range for this file [0,{}]",
                corrections.len() as i64 - 1
            ),
        })
}

#[cfg(test)]
mod lut3d_order_tests {
    //! Regression coverage for `docs/ROADMAP.md` P0 #1 / `BUG11.md` item 3:
    //! `vfx_lut::Lut3D::data` is Blue-fastest, but `compile_lut3d` used to
    //! flatten it straight into `ProcessorOp::Lut3d.lut` with no reorder,
    //! while the CPU apply / GPU texture upload both read it Red-fastest —
    //! silently swapping the R and B axes for every non-identity,
    //! non-separable 3D LUT. Fixed by `vfx_lut::swap_red_blue_fastest_order`,
    //! called once inside `compile_lut3d` (and again at the `Transform::Lut3D`
    //! boundary, which had the OPPOSITE mislabeling — see that test below).
    //!
    //! Both tests here use an asymmetric (R<->B swap) LUT and sample an
    //! off-diagonal grid vertex deliberately: an identity or diagonal-only
    //! LUT cannot distinguish the two axis orders (see the `roundtrip_3d`
    //! vs `roundtrip_3d_asymmetric_detects_axis_swap` split in
    //! `vfx-lut/src/cube.rs` for the same trap in that crate's own test).
    use super::*;

    fn asymmetric_lut(size: usize) -> vfx_lut::Lut3D {
        // Blue-fastest data (vfx_lut::Lut3D's own contract), mapping
        // (r,g,b) -> (b,g,r).
        let mut data = vec![[0.0f32; 3]; size * size * size];
        for r in 0..size {
            for g in 0..size {
                for b in 0..size {
                    let rf = r as f32 / (size - 1) as f32;
                    let gf = g as f32 / (size - 1) as f32;
                    let bf = b as f32 / (size - 1) as f32;
                    let idx = b + size * (g + size * r);
                    data[idx] = [bf, gf, rf]; // swap R and B
                }
            }
        }
        vfx_lut::Lut3D::from_data(data, size)
            .unwrap()
            .with_interpolation(vfx_lut::Interpolation::Nearest)
    }

    #[test]
    fn compile_lut3d_matches_vfx_lut_apply_off_diagonal() {
        let lut = asymmetric_lut(3);

        // Grid vertex r=2 (1.0), g=1 (0.5), b=0 (0.0) -> expect (0.0, 0.5, 1.0).
        let input = [1.0f32, 0.5, 0.0];
        let expected = [0.0f32, 0.5, 1.0];

        let via_vfx_lut = lut.apply(input);
        assert_eq!(
            via_vfx_lut, expected,
            "sanity: Lut3D's own apply must match"
        );

        // Same point through the actual ProcessorOp::Lut3d path
        // (FileTransform's real path: vfx_lut::cube::read_3d /
        // read_spi3d / read_3dl / etc. all produce a `vfx_lut::Lut3D`
        // exactly like this one).
        let mut processor = crate::processor::Processor::new();
        processor.compile_lut3d(&lut, Interpolation::Nearest, true);
        let mut pixels = [input];
        processor.apply_rgb(&mut pixels);

        assert_eq!(
            pixels[0], expected,
            "ProcessorOp::Lut3d disagrees with vfx_lut::Lut3D's own apply for an \
             asymmetric LUT off the r==b diagonal"
        );
    }

    #[test]
    fn inline_lut3d_transform_matches_after_compile() {
        // The OTHER half of the same bug class: `Transform::Lut3D`'s
        // `Lut3DTransform::data` is Red-fastest (its own documented
        // contract), and used to be copied straight into a
        // `vfx_lut::Lut3D` (Blue-fastest contract) with no reorder —
        // mislabeling, not a missing reorder. Build a
        // `Lut3DTransform` with the SAME (r,g,b) -> (b,g,r) mapping,
        // written in genuinely Red-fastest order, and check it compiles
        // to the same answer as the vfx_lut-sourced LUT above.
        let size = 3usize;
        let mut rf_data = vec![[0.0f32; 3]; size * size * size];
        for r in 0..size {
            for g in 0..size {
                for b in 0..size {
                    let rf = r as f32 / (size - 1) as f32;
                    let gf = g as f32 / (size - 1) as f32;
                    let bf = b as f32 / (size - 1) as f32;
                    let idx = r + size * (g + size * b); // Red-fastest
                    rf_data[idx] = [bf, gf, rf]; // swap R and B
                }
            }
        }
        let lut3d_transform = crate::transform::Lut3DTransform {
            data: rf_data,
            size,
            domain_min: [0.0; 3],
            domain_max: [1.0; 3],
            interpolation: Interpolation::Nearest,
            direction: crate::transform::TransformDirection::Forward,
        };
        let transform = crate::transform::Transform::Lut3D(lut3d_transform);

        let mut processor = crate::processor::Processor::new();
        processor
            .compile_transform(&transform, crate::transform::TransformDirection::Forward)
            .unwrap();

        let input = [1.0f32, 0.5, 0.0];
        let expected = [0.0f32, 0.5, 1.0];
        let mut pixels = [input];
        processor.apply_rgb(&mut pixels);

        assert_eq!(
            pixels[0], expected,
            "inline Transform::Lut3D disagrees with the equivalent vfx_lut::Lut3D-sourced \
             LUT for an asymmetric LUT off the r==b diagonal"
        );
    }
}
