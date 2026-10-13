//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/openjph/ojph_file.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/others/ojph_file.cpp`

use crate::openjph::ojph_defs::{Si64, Ui8, ojph_max, ojph_min};
use crate::openjph::ojph_mem::{ojph_aligned_free, ojph_aligned_malloc};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::ptr as raw_ptr;
use std::slice;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum OutfileSeek {
    OjphSeekSet = 0,
    OjphSeekCur = 1,
    OjphSeekEnd = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum InfileSeek {
    OjphSeekSet = 0,
    OjphSeekCur = 1,
    OjphSeekEnd = 2,
}

pub trait OutfileBase {
    fn write(&mut self, ptr: &[u8]) -> crate::openjph::ojph_message::OjphResult<usize>;

    fn tell(&mut self) -> Si64 {
        0
    }

    fn seek(&mut self, _offset: Si64, _origin: OutfileSeek) -> i32 {
        -1
    }

    fn flush(&mut self) -> crate::openjph::ojph_message::OjphResult<()> {
        Ok(())
    }

    fn close(&mut self) {}
}

pub trait InfileBase {
    fn read(&mut self, ptr: &mut [u8]) -> usize;

    fn seek(&mut self, offset: Si64, origin: InfileSeek) -> i32;

    fn tell(&mut self) -> Si64;

    fn eof(&mut self) -> bool;

    fn close(&mut self) {}
}

#[derive(Debug, Default)]
pub struct J2cOutfile {
    /// Buffered: the codestream writer emits many 2-4 byte marker fields, which would otherwise be
    /// one syscall each.
    fh: Option<BufWriter<File>>,
}

impl J2cOutfile {
    pub fn open<P: AsRef<Path>>(&mut self, filename: P) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.fh.is_none());
        let filename = filename.as_ref();
        self.fh = Some(BufWriter::new(File::create(filename).map_err(|_| {
            crate::ojph_error!(0x00060001, "failed to open {} for writing", filename.display())
        })?));
        Ok(())
    }
}

impl OutfileBase for J2cOutfile {
    fn write(&mut self, ptr: &[u8]) -> crate::openjph::ojph_message::OjphResult<usize> {
        debug_assert!(self.fh.is_some());
        match self.fh.as_mut() {
            // `write_all`: a short write is an error, never a silently truncated file.
            Some(fh) => fh.write_all(ptr).map(|()| ptr.len()).map_err(|e| {
                crate::ojph_error!(0x00060007, "failed to write to output file: {}", e)
            }),
            None => Ok(0),
        }
    }

    fn tell(&mut self) -> Si64 {
        debug_assert!(self.fh.is_some());
        match self.fh.as_mut() {
            Some(fh) => fh.stream_position().unwrap_or(0) as Si64,
            None => 0,
        }
    }

    fn flush(&mut self) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.fh.is_some());
        if let Some(fh) = self.fh.as_mut() {
            fh.flush().map_err(|e| {
                crate::ojph_error!(0x00060006, "failed to flush output file: {}", e)
            })?;
        }
        Ok(())
    }

    fn close(&mut self) {
        debug_assert!(self.fh.is_some());
        // `close` cannot report errors; callers that need to know call `flush()` first (the
        // codestream writer does). Dropping the BufWriter flushes any remainder.
        self.fh = None;
    }
}

#[derive(Debug)]
pub struct MemOutfile {
    is_open: bool,
    clear_mem: bool,
    buf_size: usize,
    used_size: usize,
    buf: *mut Ui8,
    cur_ptr: *mut Ui8,
}

impl Default for MemOutfile {
    fn default() -> Self {
        Self {
            is_open: false,
            clear_mem: false,
            buf_size: 0,
            used_size: 0,
            buf: raw_ptr::null_mut(),
            cur_ptr: raw_ptr::null_mut(),
        }
    }
}

