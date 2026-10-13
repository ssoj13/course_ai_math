//! On-disk numbered image clipboard store.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::clip_format::ClipFormat;

const INDEX_NAME: &str = "index.json";
const THUMBS_DIR: &str = "thumbs";
const THUMB_MAX: u32 = 128;

/// One slot in the clipboard.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipEntry {
    pub id: u32,
    /// Path relative to the store root (`0001.jpg`).
    pub rel_path: String,
    /// Optional thumb relative path (`thumbs/0001.jpg`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_rel: Option<String>,
    pub width: u32,
    pub height: u32,
    pub format: ClipFormat,
    pub created_unix: u64,
}

impl ClipEntry {
    /// Absolute path to the encoded frame.
    pub fn path(&self, root: &Path) -> PathBuf {
        root.join(&self.rel_path)
    }

    pub fn thumb_path(&self, root: &Path) -> Option<PathBuf> {
        self.thumb_rel.as_ref().map(|r| root.join(r))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct IndexFile {
    #[serde(default)]
    entries: Vec<ClipEntry>,
    #[serde(default)]
    next_id: u32,
}

/// Errors from [`ClipStore`] operations.
#[derive(Debug, thiserror::Error)]
pub enum ClipError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Msg(String),
    #[error("encode feature unavailable: {0}")]
    Feature(String),
}

pub type Result<T> = std::result::Result<T, ClipError>;

/// Persistent multi-slot image clipboard rooted at a directory.
#[derive(Debug)]
pub struct ClipStore {
    root: PathBuf,
    index: IndexFile,
}

impl ClipStore {
    /// Open (or create) a store at `root`. Creates `root/` and `thumbs/`.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)?;
        fs::create_dir_all(root.join(THUMBS_DIR))?;
        let index_path = root.join(INDEX_NAME);
        let index = if index_path.exists() {
            let text = fs::read_to_string(&index_path)?;
            serde_json::from_str(&text)?
        } else {
            IndexFile {
                entries: Vec::new(),
                next_id: 1,
            }
        };
        let store = Self { root, index };
        store.persist_index()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list(&self) -> &[ClipEntry] {
        &self.index.entries
    }

    pub fn get(&self, id: u32) -> Option<&ClipEntry> {
        self.index.entries.iter().find(|e| e.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.index.entries.is_empty()
    }

    /// Encode RGBA8 pixels and append a slot.
    pub fn push_rgba8(
        &mut self,
        width: u32,
        height: u32,
        rgba: &[u8],
        format: ClipFormat,
    ) -> Result<ClipEntry> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| ClipError::Msg("dimensions overflow".into()))?;
        if rgba.len() != expected {
            return Err(ClipError::Msg(format!(
                "rgba len {} != {}x{}x4",
                rgba.len(),
                width,
                height
            )));
        }
        if matches!(format, ClipFormat::Exr) {
            return Err(ClipError::Msg(
                "push_rgba8 cannot write Exr — use push_f16_rgba or push_f32_rgba".into(),
            ));
        }
        let id = self.alloc_id();
        let rel = format!("{id:04}.{}", format.extension());
        let abs = self.root.join(&rel);
        encode_rgba8_to_path(&abs, width, height, rgba, format)?;
        let thumb_rel = write_thumb(&self.root, id, width, height, rgba)?;
        let entry = ClipEntry {
            id,
            rel_path: rel,
            thumb_rel: Some(thumb_rel),
            width,
            height,
            format,
            created_unix: now_unix(),
        };
        self.index.entries.push(entry.clone());
        self.persist_index()?;
        Ok(entry)
    }

    /// Write an already-encoded payload (JPEG/PNG/EXR bytes) as a new slot.
    pub fn push_encoded(
        &mut self,
        bytes: &[u8],
        format: ClipFormat,
        width: u32,
        height: u32,
    ) -> Result<ClipEntry> {
        let id = self.alloc_id();
        let rel = format!("{id:04}.{}", format.extension());
        let abs = self.root.join(&rel);
        fs::write(&abs, bytes)?;
        let thumb_rel = match decode_rgba8_thumb(bytes, format) {
            Some((w, h, rgba)) => write_thumb(&self.root, id, w, h, &rgba).ok(),
            None => None,
        };
        let entry = ClipEntry {
            id,
            rel_path: rel,
            thumb_rel,
            width,
            height,
            format,
            created_unix: now_unix(),
        };
        self.index.entries.push(entry.clone());
        self.persist_index()?;
        Ok(entry)
    }

