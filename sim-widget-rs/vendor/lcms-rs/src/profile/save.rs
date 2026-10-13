//! Profile serialization — the two-pass save (`cmsSaveProfileToIOhandler`), its `SaveTags`
//! and `SetLinks` helpers, the memory/file/size front-ends, and `cmsMD5computeID`.
//!
//! ## Two passes
//! The ICC header carries the total profile size, so the size must be known *before* the
//! header is written. lcms solves this by serializing twice: pass 1 runs over a byte-counting
//! [`NullIo`] purely to learn `UsedSpace`; pass 2 emits the real bytes to the target with that
//! size stamped in. Tag offsets/sizes are identical across passes because the layout is
//! deterministic, so the offsets computed in pass 1 (patched by [`set_links`]) are the ones the
//! pass-2 header records.

use std::io::SeekFrom;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::cms_err;
use crate::error::{ErrorCode, Result};
use crate::hash::md5::md5;
use crate::io::handler::{FileIo, IoHandler, MemIo, NullIo};
use crate::context::Context;
use crate::tags::get_descriptor_thr;
use crate::tags::get_handler_thr;
use crate::tags::handler::{write_alignment, write_type_base};

use super::{Profile, TagData, TagEntry};

impl Profile {
    // -----------------------------------------------------------------------------------
    // cmsSaveProfileToIOhandler (two-pass) + public front-ends
    // -----------------------------------------------------------------------------------

    /// `cmsSaveProfileToIOhandler` — serialize the profile, returning the byte size.
    ///
    /// With `target = None` only the size is computed (the `io == NULL` path). Otherwise the
    /// bytes are written to `target` after a size-measuring first pass. Generic over the target
    /// backing store to avoid trait-object variance against the stored `'static` `io` handle.
    fn compute_and_save<W: IoHandler + ?Sized>(&mut self, target: Option<&mut W>) -> Result<u32> {
        let n = self.tags.len();
        let mut offsets = vec![0u32; n];
        let mut sizes = vec![0u32; n];
        let version = self.profile_version();

        // Pass 1: measure with a NullIo (bytes discarded, only UsedSpace matters).
        let used_space;
        {
            let mut null = NullIo::new();
            self.write_header(&mut null, 0, &offsets, &sizes)?;
            // `tags` (shared) and the shared backing `io` handle are disjoint reads of `self`;
            // lock the backing store for the blind-copy of untouched on-disk tags.
            let tags = &self.tags;
            let source_arc = self.io.clone();
            let mut source_guard = lock_source(source_arc.as_ref())?;
            let source = source_guard.as_deref_mut();
            save_tags(self.context(), tags, source, version, &mut null, &mut offsets, &mut sizes)?;
            used_space = null.used_space() as u32;
        }

        // Pass 2: emit to the real target with the measured size in the header.
        if let Some(out) = target {
            set_links(&self.tags, &mut offsets, &mut sizes);
            self.write_header(out, used_space, &offsets, &sizes)?;
            let tags = &self.tags;
            let source_arc = self.io.clone();
            let mut source_guard = lock_source(source_arc.as_ref())?;
            let source = source_guard.as_deref_mut();
            save_tags(self.context(), tags, source, version, out, &mut offsets, &mut sizes)?;
        }

        Ok(used_space)
    }

    /// `cmsSaveProfileToIOhandler` — serialize to a caller-provided backing store.
    pub fn save_to_io(&mut self, io: &mut dyn IoHandler) -> Result<u32> {
        self.compute_and_save(Some(io))
    }

    /// `cmsSaveProfileToMem` with `MemPtr == NULL` — the byte size the profile would occupy.
    pub fn save_size(&mut self) -> Result<u32> {
        self.compute_and_save(Option::<&mut NullIo>::None)
    }

    /// `cmsSaveProfileToMem` — serialize the whole profile to a fresh byte buffer.
    pub fn save_to_mem(&mut self) -> Result<Vec<u8>> {
        let mut mem = MemIo::new_write();
        self.compute_and_save(Some(&mut mem))?;
        Ok(mem.into_inner())
    }

    /// `cmsSaveProfileToFile` — serialize to a disk file, removing it on failure.
    pub fn save_to_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let path = path.as_ref();
        let mut io = FileIo::create_write(path)?;
        let rc = self.compute_and_save(Some(&mut io));
        // Close the file handle before any cleanup (Windows won't remove an open file).
        drop(io);
        match rc {
            Ok(n) if n != 0 => Ok(()),
            other => {
                let _ = std::fs::remove_file(path);
                other.and_then(|_| Err(cms_err!(ErrorCode::Write, "profile save produced no bytes")))
            }
        }
    }

    // -----------------------------------------------------------------------------------
    // cmsMD5computeID
    // -----------------------------------------------------------------------------------

    /// `cmsMD5computeID` — set the profile ID to the MD5 of the profile per ICC 4.4 §7.2.18.
    ///
    /// Following the reference exactly: zero `flags`, `RenderingIntent` and `ProfileID`, serialize
    /// the whole profile to memory, hash every byte, restore the header, then store the digest.
    /// Because those three fields sit at fixed header offsets (44 / 64 / 84), this is equivalent
    /// to the spec's "zero these bytes of the serialized image" description, but reproduces the C
    /// byte-for-byte.
    pub fn compute_id(&mut self) -> Result<()> {
        // Save the header fields the spec excludes from the hash.
        let keep_flags = self.flags;
        let keep_intent = self.rendering_intent;
        let keep_id = self.profile_id;

        self.flags = 0;
        self.rendering_intent = 0;
        self.profile_id = [0u8; 16];

        let bytes = self.save_to_mem();

        // Restore the header regardless of the outcome (C's `memmove(Icc, &Keep, ...)`).
        self.flags = keep_flags;
        self.rendering_intent = keep_intent;
        self.profile_id = keep_id;

        self.profile_id = md5(&bytes?);
        Ok(())
    }
}