impl Drop for MemOutfile {
    fn drop(&mut self) {
        unsafe {
            if !self.buf.is_null() {
                ojph_aligned_free(self.buf as *mut _);
            }
        }
        self.is_open = false;
        self.clear_mem = false;
        self.buf_size = 0;
        self.used_size = 0;
        self.buf = raw_ptr::null_mut();
        self.cur_ptr = raw_ptr::null_mut();
    }
}

impl MemOutfile {
    pub const ALIGNED_ALLOC_MASK: usize = 4096 - 1;

    #[inline]
    pub fn open(
        &mut self,
        initial_size: usize,
        clear_mem: bool,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(!self.is_open);
        debug_assert!(self.cur_ptr == self.buf);

        self.is_open = true;
        self.clear_mem = clear_mem;
        self.expand_storage(initial_size, self.clear_mem)?;
        self.used_size = 0;
        self.cur_ptr = self.buf;
        Ok(())
    }

    #[inline]
    pub fn get_data(&self) -> &[Ui8] {
        if self.buf.is_null() || self.used_size == 0 {
            &[]
        } else {
            unsafe { slice::from_raw_parts(self.buf, self.used_size) }
        }
    }

    #[inline]
    pub const fn get_used_size(&self) -> usize {
        self.used_size
    }

    #[inline]
    pub const fn get_buf_size(&self) -> usize {
        self.buf_size
    }

    pub fn write_to_file<P: AsRef<Path>>(
        &self,
        file_name: P,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(!self.is_open);
        let file_name = file_name.as_ref();
        let mut f = File::create(file_name).map_err(|_| {
            crate::ojph_error!(0x00060003, "failed to open {} for writing", file_name.display())
        })?;
        let written = f.write(self.get_data()).map_err(|e| {
            crate::ojph_error!(0x00060004, "failed writing to {}: {}", file_name.display(), e)
        })?;
        if written != self.used_size {
            return Err(crate::ojph_error!(
                0x00060004,
                "failed writing to {}",
                file_name.display()
            ));
        }
        Ok(())
    }

    fn expand_storage(
        &mut self,
        mut needed_size: usize,
        clear_all: bool,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        if needed_size > self.buf_size {
            needed_size += (needed_size + 1) >> 1;
            needed_size = (needed_size + Self::ALIGNED_ALLOC_MASK) & !Self::ALIGNED_ALLOC_MASK;

            let cur_offset = if self.buf.is_null() || self.cur_ptr.is_null() {
                0
            } else {
                self.tell() as usize
            };
            let new_buf = unsafe {
                ojph_aligned_malloc(Self::ALIGNED_ALLOC_MASK + 1, needed_size) as *mut Ui8
            };
            if new_buf.is_null() {
                return Err(crate::ojph_error!(
                    0x00060005,
                    "failed to allocate memory ({} bytes)",
                    needed_size
                ));
            }

            if !self.buf.is_null() {
                unsafe {
                    if !clear_all {
                        raw_ptr::copy_nonoverlapping(self.buf, new_buf, self.used_size);
                    }
                    ojph_aligned_free(self.buf as *mut _);
                }
            }
            self.cur_ptr = unsafe { new_buf.add(cur_offset) };
            self.buf = new_buf;

            if self.clear_mem && !clear_all {
                // Match OpenJPH exactly: newly grown capacity is zeroed only when
                // `clear_mem` was requested by the caller. A later `seek()` into a
                // hole therefore leaves gap bytes unspecified when `clear_mem` is
                // false; tests must not treat those bytes as part of the contract.
                unsafe {
                    raw_ptr::write_bytes(
                        self.buf.add(self.buf_size),
                        0,
                        needed_size - self.buf_size,
                    );
                }
            }
            self.buf_size = needed_size;
        }

        if clear_all && !self.buf.is_null() {
            unsafe {
                raw_ptr::write_bytes(self.buf, 0, self.buf_size);
            }
        }
        Ok(())
    }
}

