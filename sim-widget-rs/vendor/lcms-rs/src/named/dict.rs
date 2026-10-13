//! Dictionaries and profile-sequence descriptions — port of the last two sections of
//! `cmsnamed.c`.
//!
//! * A dictionary (`cmsHANDLE` over `_cmsDICT`) is a list of name/value pairs, each with
//!   an optional localized display name and display value ([`Mlu`]). The C version is a
//!   singly linked list of heap nodes; we use a [`Vec`].
//! * A profile sequence description (`cmsSEQ`) is a fixed-length array of per-profile
//!   descriptors (`cmsPSEQDESC`), each carrying device identifiers plus localized
//!   manufacturer/model/description strings.

use crate::error::{ErrorCode, Result};
use crate::cms_err;
use crate::named::mlu::Mlu;
use crate::types::sig::Signature;

// Dictionaries --------------------------------------------------------------------------

/// A dictionary entry (`cmsDICTentry`).
///
/// The C node stores `Name`/`Value` as `wchar_t*`; we store owned [`String`]s (the
/// wide↔UTF-8 conversion belongs to the profile-I/O layer). `Name` is required; the
/// other three are optional (`Value`, `DisplayName`, `DisplayValue` may be `NULL`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictEntry {
    /// Entry key (`cmsDICTentry::Name`), always present.
    pub name: String,
    /// Entry value (`cmsDICTentry::Value`), optional.
    pub value: Option<String>,
    /// Localized display name (`cmsDICTentry::DisplayName`), optional.
    pub display_name: Option<Mlu>,
    /// Localized display value (`cmsDICTentry::DisplayValue`), optional.
    pub display_value: Option<Mlu>,
}

/// A dictionary (`_cmsDICT`).
///
/// `cmsDictAlloc` → [`Dict::new`]; `cmsDictFree` → [`Drop`]; `cmsDictDup` → [`Clone`].
///
/// **Ordering:** the C list is prepend-only, so `cmsDictGetEntryList` yields newest-first.
/// [`Self::add_entry`] reproduces that (it inserts at the front) and [`Self::entries`]
/// yields newest-first. We do *not* reproduce the incidental order-reversal in C's
/// `cmsDictDup` (an artifact of re-prepending during the copy walk): [`Clone`] preserves
/// order, which is cleaner and semantically equivalent since dictionaries are unordered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dict {
    entries: Vec<DictEntry>,
}

impl Dict {
    /// Allocate an empty dictionary (`cmsDictAlloc`).
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Add an entry (`cmsDictAddEntry`). `name` is required (the C code asserts it);
    /// `value`, `display_name` and `display_value` are optional. Inserted at the front to
    /// match the C prepend semantics.
    pub fn add_entry(
        &mut self,
        name: &str,
        value: Option<&str>,
        display_name: Option<Mlu>,
        display_value: Option<Mlu>,
    ) {
        self.entries.insert(
            0,
            DictEntry {
                name: name.to_string(),
                value: value.map(str::to_string),
                display_name,
                display_value,
            },
        );
    }

    /// The entries, newest-first (`cmsDictGetEntryList` + `cmsDictNextEntry`).
    pub fn entries(&self) -> &[DictEntry] {
        &self.entries
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the dictionary is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// Profile sequence descriptions ---------------------------------------------------------

/// A per-profile sequence descriptor (`cmsPSEQDESC`).
///
/// Fields come from the profile-sequence-description tag and the profile-sequence-
/// identifier tag. `profile_id` is the 16-byte MD5 profile ID (`cmsProfileID`).
///
/// [`Default`] is hand-written (rather than derived) because [`Signature`] has no
/// `Default`; it yields the all-zero descriptor the C code gets from `calloc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSequenceDesc {
    /// Device manufacturer signature (`cmsPSEQDESC::deviceMfg`).
    pub device_mfg: Signature,
    /// Device model signature (`cmsPSEQDESC::deviceModel`).
    pub device_model: Signature,
    /// Device attributes bitfield (`cmsPSEQDESC::attributes`, `cmsUInt64Number`).
    pub attributes: u64,
    /// Technology signature (`cmsPSEQDESC::technology`).
    pub technology: Signature,
    /// 16-byte MD5 profile ID (`cmsPSEQDESC::ProfileID`).
    pub profile_id: [u8; 16],
    /// Localized manufacturer name (`cmsPSEQDESC::Manufacturer`).
    pub manufacturer: Option<Mlu>,
    /// Localized model name (`cmsPSEQDESC::Model`).
    pub model: Option<Mlu>,
    /// Localized description (`cmsPSEQDESC::Description`).
    pub description: Option<Mlu>,
}

impl Default for ProfileSequenceDesc {
    fn default() -> Self {
        Self {
            device_mfg: Signature(0),
            device_model: Signature(0),
            attributes: 0,
            technology: Signature(0),
            profile_id: [0; 16],
            manufacturer: None,
            model: None,
            description: None,
        }
    }
}

/// A profile sequence description (`cmsSEQ`).
///
/// `cmsAllocProfileSequenceDescription` → [`Seq::new`];
/// `cmsFreeProfileSequenceDescription` → [`Drop`];
/// `cmsDupProfileSequenceDescription` → [`Clone`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seq {
    /// The per-profile descriptors (`cmsSEQ::seq`, length `cmsSEQ::n`).
    pub seq: Vec<ProfileSequenceDesc>,
}

