// ── sRGB OETF (IEC 61966-2-1) ─────────────────────────────────────────────────

/// Linear value below which the transfer function is the linear segment.
pub const SRGB_CUT: f32 = 0.003_130_8;
/// Slope of the linear segment.
pub const SRGB_SLOPE: f32 = 12.92;
/// Scale of the power segment.
pub const SRGB_SCALE: f32 = 1.055;
/// Offset of the power segment.
pub const SRGB_OFFSET: f32 = 0.055;
/// Exponent of the power segment (`1/2.4`).
pub const SRGB_EXP: f32 = 1.0 / 2.4;

/// The sRGB opto-electronic transfer function (linear -> encoded), extended:
/// continued above 1.0 and mirrored below 0.0 (`oetf(-v) == -oetf(v)`), the
/// canvas encoding. An SDR surface clips the result to 0..1 in the present pass.
pub fn oetf(v: f32) -> f32 {
    let a = v.abs();
    let e = if a <= SRGB_CUT {
        SRGB_SLOPE * a
    } else {
        SRGB_SCALE * a.powf(SRGB_EXP) - SRGB_OFFSET
    };
    e.copysign(v)
}

/// The inverse of [`oetf`] (encoded -> linear), extended the same way: the CPU
/// twin of [`eotf_wgsl`], which the present pass runs; the HDR export uses it to
/// turn the canvas back into display light.
pub fn eotf(e: f32) -> f32 {
    let a = e.abs();
    let v = if a <= SRGB_CUT * SRGB_SLOPE {
        a / SRGB_SLOPE
    } else {
        ((a + SRGB_OFFSET) / SRGB_SCALE).powf(1.0 / SRGB_EXP)
    };
    v.copysign(e)
}

/// The WGSL twin of [`oetf`], generated from the same constants. Declares
/// `fn srgb_oetf(v: f32) -> f32`; spliced into the tile shader.
pub fn oetf_wgsl() -> String {
    format!(
        "\
fn srgb_oetf(v: f32) -> f32 {{
    let a = abs(v);
    var e: f32;
    if a <= {cut} {{
        e = {slope} * a;
    }} else {{
        e = {scale} * pow(a, {exp}) - {offset};
    }}
    return select(e, -e, v < 0.0);
}}
",
        cut = fl(SRGB_CUT),
        slope = fl(SRGB_SLOPE),
        scale = fl(SRGB_SCALE),
        exp = fl(SRGB_EXP),
        offset = fl(SRGB_OFFSET),
    )
}

/// The WGSL twin of [`eotf`], generated from the same constants. Declares
/// `fn srgb_eotf(e: f32) -> f32`; spliced into the present shader.
pub fn eotf_wgsl() -> String {
    format!(
        "\
fn srgb_eotf(e: f32) -> f32 {{
    let a = abs(e);
    var v: f32;
    if a <= {cut} {{
        v = a / {slope};
    }} else {{
        v = pow((a + {offset}) / {scale}, {inv_exp});
    }}
    return select(v, -v, e < 0.0);
}}
",
        cut = fl(SRGB_CUT * SRGB_SLOPE),
        slope = fl(SRGB_SLOPE),
        scale = fl(SRGB_SCALE),
        inv_exp = fl(1.0 / SRGB_EXP),
        offset = fl(SRGB_OFFSET),
    )
}

/// Format an `f32` as a WGSL float literal that round-trips exactly (`{:?}`
/// prints the shortest representation that parses back to the same `f32`, and
/// always includes a `.` or an exponent so WGSL reads it as `f32`, not `i32`).
pub(crate) fn fl(v: f32) -> String {
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{s}.0")
    }
}

/// A 3x3 matrix by COLUMNS, like WGSL's `mat3x3<f32>(c0, c1, c2)` constructor.
type Mat3 = [[f32; 3]; 3];

/// CIE xy chromaticities of a colour space: red, green, blue, white.
struct Primaries([[f64; 2]; 4]);

/// ITU-R BT.709 primaries (sRGB's too), D65 white.
const REC709: Primaries = Primaries([
    [0.640, 0.330],
    [0.300, 0.600],
    [0.150, 0.060],
    [0.3127, 0.3290],
]);
/// ITU-R BT.2020 primaries, D65 white.
const REC2020: Primaries = Primaries([
    [0.708, 0.292],
    [0.170, 0.797],
    [0.131, 0.046],
    [0.3127, 0.3290],
]);

