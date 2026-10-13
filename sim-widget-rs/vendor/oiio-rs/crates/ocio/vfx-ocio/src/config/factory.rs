//! `Processor` construction from `Config` — the OCIO factory API.
//!
//! Extracted from `config.rs` during STRUCT.2.next Step 5. This is the
//! second large contiguous block in `impl Config`: all the methods that
//! resolve a (src, dst[, view, looks, ...]) tuple to a compiled
//! `Processor`. The block runs from the basic `processor(src, dst)`
//! convenience entry, through context-aware and option-aware variants,
//! through `display_processor` + `processor_with_looks` +
//! `expand_transform`, into the multi-config processor builders
//! (`processor_from_configs*`, `identify_interchange_space`,
//! `identify_builtin_colorspace`), and ends at the private
//! `append_look_transforms` helper.
//!
//! Public entry points (all `pub fn`, unchanged location-wise):
//! - `processor`, `processor_with_context`, `processor_with_opts`
//! - `display_processor`, `display_processor_with_context`
//! - `processor_with_looks`
//! - `expand_transform`
//! - `processor_from_transform`, `processor_from_transform_with_opts`
//! - `processor_from_configs`, `processor_from_configs_with_interchange`,
//!   `processor_from_configs_with_opts`, `processor_from_configs_to_display`
//! - `identify_interchange_space`, `identify_builtin_colorspace`
//!
//! Private helpers (used only inside factory.rs):
//! - `processor_with_opts_and_context`,
//!   `processor_with_opts_and_context_internal`
//! - `display_processor_with_context_internal`
//! - `append_look_transforms`

use super::*;
use crate::colorspace::ColorSpace;
use crate::display::{ReferenceSpaceType, ViewTransform};

impl super::Config {
    // ========================================================================
    // ColorSpace → reference chain primitives (OCIO C++ parity)
    //
    // These mirror `BuildColorSpaceToReferenceOps`,
    // `BuildColorSpaceFromReferenceOps`, and `BuildReferenceConversionOps`
    // in `OpenColorIO/src/OpenColorIO/transforms/ColorSpaceTransform.cpp`.
    // They are the single source of truth for chain construction —
    // every higher-level processor builder (`processor`,
    // `processor_with_looks`, `expand_transform`, `display_processor`,
    // `processor_from_configs*`) must go through these.
    // ========================================================================

    /// Returns the transform that takes a value from `cs` into its
    /// native reference space (scene or display), picking the
    /// correct accessor by [`ColorSpace::reference_space_type`] and
    /// auto-inverting when only the opposite-direction transform is
    /// set. Returns `None` when no transform pair is defined — that
    /// is a valid silent no-op leg, matching OCIO C++
    /// `BuildColorSpaceToReferenceOps` (ColorSpaceTransform.cpp:272-308).
    fn cs_to_reference(cs: &ColorSpace) -> Option<Transform> {
        match cs.reference_space_type() {
            ReferenceSpaceType::Scene => cs
                .to_reference()
                .cloned()
                .or_else(|| cs.from_reference().map(|t| t.clone().inverse())),
            ReferenceSpaceType::Display => cs
                .to_display_reference()
                .cloned()
                .or_else(|| cs.from_display_reference().map(|t| t.clone().inverse())),
        }
    }

    /// Returns the transform that takes a value from the reference
    /// space into `cs`. Symmetric counterpart to
    /// [`Self::cs_to_reference`]; mirrors OCIO C++
    /// `BuildColorSpaceFromReferenceOps`.
    fn cs_from_reference(cs: &ColorSpace) -> Option<Transform> {
        match cs.reference_space_type() {
            ReferenceSpaceType::Scene => cs
                .from_reference()
                .cloned()
                .or_else(|| cs.to_reference().map(|t| t.clone().inverse())),
            ReferenceSpaceType::Display => cs
                .from_display_reference()
                .cloned()
                .or_else(|| cs.to_display_reference().map(|t| t.clone().inverse())),
        }
    }

    /// Two colorspaces are in the same equality group when both
    /// declare the same non-empty `equality_group` string. In that
    /// case OCIO short-circuits chain construction to a no-op
    /// (`AreColorSpacesInSameEqualityGroup`,
    /// ColorSpaceTransform.cpp:248-249).
    fn in_same_equality_group(a: &ColorSpace, b: &ColorSpace) -> bool {
        let g = a.equality_group();
        !g.is_empty() && g == b.equality_group()
    }

    /// Picks the view transform that bridges the scene and display
    /// reference spaces. Mirrors OCIO C++
    /// `Config::getDefaultSceneToDisplayViewTransform`
    /// (Config.cpp:4544-4569): honours `default_view_transform`
    /// only if that VT itself is scene-referred, otherwise scans
    /// the registered view transforms for the first scene-referred
    /// entry. Returns an error when none exists — OCIO C++ throws
    /// here ("Can not be the case for a valid config",
    /// ColorSpaceTransform.cpp:357-362) and so do we, because a
    /// silent identity bridge would produce mathematically wrong
    /// output without any signal.
    fn default_scene_to_display_view_transform(&self) -> OcioResult<&ViewTransform> {
        if let Some(name) = self.default_view_transform()
            && let Some(vt) = self.displays.view_transform(name)
            && matches!(vt.reference_space_type(), ReferenceSpaceType::Scene)
        {
            return Ok(vt);
        }
        self.displays
            .view_transforms()
            .iter()
            .find(|vt| matches!(vt.reference_space_type(), ReferenceSpaceType::Scene))
            .ok_or_else(|| OcioError::InvalidTransform {
                reason: "scene↔display reference crossing requested but no \
                         scene-referred view transform is defined"
                    .into(),
            })
    }

    /// Resolves the transform that converts data from `from` reference
    /// space to `to` reference space, using the config's default
    /// scene-to-display view transform. Returns `Ok(None)` when
    /// `from == to` (no bridge needed). Mirrors OCIO C++
    /// `BuildReferenceConversionOps` (ColorSpaceTransform.cpp:348-394).
    fn reference_bridge(
        &self,
        from: ReferenceSpaceType,
        to: ReferenceSpaceType,
    ) -> OcioResult<Option<Transform>> {
        if from == to {
            return Ok(None);
        }
        let vt = self.default_scene_to_display_view_transform()?;
        let bridge = match (from, to) {
            (ReferenceSpaceType::Scene, ReferenceSpaceType::Display) => vt
                .from_scene_reference()
                .cloned()
                .or_else(|| vt.to_scene_reference().map(|t| t.clone().inverse())),
            (ReferenceSpaceType::Display, ReferenceSpaceType::Scene) => vt
                .to_scene_reference()
                .cloned()
                .or_else(|| vt.from_scene_reference().map(|t| t.clone().inverse())),
            // from == to already handled above
            _ => None,
        };
        Ok(bridge)
    }

