//! Display-light PNG and PNG-sequence video export shared by applications.
//! Extracted from WarpBro: Rec.709 display light, PQ/HLG arithmetic and Capture metadata.
use std::ffi::OsString;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

mod output_view;
pub use output_view::{OutputKind, OutputView, OutputViewEncoding, resolve_output_view};

/// The external ffmpeg executable, searched once using absolute PATH entries.
pub fn ffmpeg() -> Result<&'static Path, String> {
    static FOUND: std::sync::LazyLock<Option<PathBuf>> = std::sync::LazyLock::new(|| {
        let exe = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        std::env::split_paths(&std::env::var_os("PATH")?)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(exe))
            .find(|p| p.is_file())
    });
    FOUND
        .as_deref()
        .ok_or_else(|| "ffmpeg is not on PATH: the PNG export's video needs it".into())
}

/// Refuse an existing `dst` unless `overwrite` grants it; checked before any work starts so a
/// refused export costs nothing. [`publish`] repeats the check at the rename itself.
fn check_overwrite(dst: &Path, overwrite: bool) -> Result<(), String> {
    if !overwrite && dst.try_exists().map_err(|e| e.to_string())? {
        return Err(format!("output exists: {}", dst.display()));
    }
    Ok(())
}

/// Refuse an output that is the input file itself (in-place encoding would read a file while
/// truncating it). A path that does not exist cannot be the other one.
fn ensure_distinct(input: &Path, output: &Path) -> Result<(), String> {
    let id = |p: &Path| match std::fs::canonicalize(p) {
        Ok(c) => Ok(Some(c)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    };
    match (id(input)?, id(output)?) {
        (Some(a), Some(b)) if a == b => Err(format!(
            "output {} is the input file; cannot encode in place",
            output.display()
        )),
        _ => Ok(()),
    }
}

/// A temp file next to `dst` (a sibling, so the final rename stays on one volume and is
/// atomic). It keeps `dst`'s extension and is deleted on drop, so an error, a cancel or a
/// panic never leaves a half-written file at `dst`.
fn temp_for(dst: &Path, overwrite: bool) -> Result<tempfile::NamedTempFile, String> {
    check_overwrite(dst, overwrite)?;
    let dir = dst
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let suffix = dst
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    tempfile::Builder::new()
        .prefix(".export-")
        .suffix(&suffix)
        .tempfile_in(dir)
        .map_err(|e| e.to_string())
}

/// Rename the finished temp over `dst`. Without `overwrite` the rename itself refuses a `dst`
/// that appeared meanwhile.
fn publish(tmp: tempfile::TempPath, dst: &Path, overwrite: bool) -> Result<(), String> {
    let done = if overwrite {
        tmp.persist(dst)
    } else {
        tmp.persist_noclobber(dst)
    };
    done.map_err(|e| format!("{}: {}", dst.display(), e.error))
}

struct OwnedChild(std::process::Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // This guard precedes scoped reader joins, including unwinding callbacks.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Encode a finished PNG sequence. True means atomically published, false means cancelled.
/// The callback runs on the calling thread and receives completed frame counts.
pub fn encode_video(
    options: &PngVideoOptions<'_>,
    levels: Option<HdrLevels>,
    video: &Path,
    overwrite: bool,
    cancel: &AtomicBool,
    progress: impl FnMut(u32),
) -> Result<bool, String> {
    if cancel.load(Ordering::Acquire) {
        return Ok(false);
    }
    encode_video_with(
        ffmpeg()?,
        options,
        levels,
        (video, overwrite),
        cancel,
        progress,
    )
}

fn encode_video_with(
    executable: &Path,
    options: &PngVideoOptions<'_>,
    levels: Option<HdrLevels>,
    destination: (&Path, bool),
    cancel: &AtomicBool,
    mut progress: impl FnMut(u32),
) -> Result<bool, String> {
    let (video, overwrite) = destination;
    if options.frame_count == 1 {
        ensure_distinct(options.input, video)?;
    }
    // ffmpeg writes the temp by path: our handle is closed (Windows would block it), the path
    // still deletes the file on drop.
    let out = temp_for(video, overwrite)?.into_temp_path();
    let args = ffmpeg_args(options, levels, &out)?;
    let result = std::thread::scope(|scope| -> Result<bool, String> {
        let mut child = OwnedChild(
            std::process::Command::new(executable)
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("ffmpeg: {e}"))?,
        );
        let stdout = child
            .0
            .stdout
            .take()
            .ok_or("ffmpeg has no progress stream")?;
        let mut stderr = child.0.stderr.take().ok_or("ffmpeg has no error stream")?;
        let (send, receive) = std::sync::mpsc::sync_channel(16);
        let read_progress = scope.spawn(move || -> Result<(), String> {
            for line in std::io::BufReader::new(stdout).lines() {
                let line = line.map_err(|e| format!("ffmpeg progress: {e}"))?;
                if let Some(frame) = line
                    .strip_prefix("frame=")
                    .and_then(|s| s.trim().parse::<u32>().ok())
                {
                    match send.try_send(frame) {
                        Ok(()) | Err(std::sync::mpsc::TrySendError::Full(_)) => {}
                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => break,
                    }
                }
            }
            Ok(())
        });
        let read_error = scope.spawn(move || -> Result<String, String> {
            let mut bytes = Vec::new();
            stderr
                .read_to_end(&mut bytes)
                .map_err(|e| format!("ffmpeg stderr: {e}"))?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        });
        let status = loop {
            while let Ok(frame) = receive.try_recv() {
                progress(frame.min(options.frame_count));
            }
            if cancel.load(Ordering::Acquire) {
                return Ok(false);
            }
            if let Some(status) = child
                .0
                .try_wait()
                .map_err(|e| format!("ffmpeg wait: {e}"))?
            {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        read_progress
            .join()
            .map_err(|_| "ffmpeg progress reader panicked")??;
        while let Ok(frame) = receive.try_recv() {
            progress(frame.min(options.frame_count));
        }
        let error = read_error
            .join()
            .map_err(|_| "ffmpeg error reader panicked")??;
        if cancel.load(Ordering::Acquire) {
            return Ok(false);
        }
        if !status.success() {
            return Err(error
                .lines()
                .rev()
                .find(|s| !s.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("ffmpeg exited with {status}")));
        }
        Ok(true)
    })?;
    if result {
        publish(out, video, overwrite)?;
    }
    Ok(result)
}

/// Measure a tone-mapped display view's peak, using WarpBro's high scene-light probe.
/// `apply` must return linear Rec.709 display light; `unit_nits` describes its 1.0.
pub fn measure_display_peak(
    apply: impl FnOnce(&mut [[f32; 3]]),
    unit_nits: f32,
) -> Result<f32, String> {
    if !unit_nits.is_finite() || unit_nits <= 0.0 {
        return Err("Display-light unit must be finite and positive".into());
    }
    // ACES 2.0's asymptotic tone scale reaches its peak at this probe to f32 precision.
    const PEAK_PROBE: f32 = 1.0e5;
    let mut probe = [[PEAK_PROBE; 3]];
    apply(&mut probe);
    let peak_nits = unit_nits * probe[0][0].max(probe[0][1]).max(probe[0][2]);
    if probe[0].iter().any(|v| !v.is_finite())
        || !peak_nits.is_finite()
        || peak_nits <= 0.0
        || peak_nits > PQ_PEAK_NITS * 1.001
    {
        return Err(format!(
            "OCIO view has no measurable peak ({peak_nits} nits): not a tone-mapped HDR view"
        ));
    }
    Ok(peak_nits)
}

/// SDR video's reference display gamma (BT.1886), shared with its CPU adapters.
pub const BT1886_GAMMA: f32 = 2.4;

/// WarpBro's SDR video code for relative linear display light.
pub fn bt1886_code(light: f32) -> f32 {
    light.clamp(0.0, 1.0).powf(1.0 / BT1886_GAMMA)
}

/// Meaning of linear Rec.709 display light. Absolute light uses 1.0 = 100 nits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DisplayLight {
    Relative,
    Absolute { peak_nits: f32 },
}
impl DisplayLight {
    pub fn hdr(self) -> bool {
        matches!(self, Self::Absolute { .. })
    }
}

/// Invert extended-sRGB display codes without clipping RGB or transforming alpha.
pub fn display_encoded_to_linear(p: [f32; 4]) -> [f32; 4] {
    [
        crate::transfer::eotf(p[0]),
        crate::transfer::eotf(p[1]),
        crate::transfer::eotf(p[2]),
        p[3],
    ]
}

/// How a PNG encodes a frame's display light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PngEncoding {
    /// 8-bit sRGB / BT.709 SDR: the SDR view's codes (an HDR view is rendered for SDR too).
    #[default]
    Sdr8,
    /// 16-bit BT.2100 PQ, BT.2020 primaries; `cICP`, `mDCV`, `cLLI`.
    Hdr10,
    /// 16-bit BT.2100 HLG, BT.2020 primaries, for a display of the given peak; `cICP`, `mDCV`.
    Hlg,
}
impl PngEncoding {
    pub const ALL: [Self; 3] = [Self::Sdr8, Self::Hdr10, Self::Hlg];
    pub fn label(self) -> &'static str {
        match self {
            Self::Sdr8 => "SDR · 8-bit sRGB / BT.709",
            Self::Hdr10 => "HDR10 · 16-bit PQ / BT.2020",
            Self::Hlg => "HLG · 16-bit BT.2020",
        }
    }
    /// Signal and primaries, independent of the file or codec's sample precision.
    pub fn signal_label(self) -> &'static str {
        match self {
            Self::Sdr8 => "SDR · sRGB / BT.709",
            Self::Hdr10 => "HDR10 · PQ / BT.2020",
            Self::Hlg => "HLG · BT.2020",
        }
    }
    pub fn hdr(self) -> bool {
        self != Self::Sdr8
    }
    /// What a monitor shows: HDR10 for an HDR output, else SDR. PQ, not HLG: the monitor shows
    /// absolute display light, which PQ encodes as it is.
    pub fn displayed(hdr: bool) -> Self {
        if hdr { Self::Hdr10 } else { Self::Sdr8 }
    }
    /// The file suffix after the stem, for every PNG writer (snapshot, export, CLI): HDR PNGs
    /// name their transfer so they are not taken for SDR (an HDR10 PNG in a viewer that
    /// ignores `cICP` looks washed out).
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Sdr8 => "png",
            Self::Hdr10 => "pq.png",
            Self::Hlg => "hlg.png",
        }
    }
}

