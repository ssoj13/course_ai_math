//! ICC profile object, tag dictionary, and tag read/write — a pure-Rust port of `cmsio0.c`.
//!
//! This is the layer that ties the endian I/O ([`crate::io`]) and the tag-type serializers
//! ([`crate::tags`]) into a whole ICC profile: opening a profile from memory/file/`IoHandler`,
//! the in-memory tag dictionary, on-demand cooked/raw tag reads, tag writing/linking, the
//! 128-byte header accessors, the two-pass serializer, and `cmsMD5computeID`.
//!
//! ## What replaces the C parallel arrays
//! `_cmsICCPROFILE` (lcms2_internal.h) keeps seven fixed-size parallel arrays
//! (`TagNames`/`TagLinked`/`TagSizes`/`TagOffsets`/`TagSaveAsRaw`/`TagPtrs`/`TagTypeHandlers`)
//! indexed 0..`TagCount`, capped at `MAX_TABLE_TAG == 100`. Here that becomes one typed
//! `Vec<TagEntry>` ([`Profile::tags`]); a [`TagEntry`] holds the signature, optional link,
//! the raw flag, the payload ([`TagData`]) and the recorded type handler. A **deleted** tag
//! keeps its slot with `sig == 0` (matching C's "zero as a mark of deleted"), so positions
//! and [`Profile::tag_count`] stay in step with the reference.
//!
//! ## On-demand reads
//! A profile opened for reading keeps its backing store in [`Profile::io`]; a tag whose bytes
//! have not been decoded yet is [`TagData::OnDisk`] (its original offset/size). The first
//! [`Profile::read_tag`] seeks there, decodes via [`crate::tags`], and caches the result as
//! [`TagData::Cooked`] — exactly as `cmsReadTag` promotes `TagPtrs[n]` from `NULL`.
//!
//! Header serialization lives in [`header`]; the two-pass save and `computeID` in [`save`].

use std::sync::{Arc, Mutex};

use crate::cms_err;
use crate::context::Context;
use crate::error::{ErrorCode, Result};
use crate::io::handler::{FileIo, IoHandler, MemIo};
use crate::tags::descriptor::TagDescriptor;
use crate::tags::handler::{read_type_base, seek_to};
use crate::tags::{get_descriptor_thr, get_handler_thr, TagValue, TypeHandler};
use crate::types::color::DateTime;
use crate::types::sig::Signature;

mod header;
pub mod read_hl;
mod save;

/// Maximum supported tags in a profile (`MAX_TABLE_TAG`, lcms2_internal.h).
pub const MAX_TABLE_TAG: usize = 100;

/// Maximum number of type signatures checked per tag (`MAX_TYPES_IN_LCMS_PLUGIN`).
const MAX_TYPES_IN_LCMS_PLUGIN: usize = 20;

/// The CMM signature LittleCMS stamps into profiles it creates (`lcmsSignature`, `'lcms'`).
pub const LCMS_SIGNATURE: u32 = 0x6c63_6d73;

/// Primary-platform signature `cmsSigMacintosh` (`'APPL'`).
const SIG_MACINTOSH: u32 = 0x4150_504c;
/// Primary-platform signature `cmsSigMicrosoft` (`'MSFT'`).
const SIG_MICROSOFT: u32 = 0x4d53_4654;
/// Default device class `cmsSigDisplayClass` (`'mntr'`).
const SIG_DISPLAY_CLASS: Signature = Signature::new(b"mntr");

/// The decoded payload of one tag — the typed replacement for `TagPtrs[i]` (an opaque `void*`).
///
/// The `Cooked` variant wraps a [`TagValue`], which is deliberately unboxed (see its own note):
/// a `TagData` is built once per tag on read/write, never in a hot loop, so boxing to shrink the
/// enum would only obscure the 1:1 mapping to lcms's decoded objects.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum TagData {
    /// No payload: a link entry or a deleted placeholder (C's `TagPtrs[i] == NULL`).
    None,
    /// A decoded, in-memory value (`cmsReadTag`/`cmsWriteTag` cooked data).
    Cooked(TagValue),
    /// Raw bytes to be written verbatim (`TagSaveAsRaw`, from `cmsWriteRawTag`).
    Raw(Vec<u8>),
    /// Not yet read: the original on-disk `(offset, size)` in the backing store.
    OnDisk {
        /// Byte offset of the tag data in [`Profile::io`] (`TagOffsets[i]`).
        offset: u32,
        /// Byte size of the tag data on disk (`TagSizes[i]`).
        size: u32,
    },
}

