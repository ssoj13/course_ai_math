//! `cmsIOHANDLER` backends — a pure-Rust port of the IOHANDLER section of `cmsio0.c`.
//!
//! lcms2 abstracts every storage medium (memory block, disk file, byte counter) behind a
//! single `cmsIOHANDLER` struct holding C function pointers for `Read`/`Write`/`Seek`/
//! `Tell`/`Close` plus the bookkeeping fields `UsedSpace` and `ReportedSize`. This module
//! replaces the function-pointer table with the [`IoHandler`] trait and three concrete
//! backends:
//!
//! - [`MemIo`]  — in-memory buffer (`cmsOpenIOhandlerFromMem`).
//! - [`FileIo`] — disk file (`cmsOpenIOhandlerFromFile`, access `"r"`/`"w"`).
//! - [`NullIo`] — counting sink (`cmsOpenIOhandlerFromNULL`).
//!
//! The byte ordering of ICC data is handled one layer up in [`crate::io::endian`]; this
//! layer is purely byte-level. There is **no `unsafe`** here: the C manual buffer/`memmove`
//! bookkeeping is expressed with [`std::io::Cursor`], [`std::fs::File`], and a plain
//! position counter, so the standard library enforces the bounds the C code checked by hand.
//!
//! ## `UsedSpace` — the two-pass save contract
//!
//! Profile serialization runs twice: first over a [`NullIo`] to learn the total byte size
//! (which must appear in the ICC header), then over a real [`MemIo`]/[`FileIo`] to emit the
//! bytes. Correctness of that first pass depends entirely on [`IoHandler::used_space`], so
//! every backend reports it as the **high-water mark of the write position** — the largest
//! byte offset any write has reached. For the strictly sequential writes a profile save
//! performs this equals the cumulative byte count; unlike C's `FileWrite` (`UsedSpace +=
//! size`) it does not double-count when the header is patched by seeking back to offset 0.
//!
//! `Close` from the C API has no analogue: [`Drop`] releases the file handle / buffer.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::cms_err;
use crate::error::{ErrorCode, Result};

/// Abstraction over a seekable byte stream, replacing lcms2's `cmsIOHANDLER`.
///
/// The `Read`/`Write`/`Seek` supertraits stand in for the C `Read`/`Write`/`Seek` function
/// pointers; [`tell`](Self::tell) mirrors `Tell`, and the two accessors carry the
/// `UsedSpace`/`ReportedSize` header fields. See the module docs for the `used_space`
/// contract that the two-pass profile save relies on.
pub trait IoHandler: Read + Write + Seek {
    /// Current stream position (`Tell` / `iohandler->Tell`).
    fn tell(&mut self) -> Result<u64>;

    /// High-water mark of the write position (`iohandler->UsedSpace`).
    ///
    /// This is the total logical size the stream would occupy — the value the profile
    /// save's first ([`NullIo`]) pass writes into the ICC header.
    fn used_space(&self) -> u64;

    /// Total size of the underlying medium if known (`iohandler->ReportedSize`), else `0`.
    ///
    /// Set to the source length when opening an existing buffer/file for reading; `0` for a
    /// write target (matching the C creators), which is why it is unknown rather than an error.
    fn reported_size(&self) -> u64 {
        0
    }
}

/// Map a low-level [`io::Error`] to a crate [`Error`](crate::error::Error) with `code`.
#[inline]
fn io_err(code: ErrorCode, ctx: &str, e: &io::Error) -> crate::error::Error {
    cms_err!(code, "{ctx}: {e}")
}

// ------------------------------------------------------------------------------------------
// MemIo — in-memory buffer (cmsOpenIOhandlerFromMem)
// ------------------------------------------------------------------------------------------

/// In-memory IO backend, port of the `FILEMEM` handler.
///
/// Wraps a [`Cursor<Vec<u8>>`], which supplies read/write/seek and, for write mode, grows on
/// demand — so unlike the C `'w'` handler there is no fixed pre-allocated block to overflow
/// or truncate against. Construct it read-only from existing bytes with [`from_bytes`](Self::from_bytes)
/// or as a growable write target with [`new_write`](Self::new_write); recover the buffer with
/// [`into_inner`](Self::into_inner).
pub struct MemIo {
    inner: Cursor<Vec<u8>>,
    /// High-water mark of the write position (`UsedSpace`).
    used_space: u64,
    /// Source length for read mode, `0` for write mode (`ReportedSize`).
    reported_size: u64,
}