/// How a frame's display light becomes nits in an HDR PNG.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HdrScale {
    /// Nits of the light's 1.0: 100 for an HDR view's absolute light, the SDR white for
    /// relative light (the monitor's when it shows HDR, else BT.2408's 203).
    pub unit_nits: f32,
    /// The mastering display (`mDCV`, HLG system gamma): the HDR view's measured peak, or the
    /// brightest relative light in nits.
    pub peak_nits: f32,
}

/// The BT.2100 HLG reference display: the peak an HLG file of relative light is graded for,
/// so its SDR white (BT.2408: 203 nits) lands near 75 % signal, not at full signal.
pub const HLG_REFERENCE_PEAK_NITS: f32 = 1000.0;

/// The most light PQ carries (BT.2100).
const PQ_PEAK_NITS: f32 = 10_000.0;

/// The one rule from a frame's light to HDR file nits (`HdrScale`) for `encoding`, for every
/// PNG writer. Absolute light: 1.0 = 100 nits, the view's measured peak. Relative light: 1.0
/// at `sdr_white_nits`; the peak is the brightest light for PQ, the HLG reference display for
/// HLG (HLG is relative to its display).
pub fn hdr_scale(
    kind: DisplayLight,
    light: &[[f32; 4]],
    sdr_white_nits: f32,
    encoding: PngEncoding,
) -> HdrScale {
    let (unit_nits, peak_nits) = match (kind, encoding) {
        (DisplayLight::Absolute { peak_nits }, _) => (100.0, peak_nits),
        (DisplayLight::Relative, PngEncoding::Hlg) => (sdr_white_nits, HLG_REFERENCE_PEAK_NITS),
        (DisplayLight::Relative, _) => {
            let max = light
                .iter()
                .map(|p| p[0].max(p[1]).max(p[2]))
                .fold(1.0f32, f32::max);
            (sdr_white_nits, sdr_white_nits * max)
        }
    };
    HdrScale {
        unit_nits,
        peak_nits: if peak_nits.is_finite() {
            peak_nits.min(PQ_PEAK_NITS)
        } else {
            peak_nits
        },
    }
}