/// One tag-dictionary entry — replaces one column across C's seven parallel `Tag*` arrays.
#[derive(Debug, Clone)]
pub struct TagEntry {
    /// The tag signature (`TagNames[i]`); `Signature(0)` marks a deleted placeholder.
    pub sig: Signature,
    /// The tag this one is linked to, if any (`TagLinked[i]`, `0` = none).
    pub linked: Option<Signature>,
    /// Write the payload uncooked (`TagSaveAsRaw[i]`).
    pub save_as_raw: bool,
    /// The payload (`TagPtrs[i]` + its on-disk `TagOffsets`/`TagSizes`).
    pub data: TagData,
    /// The type handler recorded for the payload (`TagTypeHandlers[i]`).
    pub type_handler: Option<TypeHandler>,
    /// Where this tag came from on disk (`TagOffsets[i]`/`TagSizes[i]`), independent of whether
    /// the payload has since been decoded.
    ///
    /// C keeps those two arrays alive across a read: `cmsReadTag` populates `TagPtrs[i]` but
    /// never clears `TagOffsets[i]`, and only `cmsWriteTag` (`cmsio0.c:2003`) and
    /// `cmsWriteRawTag` (`:2210`) zero it. Folding the offsets into [`TagData::OnDisk`] alone
    /// would destroy them the moment a tag is read, so `cmsGetTagOffsetAndSize` would report
    /// `(0, 0)` for every tag anyone had looked at.
    ///
    /// Note this does NOT affect saving: C's blind-copy path is gated on `TagPtrs[i] == NULL`
    /// (`cmsio0.c:1431`), so a tag that has been read is re-serialized there too.
    pub origin: Option<(u32, u32)>,
}

impl TagEntry {
    /// A fresh placeholder for `sig` with no payload (a slot `_cmsNewTag` just claimed).
    fn placeholder(sig: Signature) -> Self {
        Self {
            sig,
            linked: None,
            save_as_raw: false,
            data: TagData::None,
            type_handler: None,
            origin: None,
        }
    }
}

/// An ICC profile (`_cmsICCPROFILE` / `cmsHPROFILE`).
///
/// Holds the decoded 128-byte header fields, the tag dictionary, and — for a profile opened
/// for reading — the backing store used to satisfy on-demand tag reads and blind tag copies
/// during save.
///
/// [`Clone`] is a **deep copy of the tag dictionary** but a **shared** backing store: the
/// `io` handle is an `Arc<Mutex<..>>`, so a clone reads the same on-disk bytes (each read is a
/// lock+seek+read) while owning an independent, cache-able tag dictionary. This is what lets a
/// single profile be aliased at two slots of a transform chain — e.g. the black-point
/// round-trip `[Lab, prof, prof, Lab]` (`cmssamp.c`), which the `&mut [&mut Profile]` linking
/// API otherwise forbids.
#[derive(Clone)]
pub struct Profile {
    // --- header (cmsICCHeader, decoded) ---
    /// Creation date/time (`Created`).
    created: DateTime,
    /// CMM signature (`CMM`).
    cmm: u32,
    /// Encoded format version, e.g. `0x02100000` (`Version`).
    version: u32,
    /// Profile/device class (`DeviceClass`).
    device_class: Signature,
    /// Data colour space (`ColorSpace`).
    color_space: Signature,
    /// Profile connection space (`PCS`).
    pcs: Signature,
    /// Header rendering intent (`RenderingIntent`).
    rendering_intent: u32,
    /// Primary platform (`platform`).
    platform: Signature,
    /// Header flags (`flags`).
    flags: u32,
    /// Device manufacturer (`manufacturer`).
    manufacturer: u32,
    /// Device model (`model`).
    model: u32,
    /// Device attributes (`attributes`).
    attributes: u64,
    /// Profile creator (`creator`).
    creator: u32,
    /// 16-byte profile ID / MD5 fingerprint (`ProfileID`).
    profile_id: [u8; 16],

