//! OCIO's `BuiltinTransformRegistry`: every `BuiltinTransform` style, by its exact OCIO name,
//! as the list of ops OCIO creates for it.
//!
//! Layout mirrors the reference's `transforms/builtins/` directory: [`color_matrix`]
//! (`ColorMatrixHelpers.cpp`), [`op_helpers`] (`OpHelpers.cpp` and the `Create*Op` helpers),
//! [`aces`] (`ACES.cpp`), [`cameras`] (the six vendor files) and [`displays`] (`Displays.cpp`).
//! A builtin is expanded into ordinary [`Transform`]s and compiled through the same path as the
//! equivalent transforms in a config (`processor/compile.rs`), so there is no second
//! implementation of any op here.
//!
//! # Style lookup
//!
//! Exactly OCIO's rule (`BuiltinTransformImpl::setStyle`, `BuiltinTransform.cpp:63-77`): a
//! style matches a registered name case-insensitively and otherwise verbatim. There are no
//! aliases. The previous lookup stripped `-`, `_` and spaces and accepted invented names
//! (`logc3toaces`, `arrialexa`, `sRGB_to_Linear`, ...), so configs written against this crate
//! did not load in OCIO and vice versa.

mod aces;
mod cameras;
pub(crate) mod color_matrix;
mod displays;
mod op_helpers;

use crate::transform::Transform;
use std::sync::LazyLock;

type BuildOps = Box<dyn Fn(&mut Vec<Transform>) + Send + Sync>;

/// One registered builtin (OCIO `BuiltinTransformRegistryImpl::BuiltinData`).
struct Builtin {
    style: String,
    description: &'static str,
    build: BuildOps,
}

/// The registry, in OCIO's registration order (`BuiltinTransformRegistryImpl::registerAll`).
pub(crate) struct Registry {
    builtins: Vec<Builtin>,
}

impl Registry {
    fn add(
        &mut self,
        style: &'static str,
        description: &'static str,
        build: impl Fn(&mut Vec<Transform>) + Send + Sync + 'static,
    ) {
        self.add_owned(style.to_string(), description, build);
    }

    fn add_owned(
        &mut self,
        style: String,
        description: &'static str,
        build: impl Fn(&mut Vec<Transform>) + Send + Sync + 'static,
    ) {
        debug_assert!(
            !self
                .builtins
                .iter()
                .any(|b| b.style.eq_ignore_ascii_case(&style)),
            "duplicate builtin style {style}"
        );
        self.builtins.push(Builtin {
            style,
            description,
            build: Box::new(build),
        });
    }

    fn find(&self, style: &str) -> Option<&Builtin> {
        // Platform::Strcasecmp: ASCII case-insensitive, otherwise exact.
        self.builtins
            .iter()
            .find(|b| b.style.eq_ignore_ascii_case(style))
    }
}

static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    let mut r = Registry {
        builtins: Vec::new(),
    };
    // BuiltinTransformRegistry.cpp:100-123.
    r.add("IDENTITY", "", |_ops| {});
    aces::register_all(&mut r);
    cameras::register_all(&mut r);
    displays::register_all(&mut r);
    r
});

/// The ops of builtin `style`, in forward order, or `None` if OCIO has no such style.
pub fn get_builtin(style: &str) -> Option<Vec<Transform>> {
    REGISTRY.find(style).map(|b| {
        let mut ops = Vec::new();
        (b.build)(&mut ops);
        ops
    })
}

/// Every registered style, in OCIO's registry order (`BuiltinTransformRegistry::getBuiltinStyle`).
pub fn builtin_styles() -> impl Iterator<Item = &'static str> {
    REGISTRY.builtins.iter().map(|b| b.style.as_str())
}

/// The description of builtin `style` (`BuiltinTransformRegistry::getBuiltinDescription`).
pub fn builtin_description(style: &str) -> Option<&'static str> {
    REGISTRY.find(style).map(|b| b.description)
}

