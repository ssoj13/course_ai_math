//! The 128-byte ICC profile header: decode (`_cmsReadHeader`), encode (`_cmsWriteHeader`),
//! the version codec (`_validatedVersion`, `BaseToBase`, `cmsGet/SetProfileVersion`), and the
//! per-field header accessors (`cmsGet/SetHeader*`, `cmsGet/SetPCS/ColorSpace/DeviceClass`).
//!
//! Field offsets follow `cmsICCHeader` (lcms2.h): `size@0`, `cmmId@4`, `version@8`,
//! `deviceClass@12`, `colorSpace@16`, `pcs@20`, `date@24` (12 bytes), `magic@36`,
//! `platform@40`, `flags@44`, `manufacturer@48`, `model@52`, `attributes@56` (8 bytes),
//! `renderingIntent@64`, `illuminant@68` (12 bytes), `creator@80`, `profileID@84` (16 bytes),
//! `reserved@100` (28 bytes) — 128 bytes total.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::color::d50_xyz;
use crate::error::{ErrorCode, Result};
use crate::io::endian::{read_datetime, read_u32, read_u64, read_xyz, write_datetime, write_xyz};
use crate::io::handler::IoHandler;
use crate::tags::descriptor::TagDescriptor;
use crate::context::Context;
use crate::tags::get_descriptor_thr;
use crate::types::color::DateTime;
use crate::cms_err;
use crate::types::sig::{Signature, MAGIC_NUMBER};

use super::{Profile, TagData, TagEntry, MAX_TABLE_TAG};

impl Profile {
    // -----------------------------------------------------------------------------------
    // _cmsReadHeader
    // -----------------------------------------------------------------------------------

    /// `_cmsReadHeader` — decode and validate the header + tag directory from [`Self::io`].
    pub(crate) fn read_header(&mut self) -> Result<()> {
        // Share-clone the backing handle (cheap Arc bump) and lock it for the header read.
        // The lock guard borrows the local `io_arc`, not `self`, so the disjoint header fields
        // can be assigned while it is held.
        let io_arc = self
            .io
            .clone()
            .ok_or_else(|| cms_err!(ErrorCode::Read, "no backing store"))?;

        let header_size;
        {
            let mut guard = io_arc
                .lock()
                .map_err(|_| cms_err!(ErrorCode::Internal, "profile io lock poisoned"))?;
            // STRICTER THAN C BY DESIGN. C keeps `ReportedSize` in a `cmsUInt32Number` (`cmsio0.c:295`,
    // `:494`), so it simply cannot represent a >= 4 GiB stream and the case does not arise there.
    // This port's `IoHandler::reported_size` returns `u64`, so a bare `as u32` would WRAP - a
    // 4 GiB+1 file would collapse `header_size`, fail every tag bounds check, and open an empty
    // profile with a corruption signal instead of failing. Clamping keeps the failure loud.
            // Clamp rather than truncate: a >= 4 GiB store whose length mod 2^32 is small would
            // otherwise collapse `header_size` and silently open an empty tag directory.
            let reported_size = guard.reported_size().min(u32::MAX as u64) as u32;
            // --- fixed 128-byte header ---
            // The endian helpers are generic over a `Sized` reader, so feed them `&mut io` (a
            // reborrow of the `&mut dyn IoHandler`) rather than the unsized handler itself.
            let mut io: &mut (dyn IoHandler + Send) = &mut *guard;

            let size = read_u32(&mut io)?; // size@0
            self.cmm = read_u32(&mut io)?; // cmmId@4

            // version@8 — validate the raw big-endian bytes *before* interpreting the word.
            let mut ver = [0u8; 4];
            io.read_exact(&mut ver).map_err(|e| cms_err!(ErrorCode::Read, "version read: {e}"))?;
            self.version = u32::from_be_bytes(validated_version(ver));

            self.device_class = Signature(read_u32(&mut io)?); // deviceClass@12
            self.color_space = Signature(read_u32(&mut io)?); // colorSpace@16
            self.pcs = Signature(read_u32(&mut io)?); // pcs@20
            self.created = read_datetime(&mut io)?; // date@24 (12 bytes)
            let magic = read_u32(&mut io)?; // magic@36
            self.platform = Signature(read_u32(&mut io)?); // platform@40
            self.flags = read_u32(&mut io)?; // flags@44
            self.manufacturer = read_u32(&mut io)?; // manufacturer@48
            self.model = read_u32(&mut io)?; // model@52
            self.attributes = read_u64(&mut io)?; // attributes@56 (8 bytes)
            self.rendering_intent = read_u32(&mut io)?; // renderingIntent@64
            let _illuminant = read_xyz(&mut io)?; // illuminant@68 (12 bytes) — always D50; discarded
            self.creator = read_u32(&mut io)?; // creator@80
            io.read_exact(&mut self.profile_id) // profileID@84 (16 bytes)
                .map_err(|e| cms_err!(ErrorCode::Read, "profile id read: {e}"))?;
            let mut reserved = [0u8; 28]; // reserved@100 (28 bytes)
            io.read_exact(&mut reserved).map_err(|e| cms_err!(ErrorCode::Read, "reserved read: {e}"))?;

            // Validate magic ('acsp').
            if magic != MAGIC_NUMBER.as_u32() {
                return Err(cms_err!(ErrorCode::BadSignature, "not an ICC profile, invalid signature"));
            }
            // Unsupported version / device class are hard errors.
            if self.version > 0x0500_0000 {
                return Err(cms_err!(ErrorCode::UnknownExtension, "unsupported profile version 0x{:x}", self.version));
            }
            if !valid_device_class(self.device_class) {
                return Err(cms_err!(ErrorCode::UnknownExtension, "unsupported device class {}", self.device_class));
            }

            // HeaderSize = min(reported size in header, backing store size).
            let mut hs = size;
            if hs >= reported_size {
                hs = reported_size;
            }
            header_size = hs;
        }

        self.read_tag_directory(header_size)
    }