    // --- dictionary + backing ---
    /// The tag dictionary (replaces the parallel `Tag*` arrays).
    tags: Vec<TagEntry>,
    /// Backing store for on-demand `OnDisk` reads / blind tag copies (`IOhandler`).
    ///
    /// Behind an `Arc<Mutex<..>>` so [`Profile`] can be [`Clone`]d cheaply while the clones
    /// share one seekable byte source (see the type-level note). The `+ Send` bound keeps
    /// `Profile` `Send + Sync`; all backends ([`MemIo`]/[`FileIo`]/[`NullIo`]) are `Send`.
    io: Option<Arc<Mutex<dyn IoHandler + Send>>>,
    /// The owning context (`ContextID`).
    context: Context,
}

impl Profile {
    // -----------------------------------------------------------------------------------
    // Construction / open (cmsCreateProfilePlaceholder, cmsOpenProfileFrom*)
    // -----------------------------------------------------------------------------------

    /// `cmsCreateProfilePlaceholder` — an empty profile with lcms's defaults.
    ///
    /// Version 2.1, CMM and creator `'lcms'`, display class, the host platform, and the
    /// current time as the creation date.
    pub fn create_placeholder(context: Context) -> Self {
        let platform = if cfg!(windows) { SIG_MICROSOFT } else { SIG_MACINTOSH };
        Self {
            created: header::utc_now(),
            cmm: LCMS_SIGNATURE,
            version: 0x0210_0000,
            device_class: SIG_DISPLAY_CLASS,
            color_space: Signature(0),
            pcs: Signature(0),
            rendering_intent: 0,
            platform: Signature(platform),
            flags: 0,
            manufacturer: 0,
            model: 0,
            attributes: 0,
            creator: LCMS_SIGNATURE,
            profile_id: [0u8; 16],
            tags: Vec::new(),
            io: None,
            context,
        }
    }

