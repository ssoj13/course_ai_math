//! What the shared interiors are for: cloning a tree must not depend on its
//! size.
//!
//! This is a property, not a microbenchmark — it asserts a ratio rather than a
//! wall-clock number, so it stays meaningful on a slow CI box. The numbers it
//! prints are the interesting part when someone is tuning.

use box_rs::{BoxList, BoxValue};
use std::time::Instant;

/// A tree of `leaves` leaves, nested `depth` levels deep.
fn tree(depth: usize, width: usize) -> BoxValue {
    if depth == 0 {
        return BoxValue::from(1.234_f64);
    }
    let mut map = box_rs::BoxMap::new();
    for i in 0..width {
        map.insert(format!("k{i}"), tree(depth - 1, width));
    }
    BoxValue::obj(map)
}

fn time_clones(v: &BoxValue, n: usize) -> f64 {
    let start = Instant::now();
    let mut sink = 0usize;
    for _ in 0..n {
        let c = v.clone();
        // Keep the clone observable so it cannot be optimised away.
        sink += std::hint::black_box(&c).as_object().map_or(0, |m| m.len());
    }
    assert!(sink > 0);
    start.elapsed().as_secs_f64() / n as f64
}

#[test]
fn cloning_cost_does_not_grow_with_the_tree() {
    let small = tree(2, 3); // 9 leaves
    let big = tree(4, 6); // 1296 leaves

    // Warm up, then measure.
    time_clones(&small, 1000);
    let t_small = time_clones(&small, 20_000);
    let t_big = time_clones(&big, 20_000);

    println!(
        "clone: small(9 leaves) {:.0} ns, big(1296 leaves) {:.0} ns, ratio {:.2}",
        t_small * 1e9,
        t_big * 1e9,
        t_big / t_small
    );

    // A deep copy would be ~144x apart. Allow a wide margin for timer noise on
    // sub-microsecond work; anything near proportional means the interiors
    // stopped being shared.
    assert!(
        t_big < t_small * 10.0,
        "cloning a 144x bigger tree took {:.2}x longer — the interiors are being deep-copied",
        t_big / t_small
    );
}

/// The cost that moved: the first write after a clone pays for the copy, and
/// only for the level it writes to.
#[test]
fn the_first_write_after_a_clone_is_the_one_that_copies() {
    let mut a = BoxValue::array((0..1000).map(BoxValue::from).collect::<BoxList>());
    let b = a.clone();
    a.as_array_mut().expect("array").push(BoxValue::from(-1));

    assert_eq!(a.as_array().map(Vec::len), Some(1001));
    assert_eq!(
        b.as_array().map(Vec::len),
        Some(1000),
        "the clone moved with it"
    );
}