impl OutfileBase for MemOutfile {
    fn write(&mut self, ptr: &[u8]) -> crate::openjph::ojph_message::OjphResult<usize> {
        debug_assert!(self.is_open);
        debug_assert!(self.buf_size > 0);
        debug_assert!(!self.buf.is_null());
        debug_assert!(!self.cur_ptr.is_null());

        let needed_size = self.tell() as usize + ptr.len();
        self.expand_storage(needed_size, false)?;

        unsafe {
            raw_ptr::copy_nonoverlapping(ptr.as_ptr(), self.cur_ptr, ptr.len());
            self.cur_ptr = self.cur_ptr.add(ptr.len());
        }
        self.used_size = ojph_max(self.used_size, self.tell() as usize);
        Ok(ptr.len())
    }

    fn tell(&mut self) -> Si64 {
        if self.buf.is_null() || self.cur_ptr.is_null() {
            0
        } else {
            unsafe { self.cur_ptr.offset_from(self.buf) as Si64 }
        }
    }

    fn seek(&mut self, mut offset: Si64, origin: OutfileSeek) -> i32 {
        match origin {
            OutfileSeek::OjphSeekSet => {}
            OutfileSeek::OjphSeekCur => {
                offset += self.tell();
            }
            OutfileSeek::OjphSeekEnd => {
                offset += self.used_size as Si64;
            }
        }

        if offset < 0 {
            return -1;
        }

        // `seek`'s own contract (matching C `fseek`) already signals failure via `-1`; an
        // allocation failure here maps into that existing convention rather than widening
        // `OutfileBase::seek`'s signature beyond what `write`/`open`/`flush` need.
        if self.expand_storage(offset as usize, false).is_err() {
            return -1;
        }
        unsafe {
            self.cur_ptr = self.buf.add(offset as usize);
        }
        0
    }

    fn close(&mut self) {
        self.is_open = false;
        self.cur_ptr = self.buf;
    }
}

/// Output sink that APPENDS the codestream to a caller-owned `Vec<u8>` (not part of the C++ API).
///
/// Why: [`MemOutfile`] owns its own buffer, so an encoder that wants the bytes in its own container
/// pays one raw-tile-sized allocation per codestream plus a full copy out of [`MemOutfile::get_data`].
/// A tile codec that encodes thousands of tiles into one reused `Vec` (exv-rs) gets both for free
/// here: the `Vec`'s capacity is reused and the bytes land in place. Positions (`tell`/`seek`) are
/// relative to the `Vec`'s length at construction, so prior contents are never touched; `seek` +
/// `write` overwrite in place like [`MemOutfile`]. Entirely safe code.
pub struct VecOutfile<'a> {
    buf: &'a mut Vec<u8>,
    /// Length of `buf` when this sink was created: position 0 of the codestream.
    base: usize,
    /// Absolute write position inside `buf` (`base..=buf.len()`).
    pos: usize,
}

impl<'a> VecOutfile<'a> {
    /// Append to `buf` (its existing bytes stay as they are).
    pub fn new(buf: &'a mut Vec<u8>) -> Self {
        let base = buf.len();
        Self { buf, base, pos: base }
    }
}

impl OutfileBase for VecOutfile<'_> {
    fn write(&mut self, data: &[u8]) -> crate::openjph::ojph_message::OjphResult<usize> {
        let end = self.pos + data.len();
        let overlap = end.min(self.buf.len()).saturating_sub(self.pos);
        self.buf[self.pos..self.pos + overlap].copy_from_slice(&data[..overlap]);
        self.buf.extend_from_slice(&data[overlap..]);
        self.pos = end;
        Ok(data.len())
    }

    fn tell(&mut self) -> Si64 {
        (self.pos - self.base) as Si64
    }

    fn seek(&mut self, offset: Si64, origin: OutfileSeek) -> i32 {
        let from = match origin {
            OutfileSeek::OjphSeekSet => 0,
            OutfileSeek::OjphSeekCur => self.tell(),
            OutfileSeek::OjphSeekEnd => (self.buf.len() - self.base) as Si64,
        };
        let Some(rel) = from.checked_add(offset).filter(|&r| r >= 0) else {
            return -1;
        };
        // Seeking past the end zero-fills the gap (never exposes stale bytes).
        let abs = self.base + rel as usize;
        if abs > self.buf.len() {
            self.buf.resize(abs, 0);
        }
        self.pos = abs;
        0
    }
}

