//! Unified accessor for colorspace / view names used by the test
//! suite.
//!
//! Tests historically hard-coded `"sRGB"`, `"Linear sRGB"`,
//! `"Rec.709"` because the dormant programmatic ACES 1.3 config
//! used those exact names. The official embedded
//! `OpenColorIO-Config-ACES` release renames them
//! (`"sRGB - Display"`, `"Linear Rec.709 (sRGB)"`,
//! `"Rec.1886 Rec.709 - Display"`, …). [`TestSchema`] resolves
//! the canonical names once from the loaded config and lets tests
//! reference fields instead of string literals — swap configs by
//! changing one function rather than ~200 call sites.
//!
//! ```no_run
//! use vfx_ocio::builtin::{self, TestSchema};
//!
//! let cfg    = builtin::aces_2_0_studio_all_views();
//! let schema = TestSchema::from_config(&cfg);
//! let proc   = cfg.processor(&schema.working_space, &schema.display_encoded_srgb).unwrap();
//! ```
//!
//! Each slot is resolved via [`Config::canonical_name`], which
//! handles role indirection (`scene_linear`, `aces_interchange`, …)
//! and alias / canonical lookup uniformly. A missing slot panics
//! with a clear message — TestSchema is a lens onto a real
//! production config, not a compatibility shim. Tests that build
//! synthetic minimal fixtures keep their own literal strings.

use crate::Config;
use std::sync::LazyLock;

/// Canonical colorspace / display / view names extracted from a
/// concrete OCIO config so tests can reference them by field
/// instead of string literal.
#[derive(Debug, Clone)]
pub struct TestSchema {
    /// Scene-linear working space — usually `ACEScg`.
    pub working_space: String,
    /// ACES interchange space — usually `ACES2065-1`.
    pub interchange: String,
    /// Compositing / color-timing log space — usually `ACEScct`.
    pub log_working_space: String,
    /// Display-encoded sRGB output colorspace.
    pub display_encoded_srgb: String,
    /// Display-encoded Rec.709 (BT.1886) output colorspace.
    pub display_encoded_rec709: String,
    /// Scene-linear Rec.709 (sRGB primaries) colorspace.
    pub linear_rec709: String,
    /// Texture-side sRGB-encoded Rec.709 (the `texture_paint` role
    /// target).
    pub texture_srgb: String,
    /// Default display.
    pub default_display: String,
    /// Default view for [`default_display`](Self::default_display): the
    /// first active view (in the ACES configs the ACES output view, as
    /// `active_views` lists it first).
    pub default_view: String,
    /// First view of [`default_display`](Self::default_display)
    /// that carries a non-empty `view_transform` — the canonical
    /// tone-mapped output view for the active display (typically
    /// `"ACES 2.0 - SDR 100 nits (Rec.709)"` in the embedded v4
    /// ACES config). Tests exercising the ACES output path should
    /// use this against `display_processor()`.
    pub tone_mapped_view: String,
    /// Raw passthrough colorspace.
    pub raw: String,
    /// Data colorspace (the `data` role target).
    pub data: String,
}

impl TestSchema {
    /// Resolves the schema against the config the unit tests use,
    /// [`builtin::aces_2_0_studio_all_views`](super::aces_2_0_studio_all_views).
    pub fn from_test_config() -> Self {
        Self::from_config(&super::aces_2_0_studio_all_views())
    }

