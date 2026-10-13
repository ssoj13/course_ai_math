//! OCIO's `LegacyViewingPipeline`: the whole viewer transform around a [`DisplayViewTransform`].
//!
//! Port of `OpenColorAppHelpers.h:349-420` and `apphelpers/LegacyViewingPipeline.cpp`. It is the
//! structure every OCIO viewer builds on (`ociodisplay`, `pyociodisplay`, `ociobakelut`): an
//! exposure-style correction in `scene_linear` whatever the input space, a grade in
//! `color_timing`, looks, a channel view, the display transform and a display-side correction,
//! compiled into ONE [`Processor`] (so one CPU pass and one shader).
//!
//! ```text
//! input --> [scene_linear: linear_cc] --> [color_timing: color_timing_cc] --> looks
//!       --> channel_view --> display/view transform (no looks) --> display_cc
//! ```
//!
//! Dynamic exposure/gamma: give `linear_cc` / `display_cc` an
//! [`ExposureContrastTransform`](crate::ExposureContrastTransform) with
//! [`EcDynamic::EXPOSURE`](crate::EcDynamic::EXPOSURE) / `GAMMA`, then move the dial with
//! [`Processor::set_dynamic`] (CPU) or the op's uniform (GPU, found with\n//! [`GpuProcessor::dynamic_uniforms`](crate::GpuProcessor::dynamic_uniforms)) instead of rebuilding.\n//!\n//! Naming: the crate's style (`processor`, `processor_with_context`, setters taking\n//! `Option<Transform>`); OCIO's `getProcessor` / `setLinearCC` / ... are the same calls.

use crate::TransformDirection;
use crate::config::Config;
use crate::context::Context;
use crate::error::{OcioError, OcioResult};
use crate::processor::Processor;
use crate::role::names;
use crate::transform::{ColorSpaceTransform, DisplayViewTransform, LookTransform, Transform};

/// Builds the viewer processor around a display/view transform. The pipeline order and the
/// dynamic exposure/gamma hooks are described in the module documentation.
///
/// Setters take ownership of their transform, where OCIO copies (`createEditableCopy()`).
#[derive(Debug, Clone, Default)]
pub struct LegacyViewingPipeline {
    /// The display/view transform, with its own looks bypassed (the pipeline applies them).
    dvt: Option<DisplayViewTransform>,
    /// `looks_bypass` of the transform as the caller set it, before the pipeline forced it on.
    dvt_looks_bypass: bool,
    linear_cc: Option<Transform>,
    color_timing_cc: Option<Transform>,
    channel_view: Option<Transform>,
    display_cc: Option<Transform>,
    looks_override_enabled: bool,
    looks_override: String,
}

impl LegacyViewingPipeline {
    /// An empty pipeline; a display/view transform is required before [`Self::processor`].
    pub fn new() -> Self {
        Self::default()
    }

    /// The display/view transform (OCIO `getDisplayViewTransform`). Its `looks_bypass` is
    /// always on: the pipeline applies the view's looks itself. Its own `looks_override*`
    /// fields are not used; the pipeline has [`Self::set_looks_override`].
    pub fn display_view_transform(&self) -> Option<&DisplayViewTransform> {
        self.dvt.as_ref()
    }

    /// Sets the display/view transform (`None` clears it; OCIO `setDisplayViewTransform`).
    /// The caller's `looks_bypass` is remembered and honoured when choosing the view's looks.
    pub fn set_display_view_transform(&mut self, dvt: Option<DisplayViewTransform>) {
        self.dvt_looks_bypass = dvt.as_ref().is_some_and(|d| d.looks_bypass);
        self.dvt = dvt.map(|d| DisplayViewTransform {
            looks_bypass: true,
            looks_override_enabled: false,
            looks_override: String::new(),
            ..d
        });
    }

    /// The correction applied in `scene_linear` (OCIO `getLinearCC`).
    pub fn linear_cc(&self) -> Option<&Transform> {
        self.linear_cc.as_ref()
    }

    /// Sets the correction applied in `scene_linear` (exposure, ...; OCIO `setLinearCC`). A
    /// no-op transform is skipped, with its colour space conversion; a dynamic one is never
    /// a no-op.
    pub fn set_linear_cc(&mut self, cc: Option<Transform>) {
        self.linear_cc = cc;
    }