    /// Builds the canonical three-phase ColorSpaceTransform chain
    /// from `src_cs` to `dst_cs`: src→reference, optional
    /// scene↔display bridge, reference→dst. Honours OCIO C++
    /// short-circuits — equality groups and the `data_bypass`
    /// branch for data colorspaces — and returns an empty vector
    /// in those cases. Mirrors `BuildColorSpaceOps`
    /// (ColorSpaceTransform.cpp:236-270).
    ///
    /// **Intentional divergence from OCIO C++:** the reference
    /// implementation wraps each `BuildColorSpace{To,From}ReferenceOps`
    /// leg with `CreateGpuAllocationNoOp` markers
    /// (ColorSpaceTransform.cpp:284-292, 337-344). Those exist only
    /// so the legacy OpenGL `LegacyShaderGen` path can emit
    /// pack/unpack wrappers around the math when targeting
    /// fixed-point texture samplers. Every shader target vfx-ocio
    /// supports (WGSL/wgpu, Metal, modern GLSL/HLSL/Vulkan) reads
    /// float textures directly and writes the math out without
    /// such a wrapper, so the marker is omitted here.
    /// See the `AllocationTransform` rustdoc in `transform.rs`
    /// for the broader rationale.
    fn build_colorspace_chain(
        &self,
        src_cs: &ColorSpace,
        dst_cs: &ColorSpace,
        data_bypass: bool,
    ) -> OcioResult<Vec<Transform>> {
        if data_bypass && (src_cs.is_data() || dst_cs.is_data()) {
            return Ok(Vec::new());
        }
        if Self::in_same_equality_group(src_cs, dst_cs) {
            return Ok(Vec::new());
        }
        let mut chain = Vec::new();
        if let Some(t) = Self::cs_to_reference(src_cs) {
            chain.push(t);
        }
        if let Some(t) =
            self.reference_bridge(src_cs.reference_space_type(), dst_cs.reference_space_type())?
        {
            chain.push(t);
        }
        if let Some(t) = Self::cs_from_reference(dst_cs) {
            chain.push(t);
        }
        Ok(chain)
    }

    /// Creates a processor for conversion between two color spaces.
    pub fn processor(&self, src: &str, dst: &str) -> OcioResult<Processor> {
        self.processor_with_opts(src, dst, OptimizationLevel::default())
    }

    /// Creates a processor for conversion between two color spaces using a context.
    pub fn processor_with_context(
        &self,
        src: &str,
        dst: &str,
        context: &Context,
    ) -> OcioResult<Processor> {
        self.processor_with_opts_and_context(context, src, dst, OptimizationLevel::default())
    }

    /// Creates a processor with optimization level.
    pub fn processor_with_opts(
        &self,
        src: &str,
        dst: &str,
        optimization: OptimizationLevel,
    ) -> OcioResult<Processor> {
        self.processor_with_opts_and_context_internal(None, src, dst, optimization)
    }

    fn processor_with_opts_and_context(
        &self,
        context: &Context,
        src: &str,
        dst: &str,
        optimization: OptimizationLevel,
    ) -> OcioResult<Processor> {
        self.processor_with_opts_and_context_internal(Some(context), src, dst, optimization)
    }

    fn processor_with_opts_and_context_internal(
        &self,
        context: Option<&Context>,
        src: &str,
        dst: &str,
        optimization: OptimizationLevel,
    ) -> OcioResult<Processor> {
        // Resolve each endpoint: prefer ColorSpace, fall back to
        // NamedTransform. Mirrors OCIO C++ `BuildColorSpaceOps`
        // (ColorSpaceTransform.cpp:179-200): missing names throw,
        // and the four endpoint combinations below dispatch to
        // different chain shapes.
        let src_cs = self.colorspace(src);
        let dst_cs = self.colorspace(dst);
        let src_nt = if src_cs.is_none() {
            Some(
                self.named_transform(src)
                    .ok_or_else(|| OcioError::ColorSpaceNotFound { name: src.into() })?,
            )
        } else {
            None
        };
        let dst_nt = if dst_cs.is_none() {
            Some(
                self.named_transform(dst)
                    .ok_or_else(|| OcioError::ColorSpaceNotFound { name: dst.into() })?,
            )
        } else {
            None
        };

        let transforms = match (src_cs, src_nt, dst_cs, dst_nt) {
            (Some(s), _, Some(d), _) => self.build_colorspace_chain(s, d, true)?,
            // (src NT, dst NT): src forward + dst inverse — the
            // surrounding colorspaces are intentionally ignored
            // (ColorSpaceTransform.cpp:203-204).
            (None, Some(snt), None, Some(dnt)) => Self::named_transform_chain(snt, dnt),
            // (src NT, dst CS): src forward, dst ignored.
            (None, Some(snt), Some(_), None) => {
                Self::single_named_transform(snt, /*forward=*/ true)
            }
            // (src CS, dst NT): dst inverse, src ignored.
            (Some(_), None, None, Some(dnt)) => {
                Self::single_named_transform(dnt, /*forward=*/ false)
            }
            // The remaining cases are guarded by the resolution
            // step above (one of the two must be Some).
            _ => unreachable!("either ColorSpace or NamedTransform resolved for both endpoints"),
        };

        if transforms.is_empty() {
            return Ok(Processor::new());
        }

        // A colour space's own transforms may be ColorSpace/Look/DisplayView transforms, which
        // need the config (OCIO builds them through the config too).
        let group = self.expand_with(context, &Transform::group(transforms))?;
        Processor::from_transform_with_context_and_proxy(
            &group,
            TransformDirection::Forward,
            optimization,
            context.cloned(),
            self.io_proxy.clone(),
        )
    }