/// Lock the (optional) shared backing store for the blind-copy pass. Returns `None` when the
/// profile has no backing store (a built-in profile), matching C's `Icc->IOhandler == NULL`.
fn lock_source<'a>(
    arc: Option<&'a Arc<Mutex<dyn IoHandler + Send>>>,
) -> Result<Option<MutexGuard<'a, dyn IoHandler + Send + 'static>>> {
    match arc {
        Some(a) => Ok(Some(
            a.lock().map_err(|_| cms_err!(ErrorCode::Internal, "profile io lock poisoned"))?,
        )),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------------------
// SaveTags / SetLinks (free functions, to keep the tags/io field borrows disjoint)
// ---------------------------------------------------------------------------------------

/// `SaveTags` — write every tag body, recording its `(offset, size)` into `offsets`/`sizes`.
///
/// Cooked tags are re-serialized through the tag descriptor's chosen type (which may differ
/// from the stored one if the version changed); raw tags are copied verbatim; an untouched
/// on-disk tag is blind-copied from the original `source`. Linked and deleted tags are skipped.
fn save_tags<Wout, Src>(
    ctx: &Context,
    tags: &[TagEntry],
    mut source: Option<&mut Src>,
    version: f64,
    mut out: &mut Wout,
    offsets: &mut [u32],
    sizes: &mut [u32],
) -> Result<()>
where
    Wout: IoHandler + ?Sized,
    Src: IoHandler + ?Sized,
{
    for i in 0..tags.len() {
        let tag = &tags[i];
        if tag.sig.as_u32() == 0 {
            continue; // deleted placeholder
        }
        if tag.linked.is_some() {
            continue; // linked tags are not written (SetLinks fills their offset/size)
        }

        let begin = out.used_space() as u32;
        offsets[i] = begin;

        match &tag.data {
            TagData::OnDisk { offset, size } => {
                // Untouched on-disk tag: blind-copy the original block from the source handler.
                if *offset != 0 {
                    if let Some(src) = source.as_deref_mut() {
                        src.seek(SeekFrom::Start(*offset as u64))
                            .map_err(|e| cms_err!(ErrorCode::Seek, "seek source tag: {e}"))?;
                        let mut mem = vec![0u8; *size as usize];
                        src.read_exact(&mut mem)
                            .map_err(|e| cms_err!(ErrorCode::Read, "read source tag: {e}"))?;
                        out.write_all(&mem)
                            .map_err(|e| cms_err!(ErrorCode::Write, "copy tag: {e}"))?;
                        sizes[i] = out.used_space() as u32 - begin;
                        write_alignment(&mut out)?;
                    }
                }
                continue;
            }
            TagData::None => continue,
            TagData::Raw(bytes) => {
                out.write_all(bytes).map_err(|e| cms_err!(ErrorCode::Write, "raw tag: {e}"))?;
            }
            TagData::Cooked(value) => {
                // Re-decide the on-disk type from the version + value (as the reference does at
                // save time), then write the type base and the body.
                let descriptor = match get_descriptor_thr(ctx, tag.sig) {
                    Some(d) => d,
                    None => continue, // unsupported: ignore, matching C
                };
                // C indexes a fixed `SupportedTypes[MAX_TYPES_IN_LCMS_PLUGIN]` array
                // (`cmsio0.c:1490`); an empty plugin list yields garbage, `_cmsGetTagTypeHandler`
                // then returns NULL and the tag is *skipped*, not fatal (`:1493-1498`). So the
                // empty case takes the same `continue`, keeping the save going.
                let type_sig = match descriptor.decide_type {
                    Some(decide) => decide(version, value),
                    None => match descriptor.supported_types.first() {
                        Some(&t) => t,
                        None => continue,
                    },
                };
                let handler = match get_handler_thr(ctx, type_sig) {
                    Some(h) => h,
                    None => continue, // no handler: C signals internal error and continues
                };
                write_type_base(&mut out, handler.signature())?;
                handler.write(&mut out, value, version)?;
            }
        }

        sizes[i] = out.used_space() as u32 - begin;
        write_alignment(&mut out)?;
    }
    Ok(())
}

/// `SetLinks` — copy each linked tag's `(offset, size)` from the tag it links to.
fn set_links(tags: &[TagEntry], offsets: &mut [u32], sizes: &mut [u32]) {
    for i in 0..tags.len() {
        if let Some(linked) = tags[i].linked {
            if let Some(j) = tags.iter().position(|t| t.sig == linked) {
                offsets[i] = offsets[j];
                sizes[i] = sizes[j];
            }
        }
    }
}
