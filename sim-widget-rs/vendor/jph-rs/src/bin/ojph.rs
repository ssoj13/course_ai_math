//! `ojph` — raster image <-> HTJ2K/JPEG 2000 codestream converter, built on the safe
//! [`jph_rs::mem`] API (no raw pointers, no codec plumbing of its own).
//!
//! Run `ojph` (or `ojph <command>`) with no arguments for help. Commands have a long and a short
//! name: `encode|e`, `decode|d`, `convert|c` (direction inferred from the file extensions) and
//! `verify|v`. Running `ojph <input> <output> [options]` is shorthand for `convert`.

use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use image::ColorType;
use jph_rs::mem::{
    DecodeParams, EncodeOptions, EncodeParams, Image, Plane, Progression, Quant, Rounding, try_decode_with, try_encode_with,
};

type Res<T> = Result<T, String>;

/// Raster extensions the `image` crate reads/writes for us, and codestream extensions.
const IMG_EXTS: &[&str] = &["png", "jpg", "jpeg", "bmp", "tga", "tiff", "webp", "ppm", "pgm"];
const J2C_EXTS: &[&str] = &["j2c", "jph", "j2k"];

// ---------------------------------------------------------------------------------------------
// Option parsing
// ---------------------------------------------------------------------------------------------

/// One accepted option: all its spellings (first = canonical key) and whether it takes a value.
struct Opt(&'static [&'static str], bool);

const ENCODE_OPTS: &[Opt] = &[
    Opt(&["-q", "--qstep", "-qstep"], true),
    Opt(&["-Q", "--qfactor", "-qfactor"], true),
    Opt(&["-n", "--num-decomps", "-num_decomps"], true),
    Opt(&["-b", "--block-size", "-block_size"], true),
    Opt(&["-p", "--prog-order", "-prog_order"], true),
    Opt(&["-r", "--reversible", "-reversible"], true),
    Opt(&["-c", "--colour-trans", "-colour_trans", "-color_trans"], true),
    Opt(&["-t", "--tile-size", "-tile_size"], true),
    Opt(&["-T", "--tlm", "-tlm_marker"], false),
    Opt(&["-m", "--com", "-com"], true),
    Opt(&["--tilepart-res"], false),
    Opt(&["--tilepart-comp"], false),
    Opt(&["--rounding"], true),
];
const DECODE_OPTS: &[Opt] = &[
    Opt(&["-s", "--reduce", "-skip_res"], true),
    Opt(&["-R", "--resilient", "-resilient"], false),
];

/// Positional arguments plus `canonical option -> value` ("" for flags).
struct Parsed {
    pos: Vec<String>,
    opts: HashMap<&'static str, String>,
}

impl Parsed {
    fn get(&self, key: &str) -> Option<&str> {
        self.opts.get(key).map(String::as_str)
    }

    /// Parse option `key` with `f`, naming the option in the error.
    fn val<T>(&self, key: &str, f: impl FnOnce(&str) -> Res<T>) -> Res<Option<T>> {
        self.get(key).map(|v| f(v).map_err(|e| format!("{key}: {e}"))).transpose()
    }
}

/// Split `args` into positionals and the options in `spec`. Accepts `-x v`, `--long v`,
/// `--long=v`; an unknown option or a missing value is an error (never silently defaulted).
fn parse_args(args: &[String], spec: &[Opt]) -> Res<Parsed> {
    let (mut pos, mut opts) = (Vec::new(), HashMap::new());
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !a.starts_with('-') || a == "-" {
            pos.push(a.clone());
            continue;
        }
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let o = spec.iter().find(|o| o.0.contains(&name)).ok_or_else(|| format!("unknown option {a}"))?;
        let v = match (o.1, inline) {
            (true, Some(v)) => v,
            (true, None) => it.next().cloned().ok_or_else(|| format!("option {name} needs a value"))?,
            (false, None) => String::new(),
            (false, Some(_)) => return Err(format!("option {name} takes no value")),
        };
        opts.insert(o.0[0], v);
    }
    Ok(Parsed { pos, opts })
}