/// The light an HDR PNG records (`mDCV`, `cLLI`): what a video made of the sequence carries
/// as its HDR metadata (`export::ExportSettings::ffmpeg_args`), aggregated over the frames
/// (`Self::merge`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HdrLevels {
    /// The mastering display's peak (`HdrScale::peak_nits`).
    pub peak_nits: f32,
    /// Measured content light level; PQ only (HLG is relative to its display).
    pub content: Option<crate::screenshot::ContentLight>,
}
impl HdrLevels {
    /// The levels of a clip: the brightest mastering peak, MaxCLL and MaxFALL of all frames.
    pub fn merge(self, other: Self) -> Self {
        let content = match (self.content, other.content) {
            (Some(a), Some(b)) => Some(crate::screenshot::ContentLight {
                max_cll: a.max_cll.max(b.max_cll),
                max_fall: a.max_fall.max(b.max_fall),
            }),
            (a, b) => a.or(b),
        };
        Self {
            peak_nits: self.peak_nits.max(other.peak_nits),
            content,
        }
    }
}

/// Write display light as a PNG: the one encoder behind the viewport, export and CLI writers.
/// `light` is linear Rec.709 display light, `scale` says how it becomes nits (`hdr_scale`);
/// `sdr` yields the 8-bit sRGB codes, read only for SDR. Returns what an HDR file records
/// (None for SDR).
#[allow(clippy::too_many_arguments)]
pub fn write_png(
    path: &Path,
    width: usize,
    height: usize,
    light: &[[f32; 4]],
    sdr: impl FnOnce() -> Vec<u8>,
    encoding: PngEncoding,
    scale: HdrScale,
    overwrite: bool,
) -> Result<Option<HdrLevels>, String> {
    let count = width
        .checked_mul(height)
        .filter(|n| *n > 0)
        .ok_or("PNG dimensions must be positive and fit usize")?;
    let width = u32::try_from(width).map_err(|_| "PNG width exceeds u32")?;
    let height = u32::try_from(height).map_err(|_| "PNG height exceeds u32")?;
    if !scale.unit_nits.is_finite()
        || scale.unit_nits <= 0.0
        || !scale.peak_nits.is_finite()
        || !(0.0..=PQ_PEAK_NITS).contains(&scale.peak_nits)
        || scale.peak_nits == 0.0
    {
        return Err("PNG light scale must be finite and positive, with peak <= 10000 nits".into());
    }
    if encoding.hdr() && (light.len() != count || light.iter().flatten().any(|v| !v.is_finite())) {
        return Err("HDR PNG needs finite RGBA display light for every pixel".into());
    }
    if encoding.hdr()
        && light.iter().any(|p| {
            crate::rec2020_nits([p[0], p[1], p[2]], scale.unit_nits)
                .iter()
                .any(|v| !v.is_finite())
        })
    {
        return Err("HDR PNG display light overflows its nits scale".into());
    }
    let peak_nits = scale.peak_nits;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    check_overwrite(path, overwrite)?;
    let u16s = |codes: [f32; 3]| codes.map(|c| (c.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16);
    let nits = |p: &[f32; 4]| crate::rec2020_nits([p[0], p[1], p[2]], scale.unit_nits);
    let hdr = |code: &dyn Fn([f32; 3]) -> [f32; 3]| {
        crate::screenshot::Pixels::Rgba16(
            light
                .iter()
                .flat_map(|p| {
                    let [r, g, b] = u16s(code(nits(p)));
                    [r, g, b, (p[3].clamp(0.0, 1.0) * 65535.0 + 0.5) as u16]
                })
                .collect(),
        )
    };
    let (output, pixels) = match encoding {
        PngEncoding::Sdr8 => (crate::Output::Sdr8, crate::screenshot::Pixels::Rgba8(sdr())),
        PngEncoding::Hdr10 => (
            crate::Output::Hdr10,
            hdr(&|n| n.map(|v| crate::pq(v.clamp(0.0, 10000.0)))),
        ),
        PngEncoding::Hlg => (
            crate::Output::Hlg,
            // Unclamped, as the present pass (`present::encode`): `hlg` takes the whole pixel's
            // luminance and clips only the signal; clamping a negative BT.2020 component first
            // would change that luminance and every channel's code with it.
            hdr(&|n| crate::hlg(n, peak_nits)),
        ),
    };
    if let crate::screenshot::Pixels::Rgba8(ref bytes) = pixels
        && count.checked_mul(4) != Some(bytes.len())
    {
        return Err("SDR PNG needs four byte samples per pixel".into());
    }
    let capture = crate::screenshot::Capture {
        output,
        width,
        height,
        // The file says where its SDR white is and which display it was graded for.
        white_nits: scale.unit_nits,
        peak_nits: if encoding.hdr() {
            peak_nits
        } else {
            scale.unit_nits
        },
        pixels,
    };
    let mut out = temp_for(path, overwrite)?;
    let content = capture
        .write_png_measured(&mut out)
        .map_err(|e| e.to_string())?;
    publish(out.into_temp_path(), path, overwrite)?;
    Ok(encoding.hdr().then_some(HdrLevels { peak_nits, content }))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PngVideo {
    #[default]
    Off,
    /// ProRes 4444 XQ, 10-bit 4:4:4 `.mov` (the most `prores_ks` takes): a master for editing
    /// and grading.
    ProRes,
    /// HEVC Main10 4:2:0 `.mp4` (libx265, CRF = Quality); HDR10 metadata for PQ.
    Hevc,
}
impl PngVideo {
    pub const ALL: [Self; 3] = [Self::Off, Self::ProRes, Self::Hevc];
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::ProRes => "ProRes 4444 XQ · .mov",
            Self::Hevc => "HEVC 10-bit · .mp4",
        }
    }
    pub fn extension(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::ProRes => Some("mov"),
            Self::Hevc => Some("mp4"),
        }
    }
}