    /// (src NT, dst NT) — concatenate srcNT.forward and
    /// dstNT.inverse. Used by `processor()` when both endpoints
    /// resolve to named transforms (ColorSpaceTransform.cpp:203-213).
    fn named_transform_chain(
        src: &crate::config::NamedTransform,
        dst: &crate::config::NamedTransform,
    ) -> Vec<Transform> {
        let mut out = Vec::with_capacity(2);
        if let Some(t) = Self::named_transform_in_direction(src, true) {
            out.push(t);
        }
        if let Some(t) = Self::named_transform_in_direction(dst, false) {
            out.push(t);
        }
        out
    }

    /// Single named-transform leg in the requested direction.
    fn single_named_transform(nt: &crate::config::NamedTransform, forward: bool) -> Vec<Transform> {
        Self::named_transform_in_direction(nt, forward)
            .map(|t| vec![t])
            .unwrap_or_default()
    }

    /// Resolves a named transform's body in the requested direction.
    /// Honours the explicit `inverse` slot when set, otherwise
    /// inverts the `forward` slot. Returns `None` only when the
    /// named transform has no body at all.
    fn named_transform_in_direction(
        nt: &crate::config::NamedTransform,
        forward: bool,
    ) -> Option<Transform> {
        if forward {
            nt.forward
                .as_ref()
                .cloned()
                .or_else(|| nt.inverse.as_ref().map(|t| t.clone().inverse()))
        } else {
            nt.inverse
                .as_ref()
                .cloned()
                .or_else(|| nt.forward.as_ref().map(|t| t.clone().inverse()))
        }
    }

    /// Creates a display processor.
    pub fn display_processor(&self, src: &str, display: &str, view: &str) -> OcioResult<Processor> {
        let dvt = DisplayViewTransform {
            src: src.to_string(),
            display: display.to_string(),
            view: view.to_string(),
            ..DisplayViewTransform::default()
        };
        self.processor_for_display_view_transform_internal(None, &dvt)
    }

    /// Creates a display processor using a context.
    pub fn display_processor_with_context(
        &self,
        context: &Context,
        src: &str,
        display: &str,
        view: &str,
    ) -> OcioResult<Processor> {
        let dvt = DisplayViewTransform {
            src: src.to_string(),
            display: display.to_string(),
            view: view.to_string(),
            ..DisplayViewTransform::default()
        };
        self.processor_for_display_view_transform_internal(Some(context), &dvt)
    }

    /// Creates a processor from a fully-configured [`DisplayViewTransform`].
    ///
    /// Mirrors OCIO C++ `Config::getProcessor(ConstDisplayViewTransformRcPtr)`.
    /// Honours `looks_bypass`, `looks_override` / `looks_override_enabled`,
    /// and the `direction` flags on the DVT.
    pub fn processor_for_display_view_transform(
        &self,
        dvt: &DisplayViewTransform,
    ) -> OcioResult<Processor> {
        self.processor_for_display_view_transform_internal(None, dvt)
    }

    /// Same as [`Self::processor_for_display_view_transform`] but with
    /// an explicit OCIO [`Context`].
    pub fn processor_for_display_view_transform_with_context(
        &self,
        context: &Context,
        dvt: &DisplayViewTransform,
    ) -> OcioResult<Processor> {
        self.processor_for_display_view_transform_internal(Some(context), dvt)
    }

    /// The display/view chain of a DisplayViewTransform, forward: source colour space, the
    /// view's looks, the view transform, the display colour space (OCIO `BuildDisplayOps`).
    /// The one builder behind `display_processor*`, `processor_for_display_view_transform*` and
    /// `expand_transform`'s DisplayView arm. Empty when a data colour space is bypassed.
    fn display_view_chain(
        &self,
        ctx: Option<&Context>,
        dvt: &DisplayViewTransform,
    ) -> OcioResult<Vec<Transform>> {
        let src = dvt.src.as_str();
        let display = dvt.display.as_str();
        let view = dvt.view.as_str();

        if self.displays.display(display).is_none() {
            return Err(OcioError::DisplayNotFound {
                name: display.into(),
            });
        }

        let v = self
            .find_view(display, view)
            .ok_or_else(|| OcioError::ViewNotFound {
                display: display.into(),
                view: view.into(),
            })?;

        let mut transforms = Vec::new();

        let src_cs = self
            .colorspace(src)
            .ok_or_else(|| OcioError::ColorSpaceNotFound { name: src.into() })?;
        // src colorspace → its own reference (scene or display).
        if let Some(t) = Self::cs_to_reference(src_cs) {
            transforms.push(t);
        }
        // Bridge to scene reference when src is display-referred —
        // the view transform machinery downstream assumes scene
        // input (its `from_scene_reference` accessor).
        if let Some(t) =
            self.reference_bridge(src_cs.reference_space_type(), ReferenceSpaceType::Scene)?
        {
            transforms.push(t);
        }

        // OCIO C++ `BuildDisplayOps` resolution order:
        // 1. `looks_override_enabled` wins — override replaces the
        //    view's embedded looks entirely.
        // 2. Otherwise, `looks_bypass` suppresses the view's looks.
        // 3. Otherwise, apply the view's embedded looks.
        if dvt.looks_override_enabled {
            if !dvt.looks_override.is_empty() {
                self.append_look_transforms(&mut transforms, &dvt.looks_override, ctx)?;
            }
        } else if !dvt.looks_bypass
            && let Some(looks) = v.looks()
        {
            self.append_look_transforms(&mut transforms, looks, ctx)?;
        }

        // Apply the view transform, if any. Track the reference space
        // the data ends up in so the final dst leg can pick the right
        // bridge / accessor.
        //
        // OCIO v2 semantics:
        //   - Scene-referred VT: `from_scene_reference` takes scene→display.
        //   - Display-referred VT: needs display input; we pre-bridge
        //     scene→display here so its body sees display-referred data.
        //     Its `from_display_reference` body then stays inside the
        //     display reference space.
        // After any VT the data is in display reference.
        let mut current_rst = ReferenceSpaceType::Scene;
        if let Some(vt_name) = v.view_transform() {
            let vt = self.displays.view_transform(vt_name).ok_or_else(|| {
                OcioError::Validation(format!("view transform not found: {}", vt_name))
            })?;
            match vt.reference_space_type() {
                ReferenceSpaceType::Scene => {
                    if let Some(t) = vt
                        .from_scene_reference()
                        .cloned()
                        .or_else(|| vt.to_scene_reference().map(|t| t.clone().inverse()))
                    {
                        transforms.push(t);
                    }
                    current_rst = ReferenceSpaceType::Display;
                }
                ReferenceSpaceType::Display => {
                    if let Some(t) = self
                        .reference_bridge(ReferenceSpaceType::Scene, ReferenceSpaceType::Display)?
                    {
                        transforms.push(t);
                    }
                    if let Some(t) = vt
                        .from_display_reference()
                        .cloned()
                        .or_else(|| vt.to_display_reference().map(|t| t.clone().inverse()))
                    {
                        transforms.push(t);
                    }
                    current_rst = ReferenceSpaceType::Display;
                }
            }
        }

        // Display/view color space — resolve `<USE_DISPLAY_NAME>`
        // semantics via `View::effective_colorspace(display)` so a
        // shared view authored with `display_colorspace:
        // <USE_DISPLAY_NAME>` correctly targets the host display's
        // colorspace.
        let dst_name = v.effective_colorspace(display);
        let dst_cs = self
            .colorspace(dst_name)
            .ok_or_else(|| OcioError::ColorSpaceNotFound {
                name: dst_name.into(),
            })?;

        // OCIO `BuildDisplayOps`: by default a data colour space (source or display) is not
        // processed at all.
        if dvt.data_bypass && (src_cs.is_data() || dst_cs.is_data()) {
            return Ok(Vec::new());
        }

        // Bridge from whatever reference space we're sitting in into
        // the dst colorspace's native reference, then go to the
        // colorspace itself via the correct accessor.
        if let Some(t) = self.reference_bridge(current_rst, dst_cs.reference_space_type())? {
            transforms.push(t);
        }
        if let Some(t) = Self::cs_from_reference(dst_cs) {
            transforms.push(t);
        }

        let mut expanded = Vec::with_capacity(transforms.len());
        for t in &transforms {
            expanded.push(self.expand_with(ctx, t)?);
        }
        Ok(expanded)
    }

