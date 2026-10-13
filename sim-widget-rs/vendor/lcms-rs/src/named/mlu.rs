//! Multi-localized unicode (MLU) objects — port of the MLU half of `cmsnamed.c`.
//!
//! An MLU (`cmsMLU`) encapsulates i18n: a set of translations, each keyed by a
//! 2-byte ISO-639 language code and a 2-byte ISO-3166 country code, mapping to a
//! wide string. ICC serializes these as UTF-16BE.
//!
//! ## Deviation from the C model (intentional, documented)
//! The C code keeps every translation in one growing byte `MemPool` and stores each
//! entry as an `(offset, len)` pair plus language/country. That sub-allocator exists
//! only to avoid many tiny `malloc`s in C; it carries no semantics. We drop it entirely
//! and store each translation as an owned [`Vec<u16>`] of UTF-16 code units.
//!
//! We canonicalize the platform-dependent C `wchar_t` to **UTF-16 code units** (`u16`),
//! which is exactly the ICC on-disk representation the profile-I/O layer will read and
//! write. Storing raw `u16` (rather than a decoded [`String`]) is lossless: it preserves
//! surrogate pairs and even lone surrogates verbatim, so `setWide`/`getWide` round-trips
//! are bit-exact — something a `String` could not guarantee. The ASCII/UTF-8 accessors
//! convert on demand, matching `cmsMLUgetASCII`/`cmsMLUgetUTF8`.

use crate::error::{ErrorCode, Result};
use crate::cms_err;

/// A single MLU translation (`_cmsMLUentry` + its slice of the C `MemPool`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MluEntry {
    /// ISO-639 language code, 2 bytes (`_cmsMLUentry::Language`). `[0, 0]` is the
    /// "no language" wildcard (`cmsNoLanguage`).
    pub language: [u8; 2],
    /// ISO-3166 country code, 2 bytes (`_cmsMLUentry::Country`). `[0, 0]` is
    /// `cmsNoCountry`.
    pub country: [u8; 2],
    /// The translation as UTF-16 code units — **exactly the units the C `MemPool` holds for
    /// this entry**, i.e. `_cmsMLUentry::Len / sizeof(wchar_t)` of them.
    ///
    /// The setters strip the terminator (`mywcslen`), so entries built through the API carry
    /// no NUL and an explicitly-empty string is a single `0`. The `mluc` reader instead stores
    /// the on-disk block verbatim via [`Mlu::add_raw`], so an entry read from a profile whose
    /// record `Len` includes the NUL keeps that NUL here — which is what lets the writer emit
    /// the original `Len` back (`cmstypes.c:1731-1732`, `:1796-1808`).
    pub text: Vec<u16>,
}

/// A multi-localized unicode object (`cmsMLU`).
///
/// `cmsMLUalloc` → [`Mlu::new`]; `cmsMLUfree` → [`Drop`] (automatic);
/// `cmsMLUdup` → [`Clone`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mlu {
    /// Translations, in insertion order (`cmsMLU::Entries[0..UsedEntries]`).
    entries: Vec<MluEntry>,
}

impl Mlu {
    /// Allocate an empty MLU (`cmsMLUalloc`). The C `nItems` capacity hint is irrelevant
    /// here — the backing [`Vec`] grows on demand.
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Number of translations held (`cmsMLUtranslationsCount`).
    pub fn translations_count(&self) -> u32 {
        self.entries.len() as u32
    }

    /// Language/country codes for translation `idx` (`cmsMLUtranslationsCodes`).
    /// Returns `None` when `idx` is out of range.
    pub fn translations_codes(&self, idx: u32) -> Option<([u8; 2], [u8; 2])> {
        self.entries
            .get(idx as usize)
            .map(|e| (e.language, e.country))
    }

    /// Core of `AddMLUBlock`: reject a duplicate language/country pair, otherwise append.
    ///
    /// Returns [`ErrorCode::AlreadyDefined`] when a translation for the pair already
    /// exists (the C code returns `FALSE` there — "Only one is allowed!").
    fn add_block(&mut self, language: [u8; 2], country: [u8; 2], text: Vec<u16>) -> Result<()> {
        if self.find(language, country).is_some() {
            return Err(cms_err!(
                ErrorCode::AlreadyDefined,
                "MLU: translation for language/country already present"
            ));
        }
        self.entries.push(MluEntry { language, country, text });
        Ok(())
    }

