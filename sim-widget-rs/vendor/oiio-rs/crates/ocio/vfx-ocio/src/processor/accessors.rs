//! Read-only / introspection methods on `Processor`.
//!
//! Extracted from `processor.rs` during STRUCT.2.next. These methods only
//! query state — they don't compile new ops or apply pixels — and were the
//! lowest-coupling group in the original 4500+ LOC `impl Processor` block.
//! The one exception is `set_dynamic`, which moves a dynamic property in place.
//!
//! Includes: `num_ops` / `is_empty` / `is_identity`, bit-depth queries,
//! `cache_id`, `is_dynamic`, `create_group_transform` (with its helper
//! `fn`s), `metadata` / `format_metadata`, and the `optimized` builder.

use super::*;

impl super::Processor {
    /// Returns the number of operations.
    #[inline]
    pub fn num_ops(&self) -> usize {
        self.ops.len()
    }

    /// Checks if processor has any operations.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Checks if processor is identity (no-op).
    pub fn is_identity(&self) -> bool {
        self.ops.is_empty()
    }

    /// Returns input bit depth hint.
    #[inline]
    pub fn input_bit_depth(&self) -> PackedBitDepth {
        self.input_bit_depth
    }

    /// Returns output bit depth hint.
    #[inline]
    pub fn output_bit_depth(&self) -> PackedBitDepth {
        self.output_bit_depth
    }

    // =========================================================================
    // OCIO Parity Methods (Processor API)
    // =========================================================================

    /// Checks if the processor is a no-op (identity).
    /// OCIO ref: Processor::isNoOp()
    #[inline]
    pub fn is_no_op(&self) -> bool {
        self.ops.is_empty()
    }

    /// Checks if the processor has channel crosstalk.
    /// Returns true if the transform is non-separable (channels affect each other).
    /// OCIO ref: Processor::hasChannelCrosstalk()
    pub fn has_channel_crosstalk(&self) -> bool {
        for op in &self.ops {
            match op {
                // Matrix can have crosstalk if off-diagonal elements are non-zero
                ProcessorOp::Matrix { matrix, .. } => {
                    // Check off-diagonal elements in 3x3 portion
                    if matrix[1].abs() > 1e-9
                        || matrix[2].abs() > 1e-9
                        || matrix[4].abs() > 1e-9
                        || matrix[6].abs() > 1e-9
                        || matrix[8].abs() > 1e-9
                        || matrix[9].abs() > 1e-9
                    {
                        return true;
                    }
                }
                // CDL saturation causes crosstalk
                ProcessorOp::Cdl { saturation, .. } => {
                    if (*saturation - 1.0).abs() > 1e-9 {
                        return true;
                    }
                }
                // 3D LUTs always have crosstalk
                ProcessorOp::Lut3d { .. } => return true,
                // Grading ops with saturation have crosstalk
                ProcessorOp::GradingPrimary { saturation, .. } => {
                    if (*saturation - 1.0).abs() > 1e-9 {
                        return true;
                    }
                }
                // Fixed functions may have crosstalk
                ProcessorOp::FixedFunction { style, .. } => {
                    use crate::transform::FixedFunctionStyle;
                    match style {
                        FixedFunctionStyle::RgbToHsv
                        | FixedFunctionStyle::HsvToRgb
                        | FixedFunctionStyle::RgbToHsyLin
                        | FixedFunctionStyle::HsyLinToRgb
                        | FixedFunctionStyle::RgbToHsyLog
                        | FixedFunctionStyle::HsyLogToRgb
                        | FixedFunctionStyle::AcesGamutComp13
                        | FixedFunctionStyle::AcesOutputTransform20 => return true,
                        _ => {}
                    }
                }
                // Other ops are typically per-channel
                _ => {}
            }
        }
        false
    }