    fn processor_for_display_view_transform_internal(
        &self,
        context: Option<&Context>,
        dvt: &DisplayViewTransform,
    ) -> OcioResult<Processor> {
        let transforms = self.display_view_chain(context, dvt)?;
        if transforms.is_empty() {
            return Ok(Processor::new());
        }

        let group = Transform::group(transforms);
        Processor::from_transform_with_context_and_proxy(
            &group,
            dvt.direction,
            OptimizationLevel::default(),
            context.cloned(),
            self.io_proxy.clone(),
        )
    }

    /// Creates a processor with looks applied.
    pub fn processor_with_looks(&self, src: &str, dst: &str, looks: &str) -> OcioResult<Processor> {
        if looks.trim().is_empty() {
            return self.processor(src, dst);
        }

        let src_cs = self
            .colorspace(src)
            .ok_or_else(|| OcioError::ColorSpaceNotFound { name: src.into() })?;
        let dst_cs = self
            .colorspace(dst)
            .ok_or_else(|| OcioError::ColorSpaceNotFound { name: dst.into() })?;

        // Data-bypass short-circuit mirrors the no-op behaviour
        // of OCIO C++ `BuildLookOps` when either endpoint is a
        // data colorspace — looks are skipped and so is the
        // surrounding chain (LookTransform.cpp:297, 359).
        if src_cs.is_data() || dst_cs.is_data() {
            return Ok(Processor::new());
        }

        let mut transforms = Vec::new();

        // Phase 1: src → src reference (correct accessor by RST).
        if let Some(t) = Self::cs_to_reference(src_cs) {
            transforms.push(t);
        }

        // Phase 2: bridge to scene reference if src is display-
        // referred. Looks always run in scene-referred space —
        // their `process_space` resolves against scene colorspaces.
        if let Some(t) =
            self.reference_bridge(src_cs.reference_space_type(), ReferenceSpaceType::Scene)?
        {
            transforms.push(t);
        }

        // Phase 3: apply the look chain (already scene-referred).
        self.append_look_transforms(&mut transforms, looks, None)?;

        // Phase 4: bridge back to dst reference if dst is
        // display-referred. The look chain ended in scene
        // reference space.
        if let Some(t) =
            self.reference_bridge(ReferenceSpaceType::Scene, dst_cs.reference_space_type())?
        {
            transforms.push(t);
        }

        // Phase 5: dst reference → dst (correct accessor by RST).
        if let Some(t) = Self::cs_from_reference(dst_cs) {
            transforms.push(t);
        }

        if transforms.is_empty() {
            return Ok(Processor::new());
        }

        let group = self.expand_with(None, &Transform::group(transforms))?;
        Processor::from_transform_with_context_and_proxy(
            &group,
            TransformDirection::Forward,
            OptimizationLevel::default(),
            None,
            self.io_proxy.clone(),
        )
    }

    /// Expands reference transforms (ColorSpace, Look, DisplayView) to their actual transforms.
    ///
    /// This recursively traverses the transform tree and expands:
    /// - ColorSpaceTransform -> src.to_reference + dst.from_reference
    /// - LookTransform -> look transforms with process space handling  
    /// - DisplayViewTransform -> display/view transform chain
    /// - GroupTransform -> recursively expanded children
    ///
    /// Other transforms are returned as-is.
    pub fn expand_transform(&self, transform: &Transform) -> OcioResult<Transform> {
        self.expand_with(None, transform)
    }