impl MemIo {
    /// Growable, empty write target (`cmsOpenIOhandlerFromMem` with access `"w"`).
    ///
    /// `ReportedSize` is `0`; the buffer starts empty and extends as bytes are written.
    pub fn new_write() -> Self {
        Self { inner: Cursor::new(Vec::new()), used_space: 0, reported_size: 0 }
    }

    /// Read backend over an owned byte block (`cmsOpenIOhandlerFromMem` with access `"r"`).
    ///
    /// The C reader copies the caller's block so it may be freed afterwards; taking ownership
    /// of the `Vec` gives the same guarantee without a copy. `ReportedSize` is the block length.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let reported_size = bytes.len() as u64;
        Self { inner: Cursor::new(bytes), used_space: 0, reported_size }
    }

    /// Consume the handler and return the underlying buffer (with any writes applied).
    pub fn into_inner(self) -> Vec<u8> {
        self.inner.into_inner()
    }
}

impl Read for MemIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for MemIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Cursor over a Vec grows the Vec as needed; track the high-water write position so
        // `used_space` stays correct even if a later seek rewinds and overwrites (as the
        // header patch of a profile save does).
        let n = self.inner.write(buf)?;
        let pos = self.inner.position();
        if pos > self.used_space {
            self.used_space = pos;
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl Seek for MemIo {
    /// C's `MemorySeek` refuses a position past the end of the block
    /// (`if (offset > ResData->Size) return FALSE`, `cmsio0.c:189-190`), which surfaces as
    /// `cmsERROR_SEEK`. A bare `Cursor::seek` accepts any position and defers the failure to the
    /// next read, misreporting it as a read error, so the bound is applied here instead.
    /// `FileIo`/`NullIo` are deliberately left unbounded: `FileSeek` (`cmsio0.c:356`) and
    /// `NULLSeek` (`cmsio0.c:60`) accept past-the-end positions too.
    ///
    /// KNOWN NARROWING vs C: `MemorySeek` bounds against `ResData->Size`, the block size handed
    /// to `cmsOpenIOhandlerFromMem`. For a READ handler that equals the content length, so this
    /// matches. For a WRITE handler C's `Size` is the whole destination buffer, so C permits
    /// seeking forward into not-yet-written space; bounding on `Vec::len()` here does not. No
    /// current caller does that - every forward seek in `src/` is `SeekFrom::End(0)`, and the
    /// directory patches in `profile/save.rs` seek backwards - but a future writer that reserves
    /// space by seeking rather than writing zeros would need a capacity tracked separately from
    /// the `Vec` length.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let len = self.inner.get_ref().len() as u64;
        let target = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => len.checked_add_signed(d),
            SeekFrom::Current(d) => self.inner.position().checked_add_signed(d),
        };
        match target {
            Some(t) if t <= len => {
                self.inner.set_position(t);
                Ok(t)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek outside the memory block",
            )),
        }
    }
}

impl IoHandler for MemIo {
    fn tell(&mut self) -> Result<u64> {
        Ok(self.inner.position())
    }

    fn used_space(&self) -> u64 {
        self.used_space
    }

    fn reported_size(&self) -> u64 {
        self.reported_size
    }
}

// ------------------------------------------------------------------------------------------
// FileIo — disk file (cmsOpenIOhandlerFromFile)
// ------------------------------------------------------------------------------------------

/// File-backed IO backend, port of the `FILE*` handler.
///
/// Opened for reading with [`open_read`](Self::open_read) or for writing with
/// [`create_write`](Self::create_write). Access enforcement is left to the OS (writing a
/// read-opened file fails at the syscall), mirroring the C code's reliance on `fopen` modes.
/// The file is closed when the handler is dropped (the C `FileClose`).
pub struct FileIo {
    file: File,
    /// High-water mark of the write position (`UsedSpace`).
    used_space: u64,
    /// File length for read mode, `0` for write mode (`ReportedSize`).
    reported_size: u64,
}

impl FileIo {
    /// Open an existing file for reading (`cmsOpenIOhandlerFromFile`, access `"r"`).
    ///
    /// `ReportedSize` is the file length, as the C reader records from `cmsfilelength`.
    pub fn open_read<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|e| io_err(ErrorCode::File, &format!("cannot open '{}'", path.display()), &e))?;
        let reported_size = file
            .metadata()
            .map_err(|e| io_err(ErrorCode::File, &format!("cannot stat '{}'", path.display()), &e))?
            .len();
        Ok(Self { file, used_space: 0, reported_size })
    }

    /// Create/truncate a file for writing (`cmsOpenIOhandlerFromFile`, access `"w"`).
    ///
    /// `ReportedSize` is `0`, matching the C write-mode creator.
    pub fn create_write<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let file = File::create(path)
            .map_err(|e| io_err(ErrorCode::File, &format!("cannot create '{}'", path.display()), &e))?;
        Ok(Self { file, used_space: 0, reported_size: 0 })
    }
}