    /// The tag-directory portion of `_cmsReadHeader`: count, per-entry sanity, link detection,
    /// and the duplicate-tag check.
    fn read_tag_directory(&mut self, header_size: u32) -> Result<()> {
        // Lock the shared backing handle once for the whole directory read. The guard borrows
        // the local `io_arc`, so `self.tags` (link detection / push) stays freely accessible.
        let io_arc = self
            .io
            .clone()
            .ok_or_else(|| cms_err!(ErrorCode::Internal, "backing store present"))?;
        let mut guard = io_arc
            .lock()
            .map_err(|_| cms_err!(ErrorCode::Internal, "profile io lock poisoned"))?;
        let mut io: &mut (dyn IoHandler + Send) = &mut *guard;

        let tag_count = read_u32(&mut io)?;
        if tag_count as usize > MAX_TABLE_TAG {
            return Err(cms_err!(ErrorCode::Range, "Too many tags ({tag_count})"));
        }

        self.tags.clear();
        let mut corrupted_tag_pos = false;

        for _ in 0..tag_count {
            let sig = Signature(read_u32(&mut io)?);
            let offset = read_u32(&mut io)?;
            let size = read_u32(&mut io)?;

            // Sanity: skip empty entries; flag (and skip) out-of-range / overflowing ones.
            if size == 0 || offset == 0 {
                continue;
            }
            match offset.checked_add(size) {
                Some(end) if end <= header_size => {}
                _ => {
                    corrupted_tag_pos = true;
                    continue;
                }
            }

            // Link detection: an identical (offset, size) already present, with a compatible
            // descriptor, means this tag aliases that one. C's loop (`cmsio0.c:960-973`) has no
            // `break`, so it keeps overwriting and the *last* compatible match wins.
            let mut linked = None;
            for existing in &self.tags {
                if let TagData::OnDisk { offset: eo, size: es } = existing.data {
                    if eo == offset
                        && es == size
                        && compatible_types(self.context(), existing.sig, sig)
                    {
                        linked = Some(existing.sig);
                    }
                }
            }

            self.tags.push(TagEntry {
                sig,
                linked,
                save_as_raw: false,
                data: TagData::OnDisk { offset, size },
                type_handler: None,
                // Survives a later read, exactly as C's TagOffsets[]/TagSizes[] do.
                origin: Some((offset, size)),
            });
        }

        // Tags cannot be duplicated.
        for i in 0..self.tags.len() {
            for j in 0..self.tags.len() {
                if i != j && self.tags[i].sig == self.tags[j].sig {
                    return Err(cms_err!(ErrorCode::Range, "Duplicate tag found"));
                }
            }
        }

        if corrupted_tag_pos {
            self.context.signal(ErrorCode::CorruptionDetected, "'size' field in header seems incorrect.");
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------
    // _cmsWriteHeader
    // -----------------------------------------------------------------------------------

    /// `_cmsWriteHeader` — serialize the header + tag directory to `out`.
    ///
    /// `used_space` is stamped into the size field (0 on the first, size-measuring pass);
    /// `offsets`/`sizes` are the per-slot directory values computed by the save (see [`super::save`]).
    /// Generic over the backing store (rather than `&mut dyn IoHandler`) so the caller's
    /// non-`'static` `NullIo`/`MemIo` reference does not fall foul of trait-object variance.
    pub(crate) fn write_header<W: IoHandler + ?Sized>(
        &self,
        mut out: &mut W,
        used_space: u32,
        offsets: &[u32],
        sizes: &[u32],
    ) -> Result<()> {
        let be = |out: &mut W, n: u32| -> Result<()> {
            out.write_all(&n.to_be_bytes()).map_err(|e| cms_err!(ErrorCode::Write, "header field: {e}"))
        };
        be(out, used_space)?; // size@0
        be(out, self.cmm)?; // cmmId@4
        be(out, self.version)?; // version@8
        be(out, self.device_class.as_u32())?; // deviceClass@12
        be(out, self.color_space.as_u32())?; // colorSpace@16
        be(out, self.pcs.as_u32())?; // pcs@20
        write_datetime(&mut out, &self.created)?; // date@24
        be(out, MAGIC_NUMBER.as_u32())?; // magic@36
        be(out, self.platform.as_u32())?; // platform@40
        be(out, self.flags)?; // flags@44
        be(out, self.manufacturer)?; // manufacturer@48
        be(out, self.model)?; // model@52
        out.write_all(&self.attributes.to_be_bytes()) // attributes@56
            .map_err(|e| cms_err!(ErrorCode::Write, "attributes: {e}"))?;
        be(out, self.rendering_intent)?; // renderingIntent@64
        write_xyz(&mut out, &d50_xyz())?; // illuminant@68 — always D50
        be(out, self.creator)?; // creator@80
        out.write_all(&self.profile_id).map_err(|e| cms_err!(ErrorCode::Write, "profile id: {e}"))?; // 84
        out.write_all(&[0u8; 28]).map_err(|e| cms_err!(ErrorCode::Write, "reserved: {e}"))?; // 100

        // --- tag directory ---
        let count = self.tags.iter().filter(|t| t.sig.as_u32() != 0).count() as u32;
        be(out, count)?;
        for (i, tag) in self.tags.iter().enumerate() {
            if tag.sig.as_u32() == 0 {
                continue; // deleted placeholder
            }
            be(out, tag.sig.as_u32())?;
            be(out, offsets[i])?;
            be(out, sizes[i])?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------------------
    // Header accessors (cmsGet/SetHeader*, cmsGet/SetPCS/ColorSpace/DeviceClass, versions)
    // -----------------------------------------------------------------------------------

    /// `cmsGetHeaderCMM`.
    pub fn header_cmm(&self) -> u32 {
        self.cmm
    }
    /// `_cmsSetHeaderCMM`.
    pub fn set_header_cmm(&mut self, cmm: u32) {
        self.cmm = cmm;
    }

    /// `cmsGetHeaderRenderingIntent`.
    pub fn header_rendering_intent(&self) -> u32 {
        self.rendering_intent
    }
    /// `cmsSetHeaderRenderingIntent`.
    pub fn set_header_rendering_intent(&mut self, intent: u32) {
        self.rendering_intent = intent;
    }

    /// `cmsGetHeaderFlags`.
    pub fn header_flags(&self) -> u32 {
        self.flags
    }
    /// `cmsSetHeaderFlags`.
    pub fn set_header_flags(&mut self, flags: u32) {
        self.flags = flags;
    }

    /// `cmsGetHeaderManufacturer`.
    pub fn header_manufacturer(&self) -> u32 {
        self.manufacturer
    }
    /// `cmsSetHeaderManufacturer`.
    pub fn set_header_manufacturer(&mut self, m: u32) {
        self.manufacturer = m;
    }

    /// `cmsGetHeaderModel`.
    pub fn header_model(&self) -> u32 {
        self.model
    }
    /// `cmsSetHeaderModel`.
    pub fn set_header_model(&mut self, m: u32) {
        self.model = m;
    }

    /// `cmsGetHeaderCreator`.
    pub fn header_creator(&self) -> u32 {
        self.creator
    }

    /// `cmsGetHeaderAttributes`.
    pub fn header_attributes(&self) -> u64 {
        self.attributes
    }
    /// `cmsSetHeaderAttributes`.
    pub fn set_header_attributes(&mut self, a: u64) {
        self.attributes = a;
    }

    /// `cmsGetHeaderProfileID` — the 16-byte profile ID.
    pub fn header_profile_id(&self) -> [u8; 16] {
        self.profile_id
    }
    /// `cmsSetHeaderProfileID`.
    pub fn set_header_profile_id(&mut self, id: [u8; 16]) {
        self.profile_id = id;
    }
    /// `cmsGetProfileID` — alias exposing the profile ID (same field lcms hashes into).
    pub fn profile_id(&self) -> [u8; 16] {
        self.profile_id
    }

    /// `cmsGetHeaderCreationDateTime`.
    pub fn header_creation_datetime(&self) -> DateTime {
        self.created
    }

    /// `cmsGetPCS`.
    pub fn pcs(&self) -> Signature {
        self.pcs
    }
    /// `cmsSetPCS`.
    pub fn set_pcs(&mut self, pcs: Signature) {
        self.pcs = pcs;
    }

    /// `cmsGetColorSpace`.
    pub fn color_space(&self) -> Signature {
        self.color_space
    }
    /// `cmsSetColorSpace`.
    pub fn set_color_space(&mut self, cs: Signature) {
        self.color_space = cs;
    }

    /// `cmsGetDeviceClass`.
    pub fn device_class(&self) -> Signature {
        self.device_class
    }
    /// `cmsSetDeviceClass`.
    pub fn set_device_class(&mut self, cl: Signature) {
        self.device_class = cl;
    }

    /// `cmsGetHeaderPlatform` — the primary-platform signature.
    pub fn platform(&self) -> Signature {
        self.platform
    }

    /// `cmsGetEncodedICCversion` — the raw encoded version word (e.g. `0x02100000`).
    pub fn encoded_icc_version(&self) -> u32 {
        self.version
    }
    /// `cmsSetEncodedICCversion`.
    pub fn set_encoded_icc_version(&mut self, version: u32) {
        self.version = version;
    }

    /// `cmsGetProfileVersion` — the decimal version (e.g. `4.3`) decoded from the word.
    pub fn profile_version(&self) -> f64 {
        let n = self.version >> 16;
        base_to_base(n, 16, 10) as f64 / 100.0
    }

    /// `cmsSetProfileVersion` — encode a decimal version (e.g. `4.2 -> 0x04200000`).
    pub fn set_profile_version(&mut self, version: f64) {
        let scaled = (version * 100.0 + 0.5).floor() as u32;
        self.version = base_to_base(scaled, 10, 16) << 16;
    }
}

// ---------------------------------------------------------------------------------------
// Free helpers (_validatedVersion, validDeviceClass, BaseToBase, CompatibleTypes, _cmsGetTime)
// ---------------------------------------------------------------------------------------

/// `_validatedVersion` — clamp the raw big-endian version bytes to the spec.
///
/// Byte 0 is the BCD major (max 9); byte 1 holds two BCD nibbles (each clamped); bytes 2 and 3
/// are reserved and forced to zero. Operates on the on-disk byte order, before the word is
/// interpreted (which the caller does with [`u32::from_be_bytes`]).
fn validated_version(mut b: [u8; 4]) -> [u8; 4] {
    if b[0] > 0x09 {
        b[0] = 0x09;
    }
    let mut hi = b[1] & 0xf0;
    let mut lo = b[1] & 0x0f;
    if hi > 0x90 {
        hi = 0x90;
    }
    if lo > 0x09 {
        lo = 0x09;
    }
    b[1] = hi | lo;
    b[2] = 0;
    b[3] = 0;
    b
}

/// `validDeviceClass` — accept the known profile classes (and zero, which old lcms defaulted to).
fn valid_device_class(cl: Signature) -> bool {
    if cl.as_u32() == 0 {
        return true;
    }
    // The eleven `cmsSig*Class` values (lcms2.h).
    let known: [Signature; 11] = [
        Signature::new(b"scnr"), // Input
        Signature::new(b"mntr"), // Display
        Signature::new(b"prtr"), // Output
        Signature::new(b"link"), // Link
        Signature::new(b"abst"), // Abstract
        Signature::new(b"spac"), // ColorSpace
        Signature::new(b"nmcl"), // NamedColor
        Signature::new(b"cenc"), // ColorEncodingSpace
        Signature::new(b"mid "), // MultiplexIdentification
        Signature::new(b"mlnk"), // MultiplexLink
        Signature::new(b"mvis"), // MultiplexVisualization
    ];
    known.contains(&cl)
}

/// `BaseToBase` — reinterpret the digits of `input` (base `base_in`) as base `base_out`.
///
/// Used only by the version codec (hex digits treated as decimal and back).
fn base_to_base(mut input: u32, base_in: u32, base_out: u32) -> u32 {
    let mut buff = [0u8; 100];
    let mut len = 0usize;
    while input > 0 && len < 100 {
        buff[len] = (input % base_in) as u8;
        input /= base_in;
        len += 1;
    }
    let mut out: u32 = 0;
    for i in (0..len).rev() {
        out = out * base_out + buff[i] as u32;
    }
    out
}

/// `CompatibleTypes` — do two tags share the same descriptor shape (element count + supported
/// type list, in order)? Absent descriptors are never compatible.
fn compatible_types(ctx: &Context, a: Signature, b: Signature) -> bool {
    match (get_descriptor_thr(ctx, a), get_descriptor_thr(ctx, b)) {
        (Some(da), Some(db)) => descriptors_match(&da, &db),
        _ => false,
    }
}

/// Structural equality of two descriptors (`nSupportedTypes`, `ElemCount`, and each type).
fn descriptors_match(a: &TagDescriptor, b: &TagDescriptor) -> bool {
    a.elem_count == b.elem_count && a.supported_types == b.supported_types
}

/// `_cmsGetTime` — the current UTC time as an ICC [`DateTime`].
///
/// ICC v4 stores the timestamp in UTC. The epoch-seconds → civil-date conversion is Howard
/// Hinnant's `civil_from_days`, valid across the whole `i64` day range with no leap-second or
/// timezone dependence.
pub(super) fn utc_now() -> DateTime {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    DateTime {
        year: year as u16,
        month: month as u16,
        day: day as u16,
        hours: (tod / 3600) as u16,
        minutes: ((tod % 3600) / 60) as u16,
        seconds: (tod % 60) as u16,
    }
}

/// Convert days-since-1970-01-01 into a `(year, month, day)` civil date (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