    /// [`Self::expand_transform`] resolving `$VAR`s in file references, and any look files,
    /// against `ctx` instead of the config's own context (OCIO's `getProcessor(context, ..)`).
    fn expand_with(&self, ctx: Option<&Context>, transform: &Transform) -> OcioResult<Transform> {
        match transform {
            Transform::ColorSpace(cst) => {
                let src_cs =
                    self.colorspace(&cst.src)
                        .ok_or_else(|| OcioError::ColorSpaceNotFound {
                            name: cst.src.clone(),
                        })?;
                let dst_cs =
                    self.colorspace(&cst.dst)
                        .ok_or_else(|| OcioError::ColorSpaceNotFound {
                            name: cst.dst.clone(),
                        })?;

                // Inverse direction is equivalent to swapping endpoints
                // — defer to build_colorspace_chain in both cases so
                // equality-group / data-bypass short-circuits and the
                // scene↔display bridge apply uniformly.
                let forward = cst.direction == TransformDirection::Forward;
                let raw_chain = if forward {
                    self.build_colorspace_chain(src_cs, dst_cs, cst.data_bypass)?
                } else {
                    self.build_colorspace_chain(dst_cs, src_cs, cst.data_bypass)?
                };

                let mut transforms = Vec::with_capacity(raw_chain.len());
                for t in raw_chain {
                    transforms.push(self.expand_with(ctx, &t)?);
                }

                Ok(if transforms.is_empty() {
                    Transform::Group(GroupTransform {
                        name: String::new(),
                        transforms: vec![],
                        direction: TransformDirection::Forward,
                    })
                } else {
                    Transform::group(transforms)
                })
            }

            Transform::Look(lt) => {
                // Mirrors OCIO C++ `LookTransform.cpp:338-344`: an
                // inverse-direction LookTransform is materialised by
                // swapping `src`/`dst`, reversing the look list, and
                // flipping each look's forward marker — *not* by
                // wrapping the forward chain in a Group::inverse().
                // The two are mathematically equivalent only when
                // every look transform round-trips perfectly; the
                // OCIO approach honours explicit per-look
                // `inverse_transform` overrides.
                use crate::look::parse_look_options;
                let inverse = lt.direction == TransformDirection::Inverse;
                let (src_name, dst_name) = if inverse {
                    (lt.dst.as_str(), lt.src.as_str())
                } else {
                    (lt.src.as_str(), lt.dst.as_str())
                };
                // Each `|` option is reversed on its own: the precedence between options stays
                // (`LookParseResult::reverse`).
                let options: Vec<Vec<(String, bool)>> = parse_look_options(&lt.looks)
                    .into_iter()
                    .map(|opt| {
                        if inverse {
                            opt.into_iter()
                                .rev()
                                .map(|(name, forward)| (name.to_string(), !forward))
                                .collect()
                        } else {
                            opt.into_iter()
                                .map(|(name, forward)| (name.to_string(), forward))
                                .collect()
                        }
                    })
                    .collect();
                let look_specs = self.pick_look_option(options, ctx)?;
                let mut transforms = Vec::new();

                if !lt.skip_color_space_conversion
                    && let Some(src_cs) = self.colorspace(src_name)
                {
                    if let Some(t) = Self::cs_to_reference(src_cs) {
                        transforms.push(self.expand_with(ctx, &t)?);
                    }
                    if let Some(t) = self.reference_bridge(
                        src_cs.reference_space_type(),
                        ReferenceSpaceType::Scene,
                    )? {
                        transforms.push(self.expand_with(ctx, &t)?);
                    }
                }

                self.append_parsed_look_transforms(&mut transforms, &look_specs)?;

                if !lt.skip_color_space_conversion
                    && let Some(dst_cs) = self.colorspace(dst_name)
                {
                    if let Some(t) = self.reference_bridge(
                        ReferenceSpaceType::Scene,
                        dst_cs.reference_space_type(),
                    )? {
                        transforms.push(self.expand_with(ctx, &t)?);
                    }
                    if let Some(t) = Self::cs_from_reference(dst_cs) {
                        transforms.push(self.expand_with(ctx, &t)?);
                    }
                }

                // Direction is baked into the chain — no final inverse.
                Ok(Transform::group(transforms))
            }

            Transform::DisplayView(dvt) => {
                let group = Transform::group(self.display_view_chain(ctx, dvt)?);
                Ok(if dvt.direction == TransformDirection::Inverse {
                    group.inverse()
                } else {
                    group
                })
            }

            Transform::Group(gt) => {
                // Recursively expand group members
                let expanded: Result<Vec<_>, _> = gt
                    .transforms
                    .iter()
                    .map(|t| self.expand_with(ctx, t))
                    .collect();
                Ok(Transform::Group(GroupTransform {
                    name: String::new(),
                    transforms: expanded?,
                    direction: gt.direction,
                }))
            }

            // A FileTransform's `src` as the config wrote it: resolved against the context
            // (`$VAR`s) and the search paths, as OCIO does when it builds the ops.
            Transform::FileTransform(ft) => {
                let raw = ft.src.to_string_lossy();
                let resolved = ctx.unwrap_or(&self.context).resolve(&raw);
                let path = self
                    .resolve_file(&resolved)
                    .unwrap_or_else(|| self.working_dir.join(&resolved));
                let mut ft = ft.clone();
                ft.src = path;
                Ok(Transform::FileTransform(ft))
            }

            // All other transforms pass through unchanged
            _ => Ok(transform.clone()),
        }
    }

    /// Creates a processor from an arbitrary transform with config context.
    ///
    /// This method expands any ColorSpaceTransform, LookTransform, or DisplayViewTransform
    /// references using the config's color space definitions before compiling.
    pub fn processor_from_transform(
        &self,
        transform: &Transform,
        direction: TransformDirection,
    ) -> OcioResult<Processor> {
        self.processor_from_transform_impl(None, transform, direction, OptimizationLevel::default())
    }

    /// Creates a processor from transform with optimization level.
    pub fn processor_from_transform_with_opts(
        &self,
        transform: &Transform,
        direction: TransformDirection,
        optimization: OptimizationLevel,
    ) -> OcioResult<Processor> {
        self.processor_from_transform_impl(None, transform, direction, optimization)
    }

    /// [`Self::processor_from_transform`] with an explicit OCIO [`Context`]
    /// (`Config::getProcessor(context, transform, direction)`): the context resolves the
    /// `$VAR`s of file references and look files, and travels with the processor.
    pub fn processor_from_transform_with_context(
        &self,
        context: &Context,
        transform: &Transform,
        direction: TransformDirection,
    ) -> OcioResult<Processor> {
        self.processor_from_transform_impl(
            Some(context),
            transform,
            direction,
            OptimizationLevel::default(),
        )
    }

    /// The one expand-and-compile path behind the three `processor_from_transform*` entry points.
    fn processor_from_transform_impl(
        &self,
        context: Option<&Context>,
        transform: &Transform,
        direction: TransformDirection,
        optimization: OptimizationLevel,
    ) -> OcioResult<Processor> {
        let expanded = self.expand_with(context, transform)?;
        Processor::from_transform_with_context_and_proxy(
            &expanded,
            direction,
            optimization,
            context.cloned(),
            self.io_proxy.clone(),
        )
    }

    /// The `LookNotFound` error for `name`, listing the config's looks as OCIO does.
    fn look_not_found(&self, name: &str) -> OcioError {
        OcioError::LookNotFound {
            name: name.into(),
            available: self.looks.names().map(str::to_string).collect(),
        }
    }

