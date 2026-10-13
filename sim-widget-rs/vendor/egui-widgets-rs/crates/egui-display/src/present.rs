//! The last step before the monitor: the canvas (everything egui and the image
//! tiles drew, in the extended-sRGB encoding of `transfer.rs`) encoded for the
//! window surface the user picked.
//!
//! [`Output`] is the user's choice (persisted, the Settings tab): SDR 8-bit, SDR
//! 10-bit, HDR10 (BT.2100 PQ), HLG (BT.2100 HLG) or scRGB (linear, extended
//! range). Each one is a (surface format, surface colour space) pair the surface
//! must report in `SurfaceCapabilities::format_capabilities`; [`Output::surface`]
//! picks it. [`PresentPass`] owns the canvas texture and the one fullscreen pass
//! that turns it into the surface's signal; [`encode`] is that pass's CPU twin.
//!
//! In light: canvas 1.0 is SDR reference white. On an SDR surface the canvas is
//! clipped to 0..1 and written as is (it already is sRGB-encoded); on an HDR
//! surface it is decoded to linear light and scaled by the SDR white level in
//! nits ([`Target::white`]), which the OS reports (`DisplayHdrInfo`) or the user
//! sets: scRGB 1.0 is 80 nits (IEC 61966-2-2), PQ is absolute nits in BT.2020.
//! HLG is relative to the display's nominal peak ([`Target::peak`]): the display
//! applies the BT.2100 OOTF for that peak, so the pass applies its inverse first.

use serde::{Deserialize, Serialize};

use crate::transfer::{self, fl};

/// Format of the canvas every UI pass draws into.
pub const CANVAS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Nits of scRGB 1.0 (IEC 61966-2-2).
pub const SCRGB_NITS: f32 = 80.0;
/// SDR reference white on a PQ or HLG signal when the OS does not say
/// (ITU-R BT.2408: HDR reference white, 75% HLG on a 1000-nit display).
pub const HDR_REF_WHITE: f32 = 203.0;
/// Nominal display peak of the BT.2100 HLG reference OOTF (system gamma 1.2).
pub const HLG_REF_PEAK: f32 = 1000.0;

/// Output luminance contract shared by image processing and presentation.
/// Built by [`crate::DisplayPrefs::target`] from the effective output and OS metadata.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    /// An HDR output: canvas values above 1.0 and below 0.0 are shown, not clipped.
    pub hdr: bool,
    /// SDR reference white, nits (HDR outputs).
    pub white: f32,
    /// The display's nominal peak, nits: the `L_W` of the HLG OOTF (HLG only).
    pub peak: f32,
}

impl Default for Target {
    /// SDR, white at scRGB's 80 nits, the HLG reference peak (only an HDR output
    /// reads them).
    fn default() -> Self {
        Self {
            hdr: false,
            white: SCRGB_NITS,
            peak: HLG_REF_PEAK,
        }
    }
}

/// What the window shows: one surface format + colour space pair.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Output {
    /// 8-bit sRGB: every display, every backend.
    #[default]
    Sdr8,
    /// 10-bit sRGB (`Rgb10a2Unorm`): less banding on a 10-bit panel.
    Sdr10,
    /// HDR10: BT.2100 PQ, BT.2020 primaries (`Rgb10a2Unorm`).
    Hdr10,
    /// BT.2100 HLG, BT.2020 primaries (`Rgb10a2Unorm`): Vulkan and Metal only.
    Hlg,
    /// scRGB: linear BT.709, extended range (`Rgba16Float`); the Windows compositor's own space.
    Scrgb,
}

impl Output {
    /// Every output, in menu order.
    pub const ALL: [Output; 5] = [Self::Sdr8, Self::Sdr10, Self::Hdr10, Self::Hlg, Self::Scrgb];