/// The H.273 colour description of a PNG encoding, in ffmpeg's names: primaries, transfer,
/// matrix (the YUV a video derives from the PNG's RGB).
pub fn video_tags(png: PngEncoding) -> [&'static str; 3] {
    match png {
        // Video codes, not the PNGs' sRGB ones: `sdr_to_video` re-encodes them for BT.1886.
        PngEncoding::Sdr8 => ["bt709", "bt709", "bt709"],
        PngEncoding::Hdr10 => ["bt2020", "smpte2084", "bt2020nc"],
        PngEncoding::Hlg => ["bt2020", "arib-std-b67", "bt2020nc"],
    }
}

/// The ffmpeg filters that turn an SDR PNG's sRGB codes into SDR video codes: sRGB's EOTF to
/// relative display light, then `color::bt1886_code` (`L^(1/2.4)`), at 16 bits (`lutrgb`
/// tabulates every code). Ends with a `,` for the filters that follow.
pub fn sdr_to_video() -> String {
    // Quoted, so the expression's commas stay inside it.
    let light = "if(lte(val/maxval,0.04045),val/maxval/12.92,pow((val/maxval+0.055)/1.055,2.4))";
    let code = format!("pow({light},1/{})*maxval", BT1886_GAMMA);
    format!("format=rgb48le,lutrgb=r='{code}':g='{code}':b='{code}',")
}

/// A finished PNG sequence; `input` is the literal path for one frame, otherwise a
/// caller-built image2 pattern (literal percent signs in names must be escaped).
#[derive(Clone, Copy, Debug)]
pub struct PngVideoOptions<'a> {
    pub codec: PngVideo,
    pub encoding: PngEncoding,
    pub input: &'a Path,
    pub start_number: u32,
    pub frame_count: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub qp: u32,
}
fn validate_video(options: &PngVideoOptions<'_>) -> Result<(), String> {
    if options.frame_count == 0
        || options.fps_num == 0
        || options.fps_den == 0
        || options.qp > 51
        || options.input.as_os_str().is_empty()
        || options
            .start_number
            .checked_add(options.frame_count - 1)
            .is_none()
    {
        return Err("Invalid PNG video input, frame range, frame rate or quality".into());
    }
    Ok(())
}
fn validate_levels(levels: HdrLevels) -> Result<(), String> {
    if !levels.peak_nits.is_finite()
        || !(0.0..=PQ_PEAK_NITS).contains(&levels.peak_nits)
        || levels.peak_nits == 0.0
        || levels.content.is_some_and(|c| {
            !c.max_cll.is_finite()
                || !c.max_fall.is_finite()
                || c.max_cll < 0.0
                || c.max_fall < 0.0
                || c.max_fall > c.max_cll
        })
    {
        return Err("Invalid HDR mastering or content light levels".into());
    }
    Ok(())
}