    /// Index of an entry with an exact language+country match (`SearchMLUEntry`).
    fn find(&self, language: [u8; 2], country: [u8; 2]) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| e.language == language && e.country == country)
    }

    /// Add an ASCII translation (`cmsMLUsetASCII`).
    ///
    /// Per ICC (ICC1v43_2010-12 p.61) no `\0` terminator is stored. An explicitly empty
    /// string is stored as a single `0` code unit, matching the C special case.
    pub fn set_ascii(&mut self, language: [u8; 2], country: [u8; 2], s: &str) -> Result<()> {
        let text: Vec<u16> = if s.is_empty() {
            vec![0]
        } else {
            // C widens each `char` to `wchar_t`; ASCII bytes map 1:1 to code units.
            s.bytes().map(u16::from).collect()
        };
        self.add_block(language, country, text)
    }

    /// Add a UTF-8 translation (`cmsMLUsetUTF8`).
    ///
    /// The C `decodeUTF8` turns UTF-8 into `wchar_t`; [`str::encode_utf16`] is the exact
    /// pure-Rust equivalent (emitting surrogate pairs for astral code points, since our
    /// canonical wide unit is `u16`). Empty string → single `0`.
    pub fn set_utf8(&mut self, language: [u8; 2], country: [u8; 2], s: &str) -> Result<()> {
        let text: Vec<u16> = if s.is_empty() {
            vec![0]
        } else {
            s.encode_utf16().collect()
        };
        self.add_block(language, country, text)
    }

    /// Add a wide (UTF-16) translation (`cmsMLUsetWide`).
    ///
    /// Mirrors C `mywcslen`: the string is taken up to (but excluding) the first `0`
    /// code unit; no terminator is stored. An empty wide string is stored as a single `0`.
    pub fn set_wide(&mut self, language: [u8; 2], country: [u8; 2], wide: &[u16]) -> Result<()> {
        let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        let text: Vec<u16> = if end == 0 { vec![0] } else { wide[..end].to_vec() };
        self.add_block(language, country, text)
    }

    /// Add a translation from an on-disk block, storing the units **verbatim** (`Type_MLU_Read`,
    /// `cmstypes.c:1731-1732`).
    ///
    /// The C reader deliberately bypasses `cmsMLUsetWide`: it copies the record's `Len` into
    /// `Entries[i].Len` and `Type_MLU_Write` emits that same `Len` back (`cmstypes.c:1796-1808`).
    /// Routing read data through [`Self::set_wide`] instead would apply `mywcslen`, dropping a
    /// trailing NUL that the record counted — the tag would then be rewritten two bytes shorter
    /// and every later record `Offset` would shift. This is the reader's entry point; API callers
    /// keep using the `set_*` family.
    pub(crate) fn add_raw(&mut self, language: [u8; 2], country: [u8; 2], units: Vec<u16>) -> Result<()> {
        self.add_block(language, country, units)
    }

    /// Best-match lookup (`_cmsMLUgetWide`): exact language+country wins; failing that the
    /// first entry sharing the language; failing that the first entry overall. Returns the
    /// matched entry index, or `None` when the MLU is empty.
    fn best_match(&self, language: [u8; 2], country: [u8; 2]) -> Option<usize> {
        if self.entries.is_empty() {
            return None;
        }
        let mut best: Option<usize> = None;
        for (i, e) in self.entries.iter().enumerate() {
            if e.language == language {
                if best.is_none() {
                    best = Some(i);
                }
                if e.country == country {
                    return Some(i); // exact match
                }
            }
        }
        Some(best.unwrap_or(0))
    }

    /// Wide (UTF-16) representation of the best-matching translation (`cmsMLUgetWide`),
    /// returned as owned code units (no terminator). `None` if the MLU is empty.
    pub fn get_wide(&self, language: [u8; 2], country: [u8; 2]) -> Option<Vec<u16>> {
        self.best_match(language, country)
            .map(|i| self.entries[i].text.clone())
    }

    /// ASCII (Latin-1) bytes of the best-matching translation (`cmsMLUgetASCII`).
    ///
    /// This is the **serialization** accessor: C's `cmsMLUgetASCII` narrows one wide unit to
    /// exactly one byte (`if (wc < 0xff) Buffer[i] = (char)wc; else Buffer[i] = '?';`,
    /// `cmsnamed.c:583-588`), so the byte count equals the wide-unit count. On-disk framing
    /// depends on that identity - `textDescription` stores the ASCII length and reuses it for
    /// the Unicode block - which is why this returns bytes and not a `String`.
    ///
    /// `None` if the MLU is empty.
    pub fn get_ascii_bytes(&self, language: [u8; 2], country: [u8; 2]) -> Option<Vec<u8>> {
        self.best_match(language, country).map(|i| {
            self.entries[i]
                .text
                .iter()
                .map(|&wc| if wc < 0xff { wc as u8 } else { b'?' })
                .collect()
        })
    }

    /// ASCII representation of the best-matching translation, as a `String`.
    ///
    /// **Display only — not the serialization path.** Latin-1 bytes `0x80..=0xFE` become
    /// Unicode scalars here, and each of those encodes as *two* UTF-8 bytes, so the length no
    /// longer matches C's. Anything writing a tag must use [`Self::get_ascii_bytes`].
    pub fn get_ascii(&self, language: [u8; 2], country: [u8; 2]) -> Option<String> {
        self.get_ascii_bytes(language, country)
            .map(|b| b.iter().map(|&c| c as char).collect())
    }

    /// UTF-8 representation of the best-matching translation (`cmsMLUgetUTF8`).
    ///
    /// [`String::from_utf16_lossy`] mirrors `encodeUTF8`, reconstituting surrogate pairs
    /// and substituting U+FFFD for malformed sequences. `None` if the MLU is empty.
    pub fn get_utf8(&self, language: [u8; 2], country: [u8; 2]) -> Option<String> {
        self.best_match(language, country)
            .map(|i| String::from_utf16_lossy(&self.entries[i].text))
    }

    /// The actually-used language/country for a lookup (`cmsMLUgetTranslation`).
    /// `None` if the MLU is empty.
    pub fn get_translation(&self, language: [u8; 2], country: [u8; 2]) -> Option<([u8; 2], [u8; 2])> {
        self.best_match(language, country)
            .map(|i| (self.entries[i].language, self.entries[i].country))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_roundtrip_two_languages() {
        let mut mlu = Mlu::new();
        mlu.set_ascii(*b"en", *b"US", "Hello").unwrap();
        mlu.set_ascii(*b"fr", *b"FR", "Bonjour").unwrap();

        assert_eq!(mlu.translations_count(), 2);
        assert_eq!(mlu.get_ascii(*b"en", *b"US").as_deref(), Some("Hello"));
        assert_eq!(mlu.get_ascii(*b"fr", *b"FR").as_deref(), Some("Bonjour"));
    }

    #[test]
    fn best_match_falls_back_to_language_then_first() {
        let mut mlu = Mlu::new();
        mlu.set_ascii(*b"en", *b"US", "color").unwrap();
        mlu.set_ascii(*b"en", *b"GB", "colour").unwrap();

        // Exact language, different country -> first entry sharing the language.
        assert_eq!(mlu.get_ascii(*b"en", *b"ZZ").as_deref(), Some("color"));
        // Unknown language -> first entry overall.
        assert_eq!(mlu.get_ascii(*b"de", *b"DE").as_deref(), Some("color"));
    }

    #[test]
    fn duplicate_pair_rejected() {
        let mut mlu = Mlu::new();
        mlu.set_ascii(*b"en", *b"US", "a").unwrap();
        let err = mlu.set_ascii(*b"en", *b"US", "b").unwrap_err();
        assert_eq!(err.code, ErrorCode::AlreadyDefined);
    }

    #[test]
    fn wide_roundtrip_preserves_units() {
        // Includes an astral code point (U+1F600) that becomes a surrogate pair in UTF-16.
        let wide: Vec<u16> = "Grüße 😀".encode_utf16().collect();
        let mut mlu = Mlu::new();
        mlu.set_wide(*b"de", *b"DE", &wide).unwrap();

        assert_eq!(mlu.get_wide(*b"de", *b"DE"), Some(wide.clone()));
        assert_eq!(mlu.get_utf8(*b"de", *b"DE").as_deref(), Some("Grüße 😀"));
    }

    #[test]
    fn utf8_roundtrip() {
        let mut mlu = Mlu::new();
        mlu.set_utf8(*b"ja", *b"JP", "日本語").unwrap();
        assert_eq!(mlu.get_utf8(*b"ja", *b"JP").as_deref(), Some("日本語"));
    }

    #[test]
    fn translations_codes_and_translation() {
        let mut mlu = Mlu::new();
        mlu.set_ascii(*b"en", *b"US", "x").unwrap();
        assert_eq!(mlu.translations_codes(0), Some((*b"en", *b"US")));
        assert_eq!(mlu.translations_codes(1), None);
        assert_eq!(mlu.get_translation(*b"en", *b"GB"), Some((*b"en", *b"US")));
    }

    /// C4: `add_raw` must keep the on-disk units untouched where `set_wide` truncates.
    ///
    /// `Type_MLU_Read` stores the record `Len` verbatim (`cmstypes.c:1731`), so a record whose
    /// `Len` counts the NUL keeps six units, not five — that is what `Type_MLU_Write` emits back.
    #[test]
    fn add_raw_keeps_trailing_nul_that_set_wide_would_drop() {
        let on_disk = vec![0x48u16, 0x65, 0x6c, 0x6c, 0x6f, 0x0000]; // "Hello" + NUL, Len = 12

        let mut raw = Mlu::new();
        raw.add_raw(*b"en", *b"US", on_disk.clone()).unwrap();
        assert_eq!(raw.get_wide(*b"en", *b"US"), Some(on_disk.clone()));

        let mut via_setter = Mlu::new();
        via_setter.set_wide(*b"en", *b"US", &on_disk).unwrap();
        assert_eq!(via_setter.get_wide(*b"en", *b"US").map(|w| w.len()), Some(5));
    }

    #[test]
    fn empty_mlu_returns_none() {
        let mlu = Mlu::new();
        assert_eq!(mlu.get_ascii(*b"en", *b"US"), None);
        assert_eq!(mlu.get_wide(*b"en", *b"US"), None);
    }
}
