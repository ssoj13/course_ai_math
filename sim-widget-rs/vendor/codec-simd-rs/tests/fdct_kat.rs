//! Byte-exactness of the forward DCT kernels against the C reference (`tests/data/fdct_kat.txt`, written by
//! `tests/c_oracle/gen_fdct_kat.ps1` from the unmodified FFmpeg 9.0.1 `jfdctint_template.c` / `jfdctfst.c` and
//! libjpeg-turbo `jfdctint.c`, `faandct.c`): 40 blocks per kernel (noise, extremes, checkerboard, edges, impulse, ramp).

use codec_simd::{
    fdct_faan, fdct_float_tj, fdct_ifast, fdct_ifast_tj, fdct_islow_ff8, fdct_islow_ff10, fdct_islow_tj8,
    fdct_islow_tj8_int, fdct_islow_tj12,
};

fn row(line: &str) -> (&str, Vec<i32>, Vec<i32>) {
    let (name, rest) = line.split_once(' ').expect("kernel name");
    let (i, o) = rest.split_once(" -> ").expect("arrow");
    let nums = |s: &str| {
        s.split_whitespace()
            .map(|v| v.parse::<i32>().unwrap())
            .collect::<Vec<_>>()
    };
    (name, nums(i), nums(o))
}

fn narrow(v: &[i32]) -> [i16; 64] {
    std::array::from_fn(|i| v[i] as i16)
}

#[test]
fn fdct_kernels_match_c() {
    let p = format!("{}/tests/data/fdct_kat.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&p).unwrap();
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    for (n, line) in text.lines().enumerate() {
        let (name, input, want) = row(line);
        let got: Vec<i32> = match name {
            "ff_islow_8" => {
                let mut b = narrow(&input);
                fdct_islow_ff8(&mut b);
                b.iter().map(|&v| i32::from(v)).collect()
            }
            "ff_islow_10" => {
                let mut b = narrow(&input);
                fdct_islow_ff10(&mut b);
                b.iter().map(|&v| i32::from(v)).collect()
            }
            "ff_faan" | "ff_faan_u8" => {
                let mut b = narrow(&input);
                fdct_faan(&mut b);
                b.iter().map(|&v| i32::from(v)).collect()
            }
            "ff_ifast" => {
                let mut b = narrow(&input);
                fdct_ifast(&mut b);
                b.iter().map(|&v| i32::from(v)).collect()
            }
            "tj_islow_8" => {
                let mut b = narrow(&input);
                fdct_islow_tj8(&mut b);
                b.iter().map(|&v| i32::from(v)).collect()
            }
            "tj_islow_12" => {
                let mut b: [i32; 64] = std::array::from_fn(|i| input[i]);
                fdct_islow_tj12(&mut b);
                b.to_vec()
            }
            "tj_islow_8i" => {
                let mut b: [i32; 64] = std::array::from_fn(|i| input[i]);
                fdct_islow_tj8_int(&mut b);
                b.to_vec()
            }
            "tj_ifast_8" | "tj_ifast_12" => {
                let mut b: [i32; 64] = std::array::from_fn(|i| input[i]);
                fdct_ifast_tj(&mut b);
                b.to_vec()
            }
            "tj_float" => {
                // The oracle prints the raw bits of the float results.
                let mut b: [f32; 64] = std::array::from_fn(|i| input[i] as f32);
                fdct_float_tj(&mut b);
                b.iter().map(|v| v.to_bits() as i32).collect()
            }
            other => panic!("unknown kernel {other}"),
        };
        assert_eq!(got, want, "{name} line {n}");
        *seen.entry(name.to_owned()).or_default() += 1;
    }
    assert_eq!(seen.len(), 11);
    assert!(seen.values().all(|&c| c == 40), "{seen:?}");
}