impl Read for FileIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl Write for FileIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.file.write(buf)?;
        // Update the high-water mark from the real post-write position so that a seek-back
        // header patch does not inflate `used_space` the way C's `UsedSpace += size` would.
        let pos = self.file.stream_position()?;
        if pos > self.used_space {
            self.used_space = pos;
        }
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl Seek for FileIo {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

impl IoHandler for FileIo {
    fn tell(&mut self) -> Result<u64> {
        self.file
            .stream_position()
            .map_err(|e| io_err(ErrorCode::Seek, "tell failed", &e))
    }

    fn used_space(&self) -> u64 {
        self.used_space
    }

    fn reported_size(&self) -> u64 {
        self.reported_size
    }
}

// ------------------------------------------------------------------------------------------
// NullIo — counting sink (cmsOpenIOhandlerFromNULL)
// ------------------------------------------------------------------------------------------

/// Byte-counting sink, port of the `FILENULL` handler.
///
/// Discards writes but tracks how many bytes pass through, so profile serialization can run
/// a size-only first pass and stamp the total into the ICC header before writing for real.
/// It stores no payload: a write of any length advances the virtual position and bumps
/// [`used_space`](IoHandler::used_space), returning success without keeping the data.
pub struct NullIo {
    /// Virtual current position (`FILENULL::Pointer`).
    pos: u64,
    /// High-water mark of the write position (`iohandler->UsedSpace`).
    used_space: u64,
}

impl NullIo {
    /// Create a counting sink positioned at `0`.
    pub fn new() -> Self {
        Self { pos: 0, used_space: 0 }
    }
}

impl Default for NullIo {
    fn default() -> Self {
        Self::new()
    }
}

impl Read for NullIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // C `NULLRead` advances the pointer by the request length and reports success
        // without touching the caller's buffer (it has no backing store). Rust cannot leave
        // the buffer uninitialized, so fill it with zeros — the deterministic equivalent of
        // "read succeeded, contents undefined". `used_space` tracks writes only, so it is
        // untouched here.
        buf.fill(0);
        self.pos += buf.len() as u64;
        Ok(buf.len())
    }
}

impl Write for NullIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // C `NULLWrite` guards against 32-bit pointer overflow; on 64-bit positions that is
        // effectively unreachable, but guard anyway so the counter can never silently wrap.
        let new_pos = self.pos.checked_add(buf.len() as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "NullIo position overflow")
        })?;
        self.pos = new_pos;
        if self.pos > self.used_space {
            self.used_space = self.pos;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for NullIo {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        // C `NULLSeek` is absolute (`SEEK_SET`) and unbounded; support the relative variants
        // too so the handler is a well-behaved `Seek`. `End` is measured from the high-water
        // mark, the only "size" a sink has.
        let base = match pos {
            SeekFrom::Start(off) => {
                self.pos = off;
                return Ok(self.pos);
            }
            SeekFrom::Current(delta) => (self.pos, delta),
            SeekFrom::End(delta) => (self.used_space, delta),
        };
        let new_pos = base.0.checked_add_signed(base.1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid seek to a negative or overflowing position")
        })?;
        self.pos = new_pos;
        Ok(self.pos)
    }
}

impl IoHandler for NullIo {
    fn tell(&mut self) -> Result<u64> {
        Ok(self.pos)
    }

    fn used_space(&self) -> u64 {
        self.used_space
    }
    // reported_size defaults to 0 — a sink has no source length.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mem_write_seek_read_roundtrip() {
        let mut io = MemIo::new_write();
        let data = [1u8, 2, 3, 4, 5];
        assert_eq!(io.write(&data).unwrap(), data.len());
        assert_eq!(io.tell().unwrap(), 5);

        io.seek(SeekFrom::Start(0)).unwrap();
        assert_eq!(io.tell().unwrap(), 0);