    /// Write linear RGBA f16 as OpenEXR (requires `exr` feature).
    #[cfg(feature = "exr")]
    pub fn push_f16_rgba(
        &mut self,
        width: u32,
        height: u32,
        rgba_f16: &[half::f16],
    ) -> Result<ClipEntry> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| ClipError::Msg("dimensions overflow".into()))?;
        if rgba_f16.len() != expected {
            return Err(ClipError::Msg(format!(
                "f16 rgba len {} != {}x{}x4",
                rgba_f16.len(),
                width,
                height
            )));
        }
        let f32s: Vec<f32> = rgba_f16.iter().map(|h| h.to_f32()).collect();
        self.push_f32_rgba(width, height, &f32s)
    }

    /// Write linear RGBA f32 as OpenEXR (requires `exr` feature).
    #[cfg(feature = "exr")]
    pub fn push_f32_rgba(
        &mut self,
        width: u32,
        height: u32,
        rgba_f32: &[f32],
    ) -> Result<ClipEntry> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(|| ClipError::Msg("dimensions overflow".into()))?;
        if rgba_f32.len() != expected {
            return Err(ClipError::Msg(format!(
                "f32 rgba len {} != {}x{}x4",
                rgba_f32.len(),
                width,
                height
            )));
        }
        let id = self.alloc_id();
        let rel = format!("{id:04}.exr");
        let abs = self.root.join(&rel);
        write_exr_rgba(&abs, width, height, rgba_f32)?;
        let thumb_rgba: Vec<u8> = rgba_f32
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| px.map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8))
            .collect();
        let thumb_rel = write_thumb(&self.root, id, width, height, &thumb_rgba)?;
        let entry = ClipEntry {
            id,
            rel_path: rel,
            thumb_rel: Some(thumb_rel),
            width,
            height,
            format: ClipFormat::Exr,
            created_unix: now_unix(),
        };
        self.index.entries.push(entry.clone());
        self.persist_index()?;
        Ok(entry)
    }

    #[cfg(not(feature = "exr"))]
    pub fn push_f16_rgba(
        &mut self,
        _width: u32,
        _height: u32,
        _rgba_f16: &[u16],
    ) -> Result<ClipEntry> {
        Err(ClipError::Feature("rebuild with feature `exr`".into()))
    }

    pub fn remove(&mut self, id: u32) -> Result<()> {
        let Some(pos) = self.index.entries.iter().position(|e| e.id == id) else {
            return Err(ClipError::Msg(format!("no clip id {id}")));
        };
        let entry = self.index.entries.remove(pos);
        let _ = fs::remove_file(entry.path(&self.root));
        if let Some(t) = entry.thumb_path(&self.root) {
            let _ = fs::remove_file(t);
        }
        self.persist_index()?;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        let ids: Vec<u32> = self.index.entries.iter().map(|e| e.id).collect();
        for id in ids {
            self.remove(id)?;
        }
        Ok(())
    }

    fn alloc_id(&mut self) -> u32 {
        let id = if self.index.next_id == 0 {
            1
        } else {
            self.index.next_id
        };
        self.index.next_id = id.saturating_add(1);
        id
    }

    fn persist_index(&self) -> Result<()> {
        let path = self.root.join(INDEX_NAME);
        let text = serde_json::to_string_pretty(&self.index)?;
        fs::write(path, text)?;
        Ok(())
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn encode_rgba8_to_path(
    path: &Path,
    width: u32,
    height: u32,
    rgba: &[u8],
    format: ClipFormat,
) -> Result<()> {
    #[cfg(feature = "jpeg-png")]
    {
        use image::{ImageBuffer, Rgba};
        let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| ClipError::Msg("ImageBuffer::from_raw failed".into()))?;
        match format {
            ClipFormat::Jpeg { quality } => {
                let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
                let mut file = fs::File::create(path)?;
                let mut enc =
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, quality);
                enc.encode(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(|e| ClipError::Msg(format!("jpeg encode: {e}")))?;
            }
            ClipFormat::Png => {
                img.save(path)
                    .map_err(|e| ClipError::Msg(format!("png encode: {e}")))?;
            }
            ClipFormat::Exr => {
                return Err(ClipError::Msg("internal: Exr via rgba8".into()));
            }
        }
        Ok(())
    }
    #[cfg(not(feature = "jpeg-png"))]
    {
        let _ = (path, width, height, rgba, format);
        Err(ClipError::Feature("rebuild with feature `jpeg-png`".into()))
    }
}

fn write_thumb(root: &Path, id: u32, width: u32, height: u32, rgba: &[u8]) -> Result<String> {
    #[cfg(feature = "jpeg-png")]
    {
        use image::{ImageBuffer, Rgba, imageops};
        let img: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_raw(width, height, rgba.to_vec())
            .ok_or_else(|| ClipError::Msg("thumb ImageBuffer failed".into()))?;
        let (tw, th) = fit_thumb(width, height, THUMB_MAX);
        let resized = imageops::resize(&img, tw, th, imageops::FilterType::Triangle);
        let rel = format!("{THUMBS_DIR}/{id:04}.jpg");
        let abs = root.join(&rel);
        let rgb = image::DynamicImage::ImageRgba8(resized).to_rgb8();
        let mut file = fs::File::create(&abs)?;
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 80);
        enc.encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| ClipError::Msg(format!("thumb jpeg: {e}")))?;
        Ok(rel)
    }
    #[cfg(not(feature = "jpeg-png"))]
    {
        let _ = (root, id, width, height, rgba);
        Err(ClipError::Feature("rebuild with feature `jpeg-png`".into()))
    }
}