/// A 3x3 matrix, row-major, in f64 (the derivations below are exact, then rounded once).
type M3 = [[f64; 3]; 3];

/// The 2x2 minor of `m` on rows `r0, r1` and columns `c0, c1`.
const fn minor(m: &M3, r0: usize, c0: usize, r1: usize, c1: usize) -> f64 {
    m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
}

/// The inverse by the adjugate.
const fn inv(m: M3) -> M3 {
    let det = m[0][0] * minor(&m, 1, 1, 2, 2) - m[0][1] * minor(&m, 1, 0, 2, 2)
        + m[0][2] * minor(&m, 1, 0, 2, 1);
    [
        [
            minor(&m, 1, 1, 2, 2) / det,
            -minor(&m, 0, 1, 2, 2) / det,
            minor(&m, 0, 1, 1, 2) / det,
        ],
        [
            -minor(&m, 1, 0, 2, 2) / det,
            minor(&m, 0, 0, 2, 2) / det,
            -minor(&m, 0, 0, 1, 2) / det,
        ],
        [
            minor(&m, 1, 0, 2, 1) / det,
            -minor(&m, 0, 0, 2, 1) / det,
            minor(&m, 0, 0, 1, 1) / det,
        ],
    ]
}

/// CIE XYZ of the chromaticity `xy` at Y = 1.
const fn xyz(xy: [f64; 2]) -> [f64; 3] {
    [xy[0] / xy[1], 1.0, (1.0 - xy[0] - xy[1]) / xy[1]]
}

const fn mul3(a: M3, b: M3) -> M3 {
    let mut o = [[0.0; 3]; 3];
    let mut r = 0;
    while r < 3 {
        let mut c = 0;
        while c < 3 {
            o[r][c] = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
            c += 1;
        }
        r += 1;
    }
    o
}

/// Linear RGB of `p` -> CIE XYZ: the primaries scaled so RGB (1,1,1) is the white at
/// Y = 1 (ITU-R BT.2087 §2; the derivation OCIO's `build_conversion_matrix_from_XYZ_D65` runs).
const fn to_xyz(p: &Primaries) -> M3 {
    let (r, g, b, w) = (xyz(p.0[0]), xyz(p.0[1]), xyz(p.0[2]), xyz(p.0[3]));
    let prim = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let k = inv(prim);
    let s = [
        k[0][0] * w[0] + k[0][1] * w[1] + k[0][2] * w[2],
        k[1][0] * w[0] + k[1][1] * w[1] + k[1][2] * w[2],
        k[2][0] * w[0] + k[2][1] * w[1] + k[2][2] * w[2],
    ];
    [
        [prim[0][0] * s[0], prim[0][1] * s[1], prim[0][2] * s[2]],
        [prim[1][0] * s[0], prim[1][1] * s[1], prim[1][2] * s[2]],
        [prim[2][0] * s[0], prim[2][1] * s[1], prim[2][2] * s[2]],
    ]
}

/// A row-major f64 matrix as the by-COLUMNS f32 [`Mat3`].
const fn by_cols(m: M3) -> Mat3 {
    let mut o = [[0.0f32; 3]; 3];
    let mut r = 0;
    while r < 3 {
        let mut c = 0;
        while c < 3 {
            o[c][r] = m[r][c] as f32;
            c += 1;
        }
        r += 1;
    }
    o
}

/// Linear sRGB/Rec.709 -> linear Rec.2020, by columns, derived from the primaries in
/// f64 (not the 4-digit BT.2407 table, whose rows then do not sum to exactly 1). Also
/// the present pass's HDR10 gamut conversion (`present.rs`).
pub const SRGB_TO_REC2020: Mat3 = by_cols(mul3(inv(to_xyz(&REC2020)), to_xyz(&REC709)));
/// Linear Rec.2020 -> linear sRGB/Rec.709, by columns (the exact inverse).
pub const REC2020_TO_SRGB: Mat3 = by_cols(mul3(inv(to_xyz(&REC709)), to_xyz(&REC2020)));
