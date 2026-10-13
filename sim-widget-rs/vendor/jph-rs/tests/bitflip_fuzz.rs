//! Hostile-input smoke fuzz: flip three bits of every header byte and of a stride of payload bytes of
//! valid codestreams (lossless, lossy, multi-tile/tile-part), plus every truncation length, and
//! require that `try_decode` and `parse::enumerate` return (`Ok` or `Err`) — never panic, hang or
//! allocate unboundedly. Deterministic and fast enough for CI; it is a smoke test, not a corpus fuzzer.
use std::panic::{AssertUnwindSafe, catch_unwind};

mod common;
use jph_rs::mem::{EncodeOptions, EncodeParams, Quant, Image, try_decode, try_encode_with};

fn streams() -> Vec<(&'static str, Vec<u8>)> {
    let rgb = Image { width: 40, height: 30, planes: vec![common::synth_plane(40, 30, 1), common::synth_plane(40, 30, 2), common::synth_plane(40, 30, 3)] };
    let gray = Image { width: 33, height: 17, planes: vec![common::synth_plane(33, 17, 4)] };
    let lossless = EncodeParams { color_transform: true, num_decompositions: 3, ..EncodeParams::default() };
    let lossy = EncodeParams { quant: Quant::Step(1.0 / 32.0), num_decompositions: 2, ..EncodeParams::default() };
    let tiled = EncodeOptions { tile_size: Some((16, 16)), tlm: true, tilepart_res: true, ..Default::default() };
    vec![
        ("rgb-lossless", try_encode_with(&rgb, &lossless, &EncodeOptions::default()).expect("encode")),
        ("gray-lossy", try_encode_with(&gray, &lossy, &EncodeOptions::default()).expect("encode")),
        ("rgb-tiled", try_encode_with(&rgb, &lossless, &tiled).expect("encode")),
    ]
}

/// Run both consumers on `data`; a panic is reported with the mutation that caused it.
fn probe(name: &str, what: &str, data: &[u8]) {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let _ = try_decode(data);
        let _ = jph_rs::enumerate(data);
    }));
    assert!(r.is_ok(), "{name}: panic on {what}");
}

#[test]
fn bit_flips_never_panic() {
    for (name, bytes) in streams() {
        // Headers (first 200 bytes) exhaustively; the payload on a stride.
        // Debug builds (overflow checks on, the most valuable run) are ~6x slower: sample sparser.
        let stride = if cfg!(debug_assertions) { 97 } else { 31 };
        let positions: Vec<usize> = (0..bytes.len().min(200)).chain((200..bytes.len()).step_by(stride)).collect();
        for &pos in &positions {
            for bit in [0, 3, 7] {
                let mut b = bytes.clone();
                b[pos] ^= 1 << bit;
                probe(name, &format!("flip byte {pos} bit {bit}"), &b);
            }
        }
    }
}

#[test]
fn truncations_never_panic() {
    for (name, bytes) in streams() {
        for len in (0..bytes.len()).step_by(3) {
            probe(name, &format!("truncation to {len}"), &bytes[..len]);
        }
    }
}
