//! Channel classification by name: the workspace's one owner of OIIO's alpha/Z rules.
//!
//! OpenImageIO decides which channel is alpha and which is depth from the channel *name*,
//! looking only at the part after the last `.` (the EXR layer convention), case-insensitively:
//!
//! - alpha: suffix `A` or `Alpha` (`exrinput.cpp` `query_channels`, 3.1: `Strutil::iequals(
//!   cnh[c].suffix, "A") || ... "Alpha"`; `oiiotool.cpp` `--chnames`: `iequals(name, "A") ||
//!   iends_with(name, ".A") || ...`), so `diffuse.A` and `beauty.alpha` are alpha;
//! - depth: suffix `Z` or `Depth` (same two places);
//! - the *first* matching channel wins (`if (spec.alpha_channel < 0 && ...)`).
//!
//! `deepdata.cpp` `is_or_endswithdot` is the same test phrased on the full name; it is
//! [`is_or_ends_with_dot`] here.
//!
//! Colour slots (`R`/`Red`, `G`/`Green`, `B`/`Blue`) follow the same suffix rule; they are the
//! head of OIIO's EXR channel priority list (`exrinput.cpp` `compute_special_index`).
//!
//! Every crate that needs to know "is this alpha / depth / red" asks here. Before this module
//! the rule existed as an exact `== "A"` (EXR spec codec, `ImageData::to_spec`), a
//! whole-name case-insensitive match without the suffix rule (`ImageSpec::get_alpha_channel`,
//! deep data, vfx-compute's layer classifier) and a positional fallback (vfx-compute
//! `convert.rs`, which put a `Z` channel into the alpha slot and then overwrote it).

use crate::DataFormat;

/// Splits `layer.suffix` at the last `.`, as OIIO's `split_name` (`exrinput.cpp`): the layer
/// keeps its trailing dot (`"diffuse."`), a name without a dot has an empty layer.
pub fn split_channel_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) => (&name[..=dot], &name[dot + 1..]),
        None => ("", name),
    }
}

/// The part of a channel name after its last `.` (the whole name when there is none).
pub fn channel_suffix(name: &str) -> &str {
    split_channel_name(name).1
}

/// OIIO `deepdata.cpp` `is_or_endswithdot`: `name` equals `suffix`, or ends with `.suffix`,
/// ignoring ASCII case.
pub fn is_or_ends_with_dot(name: &str, suffix: &str) -> bool {
    channel_suffix(name).eq_ignore_ascii_case(suffix)
}

/// True when OIIO would take `name` as an alpha channel (suffix `A` or `Alpha`, any case).
pub fn is_alpha_channel_name(name: &str) -> bool {
    let s = channel_suffix(name);
    s.eq_ignore_ascii_case("A") || s.eq_ignore_ascii_case("Alpha")
}

/// True when OIIO would take `name` as the depth channel (suffix `Z` or `Depth`, any case).
pub fn is_z_channel_name(name: &str) -> bool {
    let s = channel_suffix(name);
    s.eq_ignore_ascii_case("Z") || s.eq_ignore_ascii_case("Depth")
}

/// RGB slot of a colour channel: `0` for `R`/`Red`, `1` for `G`/`Green`, `2` for `B`/`Blue`
/// (suffix rule, any case); `None` for everything else, alpha and depth included.
pub fn color_slot(name: &str) -> Option<usize> {
    let s = channel_suffix(name);
    if s.eq_ignore_ascii_case("R") || s.eq_ignore_ascii_case("Red") {
        Some(0)
    } else if s.eq_ignore_ascii_case("G") || s.eq_ignore_ascii_case("Green") {
        Some(1)
    } else if s.eq_ignore_ascii_case("B") || s.eq_ignore_ascii_case("Blue") {
        Some(2)
    } else {
        None
    }
}

/// What a channel holds, as far as an operation that treats colour, alpha and data differently
/// needs to know (vfx-ops refuses colour processing on ids, masks and unknown data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    /// Colour data (`R`/`G`/`B`, luminance `Y`).
    Color,
    /// Alpha/opacity.
    Alpha,
    /// Depth/Z.
    Depth,
    /// Object or material identifiers.
    Id,
    /// Matte/mask data.
    Mask,
    /// Anything else.
    Generic,
}

/// The kind of the channel `name` stored as `format`, decided on the suffix after the last `.`
/// (any case) like the alpha and depth rules above. Integer storage cannot be colour, so a
/// `U32` channel that is not alpha, depth or a mask is an id whatever its name.
pub fn channel_kind(name: &str, format: DataFormat) -> ChannelKind {
    if is_alpha_channel_name(name) {
        return ChannelKind::Alpha;
    }
    if is_z_channel_name(name) {
        return ChannelKind::Depth;
    }
    let s = channel_suffix(name);
    let is = |want: &str| s.eq_ignore_ascii_case(want);
    if is("MASK") || is("MATTE") {
        ChannelKind::Mask
    } else if is("ID") || is("OBJECTID") || is("OBJECT_ID") || format == DataFormat::U32 {
        ChannelKind::Id
    } else if color_slot(name).is_some() || is("Y") {
        ChannelKind::Color
    } else {
        ChannelKind::Generic
    }
}