impl Seq {
    /// Allocate a sequence of `n` default descriptors
    /// (`cmsAllocProfileSequenceDescription`).
    ///
    /// Returns [`ErrorCode::Range`] when `n == 0` or `n > 255` (the C limit — "a maximum
    /// of 255 profiles linked in a devicelink").
    pub fn new(n: usize) -> Result<Self> {
        if n == 0 || n > 255 {
            return Err(cms_err!(
                ErrorCode::Range,
                "profile sequence length {n} out of range (1..=255)"
            ));
        }
        Ok(Self { seq: vec![ProfileSequenceDesc::default(); n] })
    }

    /// Number of descriptors (`cmsSEQ::n`).
    pub fn len(&self) -> usize {
        self.seq.len()
    }

    /// Always `false` — a valid [`Seq`] holds at least one descriptor.
    pub fn is_empty(&self) -> bool {
        self.seq.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_add_and_iterate_newest_first() {
        let mut dict = Dict::new();
        let mut dn = Mlu::new();
        dn.set_ascii(*b"en", *b"US", "Copyright").unwrap();

        dict.add_entry("Author", Some("Marti"), Some(dn), None);
        dict.add_entry("Year", None, None, None);

        assert_eq!(dict.len(), 2);
        // Newest-first: "Year" was added last.
        assert_eq!(dict.entries()[0].name, "Year");
        assert_eq!(dict.entries()[0].value, None);
        assert_eq!(dict.entries()[1].name, "Author");
        assert_eq!(dict.entries()[1].value.as_deref(), Some("Marti"));
        let mlu = dict.entries()[1].display_name.as_ref().unwrap();
        assert_eq!(mlu.get_ascii(*b"en", *b"US").as_deref(), Some("Copyright"));
    }

    #[test]
    fn dict_clone_is_independent() {
        let mut dict = Dict::new();
        dict.add_entry("k", Some("v"), None, None);
        let dup = dict.clone();
        dict.add_entry("k2", None, None, None);
        assert_eq!(dup.len(), 1);
        assert_eq!(dict.len(), 2);
    }

    #[test]
    fn seq_alloc_defaults() {
        let seq = Seq::new(3).unwrap();
        assert_eq!(seq.len(), 3);
        assert_eq!(seq.seq[0], ProfileSequenceDesc::default());
        assert!(seq.seq[0].manufacturer.is_none());
    }

    #[test]
    fn seq_bounds_rejected() {
        assert_eq!(Seq::new(0).unwrap_err().code, ErrorCode::Range);
        assert_eq!(Seq::new(256).unwrap_err().code, ErrorCode::Range);
    }

    #[test]
    fn seq_carries_mlu_and_ids() {
        let mut seq = Seq::new(1).unwrap();
        let mut model = Mlu::new();
        model.set_ascii(*b"en", *b"US", "Model X").unwrap();
        seq.seq[0].model = Some(model);
        seq.seq[0].device_mfg = Signature::new(b"ADBE");
        seq.seq[0].profile_id = [0xAB; 16];

        assert_eq!(seq.seq[0].device_mfg, Signature::new(b"ADBE"));
        assert_eq!(
            seq.seq[0].model.as_ref().unwrap().get_ascii(*b"en", *b"US").as_deref(),
            Some("Model X")
        );
    }
}
