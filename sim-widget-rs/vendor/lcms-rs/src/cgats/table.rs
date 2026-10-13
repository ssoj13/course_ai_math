//! Owned table model for the CGATS.17 / IT8.7 parser (`cmscgats.c`).
//!
//! The C reference keeps every string in a bump `SUBALLOCATOR` and threads the
//! properties through two intrusive singly-linked lists (`Next` for the main
//! chain, `NextSubkey` for dictionary sub-entries). The Rust port drops the
//! arena entirely — strings are owned `String`s — but faithfully reproduces the
//! two-list *semantics* so that lookups, insertion order and the sub-key
//! dictionary behaviour of `IsAvailableOnList` / `AddToList` match byte-for-byte.
//!
//! Key structures mirrored here:
//! - `KEYVALUE`  → [`KeyValue`] (one property; `next_subkey` reproduces the
//!   intrusive `NextSubkey` link, the main `Next` chain is simply `Vec` order).
//! - `TABLE`     → [`Table`].
//! - `WRITEMODE` → [`WriteMode`].

use crate::error::{Error, ErrorCode};

/// How a property value is serialised back out (`WRITEMODE` in `cmscgats.c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteMode {
    /// `WRITE_UNCOOKED` — verbatim, tab-separated.
    Uncooked,
    /// `WRITE_STRINGIFY` — wrapped in double quotes.
    Stringify,
    /// `WRITE_HEXADECIMAL` — `0x%X`.
    Hexadecimal,
    /// `WRITE_BINARY` — `0b%s`. Latent in lcms too: the writer supports it
    /// ([`super::It8::save_to_mem`]) but no setter API assigns it. Retained for
    /// `WRITEMODE` parity and forward compatibility.
    #[allow(dead_code)]
    Binary,
    /// `WRITE_PAIR` — `"subkey,value"` dictionary entry.
    Pair,
}

/// One header property (`KEYVALUE`).
#[derive(Debug, Clone)]
pub(crate) struct KeyValue {
    /// Property name (`Keyword`). Comment entries use `"# "`.
    pub keyword: String,
    /// Dictionary sub-key (`Subkey`), if this is a `WRITE_PAIR` entry.
    pub subkey: Option<String>,
    /// Property value (`Value`); `None` mirrors a `NULL` value pointer.
    pub value: Option<String>,
    /// Serialisation mode (`WriteAs`).
    pub write_as: WriteMode,
    /// Index of the next entry in the `NextSubkey` dictionary chain, if any.
    ///
    /// The main `Next` chain of the C list is represented implicitly by the
    /// order of [`KeyValueList::nodes`]; only the sub-key linkage needs an
    /// explicit pointer to be reproduced exactly.
    pub next_subkey: Option<usize>,
}

/// A property list (`KEYVALUE*` head) — reproduces `IsAvailableOnList` /
/// `AddToList` behaviour over an owned `Vec`.
///
/// Used for a table's `HeaderList` as well as the stream-wide `ValidKeywords`
/// and `ValidSampleID` lists.
#[derive(Debug, Clone, Default)]
pub(crate) struct KeyValueList {
    /// Entries in main-chain (insertion) order.
    pub nodes: Vec<KeyValue>,
}

/// Case-insensitive comparison equivalent to lcms `cmsstrcasecmp` (ASCII
/// `toupper` on each byte).
pub(crate) fn strcasecmp(a: &str, b: &str) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    let mut ai = a.bytes();
    let mut bi = b.bytes();
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let ux = x.to_ascii_uppercase();
                let uy = y.to_ascii_uppercase();
                if ux != uy {
                    return ux.cmp(&uy);
                }
            }
        }
    }
}

/// `cmsstrcasecmp(a, b) == 0`.
fn caseeq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

