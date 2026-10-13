//! In-memory (codec-only, no file I/O) encode+decode benchmark, mirrored by a matching
//! standalone C++ program built against real OpenJPH 0.31.0 (`bench_inmemory.cpp`, not
//! committed here — see `BUG1.md` Round 9 for its source and the comparison numbers).
//! Default LZCNT mode (whatever `jph_rs` uses out of the box — no override), min-of-N run
//! externally by interleaving process invocations (this binary just does one encode+decode
//! pass per run so the shell driver can interleave jph-rs/C++ invocations 1:1).
use jph_rs::mem;

fn make_rgb(w: u32, h: u32) -> mem::Image {
    let mut planes = Vec::new();
    for c in 0..3u32 {
        let mut samples = vec![0i32; (w * h) as usize];
        for (i, s) in samples.iter_mut().enumerate() {
            let x = (i as u32) % w;
            let y = (i as u32) / w;
            let t = (x ^ y).wrapping_mul(17);
            *s = (t.wrapping_add(match c {
                0 => x & 0xff,
                1 => y & 0xff,
                _ => 128,
            }) & 0xff) as i32;
        }
        planes.push(mem::Plane { width: w, height: h, bit_depth: 8, signed: false, samples });
    }
    mem::Image { width: w, height: h, planes }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let lossy = args.iter().any(|a| a == "--lossy");
    let (w, h) = (3840u32, 2160u32);
    let img = make_rgb(w, h);

    let mut params = mem::EncodeParams::default();
    params.color_transform = true;
    if lossy {
        params.quant = mem::Quant::Step(0.05);
    }

    let t0 = std::time::Instant::now();
    let encoded = mem::try_encode(&img, &params).expect("encode");
    let enc_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = std::time::Instant::now();
    let decoded = mem::try_decode(&encoded).expect("decode");
    let dec_ms = t1.elapsed().as_secs_f64() * 1000.0;

    std::hint::black_box(&decoded);
    println!("BENCH enc_ms={enc_ms:.3} dec_ms={dec_ms:.3} bytes={}", encoded.len());
}