    /// `look` and the colour space it processes in (`RunLookTokens`' two lookups and errors).
    fn look_and_space(&self, name: &str) -> OcioResult<(&crate::look::Look, &ColorSpace)> {
        let look = self
            .looks
            .get(name)
            .ok_or_else(|| self.look_not_found(name))?;
        let space = look.get_process_space().unwrap_or_default();
        let cs = self
            .colorspace(space)
            .ok_or_else(|| OcioError::LookProcessSpace {
                look: name.into(),
                space: space.into(),
            })?;
        Ok((look, cs))
    }

    /// A file a transform needs that cannot be located (OCIO `ExceptionMissingFile`): the
    /// first `FileTransform` of `t` whose path, resolved against `ctx` and the search paths,
    /// does not exist and is not served by the IO proxy.
    fn missing_file(&self, t: &Transform, ctx: Option<&Context>) -> Option<String> {
        match t {
            Transform::Group(g) => g.transforms.iter().find_map(|c| self.missing_file(c, ctx)),
            Transform::FileTransform(ft) => {
                let resolved = ctx
                    .unwrap_or(&self.context)
                    .resolve(&ft.src.to_string_lossy());
                let path = self
                    .resolve_file(&resolved)
                    .unwrap_or_else(|| self.working_dir.join(&resolved));
                let proxied = self
                    .io_proxy
                    .as_ref()
                    .is_some_and(|p| p.get_lut_data(&path.to_string_lossy()).is_some());
                (!path.exists() && !proxied).then(|| path.to_string_lossy().into_owned())
            }
            _ => None,
        }
    }

    /// `BuildLookOps`' choice among `|` options: one option is taken as is; with several, the
    /// first whose looks exist and whose files can be located wins. A missing look or process
    /// space is an error at once (only a missing FILE moves on to the next option), and when
    /// every option lacks a file the error lists them all.
    fn pick_look_option(
        &self,
        mut options: Vec<Vec<(String, bool)>>,
        ctx: Option<&Context>,
    ) -> OcioResult<Vec<(String, bool)>> {
        if options.len() <= 1 {
            return Ok(options.pop().unwrap_or_default());
        }
        let mut report = String::new();
        for (i, option) in options.iter().enumerate() {
            let mut missing = None;
            for (name, forward) in option.iter().filter(|(n, _)| !n.is_empty()) {
                let (look, _) = self.look_and_space(name)?;
                // The transform `RunLookTokens` builds for this direction.
                let t = if *forward {
                    look.get_transform().or(look.get_inverse_transform())
                } else {
                    look.get_inverse_transform().or(look.get_transform())
                };
                if let Some(path) = t.and_then(|t| self.missing_file(t, ctx)) {
                    missing = Some(format!(
                        "The specified file reference '{path}' could not be located."
                    ));
                    break;
                }
            }
            match missing {
                None => return Ok(options.swap_remove(i)),
                Some(msg) => {
                    if i != 0 {
                        report.push_str("  ...  ");
                    }
                    let tokens: Vec<String> = option
                        .iter()
                        .map(|(n, f)| if *f { n.clone() } else { format!("-{n}") })
                        .collect();
                    report.push_str(&format!("({}) {msg}", tokens.join(", ")));
                }
            }
        }
        Err(OcioError::MissingFile(report))
    }

    /// The colour space the data is in after `looks` run forward: the process space of the
    /// last look run (OCIO `LookTransform::GetLooksResultColorSpace`). `None` for an empty
    /// look string. Takes `|` fall-back options as OCIO does, so a look file missing from
    /// `context` selects the next option. Unlike OCIO the look transforms are not compiled
    /// here: only the looks, their process spaces and their files are checked.
    ///
    /// # Errors
    /// A look that is not defined or whose process space is not a colour space
    /// ([`OcioError::LookNotFound`], [`OcioError::LookProcessSpace`]); every option missing a
    /// file ([`OcioError::MissingFile`]).
    pub fn looks_result_space(&self, looks: &str, context: &Context) -> OcioResult<Option<&str>> {
        let options = crate::look::parse_look_options(looks)
            .into_iter()
            .map(|o| o.into_iter().map(|(n, f)| (n.to_string(), f)).collect())
            .collect();
        let mut result = None;
        for (name, _) in self
            .pick_look_option(options, Some(context))?
            .iter()
            .filter(|(n, _)| !n.is_empty())
        {
            result = Some(self.look_and_space(name)?.1.name());
        }
        Ok(result)
    }
    // ========================================================================
    // Cross-config processor methods (OCIO 2.2+ parity)
    // ========================================================================

