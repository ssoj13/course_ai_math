//! Screenshots in the display profile they were shown in: the [`Capture`] type (the
//! framebuffer pixels plus the metadata needed to interpret them) and its PNG encoder. No
//! wgpu here: [`crate::PresentPass::capture`] makes the capture. This crate writes no
//! files: the host owns the destination (temp sibling + atomic rename) and, for scRGB,
//! the EXR encoder.
//!
//! | Output | Pixels | PNG written by [`Capture::write_png`] |
//! |--------|--------|------|
//! | SDR 8-bit | RGBA8, sRGB codes | 8-bit sRGB PNG (`sRGB` chunk) |
//! | SDR 10-bit | RGBA16 (10-bit codes widened) | 16-bit sRGB PNG |
//! | HDR10 | RGBA16, BT.2100 PQ codes, BT.2020 | 16-bit PNG + `cICP` 9/16/0/1, `mDCV`, `cLLI` |
//! | HLG | RGBA16, BT.2100 HLG codes, BT.2020 | 16-bit PNG + `cICP` 9/18/0/1, `mDCV` |
//! | scRGB | RGBA f16, linear BT.709, 1.0 = 80 nits | none (hosts write half-float EXR) |
//!
//! A PNG also records the output, the SDR reference white and the display peak it was
//! presented for (`tEXt`), so the picture can be read back the way it was seen. `cICP` is
//! what HDR-aware viewers and browsers read (PNG 3rd edition); the png crate parses it but
//! does not write it, so the chunks are written through its public `write_chunk`.
use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::{Output, pq_nits};

/// What a user-facing "Save Screenshot" writes. The app persists it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScreenshotMode {
    /// The current display profile: the output's own signal and file format (see the
    /// module table).
    #[default]
    AsDisplayed,
    /// SDR: the canvas encoded for an 8-bit sRGB surface (HDR values clipped at SDR
    /// white), an 8-bit sRGB PNG.
    Sdr,
}

impl ScreenshotMode {
    /// Every mode, in menu order.
    pub const ALL: [Self; 2] = [Self::AsDisplayed, Self::Sdr];

    /// UI name.
    pub fn label(self) -> &'static str {
        match self {
            Self::AsDisplayed => "As displayed",
            Self::Sdr => "SDR (sRGB 8-bit)",
        }
    }

    /// The output a capture in this mode encodes for, when the window shows `shown`.
    pub fn output(self, shown: Output) -> Output {
        match self {
            Self::AsDisplayed => shown,
            Self::Sdr => Output::Sdr8,
        }
    }
}

/// Colour primaries of a capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primaries {
    /// ITU-R BT.709 / sRGB (H.273 code 1).
    Bt709,
    /// ITU-R BT.2020 / BT.2100 (H.273 code 9).
    Bt2020,
}

impl Primaries {
    /// CIE xy of red, green, blue and the D65 white.
    pub fn xy(self) -> [[f32; 2]; 4] {
        const D65: [f32; 2] = [0.3127, 0.3290];
        match self {
            Self::Bt709 => [[0.64, 0.33], [0.30, 0.60], [0.15, 0.06], D65],
            Self::Bt2020 => [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046], D65],
        }
    }

    /// ITU-T H.273 ColourPrimaries.
    pub fn h273(self) -> u8 {
        match self {
            Self::Bt709 => 1,
            Self::Bt2020 => 9,
        }
    }
}

/// Transfer function of a capture's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    /// IEC 61966-2-1 sRGB (H.273 code 13).
    Srgb,
    /// SMPTE ST 2084 / BT.2100 PQ, absolute nits (H.273 code 16).
    Pq,
    /// BT.2100 HLG (H.273 code 18).
    Hlg,
    /// Linear light, 1.0 = 80 nits (scRGB).
    Linear,
}