    /// Generates a cache ID hash for the processor.
    /// OCIO ref: Processor::getCacheID()
    pub fn cache_id(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        // Hash each op
        for (i, op) in self.ops.iter().enumerate() {
            i.hash(&mut hasher);
            // Hash op type discriminant
            std::mem::discriminant(op).hash(&mut hasher);

            // Hash op-specific data
            match op {
                ProcessorOp::Matrix { matrix, offset } => {
                    for v in matrix {
                        v.to_bits().hash(&mut hasher);
                    }
                    for v in offset {
                        v.to_bits().hash(&mut hasher);
                    }
                }
                ProcessorOp::Cdl {
                    slope,
                    offset,
                    power,
                    saturation,
                    style,
                    inverse,
                } => {
                    for v in slope {
                        v.to_bits().hash(&mut hasher);
                    }
                    for v in offset {
                        v.to_bits().hash(&mut hasher);
                    }
                    for v in power {
                        v.to_bits().hash(&mut hasher);
                    }
                    saturation.to_bits().hash(&mut hasher);
                    std::mem::discriminant(style).hash(&mut hasher);
                    inverse.is_some().hash(&mut hasher);
                }
                ProcessorOp::Transfer { style, forward } => {
                    std::mem::discriminant(style).hash(&mut hasher);
                    forward.hash(&mut hasher);
                }
                ProcessorOp::Lut1d {
                    lut,
                    size,
                    half_domain,
                    hue_adjust,
                    ..
                } => {
                    size.hash(&mut hasher);
                    half_domain.hash(&mut hasher);
                    for v in lut.iter().take(16) {
                        v.to_bits().hash(&mut hasher);
                    }
                    std::mem::discriminant(hue_adjust).hash(&mut hasher);
                }
                ProcessorOp::Lut3d { size, .. } => {
                    size.hash(&mut hasher);
                }
                // `ExposureContrastOpData::getCacheID`: the value of a DYNAMIC parameter is
                // left out (it changes at run time without making a different processor), so a
                // host keyed on this id does not rebuild when the dial moves.
                ProcessorOp::ExposureContrast {
                    exposure,
                    contrast,
                    gamma,
                    pivot,
                    log_exposure_step,
                    log_mid_gray,
                    style,
                    dynamic,
                    forward,
                } => {
                    std::mem::discriminant(style).hash(&mut hasher);
                    forward.hash(&mut hasher);
                    (dynamic.exposure, dynamic.contrast, dynamic.gamma).hash(&mut hasher);
                    for (v, is_dynamic) in [
                        (exposure, dynamic.exposure),
                        (contrast, dynamic.contrast),
                        (gamma, dynamic.gamma),
                    ] {
                        if !is_dynamic {
                            v.to_bits().hash(&mut hasher);
                        }
                    }
                    for v in [pivot, log_exposure_step, log_mid_gray] {
                        v.to_bits().hash(&mut hasher);
                    }
                }
                _ => {
                    // Generic hash for other ops
                    format!("{:?}", op).hash(&mut hasher);
                }
            }
        }

        format!("{:016x}", hasher.finish())
    }

    /// Checks if processor has any dynamic properties: an op with a parameter declared dynamic
    /// ([`crate::EcDynamic`]). OCIO ref: `Processor::isDynamic()` /
    /// `CPUProcessor::isDynamic` (any op's `isDynamic()`).
    ///
    /// The grading ops are never dynamic here: their transforms carry no dynamic flag in this
    /// crate (their uniforms are always settable on the GPU, see `GpuProcessor`).
    #[inline]
    pub fn is_dynamic(&self) -> bool {
        self.ops
            .iter()
            .any(|op| matches!(op, ProcessorOp::ExposureContrast { dynamic, .. } if dynamic.any()))
    }
    /// Creates a GroupTransform from this processor's operations.
    /// OCIO ref: Processor::createGroupTransform()
    pub fn create_group_transform(&self) -> crate::transform::GroupTransform {
        fn map_transfer_style(style: TransferStyle) -> &'static str {
            match style {
                TransferStyle::Srgb => "sRGB",
                TransferStyle::Rec709 => "Rec709",
                TransferStyle::Rec2020 => "Rec2020",
                TransferStyle::Gamma22 => "Gamma22",
                TransferStyle::Gamma24 => "Gamma24",
                TransferStyle::Gamma26 => "Gamma26",
                TransferStyle::Rec1886 => "Rec1886",
                TransferStyle::Linear => "Linear",
                TransferStyle::Pq => "PQ",
                TransferStyle::Hlg => "HLG",
                TransferStyle::AcesCct => "ACEScct",
                TransferStyle::AcesCc => "ACEScc",
                TransferStyle::Log3G10 => "Log3G10",
                TransferStyle::LogC3 => "LogC3",
                TransferStyle::LogC4 => "LogC4",
                TransferStyle::SLog3 => "SLog3",
                TransferStyle::VLog => "VLog",
                TransferStyle::BmdFilmGen5 => "BMDFilmGen5",
                TransferStyle::AppleLog => "AppleLog",
                TransferStyle::CanonCLog2 => "CanonCLog2",
                TransferStyle::CanonCLog3 => "CanonCLog3",
            }
        }