    /// Creates a processor between color spaces in two different configs.
    ///
    /// Uses interchange roles (aces_interchange for scene-referred,
    /// cie_xyz_d65_interchange for display-referred) to bridge between configs.
    ///
    /// # Arguments
    /// * `src_colorspace` - Source color space name in this config
    /// * `dst_config` - Destination config
    /// * `dst_colorspace` - Destination color space name in dst_config
    ///
    /// # Example
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use vfx_ocio::{Config, builtin};
    /// let aces_config = builtin::default_config();
    /// let studio_config = Config::from_file("studio.ocio")?;
    /// let proc = aces_config.processor_from_configs("ACEScg", &studio_config, "Log")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn processor_from_configs(
        &self,
        src_colorspace: &str,
        dst_config: &Config,
        dst_colorspace: &str,
    ) -> OcioResult<Processor> {
        self.processor_from_configs_with_opts(
            src_colorspace,
            dst_config,
            dst_colorspace,
            OptimizationLevel::default(),
        )
    }

    /// Creates a processor between configs with explicit interchange spaces.
    ///
    /// The interchange names must refer to color spaces that are equivalent
    /// between the two configs (e.g., both ACES2065-1 or both CIE XYZ D65).
    ///
    /// # Arguments
    /// * `src_colorspace` - Source color space in this config
    /// * `src_interchange` - Interchange color space in this config
    /// * `dst_config` - Destination config
    /// * `dst_colorspace` - Destination color space in dst_config
    /// * `dst_interchange` - Interchange color space in dst_config
    pub fn processor_from_configs_with_interchange(
        &self,
        src_colorspace: &str,
        src_interchange: &str,
        dst_config: &Config,
        dst_colorspace: &str,
        dst_interchange: &str,
    ) -> OcioResult<Processor> {
        // Mirrors OCIO C++ `Config::GetProcessorFromConfigs`
        // (Config.cpp:4942-4998): all four colorspaces must exist
        // before any chain is built, and when *either* endpoint is
        // a data colorspace the entire concatenated processor is
        // an explicit no-op — matching how data colorspaces are
        // treated in single-config processors.
        let src_cs =
            self.colorspace(src_colorspace)
                .ok_or_else(|| OcioError::ColorSpaceNotFound {
                    name: src_colorspace.into(),
                })?;
        self.colorspace(src_interchange)
            .ok_or_else(|| OcioError::ColorSpaceNotFound {
                name: src_interchange.into(),
            })?;
        let dst_cs =
            dst_config
                .colorspace(dst_colorspace)
                .ok_or_else(|| OcioError::ColorSpaceNotFound {
                    name: dst_colorspace.into(),
                })?;
        dst_config
            .colorspace(dst_interchange)
            .ok_or_else(|| OcioError::ColorSpaceNotFound {
                name: dst_interchange.into(),
            })?;

        if src_cs.is_data() || dst_cs.is_data() {
            return Ok(Processor::new());
        }

        let to_interchange = self.processor(src_colorspace, src_interchange)?;
        let from_interchange = dst_config.processor(dst_interchange, dst_colorspace)?;

        Processor::combine(&to_interchange, &from_interchange)
    }

    /// Creates a processor between configs with optimization level.
    pub fn processor_from_configs_with_opts(
        &self,
        src_colorspace: &str,
        dst_config: &Config,
        dst_colorspace: &str,
        optimization: OptimizationLevel,
    ) -> OcioResult<Processor> {
        // Resolve both endpoints first so we can apply the
        // OCIO C++ data-colorspace short-circuit (Config.cpp:4994-4996)
        // before any chain is built.
        let src_cs =
            self.colorspace(src_colorspace)
                .ok_or_else(|| OcioError::ColorSpaceNotFound {
                    name: src_colorspace.into(),
                })?;
        let dst_cs =
            dst_config
                .colorspace(dst_colorspace)
                .ok_or_else(|| OcioError::ColorSpaceNotFound {
                    name: dst_colorspace.into(),
                })?;
        if src_cs.is_data() || dst_cs.is_data() {
            return Ok(Processor::new());
        }

        let (src_int, dst_int) = self.identify_interchange_space(dst_config)?;
        let to_int = self.processor_with_opts(src_colorspace, &src_int, optimization)?;
        let from_int = dst_config.processor_with_opts(&dst_int, dst_colorspace, optimization)?;

        Processor::combine(&to_int, &from_int)
    }

    /// Creates a processor from a color space to a display/view in another config.
    ///
    /// # Arguments
    /// * `src_colorspace` - Source color space in this config
    /// * `dst_config` - Destination config
    /// * `dst_display` - Display name in dst_config
    /// * `dst_view` - View name in dst_config
    /// * `direction` - Transform direction
    pub fn processor_from_configs_to_display(
        &self,
        src_colorspace: &str,
        dst_config: &Config,
        dst_display: &str,
        dst_view: &str,
        direction: TransformDirection,
    ) -> OcioResult<Processor> {
        // Resolve src colorspace up front for the data short-circuit
        // (mirrors OCIO C++ `GetProcessorFromConfigs` to-display
        // variant at Config.cpp:5012-5076).
        let src_cs =
            self.colorspace(src_colorspace)
                .ok_or_else(|| OcioError::ColorSpaceNotFound {
                    name: src_colorspace.into(),
                })?;
        if src_cs.is_data() {
            return Ok(Processor::new());
        }

        let (src_int, dst_int) = self.identify_interchange_space(dst_config)?;
        let to_int = self.processor(src_colorspace, &src_int)?;
        let display_proc = dst_config.display_processor(&dst_int, dst_display, dst_view)?;

        let combined = Processor::combine(&to_int, &display_proc)?;
        if direction == TransformDirection::Inverse {
            combined.inverse()
        } else {
            Ok(combined)
        }
    }

    /// Identifies interchange color spaces between two configs.
    ///
    /// Returns a pair of (src_interchange, dst_interchange) names that can be
    /// used to bridge conversions between the configs.
    ///
    /// Precedence:
    /// 1. aces_interchange role (ACES2065-1)
    /// 2. cie_xyz_d65_interchange role
    /// 3. Heuristic matching of known color space names
    pub fn identify_interchange_space(&self, other: &Config) -> OcioResult<(String, String)> {
        use crate::role::names;

        // Try aces_interchange role first (most common)
        if let (Some(src), Some(dst)) = (
            self.role_colorspace(names::ACES_INTERCHANGE),
            other.role_colorspace(names::ACES_INTERCHANGE),
        ) {
            return Ok((src.to_string(), dst.to_string()));
        }

        // Try cie_xyz_d65_interchange for display-referred
        if let (Some(src), Some(dst)) = (
            self.role_colorspace(names::CIE_XYZ_D65_INTERCHANGE),
            other.role_colorspace(names::CIE_XYZ_D65_INTERCHANGE),
        ) {
            return Ok((src.to_string(), dst.to_string()));
        }

        // Heuristic: try known interchange space names
        let known_names = [
            "ACES2065-1",
            "ACES - ACES2065-1",
            "Linear - AP0",
            "AP0",
            "CIE-XYZ-D65",
            "CIE XYZ D65",
            "XYZ",
        ];

        for name in known_names {
            if self.colorspace(name).is_some() && other.colorspace(name).is_some() {
                return Ok((name.to_string(), name.to_string()));
            }
        }

        // Try scene_linear as last resort
        if let (Some(src), Some(dst)) = (
            self.role_colorspace(names::SCENE_LINEAR),
            other.role_colorspace(names::SCENE_LINEAR),
        ) {
            return Ok((src.to_string(), dst.to_string()));
        }

        Err(OcioError::InvalidTransform {
            reason: "No compatible interchange space found between configs. \
                      Define aces_interchange or cie_xyz_d65_interchange roles."
                .into(),
        })
    }

    /// Identifies a built-in color space that matches a given colorspace.
    ///
    /// Compares the color space's transforms to known built-in color spaces.
    pub fn identify_builtin_colorspace(&self, colorspace_name: &str) -> Option<&'static str> {
        let cs = self.colorspace(colorspace_name)?;

        // Check encoding first for quick classification
        match cs.encoding() {
            Encoding::SceneLinear => {
                // Check if it's an ACES linear space
                if colorspace_name.contains("ACEScg") || colorspace_name.contains("AP1") {
                    return Some("ACEScg");
                }
                if colorspace_name.contains("ACES2065") || colorspace_name.contains("AP0") {
                    return Some("ACES2065-1");
                }
            }
            Encoding::Sdr => {
                if colorspace_name.contains("sRGB") {
                    return Some("sRGB");
                }
                if colorspace_name.contains("Rec.709") || colorspace_name.contains("Rec709") {
                    return Some("Rec.709");
                }
            }
            Encoding::Log => {
                if colorspace_name.contains("ACEScct") {
                    return Some("ACEScct");
                }
                if colorspace_name.contains("LogC") {
                    return Some("ARRI LogC4");
                }
            }
            _ => {}
        }

        None
    }

    fn append_look_transforms(
        &self,
        transforms: &mut Vec<Transform>,
        looks: &str,
        ctx: Option<&Context>,
    ) -> OcioResult<()> {
        let options = crate::look::parse_look_options(looks)
            .into_iter()
            .map(|o| o.into_iter().map(|(n, f)| (n.to_string(), f)).collect())
            .collect();
        let owned = self.pick_look_option(options, ctx)?;
        self.append_parsed_look_transforms(transforms, &owned)
    }

    /// Walks an already-parsed look spec list, appending the
    /// resulting transforms to `transforms`. Separated from
    /// [`Self::append_look_transforms`] so callers that need to
    /// re-order or flip individual entries (e.g. inverse-direction
    /// `LookTransform`, which swaps `src`/`dst` and reverses the
    /// list per OCIO C++ `LookTransform.cpp:338-344`) can do so
    /// without re-parsing.
    fn append_parsed_look_transforms(
        &self,
        transforms: &mut Vec<Transform>,
        look_specs: &[(String, bool)],
    ) -> OcioResult<()> {
        if look_specs.is_empty() {
            return Ok(());
        }

        // Mirrors OCIO C++ `RunLookTokens` (LookTransform.cpp:203). The
        // chain enters in scene reference space; we track the active
        // colorspace as we walk the look list so consecutive looks
        // sharing the same `process_space` collapse to a single
        // entry conversion. At the end we hop back to scene
        // reference so the caller's downstream ops see the data in
        // the expected space.
        let mut current_cs: Option<&ColorSpace> = None; // None = scene reference

        for (look_name, forward) in look_specs.iter() {
            let look_name = look_name.as_str();
            let forward = *forward;
            let look = self
                .looks
                .get(look_name)
                .ok_or_else(|| self.look_not_found(look_name))?;

            // Resolve the look's process space (if any). When unset,
            // the look operates directly in the current space — OCIO
            // C++ treats this as "no conversion required".
            let process_space = match look.get_process_space() {
                Some(ps_name) => self.colorspace(ps_name),
                None => None,
            };

            // Convert current -> process_space if they differ.
            // current=None means scene reference space; comparing
            // process spaces by identity matches OCIO's
            // `cs.get() != ps.get()` pointer check.
            if let Some(ps) = process_space {
                let same = match current_cs {
                    Some(cur) => std::ptr::eq(cur, ps),
                    None => false,
                };
                if !same {
                    // current → scene reference: current_cs (if any)
                    // back to its native reference, then bridge to
                    // scene if it was display-referred. Mirrors
                    // OCIO C++ `BuildColorSpaceOps` from the look
                    // chain's perspective.
                    if let Some(cur) = current_cs {
                        if let Some(t) = Self::cs_to_reference(cur) {
                            transforms.push(t);
                        }
                        if let Some(t) = self.reference_bridge(
                            cur.reference_space_type(),
                            ReferenceSpaceType::Scene,
                        )? {
                            transforms.push(t);
                        }
                    }
                    // scene reference → process_space: bridge if ps
                    // is display-referred (looks usually live in
                    // scene-linear, but be correct if not).
                    if let Some(t) =
                        self.reference_bridge(ReferenceSpaceType::Scene, ps.reference_space_type())?
                    {
                        transforms.push(t);
                    }
                    if let Some(t) = Self::cs_from_reference(ps) {
                        transforms.push(t);
                    }
                    current_cs = Some(ps);
                }
            }

            // Apply look transform. Mirrors OCIO C++ `RunLookTokens`'s own
            // four-way switch exactly (`LookTransform.cpp:243-269`):
            //
            //   forward, look has forward transform      -> use it AS-IS
            //   forward, only inverse_transform exists    -> invert it
            //   inverse, look has inverse_transform        -> use it AS-IS
            //   inverse, only forward transform exists    -> invert it
            //
            // The bug this replaces always inverted `get_inverse_transform()`
            // when `forward == false` — but an explicit `inverse_transform`
            // already IS the "run backward" transform (ref's own case,
            // `BuildOps(..., look->getInverseTransform(), TRANSFORM_DIR_FORWARD)`
            // — no `TRANSFORM_DIR_INVERSE`), so wrapping it in `Inverse` again
            // ran it forward instead of backward. It also silently dropped a
            // look that defines only `inverse_transform` when `forward ==
            // true`, instead of inverting that transform to produce the
            // correct forward result (ref: `else if (look->getInverseTransform())
            // { ...TRANSFORM_DIR_INVERSE }`).
            let (look_transform, invert_it) = if forward {
                match look.get_transform() {
                    Some(t) => (Some(t), false),
                    None => (look.get_inverse_transform(), true),
                }
            } else {
                match look.get_inverse_transform() {
                    Some(t) => (Some(t), false),
                    None => (look.get_transform(), true),
                }
            };
            if let Some(t) = look_transform {
                if invert_it {
                    transforms.push(Transform::Group(GroupTransform {
                        name: String::new(),
                        transforms: vec![t.clone()],
                        direction: TransformDirection::Inverse,
                    }));
                } else {
                    transforms.push(t.clone());
                }
            }
        }

        // Return to scene reference so the surrounding pipeline,
        // which expects scene-referred data on the way out, picks up
        // a consistent baseline regardless of which process_space
        // the last look ran in.
        if let Some(cur) = current_cs {
            if let Some(t) = Self::cs_to_reference(cur) {
                transforms.push(t);
            }
            if let Some(t) =
                self.reference_bridge(cur.reference_space_type(), ReferenceSpaceType::Scene)?
            {
                transforms.push(t);
            }
        }

        Ok(())
    }
}