impl Transfer {
    /// ITU-T H.273 TransferCharacteristics (linear: 8).
    pub fn h273(self) -> u8 {
        match self {
            Self::Srgb => 13,
            Self::Pq => 16,
            Self::Hlg => 18,
            Self::Linear => 8,
        }
    }
}

/// Interleaved RGBA pixels, rows top first, in the output's surface format.
#[derive(Clone, Debug, PartialEq)]
pub enum Pixels {
    /// 8-bit codes (`Rgba8Unorm`).
    Rgba8(Vec<u8>),
    /// 16-bit codes: a 10-bit surface's codes widened by bit replication, so 0 and 1023
    /// land on 0 and 65535 (`Rgb10a2Unorm`).
    Rgba16(Vec<u16>),
    /// Half floats (`Rgba16Float`).
    RgbaF16(Vec<half::f16>),
}

/// One frame as the window showed it on [`Self::output`].
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    /// The output the pixels are encoded for.
    pub output: Output,
    pub width: u32,
    pub height: u32,
    /// SDR reference white, nits (canvas 1.0).
    pub white_nits: f32,
    /// The display peak, nits (HLG's `L_W`; recorded for the others).
    pub peak_nits: f32,
    pub pixels: Pixels,
}

/// Why a capture could not be written.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("png: {0}")]
    Png(#[from] png::EncodingError),
    #[error("{0:?} pixels do not fit a {1:?} file")]
    Pixels(&'static str, Output),
}

/// The `cLLI` chunk of `light`, in 0.0001 nits.
fn clli(light: ContentLight) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&((light.max_cll * 10_000.0).round() as u32).to_be_bytes());
    out.extend_from_slice(&((light.max_fall * 10_000.0).round() as u32).to_be_bytes());
    out
}

/// Content light level (CTA-861.3) in nits: the brightest pixel (MaxCLL) and the frame's
/// average (MaxFALL; over a video, the brightest frame's).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ContentLight {
    pub max_cll: f64,
    pub max_fall: f64,
}

impl Capture {
    /// The primaries of the pixels: BT.2020 for PQ and HLG, BT.709 otherwise.
    pub fn primaries(&self) -> Primaries {
        match self.output {
            Output::Hdr10 | Output::Hlg => Primaries::Bt2020,
            _ => Primaries::Bt709,
        }
    }

    /// The transfer function of the pixels.
    pub fn transfer(&self) -> Transfer {
        match self.output {
            Output::Sdr8 | Output::Sdr10 => Transfer::Srgb,
            Output::Hdr10 => Transfer::Pq,
            Output::Hlg => Transfer::Hlg,
            Output::Scrgb => Transfer::Linear,
        }
    }