    /// Returns a process-wide cached schema resolved against the
    /// test config (the all-views config). The schema is built on first call
    /// and reused — tests reference fields directly without
    /// rebuilding the config every time.
    pub fn shared() -> &'static Self {
        static SHARED: LazyLock<TestSchema> = LazyLock::new(TestSchema::from_test_config);
        &SHARED
    }

    /// Resolves the schema against an arbitrary config. Panics if
    /// any slot fails to resolve — the schema is meant for real
    /// production configs (ACES embedded releases), not synthetic
    /// test fixtures.
    pub fn from_config(cfg: &Config) -> Self {
        let resolve = |name: &str| -> String {
            let canon = cfg.canonical_name(name);
            assert!(
                !canon.is_empty(),
                "TestSchema: config has no entry resolving \"{name}\""
            );
            canon.to_string()
        };

        let default_display = cfg
            .displays()
            .default_display()
            .expect("TestSchema: config has no default display")
            .to_string();

        let default_view = cfg
            .default_view(&default_display)
            .unwrap_or_else(|| {
                panic!("TestSchema: display \"{default_display}\" has no default view")
            })
            .to_string();

        let tone_mapped_view = cfg
            .get_views(&default_display)
            .into_iter()
            .find(|v| v.view_transform().is_some())
            .map(|v| v.name().to_string())
            .unwrap_or_else(|| {
                panic!(
                    "TestSchema: display \"{default_display}\" exposes no view \
                     with a view_transform — cannot test ACES output path"
                )
            });

        Self {
            working_space: resolve("scene_linear"),
            interchange: resolve("aces_interchange"),
            log_working_space: resolve("color_timing"),
            display_encoded_srgb: resolve("sRGB - Display"),
            display_encoded_rec709: resolve("Rec.1886 Rec.709 - Display"),
            linear_rec709: resolve("Linear Rec.709 (sRGB)"),
            texture_srgb: resolve("texture_paint"),
            default_display,
            default_view,
            tone_mapped_view,
            raw: resolve("Raw"),
            data: resolve("data"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processor_bridges_scene_to_display_reference() {
        // After the ColorSpaceTransform.cpp parity fix the plain
        // `Config::processor` applies the default scene-to-display
        // view transform when crossing reference spaces, producing
        // tone-mapped output instead of an identity pass-through.
        // The exact op count varies with the optimizer's collapse
        // behaviour — assert on observable output and on the chain
        // being non-empty.
        let cfg = crate::builtin::aces_2_0_studio_all_views();
        let s = TestSchema::shared();
        let proc = cfg
            .processor(&s.working_space, &s.display_encoded_srgb)
            .unwrap();

        assert!(
            !proc.ops().is_empty(),
            "scene→display crossing must not collapse to a no-op"
        );

        let inputs = [[0.18_f32, 0.18, 0.18], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]];
        let mut pixels = inputs;
        proc.apply_rgb(&mut pixels);

        // Identity would leave 0.18 untouched — fail loudly if so.
        assert!(
            (pixels[0][0] - 0.18).abs() > 0.05,
            "ACEScg 0.18 grey landed identical to input → bridge missing, got {:?}",
            pixels[0]
        );
        // Black stays black through tone mapping.
        assert!(pixels[1][0].abs() < 0.01);
        // 1.0 ACEScg sits inside the SDR highlight roll-off and
        // lands well above mid-grey.
        assert!(pixels[2][0] > 0.6);
    }

    #[test]
    fn processor_round_trips_scene_display_reference() {
        // Forward then inverse through the bridged path should
        // restore the original value within a small tolerance —
        // the chain composes both legs symmetrically.
        let cfg = crate::builtin::aces_2_0_studio_all_views();
        let s = TestSchema::shared();
        let fwd = cfg
            .processor(&s.working_space, &s.display_encoded_srgb)
            .unwrap();
        let inv = cfg
            .processor(&s.display_encoded_srgb, &s.working_space)
            .unwrap();

        let orig = [[0.18_f32, 0.3, 0.5]];
        let mut px = orig;
        fwd.apply_rgb(&mut px);
        inv.apply_rgb(&mut px);

        for ch in 0..3 {
            assert!(
                (px[0][ch] - orig[0][ch]).abs() < 0.02,
                "round-trip drift on channel {}: {:?} -> {:?}",
                ch,
                orig[0],
                px[0]
            );
        }
    }

    #[test]
    fn processor_falls_back_to_named_transform() {
        // OCIO C++ ColorSpaceTransform.cpp:185-200: when a name in
        // processor(src, dst) doesn't resolve to a colorspace, fall
        // back to the named-transform table. The four (CS, CS),
        // (NT, CS), (CS, NT), (NT, NT) cases dispatch to different
        // chain shapes. Validate the three NT-involving cases here
        // against a synthetic config.
        use crate::{
            Config,
            colorspace::{ColorSpace, Encoding},
            config::NamedTransform,
            transform::{MatrixTransform, Transform, TransformDirection},
        };

        let mut config = Config::new();
        config.add_colorspace(
            ColorSpace::builder("Linear")
                .encoding(Encoding::SceneLinear)
                .build(),
        );

        let offset_a = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.1, 0.2, 0.3, 0.0],
            direction: TransformDirection::Forward,
        });
        let offset_b = Transform::Matrix(MatrixTransform {
            name: String::new(),
            matrix: MatrixTransform::IDENTITY,
            offset: [0.05, 0.05, 0.05, 0.0],
            direction: TransformDirection::Forward,
        });

        let nt_a = NamedTransform {
            name: "NT_A".into(),
            forward: Some(offset_a.clone()),
            ..Default::default()
        };

        let nt_b = NamedTransform {
            name: "NT_B".into(),
            forward: Some(offset_b.clone()),
            ..Default::default()
        };

        config.add_named_transform(nt_a);
        config.add_named_transform(nt_b);

        // (NT, CS): srcNT.forward, dst ignored.
        let p_nt_cs = config.processor("NT_A", "Linear").unwrap();
        let mut px = [[0.5_f32, 0.5, 0.5]];
        p_nt_cs.apply_rgb(&mut px);
        for (i, expected) in [0.6, 0.7, 0.8].iter().enumerate() {
            assert!(
                (px[0][i] - expected).abs() < 1e-5,
                "NT→CS chain: channel {} expected {} got {:?}",
                i,
                expected,
                px[0]
            );
        }

        // (CS, NT): dstNT.inverse — offset is subtracted.
        let p_cs_nt = config.processor("Linear", "NT_A").unwrap();
        let mut px = [[0.6_f32, 0.7, 0.8]];
        p_cs_nt.apply_rgb(&mut px);
        for (i, expected) in [0.5, 0.5, 0.5].iter().enumerate() {
            assert!(
                (px[0][i] - expected).abs() < 1e-5,
                "CS→NT chain: channel {} expected {} got {:?}",
                i,
                expected,
                px[0]
            );
        }

        // (NT, NT): NT_A.forward + NT_B.inverse.
        let p_nt_nt = config.processor("NT_A", "NT_B").unwrap();
        assert!(
            !p_nt_nt.ops().is_empty(),
            "NT→NT chain should combine both named transforms"
        );
    }

    #[test]
    fn default_schema_matches_embedded_v4() {
        let s = TestSchema::from_test_config();
        assert_eq!(s.working_space, "ACEScg");
        assert_eq!(s.interchange, "ACES2065-1");
        assert_eq!(s.log_working_space, "ACEScct");
        assert_eq!(s.display_encoded_srgb, "sRGB - Display");
        assert_eq!(s.display_encoded_rec709, "Rec.1886 Rec.709 - Display");
        assert_eq!(s.linear_rec709, "Linear Rec.709 (sRGB)");
        assert_eq!(s.texture_srgb, "sRGB Encoded Rec.709 (sRGB)");
        assert_eq!(s.default_display, "sRGB - Display");
        // OCIO orders views by `active_views` (PyOCIO 2.5.1 `getDefaultView("sRGB - Display")`).
        assert_eq!(s.default_view, "ACES 2.0 - SDR 100 nits (Rec.709)");
        assert_eq!(s.tone_mapped_view, "ACES 2.0 - SDR 100 nits (Rec.709)");
        assert_eq!(s.raw, "Raw");
        assert_eq!(s.data, "Raw");
    }
}
