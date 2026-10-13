//! Decode a single codestream named by `CONFFILE` (conformance triage). Lets the ojph_error!
//! message print before any panic so the exact unsupported-feature reject is visible. When
//! `CONFDUMP` is set, write each component (oracle convention: level-shift a signed component by
//! 2^(bd-1), clamp to [0, 2^bd-1]) to `<CONFDUMP>_<NN>.raw` for a tolerance comparison vs the
//! OpenHTJ2K oracle `*_<NN>.pgm`.
#[test]
#[ignore = "needs CONFFILE (a codestream path): run with --ignored"]
fn conf_one() {
    let Ok(f) = std::env::var("CONFFILE") else {
        panic!("conf_one: CONFFILE not set");
    };
    let bytes = std::fs::read(&f).unwrap();
    let img = jph_rs::mem::decode(&bytes);
    println!("OK {} {}x{} c={}", f, img.width, img.height, img.planes.len());
    if let Ok(dump) = std::env::var("CONFDUMP") {
        for (i, p) in img.planes.iter().enumerate() {
            let lvl: i32 = if p.signed { 1 << (p.bit_depth - 1) } else { 0 };
            let maxval: i32 = (1 << p.bit_depth) - 1;
            // One byte per sample for <=8-bit components, little-endian u16 for wider ones, so
            // 12/16-bit conformance components can be compared at full precision against .pgx.
            let bytes: Vec<u8> = if p.bit_depth <= 8 {
                p.samples.iter().map(|&s| (s + lvl).clamp(0, maxval) as u8).collect()
            } else {
                p.samples
                    .iter()
                    .flat_map(|&s| ((s + lvl).clamp(0, maxval) as u16).to_le_bytes())
                    .collect()
            };
            std::fs::write(format!("{dump}_{i:02}.raw"), &bytes).unwrap();
        }
    }
}