    /// UI name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Sdr8 => "SDR 8-bit",
            Self::Sdr10 => "SDR 10-bit",
            Self::Hdr10 => "HDR10 (PQ)",
            Self::Hlg => "HDR HLG",
            Self::Scrgb => "HDR scRGB",
        }
    }

    /// What it is for (the Settings tip).
    pub fn hint(self) -> &'static str {
        match self {
            Self::Sdr8 => "8-bit sRGB: works on every display.",
            Self::Sdr10 => {
                "10-bit sRGB: smoother gradients on a 10-bit panel; same picture otherwise."
            }
            Self::Hdr10 => "BT.2100 PQ in absolute nits (HDR10). Needs HDR on in the OS.",
            Self::Hlg => {
                "BT.2100 HLG, relative to the display's peak (set it under Display peak). Vulkan and Metal only: DXGI has no HLG swapchain."
            }
            Self::Scrgb => {
                "Linear extended-range BT.709, 1.0 = 80 nits. The Windows compositor's own space; needs HDR on in the OS."
            }
        }
    }

    /// Values above SDR white reach the display.
    pub fn is_hdr(self) -> bool {
        matches!(self, Self::Hdr10 | Self::Hlg | Self::Scrgb)
    }

    /// The surface pairs that give this output, preferred first. 8-bit prefers a
    /// non-sRGB format (the canvas already is sRGB-encoded); an `*Srgb` one works
    /// too, the pass then decodes and the hardware re-encodes. PQ and HLG also
    /// take `Rgba16Float` (Metal offers them on both), same signal in a float.
    fn candidates(self) -> &'static [(wgpu::TextureFormat, wgpu::SurfaceColorSpace)] {
        use wgpu::SurfaceColorSpace as C;
        use wgpu::TextureFormat as F;
        match self {
            Self::Sdr8 => &[
                (F::Bgra8Unorm, C::Srgb),
                (F::Rgba8Unorm, C::Srgb),
                (F::Bgra8UnormSrgb, C::Srgb),
                (F::Rgba8UnormSrgb, C::Srgb),
            ],
            Self::Sdr10 => &[(F::Rgb10a2Unorm, C::Srgb)],
            Self::Hdr10 => &[
                (F::Rgb10a2Unorm, C::Bt2100Pq),
                (F::Rgba16Float, C::Bt2100Pq),
            ],
            Self::Hlg => &[
                (F::Rgb10a2Unorm, C::Bt2100Hlg),
                (F::Rgba16Float, C::Bt2100Hlg),
            ],
            Self::Scrgb => &[(F::Rgba16Float, C::ExtendedSrgbLinear)],
        }
    }

    /// The surface format and colour space for this output, if the surface can be
    /// configured with them.
    pub fn surface(
        self,
        caps: &wgpu::SurfaceCapabilities,
    ) -> Option<(wgpu::TextureFormat, wgpu::SurfaceColorSpace)> {
        self.candidates().iter().copied().find(|&(format, space)| {
            space
                .to_color_spaces()
                .is_some_and(|flag| caps.color_spaces(format).contains(flag))
        })
    }

    /// How the present pass encodes for this output on `format`.
    pub fn encoding(self, format: wgpu::TextureFormat) -> Encoding {
        match self {
            Self::Sdr8 | Self::Sdr10 if format.is_srgb() => Encoding::SrgbFormat,
            Self::Sdr8 | Self::Sdr10 => Encoding::Unorm,
            Self::Hdr10 => Encoding::Pq,
            Self::Hlg => Encoding::Hlg,
            Self::Scrgb => Encoding::Scrgb,
        }
    }

    /// SDR white in nits when neither the OS nor the user says: scRGB's own 80
    /// (1.0 stays 1.0), BT.2408's 203 for PQ and HLG. Unused by SDR outputs.
    pub fn default_white(self) -> f32 {
        match self {
            Self::Hdr10 | Self::Hlg => HDR_REF_WHITE,
            _ => SCRGB_NITS,
        }
    }
}

/// Dither amplitude: one code of the surface's integer format, none for a float
/// one (the same output may land on either, e.g. HLG on Metal).
fn dither_step(format: wgpu::TextureFormat) -> f32 {
    use wgpu::TextureFormat as F;
    match format {
        F::Rgb10a2Unorm => 1.0 / 1023.0,
        F::Rgba16Float => 0.0,
        _ => 1.0 / 255.0,
    }
}

/// What the present pass writes, as its `mode` uniform encodes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Encoding {
    /// The canvas clipped to 0..1 (it is sRGB-encoded already).
    Unorm = 0,
    /// The same, decoded: an `*Srgb` format encodes again on write.
    SrgbFormat = 1,
    /// BT.2100 PQ of BT.2020 absolute nits.
    Pq = 2,
    /// Linear BT.709 over 80 nits (extended range, negatives kept).
    Scrgb = 3,
    /// BT.2100 HLG of BT.2020 nits, through the inverse OOTF of the display peak.
    Hlg = 4,
}

// ── PQ (SMPTE ST 2084) ─────────────────────────────────────────────────────────

const PQ_M1: f32 = 2610.0 / 16384.0;
const PQ_M2: f32 = 2523.0 / 4096.0 * 128.0;
const PQ_C1: f32 = 3424.0 / 4096.0;
const PQ_C2: f32 = 2413.0 / 4096.0 * 32.0;
const PQ_C3: f32 = 2392.0 / 4096.0 * 32.0;
/// PQ's full scale in nits.
pub const PQ_PEAK: f32 = 10_000.0;

/// ST 2084 inverse EOTF: absolute nits -> PQ signal (negatives clip to 0).
/// The CPU twin the GPU test holds the shader's `pq` to; the HDR PNG export
/// encodes with it too (one implementation).
pub fn pq(nits: f32) -> f32 {
    let y = (nits / PQ_PEAK).clamp(0.0, 1.0).powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * y) / (1.0 + PQ_C3 * y)).powf(PQ_M2)
}

/// ST 2084 EOTF, the inverse of [`pq`]: a PQ signal (clipped to 0..1) -> absolute nits.
/// The HDR10 screenshot measures its `cLLI` light levels with it.
pub fn pq_nits(signal: f32) -> f32 {
    let e = signal.clamp(0.0, 1.0).powf(1.0 / PQ_M2);
    PQ_PEAK * ((e - PQ_C1).max(0.0) / (PQ_C2 - PQ_C3 * e)).powf(1.0 / PQ_M1)
}

// ── HLG (ITU-R BT.2100) ────────────────────────────────────────────────────────
// OCIO's form (`transforms/builtins/Displays.cpp`, namespace HLG, and its
// `DISPLAY - CIE-XYZ-D65_to_REC.2100-HLG-1000nit`): scene light E on 0..E_MAX,
// the OETF's gamma segment below E_MAX/12, a log segment above.