#[derive(Debug, Default)]
pub struct J2cInfile {
    fh: Option<BufReader<File>>,
    reached_eof: bool,
    /// The last I/O error `read` hit. `read` itself can only return a byte count, so an error and
    /// a genuine end of file would otherwise be indistinguishable; see [`J2cInfile::take_error`].
    error: Option<std::io::Error>,
}

impl J2cInfile {
    /// The I/O error (if any) that made the last `read` return fewer bytes than asked.
    pub fn take_error(&mut self) -> Option<std::io::Error> {
        self.error.take()
    }
}

impl J2cInfile {
    pub fn open<P: AsRef<Path>>(&mut self, filename: P) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.fh.is_none());
        let filename = filename.as_ref();
        self.fh = Some(BufReader::new(File::open(filename).map_err(|_| {
            crate::ojph_error!(0x00060002, "failed to open {} for reading", filename.display())
        })?));
        self.reached_eof = false;
        self.error = None;
        Ok(())
    }
}

impl InfileBase for J2cInfile {
    fn read(&mut self, ptr: &mut [u8]) -> usize {
        debug_assert!(self.fh.is_some());
        let read = match self.fh.as_mut().map(|fh| fh.read(ptr)) {
            Some(Ok(n)) => n,
            Some(Err(e)) => {
                self.error = Some(e);
                0
            }
            None => 0,
        };
        self.reached_eof = read == 0;
        read
    }

    fn seek(&mut self, offset: Si64, origin: InfileSeek) -> i32 {
        debug_assert!(self.fh.is_some());
        self.reached_eof = false;
        let seek_from = match origin {
            InfileSeek::OjphSeekSet => SeekFrom::Start(offset as u64),
            InfileSeek::OjphSeekCur => SeekFrom::Current(offset),
            InfileSeek::OjphSeekEnd => SeekFrom::End(offset),
        };
        match self.fh.as_mut() {
            Some(fh) => fh.seek(seek_from).map(|_| 0).unwrap_or(-1),
            None => -1,
        }
    }

    fn tell(&mut self) -> Si64 {
        debug_assert!(self.fh.is_some());
        match self.fh.as_mut() {
            Some(fh) => fh.stream_position().unwrap_or(0) as Si64,
            None => 0,
        }
    }

    fn eof(&mut self) -> bool {
        self.reached_eof
    }

    fn close(&mut self) {
        debug_assert!(self.fh.is_some());
        self.fh = None;
        self.reached_eof = false;
    }
}

/// In-memory codestream source. Borrows the bytes (`'a`), so the bytes cannot be dropped while the
/// file exists; the codestream layer additionally keeps a raw pointer to the file between calls
/// (as the C++ does), so the file must also stay alive and unmoved while a `Codestream` reads it.
#[derive(Debug)]
pub struct MemInfile<'a> {
    data: *const Ui8,
    cur_ptr: *const Ui8,
    size: usize,
    _bytes: std::marker::PhantomData<&'a [Ui8]>,
}

impl Default for MemInfile<'_> {
    fn default() -> Self {
        Self {
            data: raw_ptr::null(),
            cur_ptr: raw_ptr::null(),
            size: 0,
            _bytes: std::marker::PhantomData,
        }
    }
}

impl<'a> MemInfile<'a> {
    /// Start reading `data`.
    #[inline]
    pub fn open(&mut self, data: &'a [Ui8]) {
        debug_assert!(self.data.is_null());
        self.cur_ptr = data.as_ptr();
        self.data = data.as_ptr();
        self.size = data.len();
    }

    /// Start reading `size` bytes at `data`.
    ///
    /// # Safety
    /// `data..data + size` must be valid for reads for as long as this file is used.
    #[inline]
    pub unsafe fn open_raw(&mut self, data: *const Ui8, size: usize) {
        debug_assert!(self.data.is_null());
        self.cur_ptr = data;
        self.data = data;
        self.size = size;
    }