fn p_bool(s: &str) -> Res<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("expected true/false, got `{s}`")),
    }
}

/// `W,H`, `WxH` or a single `N` (square); both must be non-zero.
fn p_size(s: &str) -> Res<(u32, u32)> {
    let n = |t: &str| t.trim().parse::<u32>().ok().filter(|&v| v > 0).ok_or_else(|| format!("bad size `{s}`"));
    match s.split_once([',', 'x']) {
        Some((w, h)) => Ok((n(w)?, n(h)?)),
        None => n(s).map(|v| (v, v)),
    }
}

fn p_num<T: std::str::FromStr>(s: &str) -> Res<T> {
    s.trim().parse().map_err(|_| format!("bad number `{s}`"))
}

// ---------------------------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------------------------

const HELP: &str = "\
ojph - raster image <-> HTJ2K / JPEG 2000 codestream converter

USAGE:
  ojph <command> [args]          ojph <input> <output> [options]   (= convert)

COMMANDS:
  encode  | e   raster image -> .j2c/.jph/.j2k
  decode  | d   .j2c/.jph/.j2k -> raster image
  convert | c   direction inferred from the file extensions
  verify  | v   compare two images (exit 1 unless bit-exact)

EXAMPLES:
  ojph e photo.png photo.j2c                    lossless
  ojph e photo.png photo.j2c -Q 80              lossy, quality factor 80
  ojph d photo.j2c out.png -s 1                 half-resolution decode
  ojph v photo.png out.png

Run `ojph <command>` without arguments for that command's options.";

const ENCODE_HELP: &str = "\
ojph encode <input image> <output.j2c|jph|j2k> [options]

  -q,  --qstep <f>        lossy quantization step (implies irreversible 9/7)
  -Q,  --qfactor <1-100>  lossy quality factor (wins over -q)
  -n,  --num-decomps <n>  decomposition levels (default 5)
  -b,  --block-size <w,h> code-block size, powers of two (default 64,64)
  -p,  --prog-order <o>   LRCP | RLCP | RPCL | PCRL | CPRL (default RPCL)
  -r,  --reversible <b>   true = 5/3 lossless, false = 9/7 (default: lossless unless -q/-Q)
  -c,  --colour-trans <b> colour transform (default: on for >= 3 components)
  -t,  --tile-size <w,h>  tile size (default: whole image)
  -T,  --tlm              write a TLM marker
  -m,  --com <text>       write a COM marker
       --tilepart-res     new tile-part per resolution
       --tilepart-comp    new tile-part per component
       --rounding <m>     lossy rounding: cpp (default, C++-compatible) | trunc (T.800, CPU-independent)

EXAMPLE: ojph encode in.png out.j2c -Q 90 -t 512,512";

const DECODE_HELP: &str = "\
ojph decode <input.j2c|jph|j2k> <output image> [options]

  -s, --reduce <n>   drop the n highest resolution levels (output is 2^n smaller)
  -R, --resilient    keep going past damaged packets

EXAMPLE: ojph decode in.j2c out.png -s 2";

const VERIFY_HELP: &str = "ojph verify <image A> <image B>\n\nCompares the pixels; exits 0 only when bit-exact.";

// ---------------------------------------------------------------------------------------------
// Image <-> mem::Image
// ---------------------------------------------------------------------------------------------