const HLG_A: f32 = 0.178_832_77;
const HLG_E_MAX: f32 = 3.0;
const HLG_E_SCALE: f32 = 3.0 / HLG_E_MAX;
const HLG_BREAK: f32 = HLG_E_MAX / 12.0;
/// Floor of the OOTF's luminance (`Renderer_REC2100_Surround`: the inverse
/// OOTF's gain `Y^(1/gamma - 1)` grows without bound as Y -> 0).
const HLG_MIN_LUM: f32 = 1e-4;
/// BT.2100 luminance weights of BT.2020 RGB.
const HLG_LUMA: [f32; 3] = [0.2627, 0.6780, 0.0593];

fn hlg_b() -> f32 {
    (1.0 - 4.0 * HLG_A) * HLG_E_MAX / 12.0
}

fn hlg_c() -> f32 {
    (12.0 / HLG_E_MAX).ln() * HLG_A + 0.5 - HLG_A * (4.0 * HLG_A).ln()
}

/// BT.2100 system gamma of the HLG OOTF for a display of `peak` nits (1.2 at 1000).
pub fn hlg_gamma(peak: f32) -> f32 {
    1.2 + 0.42 * (peak / 1000.0).log10()
}

/// HLG OETF, mirrored about 0: scene light (0..E_MAX) -> signal.
fn hlg_oetf(e: f32) -> f32 {
    let m = e.abs();
    let o = if m < HLG_BREAK {
        (m * HLG_E_SCALE).sqrt()
    } else {
        HLG_A * (m - hlg_b()).ln() + hlg_c()
    };
    o.copysign(e)
}

/// BT.2020 display light in nits -> HLG signal for a display of `peak` nits: the
/// inverse OOTF (scale to `E_MAX^gamma` at the peak, then `x * Y^(1/gamma - 1)`),
/// then the OETF, clipped to the signal's 0..1. Test-only twin of the shader's `hlg3`.
pub fn hlg(nits: [f32; 3], peak: f32) -> [f32; 3] {
    let g = hlg_gamma(peak);
    let s = nits.map(|v| v * HLG_E_MAX.powf(g) / peak);
    let y = (HLG_LUMA[0] * s[0] + HLG_LUMA[1] * s[1] + HLG_LUMA[2] * s[2])
        .abs()
        .max(HLG_MIN_LUM);
    let k = y.powf(1.0 / g - 1.0);
    s.map(|v| hlg_oetf(v * k).clamp(0.0, 1.0))
}

/// Linear Rec.709 display light (1.0 = SDR white) -> BT.2020, in nits with SDR
/// white at `white`: what the present shader feeds PQ / HLG. The HDR export
/// shares it.
pub fn rec2020_nits(lin: [f32; 3], white: f32) -> [f32; 3] {
    let m = transfer::SRGB_TO_REC2020;
    std::array::from_fn(|r| (m[0][r] * lin[0] + m[1][r] * lin[1] + m[2][r] * lin[2]) * white)
}

/// Canvas -> linear light -> BT.2020, in nits with SDR white at `white`.
fn nits2020(canvas: [f32; 3], white: f32) -> [f32; 3] {
    rec2020_nits(canvas.map(transfer::eotf), white)
}

/// The CPU twin of the present shader (dither off): one canvas value in, the
/// surface's signal out, for the white and peak of `target` (HDR outputs only).
/// Test-only: nothing on the CPU presents.
pub fn encode(canvas: [f32; 3], enc: Encoding, target: Target) -> [f32; 3] {
    let canvas = canvas.map(|v| if v.is_nan() { 0.0 } else { v });
    match enc {
        Encoding::Unorm => canvas.map(|v| v.clamp(0.0, 1.0)),
        Encoding::SrgbFormat => canvas.map(|v| transfer::eotf(v.clamp(0.0, 1.0))),
        Encoding::Scrgb => canvas.map(|v| transfer::eotf(v) * target.white / SCRGB_NITS),
        Encoding::Pq => nits2020(canvas, target.white).map(pq),
        Encoding::Hlg => hlg(nits2020(canvas, target.white), target.peak),
    }
}

/// [`encode`] of a canvas pixel composited over `backdrop` first
/// ([`crate::BackdropUniform::composite`], in linear light at framebuffer pixel `px`):
/// the CPU twin of the present shader with a backdrop. `None` is [`encode`] of the RGB.
/// Test-only, like `encode`.
pub fn encode_over(
    canvas: [f32; 4],
    backdrop: Option<&crate::BackdropUniform>,
    px: [f32; 2],
    enc: Encoding,
    target: Target,
) -> [f32; 3] {
    let [r, g, b, a] = canvas;
    let rgb = [r, g, b].map(|v| if v.is_nan() { 0.0 } else { v });
    let rgb = match backdrop {
        Some(bd) => bd.composite([rgb[0], rgb[1], rgb[2], a], px),
        None => rgb,
    };
    encode(rgb, enc, target)
}