/// Index of the first alpha channel in `names` (OIIO's `alpha_channel`), or `None`.
pub fn find_alpha_channel<S: AsRef<str>>(names: &[S]) -> Option<usize> {
    names.iter().position(|n| is_alpha_channel_name(n.as_ref()))
}

/// Index of the first depth channel in `names` (OIIO's `z_channel`), or `None`.
pub fn find_z_channel<S: AsRef<str>>(names: &[S]) -> Option<usize> {
    names.iter().position(|n| is_z_channel_name(n.as_ref()))
}

/// `find_alpha_channel` as OIIO's `int` field (`-1` when absent).
pub fn alpha_channel_index<S: AsRef<str>>(names: &[S]) -> i32 {
    find_alpha_channel(names).map_or(-1, |i| i as i32)
}

/// `find_z_channel` as OIIO's `int` field (`-1` when absent).
pub fn z_channel_index<S: AsRef<str>>(names: &[S]) -> i32 {
    find_z_channel(names).map_or(-1, |i| i as i32)
}

/// OIIO's channel priority within one EXR layer (`exrinput.cpp`
/// `ChanNameHolder::compute_special_index`), matched case-insensitively on the suffix.
const EXR_PRIORITY: [&str; 20] = [
    "R", "Red", "G", "Green", "B", "Blue", "Y", "real", "imag", "A", "Alpha", "AR", "RA", "AG",
    "GA", "AB", "BA", "Z", "Depth", "Zback",
];

/// The priority for a layer that encodes a position or normal (it has `X` and one of `Y`/`Z`),
/// `compute_special_index_xyz`: `X Y Z` follow the colours, `Z` loses its depth slot.
const EXR_PRIORITY_XYZ: [&str; 21] = [
    "R", "Red", "G", "Green", "B", "Blue", "X", "Y", "Z", "real", "imag", "A", "Alpha", "AR", "RA",
    "AG", "GA", "AB", "BA", "Depth", "Zback",
];