    /// The correction applied in `color_timing` (OCIO `getColorTimingCC`).
    pub fn color_timing_cc(&self) -> Option<&Transform> {
        self.color_timing_cc.as_ref()
    }

    /// Sets the correction applied in the `color_timing` role's space (OCIO
    /// `setColorTimingCC`).
    pub fn set_color_timing_cc(&mut self, cc: Option<Transform>) {
        self.color_timing_cc = cc;
    }

    /// The channel view (typically a [`Transform::Matrix`] swizzle; OCIO `getChannelView`).
    pub fn channel_view(&self) -> Option<&Transform> {
        self.channel_view.as_ref()
    }

    /// Sets the channel view, applied just before the display transform (OCIO
    /// `setChannelView`). With `data_bypass` on, a matrix that lets alpha into R, G or B also
    /// skips the colour space conversions, so alpha is shown unprocessed.
    pub fn set_channel_view(&mut self, cv: Option<Transform>) {
        self.channel_view = cv;
    }

    /// The correction applied after the display transform (OCIO `getDisplayCC`).
    pub fn display_cc(&self) -> Option<&Transform> {
        self.display_cc.as_ref()
    }

    /// Sets the correction applied in display space (the viewer gamma, ...; OCIO
    /// `setDisplayCC`).
    pub fn set_display_cc(&mut self, cc: Option<Transform>) {
        self.display_cc = cc;
    }

    /// Whether [`Self::looks_override`] replaces the view's looks.
    pub fn looks_override_enabled(&self) -> bool {
        self.looks_override_enabled
    }

    /// Enables the looks override; it then applies even to data colour spaces.
    pub fn set_looks_override_enabled(&mut self, enable: bool) {
        self.looks_override_enabled = enable;
    }

    /// The looks used instead of the view's when the override is enabled.
    pub fn looks_override(&self) -> &str {
        &self.looks_override
    }

    /// Sets the override looks (a look string, `"+warm, -cool"`).
    pub fn set_looks_override(&mut self, looks: &str) {
        self.looks_override = looks.to_string();
    }

    /// Checks the pipeline can build a processor (OCIO `validate()`): a display transform with
    /// a source, display and view. The individual transforms are checked when they compile;
    /// this crate has no standalone per-transform validation to run here.
    ///
    /// The DisplayViewTransform texts are `DisplayViewTransform::validate`'s
    /// (`DisplayViewTransform.cpp:90-100`), wrapped as `LegacyViewingPipeline.cpp:166-171` does.
    /// PyOpenColorIO 2.5.1 never shows the wrapper: its `DisplayViewTransform` constructor
    /// validates first, so the bare text comes out of the constructor; a transform that
    /// reaches the pipeline unvalidated (as here) gets the wrapped text, as the source says.
    pub fn validate(&self) -> OcioResult<()> {
        self.need_dvt().map(|_| ())
    }

    fn need_dvt(&self) -> OcioResult<&DisplayViewTransform> {
        let dvt = self.dvt.as_ref().ok_or_else(|| {
            OcioError::Pipeline(
                "LegacyViewingPipeline: can't create a processor without a display transform."
                    .into(),
            )
        })?;
        let bad = if dvt.src.is_empty() {
            Some("empty source color space name.")
        } else if dvt.display.is_empty() {
            Some("empty display name.")
        } else if dvt.view.is_empty() {
            Some("empty view name.")
        } else {
            None
        };
        match bad {
            Some(why) => Err(OcioError::Pipeline(format!(
                "LegacyViewingPipeline is not valid: DisplayViewTransform: {why}"
            ))),
            None => Ok(dvt),
        }
    }

    /// Builds the processor with the config's current context (OCIO `getProcessor(config)`).
    pub fn processor(&self, config: &Config) -> OcioResult<Processor> {
        self.processor_with_context(config, config.context())
    }

