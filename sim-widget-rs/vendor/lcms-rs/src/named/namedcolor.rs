//! Named color lists — port of the "Named color lists" section of `cmsnamed.c`.
//!
//! A named color list (`cmsNAMEDCOLORLIST`) is a spot-color palette: a shared `prefix`
//! and `suffix` plus a list of entries, each with a name, a PCS (Lab, encoded as 3×u16)
//! and up to [`MAX_CHANNELS`] device colorant values.
//!
//! The C code hand-manages a doubling array (`GrowNamedColorList`); we use a [`Vec`].
//! The 100 K-entry growth cap and the fixed 33-byte prefix/suffix buffers are C
//! allocator details with no semantic role, so we drop them, keeping only the observable
//! rule that prefix/suffix are truncated to 32 bytes (`cmsAllocNamedColorList`).

use crate::error::{ErrorCode, Result};
use crate::cms_err;
use crate::MAX_CHANNELS;

/// Borrowed view of one entry plus the list-level prefix/suffix, returned by
/// [`NamedColorList::info`] (the C `cmsNamedColorInfo` out-parameters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedColorInfo<'a> {
    /// Color name (`Name` out-param).
    pub name: &'a str,
    /// List prefix (`Prefix` out-param).
    pub prefix: &'a str,
    /// List suffix (`Suffix` out-param).
    pub suffix: &'a str,
    /// PCS coordinates (`PCS` out-param).
    pub pcs: [u16; 3],
    /// Device colorant, trimmed to [`NamedColorList::colorant_count`] (`Colorant`
    /// out-param).
    pub colorant: &'a [u16],
}

/// A single named color (`_cmsNAMEDCOLOR`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedColor {
    /// Color name, without prefix/suffix (`_cmsNAMEDCOLOR::Name`).
    pub name: String,
    /// PCS coordinates — always Lab, encoded (`_cmsNAMEDCOLOR::PCS`).
    pub pcs: [u16; 3],
    /// Device colorant values (`_cmsNAMEDCOLOR::DeviceColorant`). Only the first
    /// [`NamedColorList::colorant_count`] entries are meaningful; the rest stay `0`.
    pub device_colorant: [u16; MAX_CHANNELS],
}

/// A named color list (`cmsNAMEDCOLORLIST`).
///
/// `cmsAllocNamedColorList` → [`NamedColorList::new`]; `cmsFreeNamedColorList` → [`Drop`];
/// `cmsDupNamedColorList` → [`Clone`].
///
/// **Deviation:** the C struct carries an explicit `nColors` counter; we make the backing
/// [`Vec`] the single source of truth and expose the count via [`Self::count`]. Keeping a
/// separate counter would be a desync hazard for no benefit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedColorList {
    colorant_count: usize,
    prefix: String,
    suffix: String,
    list: Vec<NamedColor>,
}

/// Truncate `s` to at most `max` bytes without splitting a UTF-8 char
/// (the C code does a raw `strncpy` byte truncation to a 32-byte field).
fn truncate_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

impl NamedColorList {
    /// Allocate a list (`cmsAllocNamedColorList`).
    ///
    /// Returns [`ErrorCode::Range`] when `colorant_count` exceeds [`MAX_CHANNELS`]
    /// (`ColorantCount > cmsMAXCHANNELS` → `NULL`). `prefix`/`suffix` are truncated to
    /// 32 bytes. The C `n` pre-reservation hint is a pure capacity optimization and is
    /// omitted.
    pub fn new(colorant_count: u32, prefix: &str, suffix: &str) -> Result<Self> {
        if colorant_count as usize > MAX_CHANNELS {
            return Err(cms_err!(
                ErrorCode::Range,
                "named color list: colorant count {colorant_count} exceeds cmsMAXCHANNELS ({MAX_CHANNELS})"
            ));
        }
        Ok(Self {
            colorant_count: colorant_count as usize,
            prefix: truncate_bytes(prefix, 32),
            suffix: truncate_bytes(suffix, 32),
            list: Vec::new(),
        })
    }

    /// Number of colors in the list (`cmsNamedColorCount`).
    pub fn count(&self) -> usize {
        self.list.len()
    }

    /// The number of device colorant channels each entry uses.
    pub fn colorant_count(&self) -> usize {
        self.colorant_count
    }

    /// Re-declare how many colorant channels each entry uses.
    ///
    /// `CreateNamedColorDevicelink` does exactly this (`cmsvirt.c:1104`:
    /// `nc2->ColorantCount = cmsPipelineOutputChannels(v->Lut)`) - the list is read from a
    /// named-color profile in its own device space, then re-pointed at the *output* space of the
    /// transform before the colorants are recomputed. Existing `device_colorant` payloads are
    /// left alone; the caller is expected to overwrite them via [`Self::set_device_colorant`].
    ///
    /// Returns [`ErrorCode::Range`] above [`MAX_CHANNELS`], as [`Self::new`] does.
    pub fn set_colorant_count(&mut self, colorant_count: u32) -> Result<()> {
        if colorant_count as usize > MAX_CHANNELS {
            return Err(cms_err!(
                ErrorCode::Range,
                "named color list: colorant count {colorant_count} exceeds cmsMAXCHANNELS ({MAX_CHANNELS})"
            ));
        }
        self.colorant_count = colorant_count as usize;
        Ok(())
    }