        fn primaries_to_params(p: crate::aces2::Primaries) -> [f64; 8] {
            [
                p.red.x as f64,
                p.red.y as f64,
                p.green.x as f64,
                p.green.y as f64,
                p.blue.x as f64,
                p.blue.y as f64,
                p.white.x as f64,
                p.white.y as f64,
            ]
        }

        fn grading_style_to_hue_style(
            style: crate::grading_hue_curve::GradingStyle,
        ) -> crate::transform::GradingHueCurveStyle {
            match style {
                crate::grading_hue_curve::GradingStyle::Log => {
                    crate::transform::GradingHueCurveStyle::Log
                }
                crate::grading_hue_curve::GradingStyle::Linear => {
                    crate::transform::GradingHueCurveStyle::Linear
                }
                crate::grading_hue_curve::GradingStyle::Video => {
                    crate::transform::GradingHueCurveStyle::Video
                }
            }
        }

        fn hue_points(points: &[crate::grading_hue_curve::HueControlPoint]) -> Vec<[f64; 2]> {
            points
                .iter()
                .map(|p| [p.hue as f64, p.value as f64])
                .collect()
        }

        let mut transforms = Vec::with_capacity(self.ops.len());
        for op in &self.ops {
            let t = match op {
                ProcessorOp::Matrix { matrix, offset } => Transform::Matrix(MatrixTransform {
                    name: String::new(),
                    matrix: matrix.map(|v| v as f64),
                    offset: offset.map(|v| v as f64),
                    direction: TransformDirection::Forward,
                }),
                ProcessorOp::Lut1d {
                    lut,
                    size,
                    channels,
                    domain_min,
                    domain_max,
                    half_domain,
                    hue_adjust,
                    ..
                } => {
                    let mut red = Vec::new();
                    let mut green = None;
                    let mut blue = None;

                    if *channels == 3 {
                        let mut g = Vec::with_capacity(*size);
                        let mut b = Vec::with_capacity(*size);
                        for i in 0..*size {
                            let base = i * 3;
                            if base + 2 >= lut.len() {
                                break;
                            }
                            red.push(lut[base]);
                            g.push(lut[base + 1]);
                            b.push(lut[base + 2]);
                        }
                        green = Some(g);
                        blue = Some(b);
                    } else {
                        red.extend_from_slice(lut);
                    }

                    Transform::Lut1D(Lut1DTransform {
                        red,
                        green,
                        blue,
                        input_min: domain_min[0],
                        input_max: domain_max[0],
                        output_min: 0.0,
                        output_max: 1.0,
                        half_domain: *half_domain,
                        raw_halfs: false,
                        hue_adjust: *hue_adjust,
                        interpolation: Interpolation::Linear,
                        direction: TransformDirection::Forward,
                    })
                }
                ProcessorOp::Lut3d {
                    lut,
                    size,
                    interp,
                    domain_min,
                    domain_max,
                    ..
                } => {
                    let mut data = Vec::with_capacity(lut.len() / 3);
                    let mut i = 0;
                    while i + 2 < lut.len() {
                        data.push([lut[i], lut[i + 1], lut[i + 2]]);
                        i += 3;
                    }
                    Transform::Lut3D(Lut3DTransform {
                        data,
                        size: *size,
                        domain_min: *domain_min,
                        domain_max: *domain_max,
                        interpolation: *interp,
                        direction: TransformDirection::Forward,
                    })
                }
                ProcessorOp::Exponent {
                    value,
                    negative_style,
                } => Transform::Exponent(ExponentTransform {
                    name: String::new(),
                    value: value.map(|v| v as f64),
                    negative_style: *negative_style,
                    direction: TransformDirection::Forward,
                }),
                ProcessorOp::Log { base, forward } => Transform::Log(LogTransform {
                    name: String::new(),
                    base: *base as f64,
                    direction: if *forward {
                        TransformDirection::Forward
                    } else {
                        TransformDirection::Inverse
                    },
                }),
                ProcessorOp::Cdl {
                    slope,
                    offset,
                    power,
                    saturation,
                    style,
                    inverse,
                } => Transform::Cdl(CdlTransform {
                    name: String::new(),
                    slope: slope.map(|v| v as f64),
                    offset: offset.map(|v| v as f64),
                    power: power.map(|v| v as f64),
                    saturation: *saturation as f64,
                    style: *style,
                    direction: if inverse.is_some() {
                        TransformDirection::Inverse
                    } else {
                        TransformDirection::Forward
                    },
                }),
                ProcessorOp::Range {
                    scale,
                    offset,
                    clamp_min,
                    clamp_max,
                } => {
                    let (min_out, max_out) = if scale.abs() > 1e-12 {
                        (
                            clamp_min.unwrap_or(*offset),
                            clamp_max.unwrap_or(offset + scale),
                        )
                    } else {
                        (*offset, *offset)
                    };
                    let (min_in, max_in) = if scale.abs() > 1e-12 {
                        let min_in = (min_out - offset) / scale;
                        let max_in = min_in + (max_out - min_out) / scale;
                        (Some(min_in as f64), Some(max_in as f64))
                    } else {
                        (Some(0.0), Some(1.0))
                    };

                    Transform::Range(RangeTransform {
                        name: String::new(),
                        min_in,
                        max_in,
                        min_out: Some(min_out as f64),
                        max_out: Some(max_out as f64),
                        style: if clamp_min.is_some() || clamp_max.is_some() {
                            RangeStyle::Clamp
                        } else {
                            RangeStyle::NoClamp
                        },
                        direction: TransformDirection::Forward,
                    })
                }
                ProcessorOp::Transfer { style, forward } => {
                    Transform::BuiltinTransfer(BuiltinTransferTransform {
                        style: map_transfer_style(*style).to_string(),
                        direction: if *forward {
                            TransformDirection::Forward
                        } else {
                            TransformDirection::Inverse
                        },
                    })
                }
                ProcessorOp::ExposureContrast {
                    exposure,
                    contrast,
                    gamma,
                    pivot,
                    log_exposure_step,
                    log_mid_gray,
                    style,
                    dynamic,
                    forward,
                } => Transform::ExposureContrast(ExposureContrastTransform {
                    name: String::new(),
                    exposure: *exposure as f64,
                    contrast: *contrast as f64,
                    gamma: *gamma as f64,
                    pivot: *pivot as f64,
                    log_exposure_step: *log_exposure_step as f64,
                    log_mid_gray: *log_mid_gray as f64,
                    style: *style,
                    direction: if *forward {
                        TransformDirection::Forward
                    } else {
                        TransformDirection::Inverse
                    },
                    dynamic: *dynamic,
                }),
                ProcessorOp::FixedFunction {
                    style,
                    params,
                    forward,
                } => Transform::FixedFunction(FixedFunctionTransform {
                    name: String::new(),
                    style: *style,
                    params: params.iter().map(|v| *v as f64).collect(),
                    direction: if *forward {
                        TransformDirection::Forward
                    } else {
                        TransformDirection::Inverse
                    },
                }),
                ProcessorOp::Aces2OutputTransform { forward, .. } => {
                    let mut params = Vec::with_capacity(9);
                    params.push(1000.0);
                    params.extend_from_slice(&primaries_to_params(crate::aces2::ACES_AP0));
                    Transform::FixedFunction(FixedFunctionTransform {
                        name: String::new(),
                        style: FixedFunctionStyle::AcesOutputTransform20,
                        params,
                        direction: if *forward {
                            TransformDirection::Forward
                        } else {
                            TransformDirection::Inverse
                        },
                    })
                }
                ProcessorOp::Aces2RgbJmh { forward, .. } => {
                    let params = primaries_to_params(crate::aces2::ACES_AP0).to_vec();
                    Transform::FixedFunction(FixedFunctionTransform {
                        name: String::new(),
                        style: if *forward {
                            FixedFunctionStyle::AcesRgbToJmh20
                        } else {
                            FixedFunctionStyle::AcesJmhToRgb20
                        },
                        params,
                        direction: TransformDirection::Forward,
                    })
                }
                ProcessorOp::Aces2TonescaleCompress { forward, .. } => {
                    Transform::FixedFunction(FixedFunctionTransform {
                        name: String::new(),
                        style: FixedFunctionStyle::AcesTonescaleCompress20,
                        params: vec![1000.0],
                        direction: if *forward {
                            TransformDirection::Forward
                        } else {
                            TransformDirection::Inverse
                        },
                    })
                }
                ProcessorOp::Aces2GamutCompress { forward, .. } => {
                    let mut params = Vec::with_capacity(9);
                    params.push(1000.0);
                    params.extend_from_slice(&primaries_to_params(crate::aces2::ACES_AP0));
                    Transform::FixedFunction(FixedFunctionTransform {
                        name: String::new(),
                        style: FixedFunctionStyle::AcesGamutCompress20,
                        params,
                        direction: if *forward {
                            TransformDirection::Forward
                        } else {
                            TransformDirection::Inverse
                        },
                    })
                }
                ProcessorOp::Allocation {
                    allocation,
                    vars,
                    forward,
                } => Transform::Allocation(AllocationTransform {
                    allocation: *allocation,
                    vars: vars.iter().map(|v| *v as f64).collect(),
                    direction: if *forward {
                        TransformDirection::Forward
                    } else {
                        TransformDirection::Inverse
                    },
                }),
                ProcessorOp::GradingPrimary {
                    style,
                    forward,
                    raw,
                    ..
                } => {
                    // Reconstruction for introspection only
                    // (`create_group_transform`), not the live pixel path. Used to
                    // be a best-effort guess (the op only stored compile-time
                    // PRECOMPUTED, direction-resolved per-channel values, so the
                    // original raw RGBM master/channel split was unrecoverable —
                    // this placed each computed number as R/G/B with an identity
                    // master, reproducing the same resolved numbers but not the
                    // config author's actual split). Now that the op also carries
                    // `raw` (the never-direction-resolved `GradingPrimary` value,
                    // added so `ProcessorOp::inverse()` could re-run
                    // `grading_primary_params` instead of hand-inverting — see that
                    // fn's doc comment), this reconstruction is EXACT: `raw` IS the
                    // original value.
                    Transform::GradingPrimary(GradingPrimaryTransform {
                        name: String::new(),
                        style: *style,
                        value: raw.clone(),
                        direction: if *forward {
                            TransformDirection::Forward
                        } else {
                            TransformDirection::Inverse
                        },
                    })
                }
                ProcessorOp::GradingRgbCurve { raw, forward, .. } => {
                    // Exact: the op keeps the transform it was compiled from.
                    Transform::GradingRgbCurve(GradingRgbCurveTransform {
                        direction: if *forward {
                            TransformDirection::Forward
                        } else {
                            TransformDirection::Inverse
                        },
                        ..(**raw).clone()
                    })
                }
                ProcessorOp::GradingTone {
                    style,
                    value,
                    forward,
                    ..
                } => Transform::GradingTone(GradingToneTransform {
                    name: String::new(),
                    style: *style,
                    blacks: value.blacks,
                    shadows: value.shadows,
                    midtones: value.midtones,
                    highlights: value.highlights,
                    whites: value.whites,
                    scontrast: value.s_contrast,
                    direction: if *forward {
                        TransformDirection::Forward
                    } else {
                        TransformDirection::Inverse
                    },
                }),
                ProcessorOp::GradingHueCurve {
                    curves,
                    slopes,
                    rgb_to_hsy,
                    draw_curve_only,
                    forward,
                    ..
                } => {
                    let to_f64 = |s: &[f32]| s.iter().map(|v| *v as f64).collect::<Vec<_>>();
                    Transform::GradingHueCurve(GradingHueCurveTransform {
                        name: String::new(),
                        style: grading_style_to_hue_style(curves.style),
                        rgb_to_hsy: *rgb_to_hsy,
                        hue_hue: hue_points(&curves.hue_hue.points),
                        hue_sat: hue_points(&curves.hue_sat.points),
                        hue_lum: hue_points(&curves.hue_lum.points),
                        lum_sat: hue_points(&curves.lum_sat.points),
                        sat_sat: hue_points(&curves.sat_sat.points),
                        lum_lum: hue_points(&curves.lum_lum.points),
                        sat_lum: hue_points(&curves.sat_lum.points),
                        hue_fx: hue_points(&curves.hue_fx.points),
                        hue_hue_slopes: to_f64(&slopes[0]),
                        hue_sat_slopes: to_f64(&slopes[1]),
                        hue_lum_slopes: to_f64(&slopes[2]),
                        lum_sat_slopes: to_f64(&slopes[3]),
                        sat_sat_slopes: to_f64(&slopes[4]),
                        lum_lum_slopes: to_f64(&slopes[5]),
                        sat_lum_slopes: to_f64(&slopes[6]),
                        hue_fx_slopes: to_f64(&slopes[7]),
                        draw_curve_only: *draw_curve_only,
                        direction: if *forward {
                            TransformDirection::Forward
                        } else {
                            TransformDirection::Inverse
                        },
                    })
                }
                ProcessorOp::LogAffine {
                    base,
                    log_side_slope,
                    log_side_offset,
                    lin_side_slope,
                    lin_side_offset,
                    forward,
                } => Transform::LogAffine(LogAffineTransform {
                    name: String::new(),
                    base: *base as f64,
                    log_side_slope: log_side_slope.map(|v| v as f64),
                    log_side_offset: log_side_offset.map(|v| v as f64),
                    lin_side_slope: lin_side_slope.map(|v| v as f64),
                    lin_side_offset: lin_side_offset.map(|v| v as f64),
                    direction: if *forward {
                        TransformDirection::Forward
                    } else {
                        TransformDirection::Inverse
                    },
                }),
                ProcessorOp::LogCamera {
                    base,
                    log_side_slope,
                    log_side_offset,
                    lin_side_slope,
                    lin_side_offset,
                    lin_side_break,
                    linear_slope,
                    forward,
                    ..
                } => Transform::LogCamera(LogCameraTransform {
                    name: String::new(),
                    base: *base as f64,
                    log_side_slope: log_side_slope.map(|v| v as f64),
                    log_side_offset: log_side_offset.map(|v| v as f64),
                    lin_side_slope: lin_side_slope.map(|v| v as f64),
                    lin_side_offset: lin_side_offset.map(|v| v as f64),
                    lin_side_break: lin_side_break.map(|v| v as f64),
                    linear_slope: Some(linear_slope.map(|v| v as f64)),
                    direction: if *forward {
                        TransformDirection::Forward
                    } else {
                        TransformDirection::Inverse
                    },
                }),
                ProcessorOp::ExponentWithLinear {
                    gamma,
                    offset,
                    mirror,
                    forward,
                } => Transform::ExponentWithLinear(ExponentWithLinearTransform {
                    name: String::new(),
                    gamma: gamma.map(|v| v as f64),
                    offset: offset.map(|v| v as f64),
                    negative_style: if *mirror {
                        NegativeStyle::Mirror
                    } else {
                        NegativeStyle::Linear
                    },
                    direction: if *forward {
                        TransformDirection::Forward
                    } else {
                        TransformDirection::Inverse
                    },
                }),
            };
            transforms.push(t);
        }