/// OCIO's error for a style that is not registered (`BuiltinTransform.cpp:74-76`).
pub fn invalid_style_message(style: &str) -> String {
    format!("BuiltinTransform: invalid built-in transform style '{style}'.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuiltinTransform, Processor, TransformDirection};

    fn run(style: &str, px: [f32; 3]) -> [f32; 3] {
        let t = Transform::Builtin(BuiltinTransform {
            style: style.to_string(),
            direction: TransformDirection::Forward,
        });
        let p = Processor::from_transform(&t, TransformDirection::Forward)
            .unwrap_or_else(|e| panic!("{style}: {e}"));
        let mut v = [px];
        p.apply_rgb(&mut v);
        v[0]
    }

    /// The registry holds OCIO 2.5's 98 styles, in its order. The list is the `style` column
    /// of `tests/data/ocio_builtins_pyocio.json` (PyOpenColorIO 2.5.1 `BuiltinTransformRegistry`).
    #[test]
    fn registry_is_ocio_2_5_in_order() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/data/ocio_builtins_pyocio.json"))
                .unwrap();
        let want: Vec<&str> = data["builtins"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["style"].as_str().unwrap())
            .collect();
        let got: Vec<&str> = builtin_styles().collect();
        assert_eq!(got, want);
    }

    /// OCIO matches styles case-insensitively (`Platform::Strcasecmp`) and nothing else.
    #[test]
    fn lookup_is_case_insensitive_and_otherwise_exact() {
        assert!(get_builtin("ACEScg_to_ACES2065-1").is_some());
        assert!(get_builtin("acescg_to_aces2065-1").is_some());
        assert!(get_builtin("ACESCG_TO_ACES2065-1").is_some());
        // Separator-stripped / renamed spellings the old lookup accepted: not OCIO styles.
        for alias in [
            "acescgtoaces20651",
            "ACEScg to ACES2065-1",
            "ACEScg_to_ACES2065_1",
            "ARRI_LogC3_to_ACES2065-1",
            "logc3toaces",
            "arrialexa",
            "ACES-AP0_to_AP1",
            "sRGB_to_Linear",
            "DISPLAY_CIE-XYZ-D65_to_sRGB",
        ] {
            assert!(
                get_builtin(alias).is_none(),
                "{alias:?} is not an OCIO builtin style"
            );
        }
    }

    /// A non-OCIO style fails to compile with OCIO's own message (PyOpenColorIO 2.5.1:
    /// `BuiltinTransform().setStyle("logc3toaces")` raises "BuiltinTransform: invalid built-in
    /// transform style 'logc3toaces'.").
    #[test]
    fn non_ocio_alias_is_rejected_like_ocio() {
        let t = Transform::Builtin(BuiltinTransform {
            style: "logc3toaces".to_string(),
            direction: TransformDirection::Forward,
        });
        let err = Processor::from_transform(&t, TransformDirection::Forward).unwrap_err();
        assert!(
            err.to_string()
                .contains("BuiltinTransform: invalid built-in transform style 'logc3toaces'."),
            "{err}"
        );
    }

    #[test]
    fn identity_has_no_ops() {
        assert!(get_builtin("IDENTITY").unwrap().is_empty());
        assert_eq!(builtin_description("IDENTITY"), Some(""));
    }

    /// Every builtin compiles and inverts, except where an op has no inverse here (the
    /// half-domain Lut1D's, whose OCIO inverse is not ported; see `compile_lut1d_domain`).
    #[test]
    fn every_builtin_compiles_forward_and_inverse() {
        for style in builtin_styles() {
            for dir in [TransformDirection::Forward, TransformDirection::Inverse] {
                let t = Transform::Builtin(BuiltinTransform {
                    style: style.to_string(),
                    direction: dir,
                });
                let r = Processor::from_transform(&t, TransformDirection::Forward);
                let half_lut = style.starts_with("ADX") || style.contains("sim-DCI");
                match r {
                    Ok(_) => {}
                    Err(e) if dir == TransformDirection::Inverse && half_lut => {
                        assert!(e.to_string().contains("half-domain"), "{style}: {e}");
                    }
                    Err(e) => panic!("{style} {dir:?}: {e}"),
                }
            }
        }
    }

    /// Round trip through the inverse for curves and matrices that OCIO inverts exactly.
    #[test]
    fn inverse_round_trips() {
        for style in [
            "ACEScct_to_ACES2065-1",
            "ARRI_LOGC4_to_ACES2065-1",
            "RED_REDLOGFILM-RWG_to_ACES2065-1",
            "CURVE - LINEAR_to_ST-2084",
            "CURVE - HLG-OETF",
            "DISPLAY - CIE-XYZ-D65_to_REC.2100-HLG-1000nit",
            "DISPLAY - CIE-XYZ-D65_to_sRGB",
            "APPLE_LOG_to_ACES2065-1",
        ] {
            let fwd = Transform::Builtin(BuiltinTransform {
                style: style.into(),
                direction: TransformDirection::Forward,
            });
            let inv = Transform::Builtin(BuiltinTransform {
                style: style.into(),
                direction: TransformDirection::Inverse,
            });
            let pf = Processor::from_transform(&fwd, TransformDirection::Forward).unwrap();
            let pi = Processor::from_transform(&inv, TransformDirection::Forward).unwrap();
            let src = [[0.4_f32, 0.3, 0.2]];
            let mut v = src;
            pf.apply_rgb(&mut v);
            pi.apply_rgb(&mut v);
            for c in 0..3 {
                assert!(
                    (v[0][c] - src[0][c]).abs() < 1e-4,
                    "{style}: {:?} -> {:?}",
                    src[0],
                    v[0]
                );
            }
        }
    }

    /// Values from OCIO's own table, `tests/cpu/transforms/BuiltinTransform_tests.cpp`
    /// (ACEScct :389-391, SDR-CINEMA_1.0 :416-418, ALEXA LogC :597-599, V-Log :615-617;
    /// tolerance 1e-6 there, 1e-5 here for f32 matrix composition).
    #[test]
    fn ocio_builtin_transform_tests_values() {
        #[rustfmt::skip]
        let cases: [(&str, [f32; 3], [f32; 3]); 4] = [
            ("ACEScct_to_ACES2065-1", [0.5, 0.4, 0.3], [0.386_397_2, 0.158_557_25, 0.043_152_537]),
            ("ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-CINEMA_1.0", [0.5, 0.4, 0.3], [0.33629957, 0.318_328, 0.22867827]),
            ("ARRI_ALEXA-LOGC-EI800-AWG_to_ACES2065-1", [0.5, 0.4, 0.3], [0.401_621_43, 0.236_455_44, 0.064_83]),
            ("PANASONIC_VLOG-VGAMUT_to_ACES2065-1", [0.5, 0.4, 0.3], [0.306_918_77, 0.148_128_05, 0.046_334_438]),
        ];
        for (style, input, want) in cases {
            let got = run(style, input);
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() <= 1e-5,
                    "{style} ch{c}: {} vs OCIO {}",
                    got[c],
                    want[c]
                );
            }
        }
    }

    /// Every `BuiltinTransform` style the SHIPPED configs use must resolve.
    ///
    /// This is the check that catches the failure mode that matters: a style a config names
    /// and the registry does not know is a hard compile error, so the view would not render.
    /// Reads the embedded YAML directly rather than the parsed `Config` so it sees every style
    /// in the files, including any the config loader might drop on its way in.
    #[test]
    fn every_builtin_style_the_shipped_config_uses_resolves() {
        let mut styles: Vec<&str> = Vec::new();
        let lines = crate::builtin::embedded::REGISTRY
            .iter()
            .flat_map(|e| e.yaml.lines());
        for line in lines {
            let Some(after) = line.split("BuiltinTransform").nth(1) else {
                continue;
            };
            let Some(after) = after.split("style:").nth(1) else {
                continue;
            };
            let style = after.split('}').next().unwrap_or("").trim();
            if !style.is_empty() && !styles.contains(&style) {
                styles.push(style);
            }
        }
        assert!(
            styles.len() > 40,
            "expected the studio configs to use dozens of builtin styles, found {} \u{2014} the \
             scrape above has probably stopped matching the file's syntax, which would make \
             this test vacuous",
            styles.len()
        );
        let unresolved: Vec<&&str> = styles.iter().filter(|s| get_builtin(s).is_none()).collect();
        assert!(
            unresolved.is_empty(),
            "{} of {} builtin styles used by the shipped config do not resolve: {unresolved:#?}",
            unresolved.len(),
            styles.len()
        );
    }
}