/// The order in which OpenImageIO presents an EXR part's channels, as indices into `names`.
///
/// An EXR channel list is stored sorted by name (`A, B, G, R`); OIIO re-sorts it
/// (`exrinput.cpp` `OpenEXRInput::PartInfo::query_channels`, 3.1):
///
/// 1. by layer, the name up to and including its last `.` ([`split_channel_name`]), in byte
///    order, so unlayered channels come first;
/// 2. within a layer, by the position of the suffix in a priority list (`R Red G Green B Blue
///    Y real imag A Alpha AR RA AG GA AB BA Z Depth Zback`, any case, unlisted last); a layer
///    whose suffixes include `X` and one of `Y`/`Z` uses `R Red G Green B Blue X Y Z real imag
///    A Alpha AR RA AG GA AB BA Depth Zback` instead;
/// 3. then by suffix, in byte order.
///
/// `alpha_channel`/`z_channel` are taken from the *reordered* list ([`find_alpha_channel`]).
pub fn exr_channel_order<S: AsRef<str>>(names: &[S]) -> Vec<usize> {
    let split: Vec<(&str, &str)> = names
        .iter()
        .map(|n| split_channel_name(n.as_ref()))
        .collect();
    let has = |layer: &str, want: &str| {
        split
            .iter()
            .any(|(l, s)| *l == layer && s.eq_ignore_ascii_case(want))
    };
    let priority = |i: usize| -> usize {
        let (layer, suffix) = split[i];
        let xyz = has(layer, "X") && (has(layer, "Y") || has(layer, "Z"));
        let list: &[&str] = if xyz {
            &EXR_PRIORITY_XYZ
        } else {
            &EXR_PRIORITY
        };
        list.iter()
            .position(|p| suffix.eq_ignore_ascii_case(p))
            .unwrap_or(usize::MAX)
    };
    let keys: Vec<(&str, usize, &str)> = (0..names.len())
        .map(|i| (split[i].0, priority(i), split[i].1))
        .collect();
    let mut order: Vec<usize> = (0..names.len()).collect();
    order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kind follows the suffix after the last `.`, in any case, and integer storage
    /// turns what would be colour or generic data into an id.
    #[test]
    fn kind_by_suffix_and_storage() {
        let k = channel_kind;
        assert_eq!(k("diffuse.R", DataFormat::F16), ChannelKind::Color);
        assert_eq!(k("Y", DataFormat::F32), ChannelKind::Color);
        assert_eq!(k("diffuse.R", DataFormat::U32), ChannelKind::Id);
        assert_eq!(k("beauty.alpha", DataFormat::F32), ChannelKind::Alpha);
        assert_eq!(k("A", DataFormat::U32), ChannelKind::Alpha);
        assert_eq!(k("depth.Z", DataFormat::F32), ChannelKind::Depth);
        assert_eq!(k("crypto.matte", DataFormat::U32), ChannelKind::Mask);
        assert_eq!(k("objectId", DataFormat::F32), ChannelKind::Id);
        assert_eq!(k("N.x", DataFormat::F32), ChannelKind::Generic);
    }

    fn exr_sorted<'a>(names: &[&'a str]) -> Vec<&'a str> {
        exr_channel_order(names)
            .into_iter()
            .map(|i| names[i])
            .collect()
    }

    /// Answers from OIIO 3.1.14 (`iinfo -v` on oiiotool-written files).
    #[test]
    fn exr_order_matches_oiio() {
        // tests/fixtures/exr/chan_order.exr in vfx-io, stored order.
        let stored = [
            "A",
            "B",
            "G",
            "N.X",
            "N.Y",
            "N.Z",
            "R",
            "Y",
            "Z",
            "Zback",
            "custom",
            "diffuse.B",
            "diffuse.G",
            "diffuse.R",
            "diffuse.alpha",
        ];
        assert_eq!(
            exr_sorted(&stored),
            [
                "R",
                "G",
                "B",
                "Y",
                "A",
                "Z",
                "Zback",
                "custom",
                "N.X",
                "N.Y",
                "N.Z",
                "diffuse.R",
                "diffuse.G",
                "diffuse.B",
                "diffuse.alpha"
            ]
        );
        // `ch.exr`: R,G,B,A,Z,custom,diffuse.{R,G,B,A},P.X -> iinfo order.
        let stored = [
            "A",
            "B",
            "G",
            "P.X",
            "R",
            "Z",
            "custom",
            "diffuse.A",
            "diffuse.B",
            "diffuse.G",
            "diffuse.R",
        ];
        assert_eq!(
            exr_sorted(&stored),
            [
                "R",
                "G",
                "B",
                "A",
                "Z",
                "custom",
                "P.X",
                "diffuse.R",
                "diffuse.G",
                "diffuse.B",
                "diffuse.A"
            ]
        );
    }

    /// Without `X` a layer's `Y` is luminance (slot 6) and `Z` is depth (slot 17). `iinfo -v`
    /// on oiiotool-written files with these channel names prints exactly these lists.
    #[test]
    fn xyz_rule_needs_x() {
        assert_eq!(exr_sorted(&["P.Z", "P.Y", "P.x"]), ["P.x", "P.Y", "P.Z"]);
        assert_eq!(exr_sorted(&["P.Z", "P.Y", "P.W"]), ["P.Y", "P.Z", "P.W"]);
        assert_eq!(exr_sorted(&["P.X", "P.W"]), ["P.W", "P.X"]);
    }

    /// The names and answers below are what OIIO 3.1.14 reports: `oiiotool --pattern ...
    /// --chnames diffuse.R,diffuse.G,diffuse.B,depth.Z,diffuse.alpha -o chan.exr` then
    /// `oiiotool chan.exr --echo "{TOP.alpha_channel} {TOP.z_channel}"` prints `4 0` for the
    /// file's channel list `depth.Z, diffuse.R, diffuse.G, diffuse.B, diffuse.alpha`.
    #[test]
    fn suffix_rule_matches_oiio() {
        let names = [
            "depth.Z",
            "diffuse.R",
            "diffuse.G",
            "diffuse.B",
            "diffuse.alpha",
        ];
        assert_eq!(alpha_channel_index(&names), 4);
        assert_eq!(z_channel_index(&names), 0);
        assert_eq!(color_slot("diffuse.R"), Some(0));
        assert_eq!(color_slot("depth.Z"), None);
    }

    #[test]
    fn whole_names_and_case() {
        for a in ["A", "a", "Alpha", "ALPHA", "x.A", "a.b.alpha", ".A"] {
            assert!(is_alpha_channel_name(a), "{a}");
        }
        for not_a in ["AR", "RA", "Alphas", "A.R", "", "x.", "diffuse.Ab"] {
            assert!(!is_alpha_channel_name(not_a), "{not_a}");
        }
        for z in ["Z", "z", "Depth", "depth", "P.Z"] {
            assert!(is_z_channel_name(z), "{z}");
        }
        for not_z in ["Zback", "ZB", "Z.R"] {
            assert!(!is_z_channel_name(not_z), "{not_z}");
        }
    }

    /// First match wins (`if (spec.alpha_channel < 0 && ...)`).
    #[test]
    fn first_match_wins() {
        assert_eq!(find_alpha_channel(&["R", "A", "spec.A"]), Some(1));
        assert_eq!(find_z_channel(&["R", "G"]), None);
        assert_eq!(alpha_channel_index::<&str>(&[]), -1);
    }

    #[test]
    fn split_keeps_the_layer_dot() {
        assert_eq!(split_channel_name("a.b.R"), ("a.b.", "R"));
        assert_eq!(split_channel_name("R"), ("", "R"));
        assert!(is_or_ends_with_dot("beauty.Zback", "zback"));
        assert!(!is_or_ends_with_dot("beautyZback", "Zback"));
    }
}