        crate::transform::GroupTransform {
            name: String::new(),
            transforms,
            direction: crate::TransformDirection::Forward,
        }
    }

    /// Returns the number of transforms/operations in this processor.
    /// OCIO ref: Processor::getNumTransforms()
    #[inline]
    pub fn num_transforms(&self) -> usize {
        self.ops.len()
    }

    /// Checks if this processor has a specific dynamic property type: some op declared it
    /// dynamic (`ExposureContrastOpData::hasDynamicProperty`). OCIO ref:
    /// `Processor::hasDynamicProperty()`. Static exposure-contrast values and the grading ops
    /// (no dynamic flag in this crate) do not count.
    pub fn has_dynamic_property(&self, prop_type: crate::DynamicPropertyType) -> bool {
        self.ops.iter().any(|op| ec_slot(op, prop_type).is_some())
    }

    /// Sets a dynamic scalar property (OCIO `getDynamicProperty(type)->setDouble(v)`) on every
    /// op that declared it dynamic ([`crate::EcDynamic`]), without rebuilding the processor.
    ///
    /// OCIO shares one property object between all ops of a type; here each op is updated in
    /// place. The op keeps FORWARD parameters in both directions, so the value means the same
    /// for an inverse op (its renderer and shader invert it). Exposure is in stops. Only the
    /// exposure-contrast scalars are settable; contrast and gamma are used as given, floored
    /// at `MIN_CONTRAST` when rendered, as OCIO (which validates no range).
    ///
    /// # Errors
    /// [`OcioError::DynamicPropertyNotFound`] when no op has `prop` dynamic (OCIO's CPU
    /// processor text); [`OcioError::Transform`] for a non-scalar `prop` or a non-finite
    /// `value`.
    pub fn set_dynamic(&mut self, prop: crate::DynamicPropertyType, value: f64) -> OcioResult<()> {
        use crate::DynamicPropertyType as P;

        if !matches!(prop, P::Exposure | P::Contrast | P::Gamma) {
            return Err(OcioError::Transform(format!(
                "set_dynamic supports Exposure, Contrast and Gamma, not {prop:?}"
            )));
        }
        if !value.is_finite() {
            return Err(OcioError::Transform(format!(
                "dynamic {prop:?} must be finite, got {value}"
            )));
        }
        let mut found = false;
        for op in &mut self.ops {
            if let Some(slot) = ec_slot_mut(op, prop) {
                *slot = value as f32;
                found = true;
            }
        }
        if found {
            Ok(())
        } else {
            Err(OcioError::DynamicPropertyNotFound)
        }
    }

    /// The current value of a dynamic Exposure/Contrast/Gamma property (OCIO
    /// `getDynamicProperty(type)->getDoubleValue()`), or `None` when no op declared it dynamic.
    pub fn dynamic(&self, prop: crate::DynamicPropertyType) -> Option<f64> {
        self.ops
            .iter()
            .find_map(|op| ec_slot(op, prop))
            .map(f64::from)
    }
    /// Returns an optimized version of this processor.
    /// OCIO ref: Processor::getOptimizedProcessor()
    ///
    /// Note: Returns a new processor with optimizations applied.
    /// The original processor is not modified.
    pub fn optimized(&self, level: OptimizationLevel) -> Self {
        let mut result = Processor {
            ops: self.ops.clone(),
            input_bit_depth: self.input_bit_depth,
            output_bit_depth: self.output_bit_depth,
            context: self.context.clone(),
            io_proxy: self.io_proxy.clone(),
            files: self.files.clone(),
        };
        result.optimize(level);
        result
    }

    /// Returns processor metadata.
    /// OCIO ref: Processor::getProcessorMetadata()
    pub fn metadata(&self) -> ProcessorMetadata {
        // Was hardcoded `{num_files: 0, files: []}` — it reported "this processor uses no
        // external files" for every processor, including ones built entirely from a LUT file.
        // The stub survived because its only test asserted `num_files >= 0` on a `usize`, which
        // is always true. The reference genuinely tracks this (`ProcessorMetadata::addFile`,
        // Processor.cpp:73, fed from `FileNoOp::dumpMetadata`, NoOps.cpp:355).
        ProcessorMetadata {
            num_files: self.files.len(),
            files: self.files.clone(),
        }
    }

    /// Returns format metadata.
    /// OCIO ref: Processor::getFormatMetadata()
    pub fn format_metadata(&self) -> FormatMetadata {
        FormatMetadata {
            id: self.cache_id(),
            name: String::new(),
            attributes: Vec::new(),
        }
    }
}