impl KeyValueList {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Port of `IsAvailableOnList`. Returns `(found, last_idx)` where `last_idx`
    /// tracks the same node the C `LastPtr` would point at (the matched node
    /// when found, otherwise the last node visited — used by [`Self::add`] to
    /// splice the sub-key chain exactly as the reference does).
    ///
    /// A `key` beginning with `'#'` never matches (comment entries are ignored),
    /// mirroring `if (*Key != '#')`.
    pub fn locate(&self, key: &str, subkey: Option<&str>) -> (bool, Option<usize>) {
        let key_is_comment = key.as_bytes().first() == Some(&b'#');

        // Main `Next` chain walk.
        let mut last: Option<usize> = None;
        let mut matched: Option<usize> = None;
        for (i, p) in self.nodes.iter().enumerate() {
            last = Some(i);
            if !key_is_comment && caseeq(key, &p.keyword) {
                matched = Some(i);
                break;
            }
        }

        let mi = match matched {
            None => return (false, last),
            Some(i) => i,
        };

        // Found the key; no sub-key requested.
        let subkey = match subkey {
            None => return (true, Some(mi)),
            Some(s) => s,
        };

        // `NextSubkey` chain walk starting at the matched node. `LastPtr` is only
        // updated for nodes that actually carry a sub-key (`if (p->Subkey == NULL)
        // continue;` skips the update).
        let mut last = Some(mi);
        let mut cur = Some(mi);
        while let Some(i) = cur {
            let node = &self.nodes[i];
            if node.subkey.is_some() {
                last = Some(i);
                if let Some(sk) = &node.subkey {
                    if caseeq(subkey, sk) {
                        return (true, Some(i));
                    }
                }
            }
            cur = node.next_subkey;
        }

        (false, last)
    }

    /// Value for a property (`p->Value`); `None` when absent or the value is
    /// `NULL`, exactly as `cmsIT8GetProperty` returns.
    pub fn get_value(&self, key: &str, subkey: Option<&str>) -> Option<&str> {
        let (found, idx) = self.locate(key, subkey);
        if found {
            self.nodes[idx?].value.as_deref()
        } else {
            None
        }
    }

    /// Port of `AddToList`. Reuses an existing node when the `(key, subkey)`
    /// combination already exists, otherwise appends a new one and splices the
    /// sub-key chain. Returns the affected node index.
    ///
    /// Duplicating `NUMBER_OF_FIELDS` / `NUMBER_OF_SETS` is a hard error, matching
    /// the reference.
    pub fn add(
        &mut self,
        key: &str,
        subkey: Option<&str>,
        value: Option<&str>,
        write_as: WriteMode,
    ) -> Result<usize, Error> {
        let (found, last) = self.locate(key, subkey);

        let idx = if found {
            let p = last.expect("found implies a node index");
            if caseeq(key, "NUMBER_OF_FIELDS") || caseeq(key, "NUMBER_OF_SETS") {
                return Err(Error::new(
                    ErrorCode::CorruptionDetected,
                    format!("duplicate key <{key}>"),
                ));
            }
            p
        } else {
            let was_empty = self.nodes.is_empty();
            let new_idx = self.nodes.len();
            self.nodes.push(KeyValue {
                keyword: key.to_string(),
                subkey: subkey.map(str::to_string),
                value: None,
                write_as,
                next_subkey: None,
            });

            if !was_empty {
                // `if (Subkey != NULL && last != NULL) last->NextSubkey = p;`
                if subkey.is_some() {
                    if let Some(l) = last {
                        self.nodes[l].next_subkey = Some(new_idx);
                    }
                }
                // The `last->Next = p` main-chain splice is implicit in push order.
            }
            new_idx
        };

        self.nodes[idx].write_as = write_as;
        self.nodes[idx].value = value.map(str::to_string);
        Ok(idx)
    }
}