    /// The metadata every file carries, as key / value text.
    fn notes(&self) -> [(&'static str, String); 3] {
        [
            ("Output", self.output.label().to_owned()),
            ("SDRReferenceWhite", format!("{} nits", self.white_nits)),
            ("DisplayPeak", format!("{} nits", self.peak_nits)),
        ]
    }

    /// The PNG of an SDR / PQ / HLG capture (see the module table).
    pub fn write_png<W: Write>(&self, out: W) -> Result<(), WriteError> {
        self.write_png_measured(out).map(|_| ())
    }

    /// Write PNG and return the exact content light level written as cLLI, measured once.
    /// Accepts a writer so callers can use their existing atomic publication primitive.
    pub fn write_png_measured<W: Write>(&self, out: W) -> Result<Option<ContentLight>, WriteError> {
        let light = self.content_light();
        self.png(out, light)?;
        Ok(light)
    }

    /// The PNG with `light` (this capture's [`Self::content_light`]) as its `cLLI`.
    fn png<W: Write>(&self, out: W, light: Option<ContentLight>) -> Result<(), WriteError> {
        let mut info = png::Info::with_size(self.width, self.height);
        info.color_type = png::ColorType::Rgba;
        let data: Vec<u8> = match (&self.pixels, self.output) {
            (Pixels::Rgba8(px), Output::Sdr8) => {
                info.bit_depth = png::BitDepth::Eight;
                px.clone()
            }
            (Pixels::Rgba16(px), Output::Sdr10 | Output::Hdr10 | Output::Hlg) => {
                info.bit_depth = png::BitDepth::Sixteen;
                // PNG samples are big-endian.
                px.iter().flat_map(|v| v.to_be_bytes()).collect()
            }
            (Pixels::Rgba8(_), o) => return Err(WriteError::Pixels("8-bit", o)),
            (Pixels::Rgba16(_), o) => return Err(WriteError::Pixels("16-bit", o)),
            (Pixels::RgbaF16(_), o) => return Err(WriteError::Pixels("half-float", o)),
        };
        if self.transfer() == Transfer::Srgb {
            info.srgb = Some(png::SrgbRenderingIntent::RelativeColorimetric);
        }
        let mut writer = png::Encoder::with_info(out, info)?.write_header()?;
        if let Some(cicp) = self.cicp() {
            writer.write_chunk(png::chunk::cICP, &cicp)?;
        }
        if let Some(mdcv) = self.mdcv() {
            writer.write_chunk(png::chunk::mDCV, &mdcv)?;
        }
        if let Some(clli) = light.map(clli) {
            writer.write_chunk(png::chunk::cLLI, &clli)?;
        }
        for (key, value) in self.notes() {
            let mut text = key.as_bytes().to_vec();
            text.push(0);
            text.extend_from_slice(value.as_bytes());
            writer.write_chunk(png::chunk::tEXt, &text)?;
        }
        writer.write_image_data(&data)?;
        writer.finish()?;
        Ok(())
    }

    /// The `cICP` chunk of a PQ / HLG capture: primaries, transfer, matrix 0 (RGB),
    /// full range.
    pub fn cicp(&self) -> Option<[u8; 4]> {
        matches!(self.transfer(), Transfer::Pq | Transfer::Hlg)
            .then(|| [self.primaries().h273(), self.transfer().h273(), 0, 1])
    }

    /// The `mDCV` chunk of a PQ / HLG capture: the BT.2020 display it was presented
    /// for (primaries and white in 0.00002 units, peak in 0.0001 nits, black 0.0001 nits).
    fn mdcv(&self) -> Option<Vec<u8>> {
        self.cicp()?;
        let mut out = Vec::with_capacity(24);
        for [x, y] in self.primaries().xy() {
            out.extend_from_slice(&((x / 0.00002).round() as u16).to_be_bytes());
            out.extend_from_slice(&((y / 0.00002).round() as u16).to_be_bytes());
        }
        out.extend_from_slice(&((self.peak_nits * 10_000.0).round() as u32).to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        Some(out)
    }

    /// The content light level of a PQ capture, measured from its pixels (CTA-861.3: the
    /// brightest channel of each pixel). HLG is relative to the display, so it has none.
    /// A video of many captures takes the maximum of both over its frames.
    pub fn content_light(&self) -> Option<ContentLight> {
        let (Transfer::Pq, Pixels::Rgba16(px)) = (self.transfer(), &self.pixels) else {
            return None;
        };
        let (mut max, mut sum) = (0.0f64, 0.0f64);
        for p in px.as_chunks::<4>().0 {
            let brightest = p[..3]
                .iter()
                .map(|v| f64::from(pq_nits(f32::from(*v) / 65535.0)))
                .fold(0.0, f64::max);
            max = max.max(brightest);
            sum += brightest;
        }
        let n = (px.len() / 4).max(1) as f64;
        Some(ContentLight { max_cll: max, max_fall: sum / n })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(output: Output, pixels: Pixels) -> Capture {
        Capture {
            output,
            width: 2,
            height: 1,
            white_nits: 203.0,
            peak_nits: 1000.0,
            pixels,
        }
    }

    /// The raw chunks of a PNG byte stream, by type.
    fn chunks(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 8;
        while at + 8 <= png.len() {
            let len = u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]) as usize;
            let kind = [png[at + 4], png[at + 5], png[at + 6], png[at + 7]];
            out.push((kind, png[at + 8..at + 8 + len].to_vec()));
            at += 12 + len;
        }
        out
    }

    /// PQ and HLG PNGs carry cICP = BT.2020 primaries (9), PQ (16) or HLG (18), matrix 0,
    /// full range, before IDAT; SDR PNGs carry sRGB and no cICP. The png decoder reads
    /// the chunk back, and the pixels round-trip.
    #[test]
    fn png_cicp_chunk_bytes() {
        let px = vec![0, 32768, 65535, 65535, 1023 << 6, 0, 0, 65535];
        for (output, want) in [(Output::Hdr10, [9, 16, 0, 1]), (Output::Hlg, [9, 18, 0, 1])] {
            let mut bytes = Vec::new();
            capture(output, Pixels::Rgba16(px.clone()))
                .write_png(&mut bytes)
                .expect("png");
            let chunks = chunks(&bytes);
            let kinds: Vec<&[u8; 4]> = chunks.iter().map(|(k, _)| k).collect();
            let cicp = kinds
                .iter()
                .position(|k| *k == b"cICP")
                .expect("cICP written");
            let idat = kinds.iter().position(|k| *k == b"IDAT").expect("IDAT");
            assert!(cicp < idat, "cICP precedes the image data");
            assert_eq!(chunks[cicp].1, want, "{output:?}");
            assert!(kinds.contains(&b"mDCV"), "{output:?} has mDCV");
            assert_eq!(kinds.contains(&b"cLLI"), output == Output::Hdr10);
            let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
            let mut reader = decoder.read_info().expect("decode");
            let read = reader
                .info()
                .coding_independent_code_points
                .expect("parsed cICP");
            assert_eq!(
                [
                    read.color_primaries,
                    read.transfer_function,
                    read.matrix_coefficients,
                    u8::from(read.is_video_full_range_image)
                ],
                want
            );
            let mdcv = reader
                .info()
                .mastering_display_color_volume
                .expect("parsed mDCV");
            assert_eq!(mdcv.max_luminance, 10_000_000, "1000 nits");
            let mut buf = vec![0; reader.output_buffer_size().expect("size")];
            reader.next_frame(&mut buf).expect("frame");
            let back: Vec<u16> = buf
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_be_bytes(*b))
                .collect();
            assert_eq!(back, px);
        }
        let mut bytes = Vec::new();
        capture(
            Output::Sdr8,
            Pixels::Rgba8(vec![1, 2, 3, 255, 4, 5, 6, 255]),
        )
        .write_png(&mut bytes)
        .expect("png");
        let kinds: Vec<[u8; 4]> = chunks(&bytes).into_iter().map(|(k, _)| k).collect();
        assert!(kinds.contains(b"sRGB") && !kinds.contains(b"cICP"));
        assert!(kinds.contains(b"tEXt"), "the output / white / peak notes");
    }

    /// cLLI of a PQ capture is its brightest pixel and frame average: here one pixel at
    /// 10000 nits (code 65535) and one black.
    #[test]
    fn clli_measures_the_pixels() {
        let c = capture(
            Output::Hdr10,
            Pixels::Rgba16(vec![65535, 0, 0, 65535, 0, 0, 0, 65535]),
        );
        let clli = clli(c.content_light().expect("PQ has cLLI"));
        let max = u32::from_be_bytes([clli[0], clli[1], clli[2], clli[3]]);
        let avg = u32::from_be_bytes([clli[4], clli[5], clli[6], clli[7]]);
        assert_eq!(max, 100_000_000, "10000 nits");
        assert_eq!(avg, 50_000_000, "half of it");
        assert!(
            (pq_nits(crate::pq(203.0)) - 203.0).abs() < 0.05,
            "pq_nits inverts pq"
        );
    }
}
