//! Edit cost: appending keys one by one and moving a middle key at 10k keys.
//! `cargo run --release --example edit_bench`
use curves::{Tan, Track};
use std::time::Instant;

fn main() {
    const N: usize = 10_000;
    let mut best_append = f64::MAX;
    let mut best_move = f64::MAX;
    for _ in 0..5 {
        let mut t = Track::<f64>::new();
        let s = Instant::now();
        for k in 0..N {
            t.add(k as f64, (k as f64 * 0.37).sin(), Tan::Smooth)
                .unwrap();
        }
        best_append = best_append.min(s.elapsed().as_secs_f64());
        let s = Instant::now();
        let reps = 200;
        for r in 0..reps {
            // Stays between the neighbours: the common drag case.
            t.move_key(N / 2, (N / 2) as f64 + 0.25 * f64::from((r % 3) as u8))
                .unwrap();
        }
        best_move = best_move.min(s.elapsed().as_secs_f64() / f64::from(reps));
    }
    println!(
        "append {N} keys: {:.1} ms; move_key at {N}: {:.1} us",
        best_append * 1e3,
        best_move * 1e6
    );
}