    /// Reset the file handle (mirrors `InfileBase::close`).
    /// Provided as an inherent method so callers need not import `InfileBase`.
    #[inline]
    pub fn close_infile(&mut self) {
        self.data = raw_ptr::null();
        self.cur_ptr = raw_ptr::null();
        self.size = 0;
    }
}

impl InfileBase for MemInfile<'_> {
    fn read(&mut self, ptr: &mut [u8]) -> usize {
        let bytes_left = unsafe { self.data.add(self.size).offset_from(self.cur_ptr) };
        if bytes_left > 0 {
            let bytes_to_read = ojph_min(ptr.len(), bytes_left as usize);
            unsafe {
                raw_ptr::copy_nonoverlapping(self.cur_ptr, ptr.as_mut_ptr(), bytes_to_read);
                self.cur_ptr = self.cur_ptr.add(bytes_to_read);
            }
            bytes_to_read
        } else {
            0
        }
    }

    fn seek(&mut self, offset: Si64, origin: InfileSeek) -> i32 {
        let mut result = -1;
        match origin {
            InfileSeek::OjphSeekSet => {
                if offset >= 0 && (offset as usize) <= self.size {
                    unsafe {
                        self.cur_ptr = self.data.add(offset as usize);
                    }
                    result = 0;
                }
            }
            InfileSeek::OjphSeekCur => {
                let bytes_off = unsafe { self.cur_ptr.offset_from(self.data) as Si64 } + offset;
                if bytes_off >= 0 && (bytes_off as usize) <= self.size {
                    unsafe {
                        self.cur_ptr = self.data.add(bytes_off as usize);
                    }
                    result = 0;
                }
            }
            InfileSeek::OjphSeekEnd => {
                if offset <= 0 && self.size as isize + offset as isize >= 0 {
                    unsafe {
                        self.cur_ptr = self.data.add((self.size as Si64 + offset) as usize);
                    }
                    result = 0;
                }
            }
        }
        result
    }

    fn tell(&mut self) -> Si64 {
        unsafe { self.cur_ptr.offset_from(self.data) as Si64 }
    }

    fn eof(&mut self) -> bool {
        unsafe { self.cur_ptr >= self.data.add(self.size) }
    }

    fn close(&mut self) {
        self.data = raw_ptr::null();
        self.cur_ptr = raw_ptr::null();
        self.size = 0;
    }
}

/// Test-only `OutfileBase` sink that succeeds on its first `succeed_for` writes, then errors on
/// every subsequent one — a reusable "the Nth write fails" fixture for exercising the many
/// "Error writing to file" branches along the encode/write-headers path (`Tile::flush`,
/// `LocalCodestream::write_headers`, the marker-writer family in `ojph_params_local.rs`) without
/// needing a real disk-full or broken-pipe condition. `pub(crate)` so other modules' `#[cfg(test)]`
/// code can use it too.
#[cfg(test)]
pub(crate) struct FailingOutfile {
    writes_seen: usize,
    succeed_for: usize,
}

#[cfg(test)]
impl FailingOutfile {
    pub(crate) fn new(succeed_for: usize) -> Self {
        Self { writes_seen: 0, succeed_for }
    }
}

#[cfg(test)]
impl OutfileBase for FailingOutfile {
    fn write(&mut self, ptr: &[u8]) -> crate::openjph::ojph_message::OjphResult<usize> {
        self.writes_seen += 1;
        if self.writes_seen > self.succeed_for {
            return Err(crate::ojph_error!(
                0x0006F001,
                "FailingOutfile: injected write failure (call {})",
                self.writes_seen
            ));
        }
        Ok(ptr.len())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InfileBase, InfileSeek, J2cInfile, J2cOutfile, MemInfile, MemOutfile, OutfileBase,
        OutfileSeek, VecOutfile,
    };