/// One IT8 table (`TABLE`): a sheet type, a property header and a rectangular
/// data grid described by a data-format row.
#[derive(Debug, Clone)]
pub(crate) struct Table {
    /// First line of the sheet (`SheetType`).
    pub sheet_type: String,
    /// Column count (`nSamples`).
    pub n_samples: i32,
    /// Row count (`nPatches`).
    pub n_patches: i32,
    /// Column index of the patch identifier (`SampleID`).
    pub sample_id: i32,
    /// Property header (`HeaderList`).
    pub header: KeyValueList,
    /// Column labels (`DataFormat`); length `n_samples + 1`.
    pub data_format: Option<Vec<Option<String>>>,
    /// Flat row-major data grid (`Data`); stride `n_samples`, length
    /// `(n_samples + 1) * (n_patches + 1)` to tolerate the reference's
    /// off-by-one `SetData` bounds.
    pub data: Option<Vec<Option<String>>>,
}

impl Table {
    /// A fresh empty table with lcms's default sheet type placeholder cleared
    /// (the caller sets `"CGATS.17"` via [`Table::set_sheet_type`]).
    pub fn new() -> Self {
        Self {
            sheet_type: String::new(),
            n_samples: 0,
            n_patches: 0,
            sample_id: 0,
            header: KeyValueList::new(),
            data_format: None,
            data: None,
        }
    }

    /// `cmsIT8SetSheetType` (truncated to `MAXSTR-1`).
    pub fn set_sheet_type(&mut self, ty: &str) {
        self.sheet_type = truncate_str(ty, super::MAXSTR - 1);
    }

    /// `GetDataFormat` — the label of column `n`, if allocated.
    pub fn get_data_format(&self, n: i32) -> Option<&str> {
        if n < 0 {
            return None;
        }
        self.data_format
            .as_ref()
            .and_then(|df| df.get(n as usize))
            .and_then(|s| s.as_deref())
    }

    /// `GetData` — value at `(nSet, nField)` with the reference's bounds check.
    pub fn get_data(&self, n_set: i32, n_field: i32) -> Option<&str> {
        if n_set < 0 || n_set >= self.n_patches || n_field < 0 || n_field >= self.n_samples {
            return None;
        }
        let data = self.data.as_ref()?;
        let idx = n_set as usize * self.n_samples as usize + n_field as usize;
        data.get(idx).and_then(|s| s.as_deref())
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

/// Byte-accurate truncation to at most `max` bytes (keeping a valid UTF-8
/// boundary), mirroring `strncpy(dst, src, max); dst[max-1] = 0`.
pub(crate) fn truncate_str(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.len() < max {
        return s.to_string();
    }
    let mut end = max - 1;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// A safe `atoi` clamped to `±0x7ffffff0` (`satoi` in `cmscgats.c`). Parses an
/// optional sign then leading decimal digits, stopping at the first non-digit.
pub(crate) fn satoi(b: Option<&str>) -> i32 {
    let b = match b {
        None => return 0,
        Some(s) => s,
    };
    let bytes = b.as_bytes();
    let mut i = 0;
    let mut sign: i64 = 1;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        if bytes[i] == b'-' {
            sign = -1;
        }
        i += 1;
    }
    let mut n: i64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        n = n.saturating_mul(10).saturating_add((bytes[i] - b'0') as i64);
        // Keep within i32 range while accumulating to mirror C int overflow-free clamp.
        if n > 0x7fff_ffff {
            n = 0x7fff_ffff;
        }
        i += 1;
    }
    n *= sign;
    if n > 0x7ff_ffff0 {
        return 0x7ff_ffff0;
    }
    if n < -0x7ff_ffff0 {
        return -0x7ff_ffff0;
    }
    n as i32
}

/// Decimal-string → binary-digit-string (`satob`). `NULL`/empty → `"0"`.
pub(crate) fn satob(v: Option<&str>) -> String {
    let x = satoi(v) as u32;
    if x == 0 {
        return "0".to_string();
    }
    let mut buf = Vec::new();
    let mut x = x;
    while x != 0 {
        buf.push(b'0' + (x % 2) as u8);
        x /= 2;
    }
    buf.reverse();
    String::from_utf8(buf).expect("binary digits are ASCII")
}
