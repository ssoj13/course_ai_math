//! `Track::eval` must not allocate: a counting global allocator proves it.
use curves::{Extrap, Key, SideId, Tan, Track};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local!(static ALLOCS: Cell<usize> = const { Cell::new(0) });

struct Counting;
// SAFETY: forwards every call to the system allocator unchanged; only bumps a thread-local counter.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn eval_is_allocation_free() {
    let mut t = Track::new();
    for k in 0..50u32 {
        let tan = [Tan::Smooth, Tan::Linear, Tan::CatmullRom, Tan::Flat][k as usize % 4];
        t.insert(Key::new(f64::from(k), f64::from(k * 7 % 11)).with_tan(tan))
            .unwrap();
    }
    t.set_user(10, SideId::Out, 0.3, Some(0.2)).unwrap();
    t.set_extrap(Extrap::CycleOffset, Extrap::PingPong);
    let before = ALLOCS.with(Cell::get);
    let (mut acc, mut hint) = (0.0, 0);
    for s in 0..10_000 {
        let x = f64::from(s) * 0.017 - 20.0;
        acc += t.eval(x) + t.eval_hint(x, &mut hint);
    }
    let mut buf = [0.0; 256];
    t.eval_many(0.0, 0.1, &mut buf);
    assert!(acc.is_finite());
    assert_eq!(ALLOCS.with(Cell::get), before);
}