    #[test]
    fn vec_outfile_appends_and_seeks_relative_to_its_start() {
        let mut v = b"PRE".to_vec();
        let mut f = VecOutfile::new(&mut v);
        assert_eq!(f.write(b"abcdef").unwrap(), 6);
        assert_eq!(f.tell(), 6);
        assert_eq!(f.seek(1, OutfileSeek::OjphSeekSet), 0);
        f.write(b"XY").unwrap();
        assert_eq!(f.tell(), 3);
        assert_eq!(f.seek(-1, OutfileSeek::OjphSeekEnd), 0);
        f.write(b"ZW").unwrap();
        assert_eq!(f.seek(-8, OutfileSeek::OjphSeekCur), -1);
        assert_eq!(f.seek(9, OutfileSeek::OjphSeekSet), 0);
        f.write(b"!").unwrap();
        assert_eq!(v, b"PREaXYdeZW\0\0!");
    }
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn mem_outfile_open_errors_on_unsatisfiable_allocation() {
        let mut file = MemOutfile::default();
        // No real system can satisfy a multi-exabyte aligned allocation; this must
        // surface as the OOM guard's Err, not a process abort or a silent no-op.
        assert!(file.open(usize::MAX / 2, false).is_err());
    }

    #[test]
    fn mem_outfile_matches_reference_seek_and_growth_shape() {
        let mut file = MemOutfile::default();
        file.open(8, false).expect("open");

        assert_eq!(file.write(&[1, 2, 3]).expect("write"), 3);
        assert_eq!(file.tell(), 3);
        assert_eq!(file.seek(6, OutfileSeek::OjphSeekSet), 0);
        assert_eq!(file.write(&[9]).expect("write"), 1);
        assert_eq!(file.get_used_size(), 7);
        let data = file.get_data();
        assert_eq!(&data[..3], &[1, 2, 3]);
        // The gap between the old logical end and the later seek target is not
        // guaranteed to be zeroed when `clear_mem == false`; only the written
        // prefix and suffix are stable across the C++ and Rust implementations.
        assert_eq!(data[6], 9);
        file.close();
    }

    #[test]
    fn mem_outfile_write_to_file_matches_reference_persistence_path() {
        let mut file = MemOutfile::default();
        file.open(8, true).expect("open");
        assert_eq!(file.write(&[5, 6, 7]).expect("write"), 3);
        file.close();

        let path = std::env::temp_dir().join(format!(
            "jph-rs-{}.j2c",
            match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_nanos(),
                Err(_) => 0,
            }
        ));
        file.write_to_file(&path).expect("write_to_file");
        let persisted = fs::read(&path).unwrap_or_default();
        assert_eq!(persisted, vec![5, 6, 7]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn mem_infile_matches_reference_read_and_seek_shape() {
        let data = [10u8, 11, 12, 13];
        let mut file = MemInfile::default();
        file.open(&data);

        let mut buf = [0u8; 2];
        assert_eq!(file.read(&mut buf), 2);
        assert_eq!(buf, [10, 11]);
        assert_eq!(file.seek(-1, InfileSeek::OjphSeekCur), 0);
        assert_eq!(file.tell(), 1);
        assert_eq!(file.seek(-1, InfileSeek::OjphSeekEnd), 0);
        debug_assert!(!file.eof());
    }

    #[test]
    fn j2c_file_roundtrip_matches_reference_io_shape() {
        let path = std::env::temp_dir().join(format!(
            "jph-rs-{}.bin",
            match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_nanos(),
                Err(_) => 0,
            }
        ));

        let mut out = J2cOutfile::default();
        out.open(&path).expect("open");
        assert_eq!(out.write(&[1, 2, 3, 4]).expect("write"), 4);
        out.flush().expect("flush");
        out.close();

        let mut inp = J2cInfile::default();
        inp.open(&path).expect("open");
        let mut buf = [0u8; 4];
        assert_eq!(inp.read(&mut buf), 4);
        assert_eq!(buf, [1, 2, 3, 4]);
        let mut eof_probe = [0u8; 1];
        assert_eq!(inp.read(&mut eof_probe), 0);
        debug_assert!(inp.eof());
        inp.close();

        let _ = fs::remove_file(path);
    }
}