        let mut back = [0u8; 5];
        io.read_exact(&mut back).unwrap();
        assert_eq!(back, data);
        assert_eq!(io.into_inner(), data);
    }

    #[test]
    fn mem_used_space_is_high_water_mark() {
        let mut io = MemIo::new_write();
        io.write_all(&[0u8; 10]).unwrap();
        assert_eq!(io.used_space(), 10);

        // Rewinding and overwriting must not lower the high-water mark.
        io.seek(SeekFrom::Start(0)).unwrap();
        io.write_all(&[0xFFu8; 3]).unwrap();
        assert_eq!(io.used_space(), 10);
        assert_eq!(io.reported_size(), 0);
    }

    #[test]
    fn mem_seek_past_end_is_an_error() {
        // C's `MemorySeek` rejects `offset > Size` outright (`cmsio0.c:189-190`) instead of
        // deferring the failure to the next read.
        let mut io = MemIo::from_bytes(vec![1, 2, 3, 4]);
        assert_eq!(io.seek(SeekFrom::Start(4)).unwrap(), 4, "seeking to the end is legal");
        assert!(io.seek(SeekFrom::Start(5)).is_err());
        assert!(io.seek(SeekFrom::End(1)).is_err());
        assert!(io.seek(SeekFrom::Current(1)).is_err());
        // A rejected seek leaves the position untouched.
        assert_eq!(io.tell().unwrap(), 4);
    }

    #[test]
    fn mem_from_bytes_reports_source_len() {
        let mut io = MemIo::from_bytes(vec![9, 8, 7]);
        assert_eq!(io.reported_size(), 3);
        assert_eq!(io.used_space(), 0);

        let mut b = [0u8; 3];
        io.read_exact(&mut b).unwrap();
        assert_eq!(b, [9, 8, 7]);
    }

    #[test]
    fn null_counts_bytes_without_storing() {
        let mut io = NullIo::new();
        assert_eq!(io.write(&[0u8; 40]).unwrap(), 40);
        assert_eq!(io.write(&[0u8; 60]).unwrap(), 60);
        assert_eq!(io.tell().unwrap(), 100);
        assert_eq!(io.used_space(), 100);
        assert_eq!(io.reported_size(), 0);

        // Seeking back then writing a shorter block keeps the high-water mark.
        io.seek(SeekFrom::Start(10)).unwrap();
        io.write_all(&[0u8; 5]).unwrap();
        assert_eq!(io.tell().unwrap(), 15);
        assert_eq!(io.used_space(), 100);
    }

    #[test]
    fn null_read_yields_zeros_and_advances() {
        let mut io = NullIo::new();
        let mut buf = [0xAAu8; 8];
        assert_eq!(io.read(&mut buf).unwrap(), 8);
        assert_eq!(buf, [0u8; 8]);
        assert_eq!(io.tell().unwrap(), 8);
        // Reads never affect the write high-water mark.
        assert_eq!(io.used_space(), 0);
    }

    #[test]
    fn null_seek_end_relative_to_high_water() {
        let mut io = NullIo::new();
        io.write_all(&[0u8; 50]).unwrap();
        io.seek(SeekFrom::End(-10)).unwrap();
        assert_eq!(io.tell().unwrap(), 40);
        io.seek(SeekFrom::Current(5)).unwrap();
        assert_eq!(io.tell().unwrap(), 45);
    }

    #[test]
    fn file_write_then_read_roundtrip() {
        // Unique temp path in the OS temp dir; cleaned up at the end.
        let mut path = std::env::temp_dir();
        let unique = format!(
            "lcms_rs_fileio_{}_{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        path.push(unique);

        let data = [10u8, 20, 30, 40, 50, 60];
        {
            let mut w = FileIo::create_write(&path).unwrap();
            assert_eq!(w.write(&data).unwrap(), data.len());
            assert_eq!(w.used_space(), data.len() as u64);
            assert_eq!(w.reported_size(), 0);
            w.flush().unwrap();
        }

        {
            let mut r = FileIo::open_read(&path).unwrap();
            assert_eq!(r.reported_size(), data.len() as u64);

            let mut back = vec![0u8; data.len()];
            r.read_exact(&mut back).unwrap();
            assert_eq!(back, data);

            r.seek(SeekFrom::Start(2)).unwrap();
            assert_eq!(r.tell().unwrap(), 2);
        }

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn file_open_missing_is_error() {
        let mut path = std::env::temp_dir();
        path.push("lcms_rs_definitely_missing_file_zzz.tmp");
        match FileIo::open_read(&path) {
            Err(e) => assert_eq!(e.code, ErrorCode::File),
            Ok(_) => panic!("expected open of a missing file to fail"),
        }
    }
}