    /// Overwrite entry `n`'s device colorant, copying [`Self::colorant_count`] channels.
    ///
    /// The write target of `cmsDoTransform(xform, &i, nc2->List[i].DeviceColorant, 1)`
    /// (`cmsvirt.c:1112`). `false` if `n` is out of range.
    pub fn set_device_colorant(&mut self, n: usize, colorant: &[u16]) -> bool {
        let count = self.colorant_count.min(colorant.len());
        match self.list.get_mut(n) {
            Some(c) => {
                c.device_colorant[..count].copy_from_slice(&colorant[..count]);
                true
            }
            None => false,
        }
    }

    /// The shared name prefix.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The shared name suffix.
    pub fn suffix(&self) -> &str {
        &self.suffix
    }

    /// Append a color (`cmsAppendNamedColor`).
    ///
    /// Only the first [`Self::colorant_count`] colorant values are copied (the C loop is
    /// bounded by `ColorantCount`); remaining channels stay `0`. All three PCS values are
    /// stored. Pass zero-filled arrays for the C `NULL` cases.
    pub fn append(&mut self, name: &str, pcs: [u16; 3], colorant: [u16; MAX_CHANNELS]) {
        let mut device_colorant = [0u16; MAX_CHANNELS];
        device_colorant[..self.colorant_count].copy_from_slice(&colorant[..self.colorant_count]);
        self.list.push(NamedColor { name: name.to_string(), pcs, device_colorant });
    }

    /// Borrow the color at `n`, or `None` if out of range.
    pub fn get(&self, n: usize) -> Option<&NamedColor> {
        self.list.get(n)
    }

    /// Info about color `n` (`cmsNamedColorInfo`): name, prefix, suffix, PCS, and the
    /// device colorant slice trimmed to [`Self::colorant_count`]. `None` if out of range.
    pub fn info(&self, n: usize) -> Option<NamedColorInfo<'_>> {
        self.list.get(n).map(|c| NamedColorInfo {
            name: c.name.as_str(),
            prefix: self.prefix.as_str(),
            suffix: self.suffix.as_str(),
            pcs: c.pcs,
            colorant: &c.device_colorant[..self.colorant_count],
        })
    }

    /// Index of the color named `name`, case-insensitively (`cmsNamedColorIndex`,
    /// which uses `cmsstrcasecmp`). `None` if not found.
    pub fn index(&self, name: &str) -> Option<usize> {
        self.list.iter().position(|c| c.name.eq_ignore_ascii_case(name))
    }
}

// MPE glue: `_cmsStageAllocNamedColor` + `EvalNamedColorPCS`/`EvalNamedColor` live in
// `crate::pipeline::stage` (`Stage::alloc_named_color`); `DupNamedColorList`/`FreeNamedColorList`
// collapse into this type's `Clone`/`Drop`; `cmsGetNamedColorList` is
// `crate::cmm::transform::Transform::named_color_list`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_lookup() {
        let mut list = NamedColorList::new(4, "pre_", "_suf").unwrap();
        let mut cmyk = [0u16; MAX_CHANNELS];
        cmyk[..4].copy_from_slice(&[10, 20, 30, 40]);
        list.append("Red", [100, 200, 300], cmyk);
        list.append("Green", [1, 2, 3], [0; MAX_CHANNELS]);

        assert_eq!(list.count(), 2);
        assert_eq!(list.index("red"), Some(0)); // case-insensitive
        assert_eq!(list.index("GREEN"), Some(1));
        assert_eq!(list.index("blue"), None);

        let info = list.info(0).unwrap();
        assert_eq!(info.name, "Red");
        assert_eq!(info.prefix, "pre_");
        assert_eq!(info.suffix, "_suf");
        assert_eq!(info.pcs, [100, 200, 300]);
        assert_eq!(info.colorant, &[10, 20, 30, 40]); // trimmed to colorant_count
    }

    #[test]
    fn colorant_masked_to_count() {
        let mut list = NamedColorList::new(2, "", "").unwrap();
        let mut colorant = [0u16; MAX_CHANNELS];
        colorant[..5].copy_from_slice(&[1, 2, 3, 4, 5]);
        list.append("x", [0; 3], colorant);
        // Only the first 2 channels are retained.
        assert_eq!(&list.get(0).unwrap().device_colorant[..2], &[1, 2]);
        assert_eq!(list.get(0).unwrap().device_colorant[2], 0);
    }

    #[test]
    fn colorant_count_over_max_rejected() {
        let err = NamedColorList::new((MAX_CHANNELS + 1) as u32, "", "").unwrap_err();
        assert_eq!(err.code, ErrorCode::Range);
    }

    #[test]
    fn prefix_suffix_truncated_to_32_bytes() {
        let long = "x".repeat(40);
        let list = NamedColorList::new(1, &long, &long).unwrap();
        assert_eq!(list.prefix().len(), 32);
        assert_eq!(list.suffix().len(), 32);
    }

    #[test]
    fn out_of_range_info_is_none() {
        let list = NamedColorList::new(1, "", "").unwrap();
        assert!(list.info(0).is_none());
    }
}