fn fit_thumb(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (1, 1);
    }
    if w <= max && h <= max {
        return (w, h);
    }
    let scale = (max as f32 / w as f32).min(max as f32 / h as f32);
    (
        ((w as f32) * scale).round().max(1.0) as u32,
        ((h as f32) * scale).round().max(1.0) as u32,
    )
}

fn decode_rgba8_thumb(bytes: &[u8], format: ClipFormat) -> Option<(u32, u32, Vec<u8>)> {
    #[cfg(feature = "jpeg-png")]
    {
        if matches!(format, ClipFormat::Exr) {
            return None;
        }
        let img = image::load_from_memory(bytes).ok()?.to_rgba8();
        Some((img.width(), img.height(), img.into_raw()))
    }
    #[cfg(not(feature = "jpeg-png"))]
    {
        let _ = (bytes, format);
        None
    }
}

/// Linear RGBA f32 → float EXR through exr-core (our OpenEXR port), ZIP, atomic publish.
/// The caller has already checked `rgba.len() == width * height * 4`.
#[cfg(feature = "exr")]
fn write_exr_rgba(path: &Path, width: u32, height: u32, rgba: &[f32]) -> Result<()> {
    use exr_core::attr::Compression;
    use exr_core::{ChannelData, Image};
    use imath_rs::{Box2i, V2i};

    let err = |e: exr_core::ExrError| ClipError::Msg(format!("exr write: {e}"));
    let coord = |v: u32| {
        i32::try_from(v.saturating_sub(1)).map_err(|_| ClipError::Msg(format!("exr size {v} too large")))
    };
    let window = Box2i {
        min: V2i { x: 0, y: 0 },
        max: V2i { x: coord(width)?, y: coord(height)? },
    };
    let plane = |c: usize| ChannelData::Float(rgba.iter().skip(c).step_by(4).copied().collect());
    Image::new(window)
        .with_channel("R", plane(0))
        .with_channel("G", plane(1))
        .with_channel("B", plane(2))
        .with_channel("A", plane(3))
        .write(path, Compression::Zip)
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn temp_root(name: &str) -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!("egui-image-clip-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn push_list_remove_roundtrip() {
        let root = temp_root("roundtrip");
        let mut store = ClipStore::open(&root).expect("open");
        // 2x2 red
        let rgba = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];
        let e1 = store
            .push_rgba8(2, 2, &rgba, ClipFormat::jpeg(90))
            .expect("jpeg");
        assert_eq!(e1.id, 1);
        assert!(e1.path(&root).exists());
        assert!(e1.thumb_path(&root).unwrap().exists());

        let e2 = store.push_rgba8(2, 2, &rgba, ClipFormat::Png).expect("png");
        assert_eq!(e2.id, 2);
        assert_eq!(store.list().len(), 2);

        store.remove(1).expect("remove");
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].id, 2);

        // reopen
        let store2 = ClipStore::open(&root).expect("reopen");
        assert_eq!(store2.list().len(), 1);
        assert_eq!(store2.list()[0].format, ClipFormat::Png);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn push_encoded_preserves_bytes() {
        let root = temp_root("encoded");
        let mut store = ClipStore::open(&root).expect("open");
        let rgba = vec![
            0u8, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255,
        ];
        // encode via push then read bytes and re-push
        let first = store
            .push_rgba8(2, 2, &rgba, ClipFormat::jpeg(85))
            .expect("first");
        let bytes = fs::read(first.path(&root)).expect("read");
        let second = store
            .push_encoded(&bytes, ClipFormat::jpeg(85), 2, 2)
            .expect("encoded");
        assert_eq!(fs::read(second.path(&root)).unwrap(), bytes);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(feature = "exr")]
    #[test]
    fn push_f32_exr() {
        let root = temp_root("exr");
        let mut store = ClipStore::open(&root).expect("open");
        let rgba = vec![
            0.5f32, 0.25, 0.125, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.1, 0.2, 0.3, 1.0,
        ];
        let e = store.push_f32_rgba(2, 2, &rgba).expect("exr");
        assert!(e.format.is_exr());
        let image = exr_core::Image::read(e.path(&root)).expect("read back");
        for (c, name) in ["R", "G", "B", "A"].iter().enumerate() {
            let want: Vec<f32> = rgba.iter().skip(c).step_by(4).copied().collect();
            assert_eq!(image.channel(name).expect(name).to_f32(), want, "{name}");
        }
        let _ = fs::remove_dir_all(&root);
    }
}