/// The exposure/contrast/gamma value `op` holds for `prop`, if `op` is an ExposureContrast
/// that declared it dynamic.
fn ec_slot(op: &ProcessorOp, prop: crate::DynamicPropertyType) -> Option<f32> {
    use crate::DynamicPropertyType as P;

    let ProcessorOp::ExposureContrast {
        exposure,
        contrast,
        gamma,
        dynamic,
        ..
    } = op
    else {
        return None;
    };
    match prop {
        P::Exposure if dynamic.exposure => Some(*exposure),
        P::Contrast if dynamic.contrast => Some(*contrast),
        P::Gamma if dynamic.gamma => Some(*gamma),
        _ => None,
    }
}

/// [`ec_slot`], writable.
fn ec_slot_mut(op: &mut ProcessorOp, prop: crate::DynamicPropertyType) -> Option<&mut f32> {
    use crate::DynamicPropertyType as P;

    let ProcessorOp::ExposureContrast {
        exposure,
        contrast,
        gamma,
        dynamic,
        ..
    } = op
    else {
        return None;
    };
    match prop {
        P::Exposure if dynamic.exposure => Some(exposure),
        P::Contrast if dynamic.contrast => Some(contrast),
        P::Gamma if dynamic.gamma => Some(gamma),
        _ => None,
    }
}