fn ext_of(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

/// Read a raster file into planar components (1..4 channels, 8 or 16 bit).
fn load(path: &Path) -> Res<Image> {
    let img = image::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let (channels, depth) = match img.color() {
        ColorType::L8 => (1, 8),
        ColorType::L16 => (1, 16),
        ColorType::La8 => (2, 8),
        ColorType::La16 => (2, 16),
        ColorType::Rgba16 => (4, 16),
        ColorType::Rgb16 => (3, 16),
        ColorType::Rgba8 | ColorType::Rgba32F => (4, 8),
        _ => (3, 8),
    };
    let (w, h) = (img.width(), img.height());
    // Interleaved samples widened to i32, in the file's channel order.
    let raw: Vec<i32> = match (channels, depth) {
        (1, 8) => img.to_luma8().into_raw().into_iter().map(i32::from).collect(),
        (1, _) => img.to_luma16().into_raw().into_iter().map(i32::from).collect(),
        (2, 8) => img.to_luma_alpha8().into_raw().into_iter().map(i32::from).collect(),
        (2, _) => img.to_luma_alpha16().into_raw().into_iter().map(i32::from).collect(),
        (3, 8) => img.to_rgb8().into_raw().into_iter().map(i32::from).collect(),
        (3, _) => img.to_rgb16().into_raw().into_iter().map(i32::from).collect(),
        (_, 8) => img.to_rgba8().into_raw().into_iter().map(i32::from).collect(),
        _ => img.to_rgba16().into_raw().into_iter().map(i32::from).collect(),
    };
    let planes = (0..channels)
        .map(|c| Plane {
            width: w,
            height: h,
            bit_depth: depth,
            signed: false,
            samples: raw.iter().skip(c).step_by(channels).copied().collect(),
        })
        .collect();
    Ok(Image { width: w, height: h, planes })
}

/// Write `img` as a raster file. Signed components are level-shifted to unsigned; sample depths are
/// scaled to the 8 or 16 bits the format stores. PNM only holds 1 or 3 components, so other counts
/// fall back to PNG (with a note).
fn save(img: &Image, path: &Path) -> Res<()> {
    let first = img.planes.first().ok_or("image has no components")?;
    let (w, h) = (first.width, first.height);
    if img.planes.iter().any(|p| (p.width, p.height) != (w, h)) {
        return Err("components have different sizes (subsampled); cannot write them to a raster file".into());
    }
    let n = img.planes.len();
    let deep = first.bit_depth > 8;
    let scale = |p: &Plane, v: i32| -> u32 {
        let v = if p.signed { v + (1 << (p.bit_depth - 1)) } else { v }.max(0) as u32;
        let d = p.bit_depth;
        let t = if deep { 16 } else { 8 };
        let s = if d >= t { v >> (d - t) } else if deep { v << (t - d) } else { v };
        s.min((1u32 << t) - 1)
    };
    let mut ext = ext_of(path);
    let mut path = path.to_path_buf();
    if matches!(ext.as_str(), "ppm" | "pgm") && n != 1 && n != 3 {
        eprintln!("PPM/PGM cannot hold {n} components; writing PNG instead");
        path.set_extension("png");
        ext = "png".into();
    }
    // Interleave.
    let count = (w * h) as usize;
    let mut bytes = Vec::with_capacity(count * n * if deep { 2 } else { 1 });
    for i in 0..count {
        for p in &img.planes {
            let v = scale(p, p.samples[i]);
            match (deep, ext.as_str()) {
                (false, _) => bytes.push(v as u8),
                (true, "ppm" | "pgm") => bytes.extend((v as u16).to_be_bytes()), // PNM 16-bit is big-endian
                (true, _) => bytes.extend((v as u16).to_ne_bytes()),            // `image` wants native-endian
            }
        }
    }
    if matches!(ext.as_str(), "ppm" | "pgm") {
        let hdr = format!("{}\n{w} {h}\n{}\n", if n == 1 { "P5" } else { "P6" }, if deep { 65535 } else { 255 });
        return std::fs::write(&path, [hdr.as_bytes(), &bytes].concat()).map_err(|e| format!("{}: {e}", path.display()));
    }
    let ct = match (n, deep) {
        (1, false) => ColorType::L8,
        (1, true) => ColorType::L16,
        (2, false) => ColorType::La8,
        (2, true) => ColorType::La16,
        (3, false) => ColorType::Rgb8,
        (3, true) => ColorType::Rgb16,
        (4, false) => ColorType::Rgba8,
        (4, true) => ColorType::Rgba16,
        _ => return Err(format!("{n} components cannot be written to {ext}")),
    };
    image::save_buffer(&path, &bytes, w, h, ct).map_err(|e| format!("cannot save {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------------------------

fn encode(args: &[String]) -> Res<()> {
    let a = parse_args(args, ENCODE_OPTS)?;
    let [input, output] = a.pos.as_slice() else {
        return Err("encode needs <input image> <output codestream>".into());
    };
    if !J2C_EXTS.contains(&ext_of(Path::new(output)).as_str()) {
        return Err(format!("output `{output}` must end in .j2c, .jph or .j2k"));
    }
    let img = load(Path::new(input))?;
    let n = img.planes.len();

    let qstep = a.val("-q", p_num::<f32>)?;
    let qfactor = a.val("-Q", p_num::<u8>)?;
    // -Q wins over -q (as in OpenJPH); -r only confirms what the quantization options imply.
    let quant = match (qfactor, qstep) {
        (Some(q), _) => Quant::Factor(q),
        (None, Some(d)) => Quant::Step(d),
        (None, None) => Quant::Lossless,
    };
    if let Some(rev) = a.val("-r", p_bool)? {
        if rev != quant.is_lossless() {
            return Err(if rev {
                "-r true (lossless) contradicts -q/-Q".into()
            } else {
                "-r false (lossy) needs a quantization step (-q) or quality factor (-Q)".into()
            });
        }
    }
    let def = EncodeParams::default();
    let (block_width, block_height) = a.val("-b", p_size)?.unwrap_or((def.block_width, def.block_height));
    let params = EncodeParams {
        quant,
        num_decompositions: a.val("-n", p_num::<u8>)?.unwrap_or(def.num_decompositions),
        block_width,
        block_height,
        progression: a.val("-p", |s| s.parse::<Progression>())?.unwrap_or(def.progression),
        color_transform: a.val("-c", p_bool)?.unwrap_or(n >= 3),
    };
    let opts = EncodeOptions {
        rounding: match a.get("--rounding") {
            None | Some("cpp") => Rounding::Cpp,
            Some("trunc") => Rounding::Trunc,
            Some(s) => return Err(format!("--rounding: `{s}` is not cpp or trunc")),
        },
        tile_size: a.val("-t", p_size)?,
        tlm: a.get("-T").is_some(),
        comment: a.get("-m").map(str::to_owned),
        tilepart_res: a.get("--tilepart-res").is_some(),
        tilepart_comp: a.get("--tilepart-comp").is_some(),
    };
    let bytes = try_encode_with(&img, &params, &opts).map_err(|e| e.to_string())?;
    std::fs::write(output, &bytes).map_err(|e| format!("{output}: {e}"))?;
    println!("{input} -> {output}: {} bytes", bytes.len());
    Ok(())
}

fn decode(args: &[String]) -> Res<()> {
    let a = parse_args(args, DECODE_OPTS)?;
    let [input, output] = a.pos.as_slice() else {
        return Err("decode needs <input codestream> <output image>".into());
    };
    if !IMG_EXTS.contains(&ext_of(Path::new(output)).as_str()) {
        return Err(format!("output `{output}` must be one of: {}", IMG_EXTS.join(", ")));
    }
    let data = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let params = DecodeParams {
        reduce: a.val("-s", p_num::<u32>)?.unwrap_or(0),
        resilient: a.get("-R").is_some(),
    };
    let img = try_decode_with(&data, &params).map_err(|e| e.to_string())?;
    save(&img, Path::new(output))?;
    println!("{input} -> {output}: {}x{}, {} component(s)", img.width, img.height, img.planes.len());
    Ok(())
}

/// Direction inferred from the extensions, then `encode`/`decode`.
fn convert(args: &[String]) -> Res<()> {
    let first_two: Vec<String> = args.iter().filter(|a| !a.starts_with('-')).take(2).cloned().collect();
    let [i, o] = first_two.as_slice() else {
        return Err("convert needs <input> <output>".into());
    };
    let (ie, oe) = (ext_of(Path::new(i)), ext_of(Path::new(o)));
    if IMG_EXTS.contains(&ie.as_str()) && J2C_EXTS.contains(&oe.as_str()) {
        encode(args)
    } else if J2C_EXTS.contains(&ie.as_str()) && IMG_EXTS.contains(&oe.as_str()) {
        decode(args)
    } else {
        Err(format!("cannot infer the direction from extensions `{ie}` -> `{oe}`"))
    }
}

/// Exit code 0 only for a bit-exact match. Compares the decoded components themselves (not an RGB8
/// approximation of them), so differing depth, component count or size is a mismatch.
fn verify(args: &[String]) -> Res<bool> {
    let a = parse_args(args, &[])?;
    let [pa, pb] = a.pos.as_slice() else {
        return Err("verify needs two images".into());
    };
    let (x, y) = (load(Path::new(pa))?, load(Path::new(pb))?);
    let shape = |i: &Image| (i.width, i.height, i.planes.len(), i.planes[0].bit_depth);
    if shape(&x) != shape(&y) {
        println!("RESULT: DIFFERS (shape {:?} vs {:?})", shape(&x), shape(&y));
        return Ok(false);
    }
    let (mut max, mut sq, mut bad, mut total) = (0u32, 0f64, 0u64, 0u64);
    for (p, q) in x.planes.iter().zip(&y.planes) {
        for (&s, &t) in p.samples.iter().zip(&q.samples) {
            let d = (s - t).unsigned_abs();
            total += 1;
            bad += u64::from(d > 0);
            max = max.max(d);
            sq += f64::from(d) * f64::from(d);
        }
    }
    let mse = sq / total as f64;
    let peak = f64::from((1u32 << x.planes[0].bit_depth) - 1);
    println!("{}x{}, {} component(s), {} samples", x.width, x.height, x.planes.len(), total);
    println!("Max diff: {max}\nMismatches: {bad} ({:.2}%)\nMSE: {mse:.4}", bad as f64 / total as f64 * 100.0);
    println!("PSNR: {:.2} dB", if mse > 0.0 { 10.0 * (peak * peak / mse).log10() } else { f64::INFINITY });
    println!("RESULT: {}", if bad == 0 { "BIT-EXACT MATCH" } else { "DIFFERS" });
    Ok(bad == 0)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first().map(String::as_str) else {
        println!("{HELP}");
        return ExitCode::SUCCESS;
    };
    let rest = &args[1..];
    // A command with no arguments prints its own help (sysgo convention).
    let help = |text: &str| {
        println!("{text}");
        ExitCode::SUCCESS
    };
    let t0 = Instant::now();
    let out = match cmd {
        "-h" | "--help" | "help" => return help(HELP),
        "encode" | "e" if rest.is_empty() => return help(ENCODE_HELP),
        "decode" | "d" if rest.is_empty() => return help(DECODE_HELP),
        "convert" | "c" if rest.is_empty() => return help(HELP),
        "verify" | "v" if rest.is_empty() => return help(VERIFY_HELP),
        "encode" | "e" => encode(rest).map(|()| true),
        "decode" | "d" => decode(rest).map(|()| true),
        "convert" | "c" => convert(rest).map(|()| true),
        "verify" | "v" => verify(rest),
        _ if args.len() >= 2 && !cmd.starts_with('-') => convert(&args).map(|()| true),
        _ => Err(format!("unknown command `{cmd}` (try `ojph --help`)")),
    };
    match out {
        Ok(ok) => {
            if cmd != "verify" && cmd != "v" {
                println!("Done in {:.3}s", t0.elapsed().as_secs_f64());
            }
            if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
        Err(e) => {
            eprintln!("ojph: {e}");
            ExitCode::FAILURE
        }
    }
}