    /// `cmsOpenProfileFromIOhandlerTHR` — read a profile header from a backing store.
    ///
    /// The handler is wrapped in an `Arc<Mutex<..>>` so the profile (and its clones) can share
    /// it; it must be `Send` so [`Profile`] stays `Send + Sync`.
    pub fn open_from_io<T: IoHandler + Send + 'static>(context: Context, io: T) -> Result<Self> {
        let mut p = Self::create_placeholder(context);
        let handler: Arc<Mutex<dyn IoHandler + Send>> = Arc::new(Mutex::new(io));
        p.io = Some(handler);
        p.read_header()?;
        Ok(p)
    }

    /// `cmsOpenProfileFromMemTHR` — open a profile from an in-memory ICC image.
    ///
    /// The bytes are copied into an owned [`MemIo`], so the caller may free their buffer
    /// afterwards (matching the C reader, which copies the block).
    pub fn open_from_mem(context: Context, bytes: &[u8]) -> Result<Self> {
        let io = MemIo::from_bytes(bytes.to_vec());
        Self::open_from_io(context, io)
    }

    /// `cmsOpenProfileFromFileTHR` — open a profile from a disk file (read access).
    pub fn open_from_file<P: AsRef<std::path::Path>>(context: Context, path: P) -> Result<Self> {
        let io = FileIo::open_read(path)?;
        Self::open_from_io(context, io)
    }

    /// The owning context (`cmsGetProfileContextID`).
    pub fn context(&self) -> &Context {
        &self.context
    }

    // -----------------------------------------------------------------------------------
    // Tag directory primitives (SearchOneTag, _cmsSearchTag, _cmsNewTag)
    // -----------------------------------------------------------------------------------

    /// `SearchOneTag` — position of `sig` in the dictionary, or `None`.
    fn search_one(&self, sig: Signature) -> Option<usize> {
        self.tags.iter().position(|t| t.sig == sig)
    }

    /// `_cmsSearchTag` — find `sig`, optionally following links to the resolved data tag.
    fn search_tag(&self, sig: Signature, follow_links: bool) -> Option<usize> {
        let mut sig = sig;
        loop {
            let n = self.search_one(sig)?;
            if !follow_links {
                return Some(n);
            }
            match self.tags[n].linked {
                // A link: resolve to its target and search again (C's `sig = LinkedSig`).
                Some(linked) => sig = linked,
                None => return Some(n),
            }
        }
    }

    /// `_cmsNewTag` — the slot for `sig`, reusing (and clearing) an existing one or appending.
    ///
    /// Returns the position; the caller fills the entry. Appending past [`MAX_TABLE_TAG`] is a
    /// range error, exactly like the C cap.
    fn new_tag(&mut self, sig: Signature) -> Result<usize> {
        if let Some(i) = self.search_one(sig) {
            // Already exists: drop the previous payload, reuse the slot.
            self.tags[i] = TagEntry::placeholder(sig);
            return Ok(i);
        }
        if self.tags.len() >= MAX_TABLE_TAG {
            return Err(cms_err!(ErrorCode::Range, "Too many tags ({MAX_TABLE_TAG})"));
        }
        self.tags.push(TagEntry::placeholder(sig));
        Ok(self.tags.len() - 1)
    }

    // -----------------------------------------------------------------------------------
    // Existence / directory queries (cmsIsTag, cmsGetTagCount, cmsGetTagSignature)
    // -----------------------------------------------------------------------------------

    /// `cmsIsTag` — is `sig` present in the dictionary (without following links)?
    pub fn is_tag(&self, sig: Signature) -> bool {
        self.search_tag(sig, false).is_some()
    }

    /// `cmsGetTagCount` — number of dictionary slots (including deleted placeholders).
    pub fn tag_count(&self) -> usize {
        self.tags.len()
    }

    /// `cmsGetTagSignature` — the signature of slot `n`, or `Signature(0)` if out of range.
    pub fn tag_signature(&self, n: usize) -> Signature {
        self.tags.get(n).map(|t| t.sig).unwrap_or(Signature(0))
    }

    /// `cmsTagLinkedTo` — the tag `sig` is linked to, or `None`.
    pub fn tag_linked_to(&self, sig: Signature) -> Option<Signature> {
        let i = self.search_tag(sig, false)?;
        self.tags[i].linked
    }

    /// Read-only view of the tag dictionary.
    pub fn tags(&self) -> &[TagEntry] {
        &self.tags
    }

    // -----------------------------------------------------------------------------------
    // Reading tags (cmsReadTag, cmsReadRawTag, _cmsGetTagTrueType)
    // -----------------------------------------------------------------------------------

    /// `cmsReadTag` — decode (or return the cached) value for `sig`, following links.
    ///
    /// Returns the decoded [`TagValue`]; an `OnDisk` tag is read from the backing store and
    /// cached as `Cooked` on first access. Errors mirror the C `NULL` returns: tag absent,
    /// unsupported type, raw-only payload, truncated data, or a corrupt element count.
    pub fn read_tag(&mut self, sig: Signature) -> Result<TagValue> {
        let n = self
            .search_tag(sig, true)
            .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "tag not found"))?;

        match &self.tags[n].data {
            TagData::Cooked(value) => {
                // C `cmsio0.c:1774-1785`: the already-in-memory branch runs the same
                // descriptor-driven checks as the on-disk one.
                self.check_cooked(n, sig)?;
                Ok(value.clone())
            }
            TagData::Raw(_) => {
                // C: "We don't support read raw tags as cooked" -> goto Error.
                Err(cms_err!(ErrorCode::NotSuitable, "tag was written raw; cannot read as cooked"))
            }
            TagData::OnDisk { offset, size } => {
                let (offset, tag_size) = (*offset, *size);
                self.read_ondisk_tag(n, sig, offset, tag_size)
            }
            TagData::None => Err(cms_err!(ErrorCode::NotSuitable, "tag has no data")),
        }
    }

    /// Decode an `OnDisk` tag from the backing store and cache it (the body of `cmsReadTag`'s
    /// "we need to read it" path).
    fn read_ondisk_tag(
        &mut self,
        n: usize,
        sig: Signature,
        offset: u32,
        tag_size: u32,
    ) -> Result<TagValue> {
        if tag_size < 8 {
            return Err(cms_err!(ErrorCode::CorruptionDetected, "tag smaller than a type base"));
        }
        // A built-in profile with no backing store cannot serve on-disk reads (C's early abort).
        // Clone the shared handle (cheap Arc bump) and lock it for the read; cloning avoids
        // borrowing `self`, so the tag slot can be updated afterwards.
        let io_arc = self
            .io
            .clone()
            .ok_or_else(|| cms_err!(ErrorCode::CorruptionDetected, "corrupted built-in profile"))?;

        // C `cmsio0.c:1811-1824`: the descriptor is resolved after the seek, and a missing one
        // is fatal unless a signature-0 tag plugin disabled the checks.
        let descriptor = resolve_descriptor(self.context(), sig)?;

        let (value, handler) = {
            let mut guard = io_arc
                .lock()
                .map_err(|_| cms_err!(ErrorCode::Internal, "profile io lock poisoned"))?;
            let mut io: &mut (dyn IoHandler + Send) = &mut *guard;

            seek_to(&mut *io, offset)?;
            let base_type = read_type_base(&mut io)?;

            // C `cmsio0.c:1830-1832`.
            if let Some(desc) = &descriptor {
                if !is_type_supported(desc, base_type) {
                    return Err(cms_err!(ErrorCode::UnknownExtension, "type {base_type} not supported for {sig}"));
                }
            }

            let handler = get_handler_thr(self.context(), base_type)
                .ok_or_else(|| cms_err!(ErrorCode::UnknownExtension, "no handler for type {base_type}"))?;

            let body = tag_size - 8; // The type base is already consumed.
            let value = handler.read_thr(self.context(), &mut io, body)?;

            // The number of decoded items must meet the descriptor minimum (chad/arts want 9).
            // C `cmsio0.c:1860-1872` — inside the same `if (!avoidCheck)` gate as the two checks
            // above, which `resolve_descriptor` expresses as a `None` descriptor.
            if let Some(desc) = &descriptor {
                if elem_count(&value) < desc.elem_count {
                    return Err(cms_err!(
                        ErrorCode::CorruptionDetected,
                        "'{sig}' inconsistent item count: expected {}, got {}",
                        desc.elem_count,
                        elem_count(&value)
                    ));
                }
            }
            (value, handler)
        };

        // Cache the cooked value + its handler (C sets TagPtrs[n] / TagTypeHandlers[n]).
        self.tags[n].data = TagData::Cooked(value.clone());
        self.tags[n].type_handler = Some(handler);
        Ok(value)
    }

    /// `cmsReadRawTag` — the raw on-disk (or serialized) bytes of `sig`.
    ///
    /// For an untouched on-disk tag the original bytes are returned; for a raw-written tag its
    /// stored block; for a cooked tag the value is serialized (type base + body), reproducing
    /// the bytes it would occupy in the profile.
    pub fn read_raw_tag(&mut self, sig: Signature) -> Result<Vec<u8>> {
        let i = self
            .search_tag(sig, true)
            .ok_or_else(|| cms_err!(ErrorCode::NotSuitable, "tag not found"))?;

        match &self.tags[i].data {
            TagData::OnDisk { offset, size } => {
                let (offset, size) = (*offset, *size);
                let io_arc = self
                    .io
                    .clone()
                    .ok_or_else(|| cms_err!(ErrorCode::CorruptionDetected, "no backing store"))?;
                let mut guard = io_arc
                    .lock()
                    .map_err(|_| cms_err!(ErrorCode::Internal, "profile io lock poisoned"))?;
                let io: &mut (dyn IoHandler + Send) = &mut *guard;
                seek_to(&mut *io, offset)?;
                let mut buf = vec![0u8; size as usize];
                io.read_exact(&mut buf)
                    .map_err(|e| cms_err!(ErrorCode::Read, "raw tag read: {e}"))?;
                Ok(buf)
            }
            TagData::Raw(bytes) => Ok(bytes.clone()),
            TagData::Cooked(value) => {
                // C reaches this path via `cmsReadTag` (`cmsio0.c:2104`), so the cooked sanity
                // check applies here too, and then requires a descriptor unconditionally — the
                // `avoidCheck` gate does not extend to `cmsReadRawTag` (`cmsio0.c:2120-2124`).
                let handler = self.check_cooked(i, sig)?;
                descriptor_or_err(self.context(), sig)?;
                let value = value.clone();
                let version = self.profile_version();
                let mut mem = MemIo::new_write();
                crate::tags::handler::write_type_base(&mut mem, handler.signature())?;
                handler.write(&mut mem, &value, version)?;
                Ok(mem.into_inner())
            }
            TagData::None => Ok(Vec::new()),
        }
    }

    /// The `cmsReadTag` already-in-memory sanity check (`cmsio0.c:1774-1785`).
    ///
    /// The cached value must still carry its type handler, and — unless the descriptor-driven
    /// checks are disabled — that handler's type must be one the tag's descriptor accepts.
    /// Returns the handler so callers that need it (`read_raw_tag`) do not look it up twice.
    fn check_cooked(&self, n: usize, sig: Signature) -> Result<TypeHandler> {
        let handler = self.tags[n]
            .type_handler
            .ok_or_else(|| cms_err!(ErrorCode::Internal, "cooked tag lost its handler"))?;
        let base_type = handler.signature();
        if let Some(desc) = resolve_descriptor(self.context(), sig)? {
            if !is_type_supported(&desc, base_type) {
                return Err(cms_err!(ErrorCode::UnknownExtension, "type {base_type} not supported for {sig}"));
            }
        }
        Ok(handler)
    }

    /// `cmsGetTagOffsetAndSize` — the on-disk `(offset, size)` of dictionary slot `n`.
    ///
    /// `None` when `n` is out of range (C's `FALSE`, `cmsio0.c:665-675`); `(0, 0)` for a slot
    /// that never came from disk or whose payload has since been replaced by `write_tag` /
    /// `write_raw_tag`, matching the only two places C zeroes `TagOffsets[i]`
    /// (`cmsio0.c:2003`, `:2210`).
    ///
    /// Reading a tag does NOT clear it — see [`TagEntry::origin`].
    pub fn tag_offset_and_size(&self, n: usize) -> Option<(u32, u32)> {
        Some(self.tags.get(n)?.origin.unwrap_or((0, 0)))
    }

    /// `_cmsGetTagTrueType` — the true on-disk type of a (read) tag, or `None`.
    pub fn tag_true_type(&self, sig: Signature) -> Option<Signature> {
        let n = self.search_tag(sig, true)?;
        self.tags[n].type_handler.map(|h| h.signature())
    }

    // -----------------------------------------------------------------------------------
    // Writing tags (cmsWriteTag, cmsWriteRawTag, cmsLinkTag)
    // -----------------------------------------------------------------------------------

    /// `cmsWriteTag` — store a decoded value for `sig` (replacing any previous entry).
    ///
    /// The on-disk type is chosen from the profile version and the value via the tag
    /// descriptor's `DecideType` (or its first supported type). An unsupported tag or type is
    /// an error, mirroring the C signals.
    pub fn write_tag(&mut self, sig: Signature, value: TagValue) -> Result<()> {
        let descriptor = get_descriptor_thr(self.context(), sig)
            .ok_or_else(|| cms_err!(ErrorCode::UnknownExtension, "unsupported tag {sig}"))?;

        let version = self.profile_version();
        // C indexes a fixed `SupportedTypes[MAX_TYPES_IN_LCMS_PLUGIN]` array (`cmsio0.c:1974`), so
        // a plugin descriptor with an empty list yields garbage there; the `IsTypeSupported` check
        // that follows then rejects it with `cmsERROR_UNKNOWN_EXTENSION` (`:1978-1985`). A `Vec`
        // would panic instead, so the empty case short-circuits to that same error.
        let type_sig = match descriptor.decide_type {
            Some(decide) => decide(version, &value),
            None => *descriptor
                .supported_types
                .first()
                .ok_or_else(|| cms_err!(ErrorCode::UnknownExtension, "tag {sig} has no supported types"))?,
        };

        if !is_type_supported(&descriptor, type_sig) {
            return Err(cms_err!(ErrorCode::UnknownExtension, "unsupported type {type_sig} for tag {sig}"));
        }
        let handler = get_handler_thr(self.context(), type_sig)
            .ok_or_else(|| cms_err!(ErrorCode::UnknownExtension, "no handler for type {type_sig}"))?;

        let i = self.new_tag(sig)?;
        self.tags[i] = TagEntry {
            sig,
            linked: None,
            save_as_raw: false,
            data: TagData::Cooked(value),
            type_handler: Some(handler),
            // `cmsWriteTag` zeroes TagOffsets[i]/TagSizes[i] (`cmsio0.c:2003`): the slot no
            // longer corresponds to any region of the original file.
            origin: None,
        };
        Ok(())
    }

    /// `cmsWriteTag` with `data == NULL` — delete `sig`, keeping its slot as a placeholder.
    ///
    /// Returns whether the tag was present (the C `TRUE`/`FALSE`).
    pub fn delete_tag(&mut self, sig: Signature) -> bool {
        match self.search_tag(sig, false) {
            Some(i) => {
                // Zero marks the slot deleted (kept so positions stay stable).
                self.tags[i] = TagEntry::placeholder(Signature(0));
                true
            }
            None => false,
        }
    }

    /// `cmsWriteRawTag` — store a block of bytes for `sig`, to be written verbatim.
    pub fn write_raw_tag(&mut self, sig: Signature, data: &[u8]) -> Result<()> {
        let i = self.new_tag(sig)?;
        self.tags[i] = TagEntry {
            sig,
            linked: None,
            save_as_raw: true,
            data: TagData::Raw(data.to_vec()),
            type_handler: None,
            // `cmsWriteRawTag` zeroes TagOffsets[i]/TagSizes[i] (`cmsio0.c:2210`).
            origin: None,
        };
        Ok(())
    }

    /// `cmsLinkTag` — make `sig` share the on-disk block of `dest`.
    pub fn link_tag(&mut self, sig: Signature, dest: Signature) -> Result<()> {
        let i = self.new_tag(sig)?;
        self.tags[i] = TagEntry {
            sig,
            linked: Some(dest),
            save_as_raw: false,
            data: TagData::None,
            type_handler: None,
            origin: None,
        };
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// Free helpers (IsTypeSupported + the ReadPtr element-count bridge)
// ---------------------------------------------------------------------------------------

/// `_cmsGetTagDescriptor` with the C `NULL`-is-fatal contract (`cmsio0.c:1814-1823`).
fn descriptor_or_err(ctx: &Context, sig: Signature) -> Result<TagDescriptor> {
    get_descriptor_thr(ctx, sig)
        .ok_or_else(|| cms_err!(ErrorCode::UnknownExtension, "unknown tag {sig}"))
}

/// The descriptor `cmsReadTag` validates against, or `None` when validation is off.
///
/// C wraps every descriptor-driven check — lookup, `IsTypeSupported`, and the `ElemCount`
/// comparison — in `if (!avoidCheck)` (`cmsio0.c:1780`, `:1811`, `:1830`, `:1860`). Folding that
/// gate into the lookup gives all three call sites one shared policy: `None` means "a
/// signature-0 tag plugin turned the checks off", never "no descriptor exists".
fn resolve_descriptor(ctx: &Context, sig: Signature) -> Result<Option<TagDescriptor>> {
    if crate::plugin::avoid_type_check(Some(ctx)) {
        return Ok(None);
    }
    descriptor_or_err(ctx, sig).map(Some)
}


/// `IsTypeSupported` — is `ty` among the descriptor's accepted types (capped at 20)?
fn is_type_supported(desc: &TagDescriptor, ty: Signature) -> bool {
    desc.supported_types
        .iter()
        .take(MAX_TYPES_IN_LCMS_PLUGIN)
        .any(|&s| s == ty)
}

/// The decoded element count of a value, for the `cmsReadTag` `ElemCount` sanity check.
///
/// C's `ReadPtr` returns a per-type `*nItems`; the only built-in tags whose descriptor demands
/// more than one element are `chad`/`arts` (nine `s15Fixed16` values), so the array length is
/// the count that matters — every other decoded object counts as one, exactly as its descriptor
/// (`elem_count == 1`) expects.
fn elem_count(value: &TagValue) -> u32 {
    match value {
        TagValue::S15Fixed16Array(a) => a.len() as u32,
        _ => 1,
    }
}

#[cfg(test)]
mod tests;

/// Parity tests for the header/directory and tag-location behaviour ported above.
///
/// Kept inline (rather than in [`tests`]) so they sit next to the code they pin down.
#[cfg(test)]
mod parity_tests {
    use super::*;
    use crate::tags::descriptor::tag_sig;
    use std::io::{Read, Seek, SeekFrom, Write};

    /// A profile image whose `A2B0`/`A2B1`/`A2B2` tags all share one on-disk block.
    fn profile_with_three_aliased_tags() -> Vec<u8> {
        let mut p = Profile::create_placeholder(Context::new());
        // A raw body is enough: link detection only compares (offset, size) and descriptors.
        p.write_raw_tag(tag_sig::A_TO_B0, &[0u8; 16]).unwrap();
        p.link_tag(tag_sig::A_TO_B1, tag_sig::A_TO_B0).unwrap();
        p.link_tag(tag_sig::A_TO_B2, tag_sig::A_TO_B0).unwrap();
        p.save_to_mem().unwrap()
    }

    #[test]
    fn link_detection_keeps_the_last_compatible_match() {
        let bytes = profile_with_three_aliased_tags();
        let p = Profile::open_from_mem(Context::new(), &bytes).unwrap();

        // C's link loop has no `break` (`cmsio0.c:960-973`), so every compatible hit overwrites
        // the previous one and the highest index wins.
        assert_eq!(p.tag_linked_to(tag_sig::A_TO_B1), Some(tag_sig::A_TO_B0));
        assert_eq!(p.tag_linked_to(tag_sig::A_TO_B2), Some(tag_sig::A_TO_B1));
    }

    /// A [`MemIo`] that reports a medium larger than 4 GiB, standing in for a huge file.
    ///
    /// `0x1_0000_0080` truncates to `128` in 32 bits, which is what the old `as u32` produced.
    struct HugeReportedIo(MemIo);

    impl Read for HugeReportedIo {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }
    impl Write for HugeReportedIo {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }
    impl Seek for HugeReportedIo {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(pos)
        }
    }
    impl IoHandler for HugeReportedIo {
        fn tell(&mut self) -> Result<u64> {
            self.0.tell()
        }
        fn used_space(&self) -> u64 {
            self.0.used_space()
        }
        fn reported_size(&self) -> u64 {
            0x1_0000_0080
        }
    }

    #[test]
    fn oversized_reported_size_is_clamped_not_truncated() {
        let bytes = profile_with_three_aliased_tags();
        assert!(bytes.len() > 128, "the tag block must sit past the truncated bound");

        let io = HugeReportedIo(MemIo::from_bytes(bytes));
        let p = Profile::open_from_io(Context::new(), io).unwrap();

        // Truncating instead of clamping collapses `header_size` to 128, pushing every tag out
        // of range and opening a silently empty profile.
        assert_eq!(p.tag_count(), 3);
        assert!(p.is_tag(tag_sig::A_TO_B0));
    }

    #[test]
    fn tag_offset_and_size_reports_the_on_disk_block() {
        let bytes = profile_with_three_aliased_tags();
        let p = Profile::open_from_mem(Context::new(), &bytes).unwrap();

        let (offset, size) = p.tag_offset_and_size(0).expect("slot 0 is in range");
        assert!(offset >= 128, "tag data starts after the header");
        assert_eq!(size, 16);
        // Out of range is C's FALSE (`cmsio0.c:669-670`).
        assert!(p.tag_offset_and_size(p.tag_count()).is_none());

        // A cooked (never-on-disk) tag occupies no region of any backing store.
        let mut fresh = Profile::create_placeholder(Context::new());
        fresh.write_raw_tag(tag_sig::A_TO_B0, &[0u8; 4]).unwrap();
        assert_eq!(fresh.tag_offset_and_size(0), Some((0, 0)));
    }
}