    /// Builds the one processor for the whole pipeline (OCIO `getProcessor(config, context)`).
    ///
    /// # Errors
    /// [`OcioError::Pipeline`] for no / incomplete display transform;
    /// [`OcioError::InputColorSpaceNotFound`]; [`OcioError::RoleRequired`] for a `linear_cc`
    /// / `color_timing_cc` that is not a no-op while its role is undefined; the look errors
    /// of [`Config::looks_result_space`]; and whatever the transforms report compiling.
    pub fn processor_with_context(
        &self,
        config: &Config,
        context: &Context,
    ) -> OcioResult<Processor> {
        let dvt = self.need_dvt()?;
        let dir = dvt.direction;

        let input_name = dvt.src.as_str();
        let input_cs =
            config
                .colorspace(input_name)
                .ok_or_else(|| OcioError::InputColorSpaceNotFound {
                    name: input_name.into(),
                })?;

        // OCIO's `getView`: case-insensitive, not narrowed by the active views.
        let (display, view) = (dvt.display.as_str(), dvt.view.as_str());
        let view_def = config.find_view(display, view);
        // A missing view or colour space leaves `display_cs` empty; the display transform
        // then reports the precise error when it compiles (as in OCIO).
        let display_cs = view_def.and_then(|v| config.colorspace(v.effective_colorspace(display)));

        let display_data = display_cs.is_none_or(|cs| cs.is_data());
        let mut skip_cs = dvt.data_bypass && (input_cs.is_data() || display_data);

        // Viewing alpha: skip every colour space conversion so it is not graded.
        if dvt.data_bypass
            && let Some(Transform::Matrix(m)) = &self.channel_view
            && (m.matrix[3] > 0.0 || m.matrix[7] > 0.0 || m.matrix[11] > 0.0)
        {
            skip_cs = true;
        }

        let mut current = input_name.to_string();
        let mut dt_input = input_cs.name().to_string();
        let mut group: Vec<Transform> = Vec::new();

        for (cc, role, what) in [
            (&self.linear_cc, names::SCENE_LINEAR, "LinearCC"),
            (&self.color_timing_cc, names::COLOR_TIMING, "ColorTimingCC"),
        ] {
            let Some(cc) = cc else { continue };
            // A no-op correction is dropped together with its colour space round trip.
            if config
                .processor_from_transform_with_context(context, cc, dir)?
                .is_no_op()
            {
                continue;
            }
            let role_cs = config
                .colorspace(role)
                .ok_or_else(|| OcioError::RoleRequired {
                    role: role.into(),
                    what,
                })?;
            dt_input = role_cs.name().to_string();
            if !skip_cs {
                group.push(Transform::ColorSpace(ColorSpaceTransform {
                    src: current.clone(),
                    dst: role.to_string(),
                    data_bypass: true,
                    direction: TransformDirection::Forward,
                }));
                current = role.to_string();
            }
            group.push(cc.clone());
        }

        let mut dt = dvt.clone();
        dt.direction = TransformDirection::Forward;
        dt.src = current;

        // An enabled override always applies, even to data; otherwise follow the view, except
        // that conversions into the looks' process space are skipped for data.
        let looks = if self.looks_override_enabled {
            self.looks_override.as_str()
        } else if !self.dvt_looks_bypass && !skip_cs {
            view_def.and_then(|v| v.looks()).unwrap_or_default()
        } else {
            ""
        };
        if !looks.is_empty() {
            let out_cs = if skip_cs {
                Some(dt_input.as_str())
            } else {
                config.looks_result_space(looks, context)?
            };
            // No result space means the looks are all no-ops.
            if let Some(out_cs) = out_cs.filter(|s| !s.is_empty()) {
                group.push(Transform::Look(LookTransform {
                    src: dt_input.clone(),
                    dst: out_cs.to_string(),
                    looks: looks.to_string(),
                    skip_color_space_conversion: skip_cs,
                    direction: TransformDirection::Forward,
                }));
                dt.src = out_cs.to_string();
            }
        }

        if let Some(cv) = &self.channel_view {
            group.push(cv.clone());
        }
        // Without a display colour space the display transform stands for a named transform
        // and must run (OCIO does the same).
        if !skip_cs || display_cs.is_none() {
            group.push(Transform::DisplayView(dt));
        }
        if let Some(cc) = &self.display_cc {
            group.push(cc.clone());
        }

        config.processor_from_transform_with_context(context, &Transform::group(group), dir)
    }
}