/// The present shader: the canvas pixel under the fragment, composited over the
/// backdrop where it is not opaque, encoded by `mode`.
fn shader_src() -> String {
    let m = transfer::SRGB_TO_REC2020;
    let col = |c: [f32; 3]| format!("vec3<f32>({}, {}, {})", fl(c[0]), fl(c[1]), fl(c[2]));
    format!(
        r#"
{backdrop}
struct Params {{
    mode:        u32,
    white:       f32,
    dither_step: f32,
    peak:        f32,
    bd:          BackdropUniform,
    bd_on:       u32,
}}

@group(0) @binding(0) var canvas: texture_2d<f32>;
@group(0) @binding(1) var<uniform> p: Params;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    // One triangle over the whole target.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}}

{eotf}
// egui-wgpu 0.36's dither noise (`egui.wgsl`), so banding breaks up as it did.
fn interleaved_gradient_noise(n: vec2<f32>) -> f32 {{
    let f = 0.06711056 * n.x + 0.00583715 * n.y;
    return fract(52.9829189 * fract(f));
}}

fn pq(nits: f32) -> f32 {{
    let y = pow(clamp(nits / {peak}, 0.0, 1.0), {m1});
    return pow(({c1} + {c2} * y) / (1.0 + {c3} * y), {m2});
}}

// NaN by its bits (all-ones exponent, non-zero mantissa): WGSL lets a compiler
// assume there are no NaNs, so `x != x` may be folded away (the NVIDIA Vulkan
// driver does); integer ops cannot be.
fn is_nan3(v: vec3<f32>) -> vec3<bool> {{
    return (bitcast<vec3<u32>>(v) & vec3<u32>(0x7fffffffu)) > vec3<u32>(0x7f800000u);
}}

fn eotf3(c: vec3<f32>) -> vec3<f32> {{
    return vec3<f32>(srgb_eotf(c.r), srgb_eotf(c.g), srgb_eotf(c.b));
}}

fn oetf3(c: vec3<f32>) -> vec3<f32> {{
    return vec3<f32>(srgb_oetf(c.r), srgb_oetf(c.g), srgb_oetf(c.b));
}}

// Canvas -> linear light -> BT.2020, in nits with SDR white at `p.white`.
fn nits2020(c: vec3<f32>) -> vec3<f32> {{
    return mat3x3<f32>({m0}, {m1c}, {m2c}) * eotf3(c) * p.white;
}}

// HLG OETF (OCIO's form), mirrored about 0.
fn hlg_oetf(e: f32) -> f32 {{
    let m = abs(e);
    var o: f32;
    if m < {hlg_break} {{
        o = sqrt(m * {hlg_scale});
    }} else {{
        o = {hlg_a} * log(m - {hlg_b}) + {hlg_c};
    }}
    return select(o, -o, e < 0.0);
}}

// BT.2020 nits -> HLG signal for a display of `peak` nits: the inverse BT.2100
// OOTF, then the OETF (twin: `present::hlg`).
fn hlg3(nits: vec3<f32>, peak: f32) -> vec3<f32> {{
    let g = 1.2 + 0.42 * log(peak / 1000.0) / log(10.0);
    let s = nits * (pow({hlg_emax}, g) / peak);
    let y = max(abs(dot(s, vec3<f32>({l0}, {l1}, {l2}))), {hlg_min});
    let e = s * pow(y, 1.0 / g - 1.0);
    return clamp(vec3<f32>(hlg_oetf(e.r), hlg_oetf(e.g), hlg_oetf(e.b)), vec3<f32>(0.0), vec3<f32>(1.0));
}}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {{
    // A NaN (a broken pixel, 0/0 in an OCIO op) is black, not whatever the
    // compositor makes of it.
    let texel = textureLoad(canvas, vec2<i32>(pos.xy), 0);
    let raw = texel.rgb;
    var c = select(raw, vec3<f32>(0.0), is_nan3(raw));
    // The viewer backdrop (`backdrop.rs`) under what is not opaque: the canvas holds
    // encoded ASSOCIATED light (see its canvas contract), so OVER in linear light is
    // the decoded canvas plus the backdrop times (1 - alpha); no division by alpha,
    // so emission (a = 0, rgb > 0) stays. The result is re-encoded for the output's
    // own encoding below. Twin: `BackdropUniform::composite`. A NaN alpha compares
    // false: opaque.
    if p.bd_on == 1u && texel.a < 1.0 && backdrop_contains(p.bd, pos.xy) {{
        let a = max(texel.a, 0.0);
        c = oetf3(eotf3(c) + eotf3(backdrop_color(p.bd, pos.xy)) * (1.0 - a));
    }}
    let dither = (interleaved_gradient_noise(pos.xy) - 0.5) * 0.95 * p.dither_step;
    switch p.mode {{
        case {unorm}u: {{
            return vec4<f32>(clamp(c + vec3<f32>(dither), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
        }}
        case {srgb}u: {{
            return vec4<f32>(eotf3(clamp(c + vec3<f32>(dither), vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
        }}
        case {pqm}u: {{
            let nits = nits2020(c);
            return vec4<f32>(pq(nits.r) + dither, pq(nits.g) + dither, pq(nits.b) + dither, 1.0);
        }}
        case {hlgm}u: {{
            return vec4<f32>(hlg3(nits2020(c), p.peak) + vec3<f32>(dither), 1.0);
        }}
        default: {{
            return vec4<f32>(eotf3(c) * (p.white / {scrgb}), 1.0);
        }}
    }}
}}
"#,
        eotf = transfer::eotf_wgsl() + &transfer::oetf_wgsl(),
        backdrop = crate::backdrop::BACKDROP_WGSL,
        peak = fl(PQ_PEAK),
        m1 = fl(PQ_M1),
        m2 = fl(PQ_M2),
        c1 = fl(PQ_C1),
        c2 = fl(PQ_C2),
        c3 = fl(PQ_C3),
        unorm = Encoding::Unorm as u32,
        srgb = Encoding::SrgbFormat as u32,
        pqm = Encoding::Pq as u32,
        hlgm = Encoding::Hlg as u32,
        m0 = col(m[0]),
        m1c = col(m[1]),
        m2c = col(m[2]),
        scrgb = fl(SCRGB_NITS),
        hlg_a = fl(HLG_A),
        hlg_b = fl(hlg_b()),
        hlg_c = fl(hlg_c()),
        hlg_emax = fl(HLG_E_MAX),
        hlg_scale = fl(HLG_E_SCALE),
        hlg_break = fl(HLG_BREAK),
        hlg_min = fl(HLG_MIN_LUM),
        l0 = fl(HLG_LUMA[0]),
        l1 = fl(HLG_LUMA[1]),
        l2 = fl(HLG_LUMA[2]),
    )
}

/// Uniform block of the present shader (16-byte aligned; WGSL rounds the struct up to
/// 96 bytes after `bd_on`, hence the tail).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Params {
    mode: u32,
    white: f32,
    dither_step: f32,
    peak: f32,
    bd: crate::BackdropUniform,
    bd_on: u32,
    tail: [u32; 3],
}

/// The canvas at one size.
pub(crate) struct Canvas {
    texture: wgpu::Texture,
    pub(crate) size: (u32, u32),
    pub(crate) view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

fn canvas_bytes(device: &wgpu::Device, size: (u32, u32)) -> Option<u64> {
    let max = device.limits().max_texture_dimension_2d;
    if size.0 == 0 || size.1 == 0 || size.0 > max || size.1 > max {
        return None;
    }
    u64::from(size.0)
        .checked_mul(u64::from(size.1))?
        .checked_mul(8)
}

/// A canvas texture of `size` and its bind group for the present pass.
fn make_canvas(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    params: &wgpu::Buffer,
    size: (u32, u32),
) -> Canvas {
    assert!(
        canvas_bytes(device, size).is_some(),
        "canvas dimensions exceed device limits"
    );
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("egui_display.canvas"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: CANVAS_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("egui_display.present"),
        layout: bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: params.as_entire_binding(),
            },
        ],
    });
    Canvas {
        texture,
        size,
        view,
        bind,
    }
}

/// The present pipeline for a target of `format`: one triangle over the target running
/// `fs_main`, no blending (it writes the surface's whole signal).
pub(crate) fn make_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("egui_display.present"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// The canvas and the pass that presents it, for one surface format and output.
pub struct PresentPass {
    output: Output,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    /// The present shader and its layout, kept so a capture builds its pipeline for
    /// another format from the same compiled module (`capture.rs`).
    pub(crate) module: wgpu::ShaderModule,
    pub(crate) layout: wgpu::PipelineLayout,
    pub(crate) bgl: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    pub(crate) canvas: Option<Canvas>,
    retired: crate::retire::RetiredTextures,
    /// Dither on integer surfaces (off only in the GPU test, to compare exactly).
    pub dither: bool,
    /// The viewer backdrop under the canvas's non-opaque pixels, set each frame from
    /// [`crate::take_backdrop`]; `None` presents the canvas as it is.
    pub backdrop: Option<crate::BackdropUniform>,
}

impl PresentPass {
    /// The pass for a surface of `format` configured for `output`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, output: Output) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("egui_display.present"),
            source: wgpu::ShaderSource::Wgsl(shader_src().into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("egui_display.present"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("egui_display.present"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = make_pipeline(device, &module, &layout, format);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("egui_display.present.params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            output,
            format,
            pipeline,
            module,
            layout,
            bgl,
            params,
            canvas: None,
            retired: Default::default(),
            dither: true,
            backdrop: None,
        }
    }

    /// Surface encodings share the same canvas format. Preserve its texture
    /// and retirement accounting when changing the present pipeline.
    /// Both passes must belong to `device`; the destination must be newly created.
    pub fn inherit_canvas(&mut self, device: &wgpu::Device, previous: &mut Self) {
        assert!(
            self.canvas.is_none() && self.retired.bytes() == 0,
            "inherit_canvas requires a new destination"
        );
        self.retired = std::mem::take(&mut previous.retired);
        if let Some(mut canvas) = previous.canvas.take() {
            canvas.bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("egui_display.present"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&canvas.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.params.as_entire_binding(),
                    },
                ],
            });
            self.canvas = Some(canvas);
        }
    }

    /// Peak canvas allocation reserved before image callbacks run.
    pub fn canvas_reserve(&self, device: &wgpu::Device, w: u32, h: u32) -> u64 {
        let bytes = |size: (u32, u32)| canvas_bytes(device, size).unwrap_or(u64::MAX);
        let next = bytes((w.max(1), h.max(1)));
        self.retired
            .bytes()
            .saturating_add(self.canvas.as_ref().map_or(next, |c| {
                if c.size == (w.max(1), h.max(1)) {
                    next
                } else {
                    bytes(c.size).saturating_add(next)
                }
            }))
    }

    /// Replace the canvas before image preparation, retaining submitted work.
    pub fn prepare_canvas(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32) {
        self.retired.reap();
        let size = (w.max(1), h.max(1));
        if self.canvas.as_ref().is_some_and(|c| c.size == size) {
            return;
        }
        let next = make_canvas(device, &self.bgl, &self.params, size);
        if let Some(old) = self.canvas.replace(next) {
            let bytes = canvas_bytes(device, old.size).expect("allocated canvas layout");
            self.retired.retire(queue, old.texture, bytes);
        }
    }

    /// The canvas to draw this frame into. Call `prepare_canvas` before every frame
    /// to retain old textures until submitted GPU work finishes.
    /// Initial allocation is supported for one-shot/headless callers; resizing
    /// an existing canvas requires preparation.
    pub fn canvas(&mut self, device: &wgpu::Device, w: u32, h: u32) -> &wgpu::TextureView {
        let size = (w.max(1), h.max(1));
        if self.canvas.as_ref().is_some_and(|c| c.size != size) {
            panic!("call prepare_canvas before resizing a display canvas");
        }
        &self
            .canvas
            .get_or_insert_with(|| make_canvas(device, &self.bgl, &self.params, size))
            .view
    }

    /// The shader's uniform for encoding into a target of `format` for `output`: the one
    /// place the swapchain draw and a capture take their parameters from.
    pub(crate) fn params_for(
        &self,
        output: Output,
        format: wgpu::TextureFormat,
        target: Target,
    ) -> Params {
        Params {
            mode: output.encoding(format) as u32,
            white: target.white,
            dither_step: if self.dither {
                dither_step(format)
            } else {
                0.0
            },
            peak: target.peak,
            bd: self.backdrop.unwrap_or_else(bytemuck::Zeroable::zeroed),
            bd_on: u32::from(self.backdrop.is_some()),
            tail: [0; 3],
        }
    }

    /// Record the pass: the canvas encoded into `frame` (the surface frame) for the
    /// white and peak of `target`. Does nothing before [`Self::canvas`] made one.
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &wgpu::TextureView,
        target: Target,
    ) {
        let Some(canvas) = &self.canvas else { return };
        let params = self.params_for(self.output, self.format, target);
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui_display.present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &canvas.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(
        pairs: &[(wgpu::TextureFormat, wgpu::SurfaceColorSpaces)],
    ) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats: pairs.iter().map(|p| p.0).collect(),
            format_capabilities: pairs
                .iter()
                .map(|&(format, color_spaces)| wgpu::SurfaceFormatCapabilities {
                    format,
                    color_spaces,
                })
                .collect(),
            ..Default::default()
        }
    }

    /// Each output takes the pair the surface reports, and only that: DX12's list
    /// gives all but HLG (DXGI has none); Vulkan's HLG space on 10-bit gives it;
    /// Metal's PQ/HLG on `Rgba16Float` only is the float fallback; a surface without
    /// the HDR spaces gives only SDR.
    #[test]
    fn outputs_follow_the_surface() {
        use wgpu::SurfaceColorSpace as C;
        use wgpu::SurfaceColorSpaces as S;
        use wgpu::TextureFormat as F;
        let dx12 = caps(&[
            (F::Bgra8UnormSrgb, S::SRGB),
            (F::Bgra8Unorm, S::SRGB),
            (F::Rgb10a2Unorm, S::SRGB | S::BT2100_PQ),
            (F::Rgba16Float, S::EXTENDED_SRGB_LINEAR),
        ]);
        assert_eq!(Output::Sdr8.surface(&dx12), Some((F::Bgra8Unorm, C::Srgb)));
        assert_eq!(
            Output::Sdr10.surface(&dx12),
            Some((F::Rgb10a2Unorm, C::Srgb))
        );
        assert_eq!(
            Output::Hdr10.surface(&dx12),
            Some((F::Rgb10a2Unorm, C::Bt2100Pq))
        );
        assert_eq!(
            Output::Scrgb.surface(&dx12),
            Some((F::Rgba16Float, C::ExtendedSrgbLinear))
        );
        assert_eq!(Output::Hlg.surface(&dx12), None);
        let vulkan = caps(&[
            (F::Bgra8Unorm, S::SRGB),
            (F::Rgb10a2Unorm, S::SRGB | S::BT2100_PQ | S::BT2100_HLG),
        ]);
        assert_eq!(
            Output::Hlg.surface(&vulkan),
            Some((F::Rgb10a2Unorm, C::Bt2100Hlg))
        );
        let float_only = caps(&[
            (F::Bgra8Unorm, S::SRGB),
            (F::Rgba16Float, S::BT2100_PQ | S::BT2100_HLG),
        ]);
        assert_eq!(
            Output::Hlg.surface(&float_only),
            Some((F::Rgba16Float, C::Bt2100Hlg))
        );
        assert_eq!(
            Output::Hdr10.surface(&float_only),
            Some((F::Rgba16Float, C::Bt2100Pq))
        );
        let sdr = caps(&[(F::Bgra8UnormSrgb, S::SRGB), (F::Rgb10a2Unorm, S::SRGB)]);
        assert_eq!(
            Output::Sdr8.surface(&sdr),
            Some((F::Bgra8UnormSrgb, C::Srgb))
        );
        assert_eq!(
            Output::Sdr8.encoding(F::Bgra8UnormSrgb),
            Encoding::SrgbFormat
        );
        assert!(
            [Output::Hdr10, Output::Hlg, Output::Scrgb]
                .iter()
                .all(|o| o.surface(&sdr).is_none())
        );
    }

    fn at(white: f32, peak: f32) -> Target {
        Target {
            hdr: true,
            white,
            peak,
        }
    }

    /// The encodings by known values: SDR clips; scRGB puts SDR white at
    /// `white / 80`; PQ puts 100 nits at 0.508 (ST 2084) and 10000 at 1.0.
    #[test]
    fn encodings_by_known_values() {
        let mid = transfer::oetf(0.18);
        assert_eq!(
            encode([mid, 1.7, -0.2], Encoding::Unorm, at(0.0, 1000.0)),
            [mid, 1.0, 0.0]
        );
        let s = encode([mid, 1.0, 0.0], Encoding::SrgbFormat, at(0.0, 1000.0));
        assert!((s[0] - 0.18).abs() < 1e-6 && (s[1] - 1.0).abs() < 1e-6);
        let sc = encode(
            [1.0, transfer::oetf(4.0), transfer::oetf(-0.1)],
            Encoding::Scrgb,
            at(240.0, 1000.0),
        );
        assert!(
            (sc[0] - 3.0).abs() < 1e-5 && (sc[1] - 12.0).abs() < 1e-4 && (sc[2] + 0.3).abs() < 1e-5,
            "{sc:?}"
        );
        assert!((pq(100.0) - 0.508_078).abs() < 1e-4, "{}", pq(100.0));
        assert!((pq(10_000.0) - 1.0).abs() < 1e-6 && pq(0.0) < 1e-6);
        let w = encode([1.0, 1.0, 1.0], Encoding::Pq, at(100.0, 1000.0));
        assert!(
            w.iter().all(|v| (v - 0.508_078).abs() < 5e-4),
            "SDR white at 100 nits: {w:?}"
        );
    }

    /// The backdrop oracle on every output by hand: white at alpha 0.5 (associated light
    /// 0.5, stored encoded) over black is half of SDR white in linear light, so sRGB code oetf(0.5), scRGB
    /// `0.5 * white / 80`, PQ of `0.5 * white` nits, HLG of the same nits; over 50% Gray
    /// it adds half of that grey's linear light.
    #[test]
    fn backdrop_oracle_on_every_output() {
        let levels = at(240.0, 600.0);
        let black = crate::Backdrop {
            kind: crate::BackdropKind::Black,
            ..Default::default()
        }
        .uniform(
            &crate::BackdropView::new(egui::Pos2::ZERO, 1.0),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(8.0, 8.0)),
        );
        let gray = crate::Backdrop {
            kind: crate::BackdropKind::Gray50,
            ..Default::default()
        }
        .uniform(
            &crate::BackdropView::new(egui::Pos2::ZERO, 1.0),
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(8.0, 8.0)),
        );
        let half = transfer::oetf(0.5);
        let half_white = [half, half, half, 0.5];
        let close = |got: [f32; 3], want: f32, what: &str| {
            assert!(
                got.iter().all(|g| (g - want).abs() < 1e-4),
                "{what}: {got:?} vs {want}"
            );
        };
        for (bd, lin) in [(black, 0.5), (gray, 0.5 + 0.5 * transfer::eotf(0.5))] {
            let o = |enc| encode_over(half_white, Some(&bd), [0.5, 0.5], enc, levels);
            close(o(Encoding::Unorm), transfer::oetf(lin), "SDR code");
            close(o(Encoding::SrgbFormat), lin, "sRGB format");
            close(o(Encoding::Scrgb), lin * 240.0 / SCRGB_NITS, "scRGB");
            close(o(Encoding::Pq), pq(lin * 240.0), "PQ");
            close(o(Encoding::Hlg), hlg([lin * 240.0; 3], 600.0)[0], "HLG");
        }
    }

    /// The contract's three pixel kinds on every output, in closed form for a neutral
    /// grey (the Rec.2020 matrix rows sum to 1, so nits = linear x white): an emissive
    /// pixel (a = 0, light 3 = three SDR whites), a tiny alpha, and a partial-alpha
    /// associated pixel, over Black and 50% Gray. The linear result is
    /// `light + backdrop * (1 - a)`; each output encodes it.
    #[test]
    fn backdrop_contract_on_every_output() {
        let levels = at(240.0, 600.0);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(8.0, 8.0));
        let view = crate::BackdropView::new(egui::Pos2::ZERO, 1.0);
        for (kind, bd_lin) in [
            (crate::BackdropKind::Black, 0.0),
            (crate::BackdropKind::Gray50, transfer::eotf(0.5)),
        ] {
            let bd = crate::Backdrop {
                kind,
                ..Default::default()
            }
            .uniform(&view, rect);
            for (light, a) in [(3.0f32, 0.0f32), (3.0, 1e-6), (0.4, 0.5), (0.0, 0.0)] {
                let canvas = [transfer::oetf(light); 3];
                let lin = light + bd_lin * (1.0 - a);
                let want = |enc: Encoding| match enc {
                    Encoding::Unorm => transfer::oetf(lin).clamp(0.0, 1.0),
                    Encoding::SrgbFormat => lin.clamp(0.0, 1.0),
                    Encoding::Scrgb => lin * 240.0 / SCRGB_NITS,
                    Encoding::Pq => pq(lin * 240.0),
                    Encoding::Hlg => hlg([lin * 240.0; 3], 600.0)[0],
                };
                for enc in [
                    Encoding::Unorm,
                    Encoding::SrgbFormat,
                    Encoding::Scrgb,
                    Encoding::Pq,
                    Encoding::Hlg,
                ] {
                    let got = encode_over(
                        [canvas[0], canvas[1], canvas[2], a],
                        Some(&bd),
                        [0.5, 0.5],
                        enc,
                        levels,
                    );
                    let w = want(enc);
                    assert!(
                        got.iter().all(|g| (g - w).abs() < 2e-4 * (1.0 + w.abs())),
                        "{kind:?} light {light} a {a} {enc:?}: {got:?} vs {w}"
                    );
                }
            }
        }
    }

    /// HLG by the numbers BT.2100 / BT.2408 fix: system gamma 1.2 at 1000 nits;
    /// the display's peak is signal 1.0 whatever the peak; HDR reference white
    /// (203 nits) on a 1000-nit display is 75% HLG; black is 0; a white SDR white
    /// set to 203 lands there too through the whole chain.
    #[test]
    fn hlg_by_known_values() {
        assert!((hlg_gamma(1000.0) - 1.2).abs() < 1e-6);
        for peak in [400.0, 1000.0, 2000.0] {
            let p = hlg([peak; 3], peak);
            assert!(
                p.iter().all(|v| (v - 1.0).abs() < 1e-4),
                "peak {peak}: {p:?}"
            );
            assert_eq!(hlg([0.0; 3], peak), [0.0; 3]);
        }
        let w = hlg([203.0; 3], 1000.0);
        assert!(w.iter().all(|v| (v - 0.75).abs() < 1e-3), "203 nits: {w:?}");
        let chain = encode([1.0; 3], Encoding::Hlg, at(203.0, 1000.0));
        assert!(
            chain.iter().all(|v| (v - 0.75).abs() < 1e-3),
            "SDR white 203: {chain:?}"
        );
        assert!(
            hlg([2.0 * 1000.0, 0.0, -50.0], 1000.0) == [1.0, 0.0, 0.0],
            "clipped to the signal range"
        );
    }

    /// HLG at the 1000-nit reference peak equals OCIO's own
    /// `DISPLAY - CIE-XYZ-D65_to_REC.2100-HLG-1000nit` (vfx-ocio, which holds it to
    /// PyOpenColorIO 2.5.1): the canvas's linear BT.709 light, as XYZ in OCIO's
    /// display reference (1.0 = 100 nits), through that builtin.
    #[test]
    fn hlg_matches_ocio_at_1000_nits() {
        use vfx_ocio::color_matrix::{Adaptation, REC709, conversion_matrix_to_xyz_d65};
        let t = vfx_ocio::Transform::Builtin(vfx_ocio::BuiltinTransform {
            style: "DISPLAY - CIE-XYZ-D65_to_REC.2100-HLG-1000nit".to_owned(),
            direction: vfx_ocio::TransformDirection::Forward,
        });
        let proc =
            vfx_ocio::Processor::from_transform(&t, vfx_ocio::TransformDirection::Forward).unwrap();
        let to_xyz = conversion_matrix_to_xyz_d65(&REC709, Adaptation::None).unwrap();
        let white = 203.0;
        for lin in [
            [0.18f32; 3],
            [1.0; 3],
            [0.02, 0.05, 0.01],
            [0.9, 0.3, 0.1],
            [0.05, 0.4, 2.5],
            [4.0, 3.5, 3.0],
        ] {
            let canvas = lin.map(transfer::oetf);
            let got = encode(canvas, Encoding::Hlg, at(white, 1000.0));
            let xyz: [f32; 3] = std::array::from_fn(|r| {
                (0..3)
                    .map(|c| to_xyz[r * 4 + c] as f32 * lin[c])
                    .sum::<f32>()
                    * white
                    / 100.0
            });
            let mut want = [xyz];
            proc.apply_rgb(&mut want);
            let want = want[0].map(|v| v.clamp(0.0, 1.0));
            for c in 0..3 {
                assert!(
                    (got[c] - want[c]).abs() < 1e-3,
                    "{lin:?} ch{c}: {} vs OCIO {}",
                    got[c],
                    want[c]
                );
            }
        }
    }

    #[test]
    fn shader_validates() {
        let module = naga::front::wgsl::parse_str(&shader_src()).expect("present WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .expect("present WGSL validates");
    }
}