pub fn ffmpeg_args(
    options: &PngVideoOptions<'_>,
    levels: Option<HdrLevels>,
    output: &Path,
) -> Result<Vec<OsString>, String> {
    validate_video(options)?;
    let [primaries, transfer, matrix] = video_tags(options.encoding);
    let rate = format!("{}/{}", options.fps_num, options.fps_den);
    let mut args: Vec<OsString> = [
        "-hide_banner",
        "-nostdin",
        "-nostats",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
        "-y",
        "-framerate",
        &rate,
        "-f",
        "image2",
    ]
    .map(OsString::from)
    .to_vec();
    // The demuxer is named: a lone `.png` would otherwise open as `png_pipe`, which takes no
    // `-start_number`. A single frame is its file as it is; a sequence is `%`-escaped.
    if options.frame_count == 1 {
        args.extend([
            "-pattern_type".into(),
            "none".into(),
            "-i".into(),
            options.input.as_os_str().to_owned(),
        ]);
    } else {
        args.extend([
            "-pattern_type".into(),
            "sequence".into(),
            "-start_number".into(),
            options.start_number.to_string().into(),
            "-i".into(),
            options.input.as_os_str().to_owned(),
        ]);
    }
    // The encoders take the colour description from the filtered frames, so `setparams`
    // states it (output CLI flags would not; a PNG's cICP only would, where ffmpeg reads it).
    // ffmpeg would also lift the FIRST PNG's cLLI into the container's `clli`, contradicting
    // the clip's MaxCLL / MaxFALL: it is dropped, the clip's goes into the HEVC SEI.
    // An SDR PNG holds sRGB codes; its video holds BT.1886 ones (`color::bt1886_code`).
    let recode = if options.encoding == PngEncoding::Sdr8 {
        sdr_to_video()
    } else {
        String::new()
    };
    let filters = format!(
        "sidedata=mode=delete:type=CONTENT_LIGHT_LEVEL,{recode}\
             scale=out_color_matrix={matrix}:out_range=tv:flags=accurate_rnd+full_chroma_int,\
             setparams=color_primaries={primaries}:color_trc={transfer}:colorspace={matrix}:range=tv"
    );
    args.extend([OsString::from("-vf"), filters.into()]);
    match options.codec {
        PngVideo::Off => return Err("The PNG export encodes no video".into()),
        PngVideo::ProRes => args.extend(
            [
                "-c:v",
                "prores_ks",
                "-profile:v",
                "4444xq",
                "-pix_fmt",
                "yuv444p10le",
                "-vendor",
                "apl0",
                "-f",
                "mov",
            ]
            .map(OsString::from),
        ),
        PngVideo::Hevc => {
            let mut x265 = format!(
                "colorprim={primaries}:transfer={transfer}:colormatrix={matrix}:range=limited"
            );
            if options.encoding == PngEncoding::Hdr10 {
                let levels = levels.ok_or("An HDR10 video needs the frames' light levels")?;
                validate_levels(levels)?;
                let content = levels
                    .content
                    .ok_or("An HDR10 video needs the frames' content light level")?;
                // x265 order: G, B, R, white in 0.00002; luminance in 0.0001 nits; CLL in nits.
                let [r, g, b, w] = crate::screenshot::Primaries::Bt2020.xy();
                let xy = |[x, y]: [f32; 2]| {
                    format!("({},{})", (x / 0.00002).round(), (y / 0.00002).round())
                };
                x265 += &format!(
                    ":hdr10=1:hdr10-opt=1:repeat-headers=1:master-display=G{}B{}R{}WP{}L({},1):max-cll={},{}",
                    xy(g),
                    xy(b),
                    xy(r),
                    xy(w),
                    (levels.peak_nits * 10_000.0).round(),
                    content.max_cll.ceil(),
                    content.max_fall.ceil(),
                );
            }
            let crf = options.qp.to_string();
            args.extend(
                [
                    "-c:v",
                    "libx265",
                    "-preset",
                    "slow",
                    "-crf",
                    &crf,
                    "-pix_fmt",
                    "yuv420p10le",
                    "-tag:v",
                    "hvc1",
                    "-x265-params",
                    &x265,
                    "-f",
                    "mp4",
                ]
                .map(OsString::from),
            );
        }
    }
    args.extend(["-movflags", "+write_colr"].map(OsString::from));
    args.extend(["-frames:v".into(), options.frame_count.to_string().into()]);
    args.push(output.as_os_str().to_owned());
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screenshot::ContentLight;

    fn chunks(bytes: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        let mut at = 8;
        let mut out = Vec::new();
        while at + 12 <= bytes.len() {
            let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            out.push((
                bytes[at + 4..at + 8].try_into().unwrap(),
                bytes[at + 8..at + 8 + len].to_vec(),
            ));
            at += len + 12;
        }
        out
    }
    fn options(input: &Path, encoding: PngEncoding) -> PngVideoOptions<'_> {
        PngVideoOptions {
            codec: PngVideo::Hevc,
            encoding,
            input,
            start_number: 7,
            frame_count: 2,
            fps_num: 30000,
            fps_den: 1001,
            qp: 18,
        }
    }
    fn levels() -> HdrLevels {
        HdrLevels {
            peak_nits: 1000.0,
            content: Some(ContentLight {
                max_cll: 812.2,
                max_fall: 312.1,
            }),
        }
    }
    #[test]
    fn original_warp_peak_probe_measures_selected_view_and_rejects_unbounded_light() {
        let peak = measure_display_peak(
            |probe| {
                assert_eq!(probe, &[[1.0e5; 3]]);
                probe[0] = [8.0, 10.0, 9.0];
            },
            100.0,
        )
        .unwrap();
        assert_eq!(peak, 1000.0);
        assert_eq!(
            measure_display_peak(|p| p[0] = [2000.0 / 203.0; 3], 203.0).unwrap(),
            2000.0
        );
        assert!(measure_display_peak(|_| {}, 100.0).is_err());
        assert!(measure_display_peak(|p| p[0] = [f32::NAN, 10.0, 9.0], 100.0).is_err());
        assert!(measure_display_peak(|p| p[0] = [0.0; 3], 100.0).is_err());
        assert!(
            measure_display_peak(|_| panic!("invalid unit must not call view"), f32::INFINITY)
                .is_err()
        );
        assert!(measure_display_peak(|p| p[0] = [100.1; 3], 100.0).is_ok());
        assert!(measure_display_peak(|p| p[0] = [100.2; 3], 100.0).is_err());
    }
    #[test]
    fn bt1886_video_codes_match_warp_dark_reference() {
        assert!((bt1886_code(0.01) - 0.14677993).abs() < 1e-7);
        assert_eq!(bt1886_code(-0.1), 0.0);
        assert_eq!(bt1886_code(4.0), 1.0);
        assert!((bt1886_code(0.01) - crate::transfer::oetf(0.01)).abs() > 0.04);
        assert!((bt1886_code(crate::transfer::eotf(0.1)) - 0.14691913).abs() < 1e-6);
    }
    #[test]
    fn original_warp_scaling_and_extended_display_codes() {
        let light = [[2.0, 0.5, -0.2, 0.3]];
        assert_eq!(
            hdr_scale(DisplayLight::Relative, &light, 203.0, PngEncoding::Hdr10),
            HdrScale {
                unit_nits: 203.0,
                peak_nits: 406.0
            }
        );
        assert_eq!(
            hdr_scale(DisplayLight::Relative, &light, 203.0, PngEncoding::Hlg),
            HdrScale {
                unit_nits: 203.0,
                peak_nits: 1000.0
            }
        );
        assert_eq!(
            hdr_scale(
                DisplayLight::Absolute { peak_nits: 1200.0 },
                &light,
                203.0,
                PngEncoding::Hdr10
            ),
            HdrScale {
                unit_nits: 100.0,
                peak_nits: 1200.0
            }
        );
        assert_eq!(
            hdr_scale(
                DisplayLight::Relative,
                &[[100.0; 4]],
                203.0,
                PngEncoding::Hdr10
            )
            .peak_nits,
            10000.0
        );
        assert!(
            hdr_scale(
                DisplayLight::Absolute {
                    peak_nits: f32::NAN
                },
                &light,
                203.0,
                PngEncoding::Hdr10
            )
            .peak_nits
            .is_nan()
        );
        let p = display_encoded_to_linear([1.5, -0.5, 0.04045, 0.3]);
        assert!(p[0] > 1.0 && p[1] < 0.0);
        assert_eq!(p[3], 0.3);
        assert!((p[2] - 0.04045 / 12.92).abs() < 1e-7);
        assert!(DisplayLight::Absolute { peak_nits: 1000.0 }.hdr());
        assert!(!DisplayLight::Relative.hdr());
    }
    #[test]
    fn png_matches_warp_rgb_arithmetic_preserves_alpha_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let light = [[2.0, 0.3, -0.1, 0.5], [1.0, 1.0, 1.0, 1.0]];
        for encoding in [PngEncoding::Hdr10, PngEncoding::Hlg] {
            let path = dir.path().join(encoding.suffix());
            let scale = hdr_scale(DisplayLight::Relative, &light, 203.0, encoding);
            let result = write_png(
                &path,
                2,
                1,
                &light,
                || panic!("HDR must not read SDR"),
                encoding,
                scale,
                false,
            )
            .unwrap()
            .unwrap();
            let bytes = std::fs::read(&path).unwrap();
            let chunks = chunks(&bytes);
            assert_eq!(
                chunks.iter().find(|c| &c.0 == b"cICP").unwrap().1,
                [
                    9,
                    if encoding == PngEncoding::Hdr10 {
                        16
                    } else {
                        18
                    },
                    0,
                    1
                ]
            );
            let mdcv = &chunks.iter().find(|c| &c.0 == b"mDCV").unwrap().1;
            assert_eq!(
                u32::from_be_bytes(mdcv[16..20].try_into().unwrap()),
                (scale.peak_nits * 10000.0).round() as u32
            );
            assert_eq!(
                chunks.iter().any(|c| &c.0 == b"cLLI"),
                encoding == PngEncoding::Hdr10
            );
            let mut reader = png::Decoder::new(std::io::Cursor::new(&bytes))
                .read_info()
                .unwrap();
            let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut decoded).unwrap();
            assert_eq!(info.bit_depth, png::BitDepth::Sixteen);
            let samples: Vec<u16> = decoded[..info.buffer_size()]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|s| u16::from_be_bytes([s[0], s[1]]))
                .collect();
            for (actual, p) in samples.as_chunks::<4>().0.iter().zip(light) {
                let n = crate::rec2020_nits([p[0], p[1], p[2]], scale.unit_nits);
                let codes = if encoding == PngEncoding::Hdr10 {
                    n.map(|v| crate::pq(v.clamp(0.0, 10000.0)))
                } else {
                    // The HLG file is the present pass's signal, out-of-gamut pixels included.
                    let canvas = [p[0], p[1], p[2]].map(crate::transfer::oetf);
                    let target = crate::Target { hdr: true, white: scale.unit_nits, peak: scale.peak_nits };
                    crate::encode(canvas, crate::Encoding::Hlg, target)
                };
                let expected = codes.map(|v| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16);
                assert_eq!(&actual[..3], &expected);
                assert_eq!(actual[3], (p[3] * 65535.0 + 0.5) as u16);
            }
            assert_eq!(result.peak_nits, scale.peak_nits);
            assert_eq!(result.content.is_some(), encoding == PngEncoding::Hdr10);
        }
    }
    #[test]
    fn measured_writer_returns_exact_recorded_content_light() {
        let capture = crate::screenshot::Capture {
            output: crate::Output::Hdr10,
            width: 2,
            height: 1,
            white_nits: 203.0,
            peak_nits: 10000.0,
            pixels: crate::screenshot::Pixels::Rgba16(vec![
                0, 0, 0, 65535, 65535, 65535, 65535, 65535,
            ]),
        };
        let mut bytes = Vec::new();
        let measured = capture.write_png_measured(&mut bytes).unwrap().unwrap();
        assert_eq!(
            measured,
            ContentLight {
                max_cll: 10000.0,
                max_fall: 5000.0
            }
        );
        let chunks = chunks(&bytes);
        let clli = &chunks.iter().find(|c| &c.0 == b"cLLI").unwrap().1;
        assert_eq!(
            clli,
            &[100_000_000u32.to_be_bytes(), 50_000_000u32.to_be_bytes()].concat()
        );
    }
    #[test]
    fn invalid_png_and_no_overwrite_leave_destination_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.png");
        std::fs::write(&path, b"keep").unwrap();
        let scale = HdrScale {
            unit_nits: 203.0,
            peak_nits: 1000.0,
        };
        let write = |w, h, light: &[[f32; 4]], scale, overwrite| {
            write_png(
                &path,
                w,
                h,
                light,
                || vec![1, 2, 3, 128],
                PngEncoding::Hdr10,
                scale,
                overwrite,
            )
        };
        assert!(write(0, 1, &[], scale, true).is_err());
        assert!(write(1, 1, &[], scale, true).is_err());
        assert!(write(1, 1, &[[f32::NAN, 0.0, 0.0, 1.0]], scale, true).is_err());
        assert!(
            write(
                1,
                1,
                &[[1.0; 4]],
                HdrScale {
                    peak_nits: f32::INFINITY,
                    ..scale
                },
                true
            )
            .is_err()
        );
        assert!(write(1, 1, &[[1.0; 4]], scale, false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn sdr_bytes_and_alpha_are_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sdr.png");
        let px = vec![3, 17, 231, 128];
        assert_eq!(
            write_png(
                &path,
                1,
                1,
                &[],
                || px.clone(),
                PngEncoding::Sdr8,
                HdrScale {
                    unit_nits: 203.0,
                    peak_nits: 203.0
                },
                false
            )
            .unwrap(),
            None
        );
        let bytes = std::fs::read(path).unwrap();
        assert!(chunks(&bytes).iter().any(|c| &c.0 == b"sRGB"));
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut decoded = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut decoded).unwrap();
        assert_eq!(decoded, px);
    }
    #[test]
    fn merged_levels_and_exact_warp_video_description() {
        let merged = levels().merge(HdrLevels {
            peak_nits: 1200.0,
            content: Some(ContentLight {
                max_cll: 700.0,
                max_fall: 400.0,
            }),
        });
        assert_eq!(
            merged,
            HdrLevels {
                peak_nits: 1200.0,
                content: Some(ContentLight {
                    max_cll: 812.2,
                    max_fall: 400.0
                })
            }
        );
        let input = Path::new("frame.%06d.pq.png");
        let args = ffmpeg_args(
            &options(input, PngEncoding::Hdr10),
            Some(merged),
            Path::new("out.mp4"),
        )
        .unwrap();
        let value = |name: &str| {
            args[args.iter().position(|v| v == name).unwrap() + 1]
                .to_str()
                .unwrap()
        };
        assert_eq!(value("-framerate"), "30000/1001");
        assert_eq!(value("-frames:v"), "2");
        assert_eq!(value("-start_number"), "7");
        assert_eq!(value("-pix_fmt"), "yuv420p10le");
        assert!(value("-x265-params").contains("master-display=G(8500,39850)B(6550,2300)R(35400,14600)WP(15635,16450)L(12000000,1):max-cll=813,400"));
        assert!(value("-vf").starts_with("sidedata=mode=delete:type=CONTENT_LIGHT_LEVEL,scale="));
        assert!(
            ffmpeg_args(
                &options(input, PngEncoding::Hdr10),
                None,
                Path::new("out.mp4")
            )
            .is_err()
        );
        let mut sdr = options(input, PngEncoding::Sdr8);
        sdr.codec = PngVideo::ProRes;
        sdr.frame_count = 1;
        let args = ffmpeg_args(&sdr, None, Path::new("out.mov")).unwrap();
        assert!(args.iter().any(|v| v == "none"));
        assert!(!args.iter().any(|v| v == "-start_number"));
        assert!(args.iter().any(|v| v == "4444xq"));
        assert!(args.iter().any(|v| {
            v.to_str()
                .is_some_and(|s| s.contains("lutrgb=r='pow(if(lte(val/maxval,0.04045)"))
        }));
    }
    #[test]
    fn video_arguments_preserve_native_non_unicode_paths() {
        #[cfg(windows)]
        let name = {
            use std::os::windows::ffi::OsStringExt;
            OsString::from_wide(&[0xD800, 0x2E, 0x70, 0x6E, 0x67])
        };
        #[cfg(unix)]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            OsString::from_vec(vec![0xFF, b'.', b'p', b'n', b'g'])
        };
        #[cfg(not(any(windows, unix)))]
        let name = OsString::from("frame.png");
        let input = PathBuf::from(&name);
        let output = PathBuf::from(&name).with_extension("mp4");
        let options = PngVideoOptions {
            frame_count: 1,
            ..options(&input, PngEncoding::Hlg)
        };
        let args = ffmpeg_args(&options, None, &output).unwrap();
        let input_at = args.iter().position(|v| v == "-i").unwrap() + 1;
        assert_eq!(args[input_at], input.as_os_str());
        assert_eq!(args.last().unwrap(), output.as_os_str());
    }
    #[test]
    fn failed_spawn_and_pre_cancel_never_publish() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("input.png");
        let out = dir.path().join("output.mp4");
        std::fs::write(&out, b"keep").unwrap();
        let cancel = AtomicBool::new(true);
        assert!(
            !encode_video(
                &options(&input, PngEncoding::Hlg),
                None,
                &out,
                true,
                &cancel,
                |_| panic!("cancelled")
            )
            .unwrap()
        );
        cancel.store(false, Ordering::Release);
        assert!(
            encode_video_with(
                &dir.path().join("missing-ffmpeg"),
                &options(&input, PngEncoding::Hlg),
                None,
                (&out, true),
                &cancel,
                |_| {}
            )
            .is_err()
        );
        assert_eq!(std::fs::read(out).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn in_place_output_is_refused_through_any_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("frame.png");
        std::fs::write(&input, b"src").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let spelled = dir.path().join("sub").join("..").join("frame.png");
        assert!(ensure_distinct(&input, &spelled).is_err());
        ensure_distinct(&input, &dir.path().join("frame.mov")).unwrap();
        ensure_distinct(&dir.path().join("absent.png"), &input).unwrap();
    }
    #[test]
    fn temp_is_a_sibling_with_the_destination_extension_and_publish_respects_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let dst = dir.path().join("out").join("clip.mov");
        let tmp = temp_for(&dst, false).unwrap().into_temp_path();
        assert_eq!(tmp.parent(), dst.parent());
        assert_eq!(tmp.extension(), dst.extension());
        std::fs::write(&tmp, b"new").unwrap();
        // A destination that appeared after the up-front check is not clobbered.
        std::fs::write(&dst, b"old").unwrap();
        assert!(publish(tmp, &dst, false).is_err());
        assert_eq!(std::fs::read(&dst).unwrap(), b"old");
        assert!(temp_for(&dst, false).is_err());
        let tmp = temp_for(&dst, true).unwrap().into_temp_path();
        std::fs::write(&tmp, b"new").unwrap();
        publish(tmp, &dst, true).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(dst.parent().unwrap()).unwrap().count(), 1);
    }
    #[test]
    #[ignore = "requires external ffmpeg and ffprobe with libx265/prores_ks"]
    fn ffmpeg_real_sequence_encodes_all_transfers_and_cancels_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let light = vec![[1.0, 0.3, 2.0, 1.0]; 256];
        let cancel = AtomicBool::new(false);
        for encoding in PngEncoding::ALL {
            let mut measured = None;
            for n in 7..=8 {
                let frame = dir.path().join(format!("frame.{n:06}.png"));
                let scale = hdr_scale(DisplayLight::Relative, &light, 203.0, encoding);
                let levels = write_png(
                    &frame,
                    16,
                    16,
                    &light,
                    || vec![128; 256 * 4],
                    encoding,
                    scale,
                    true,
                )
                .unwrap();
                measured = match (measured, levels) {
                    (Some(a), Some(b)) => Some(HdrLevels::merge(a, b)),
                    (a, b) => a.or(b),
                };
            }
            let input = dir.path().join("frame.%06d.png");
            for codec in [PngVideo::Hevc, PngVideo::ProRes] {
                let mut options = options(&input, encoding);
                options.codec = codec;
                let output = dir.path().join(format!(
                    "{}.{}",
                    encoding.suffix(),
                    codec.extension().unwrap()
                ));
                let mut seen = 0;
                assert!(
                    encode_video(&options, measured, &output, false, &cancel, |n| seen =
                        seen.max(n))
                    .unwrap()
                );
                assert_eq!(seen, 2);
                assert!(std::fs::metadata(&output).unwrap().len() > 0);
                let probe = ffmpeg().unwrap().with_file_name(if cfg!(windows) {
                    "ffprobe.exe"
                } else {
                    "ffprobe"
                });
                let result = std::process::Command::new(probe)
                    .args([
                        "-v",
                        "error",
                        "-select_streams",
                        "v:0",
                        "-show_entries",
                        "stream=pix_fmt,color_transfer,color_primaries,r_frame_rate",
                        "-of",
                        "json",
                    ])
                    .arg(&output)
                    .output()
                    .unwrap();
                assert!(result.status.success());
                let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
                let stream = &value["streams"][0];
                assert_eq!(
                    stream["pix_fmt"],
                    if codec == PngVideo::Hevc {
                        "yuv420p10le"
                    } else {
                        "yuv444p12le"
                    }
                );
                assert_eq!(stream["r_frame_rate"], "30000/1001");
                assert_eq!(stream["color_transfer"], video_tags(encoding)[1]);
                cancel.store(false, Ordering::Release);
                let keep = std::fs::read(&output).unwrap();
                assert!(
                    !encode_video(&options, measured, &output, true, &cancel, |_| cancel
                        .store(true, Ordering::Release))
                    .unwrap()
                );
                assert_eq!(std::fs::read(&output).unwrap(), keep);
                cancel.store(false, Ordering::Release);
            }
        }
    }
}
