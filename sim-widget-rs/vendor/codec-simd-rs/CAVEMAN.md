# CAVEMAN — `codec-simd`

Short, paste-friendly instructions.  No philosophy.

---

## What this crate is

A box of SIMD-accelerated DSP primitives used by software video codecs.
You import it, you call functions, you get NEON on ARM and AVX2 on Intel.
No `unsafe` at the call site.

You DO NOT use this crate to:
- parse bitstreams (sequential, no SIMD speedup)
- run entropy / arithmetic decoders (also sequential — see `cabac-rs`)
- do anything outside DSP hot paths

---

## How to plug into a codec (consumer side)

### 1. Add the dependency

```toml
# Cargo.toml of your codec crate
[dependencies]
codec-simd = { git = "https://github.com/ssoj13/codec-simd-rs", branch = "main" }
```

For now this is a git dep.  Once v1.0 hits crates.io, it becomes
`codec-simd = "1"`.

### 2. Call a primitive

```rust
use codec_simd::sat_pack_i16_to_u8;

fn reconstruct_block(prediction: &[u8], residual: &[i16], out: &mut [u8]) {
    // ... your scalar code that produces an i16 buffer ...
    sat_pack_i16_to_u8(residual_buf, out);
}
```

Slices in, slices out.  Length asserts on input.  The right SIMD backend is
selected on first call (once per process).

### 3. (Optional) Inspect what was selected

```rust
println!("codec-simd tier: {}", codec_simd::arch::cpu_level_name(codec_simd::arch::cpu_level()));
```

### 4. (Optional) Force a specific tier for conformance testing

```bash
# Run your codec's conformance tests against EVERY supported tier and assert
# byte-identical output.  This is the single most important test you'll write.
for level in 0 3 5 9 ; do
    CODEC_SIMD_FORCE_LEVEL=$level cargo test --release -- conformance
done
```

If any two tiers disagree, the SIMD impl has a bug.  Bisect with
`CODEC_SIMD_MAX_LEVEL`.

---

## How to add a new primitive (contributor side)

You want to add `mac_widen_i16_to_i32`.  Steps:

### 1. Add the function pointer to `Dispatch`

In `src/dispatch.rs`:

```rust
pub struct Dispatch {
    pub sat_pack_i16_to_u8: unsafe fn(src: *const i16, dst: *mut u8, n: usize),
    pub mac_widen_i16_to_i32: unsafe fn(src_a: *const i16, src_b: *const i16, acc: *mut i32, n: usize),  // <-- new
}
```

### 2. Add a scalar reference impl

In `src/prims_scalar.rs`.  Always do scalar first; it's the correctness
ground truth that every backend is fuzz-compared against.

### 3. Add per-backend impls

`src/prims_sse2.rs`, `prims_avx2.rs`, `prims_neon.rs`, `prims_wasm.rs`.
Each backend file holds **all** primitives for that ISA.  One file per ISA,
not one file per primitive.  Follow jph-rs convention.

If a tier has no specialised impl, just don't install the function pointer
there in `Dispatch::detect()` — it will fall through to the scalar default.

### 4. Wire up `Dispatch::detect()` / `Dispatch::scalar()`

Default → scalar.  Then `if level >= X86_SSE2 { d.f = sse2::f; }`, then
SSSE3, AVX2, ...  Higher tier overwrites lower.

### 5. Add a safe public wrapper in `lib.rs`

```rust
pub fn mac_widen_i16_to_i32(a: &[i16], b: &[i16], acc: &mut [i32]) {
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), acc.len());
    unsafe {
        (dispatch::get().mac_widen_i16_to_i32)(a.as_ptr(), b.as_ptr(), acc.as_mut_ptr(), a.len());
    }
}
```

### 6. Add a cross-tier test

In `tests/cross_tier.rs`.  Generate random input, run through every
available tier via `CODEC_SIMD_FORCE_LEVEL`, assert all outputs match the
scalar reference byte-for-byte.

**Without this test, the primitive doesn't ship.**

---

## Architecture rules (don't break these)

1. **`unsafe` stays inside the crate.**  Public API takes slices, asserts
   sizes, then calls `unsafe`.  Codec consumers never write `unsafe` to use
   `codec-simd`.

2. **One file per (family × ISA).**  `prims_avx2.rs` contains every AVX2
   primitive.  `kernels_mc_avx2.rs` will contain every MC kernel that uses
   AVX2.  Do NOT split into `prims/sat_pack/avx2.rs` style nesting — jph-rs
   tried that and it's harder to grep.

3. **Scalar is always present.**  Every primitive has a scalar fallback;
   `Dispatch::scalar()` is a complete usable implementation on any platform.

4. **Bit-exact across tiers.**  If AVX2 and SSE2 produce different outputs
   for the same input, that's a bug.  No "close enough" floats in this
   crate — primitives are integer.  (Floating-point DSP comes later as a
   separate family with its own bit-exactness contract.)

5. **No `target_feature` leaking into public signatures.**  All
   `#[target_feature(enable="...")]` annotations live on the backend
   functions only.  The dispatch pointer hides them from callers.

---

## Codec → primitive cheatsheet

Which of your codec's hot paths benefit from which primitives.

| Codec hot path | Use these primitives |
|---|---|
| Add residual to prediction, clip to pixel range | `sat_pack_i16_to_u8` |
| 8-tap subpel motion compensation horizontal | `mac_widen_i16_to_i32` (planned), `pack_round_shift` (planned) |
| IDCT butterfly stage | `add_sub_pair_i16`, `mul_high_round_i16` (planned) |
| Loop filter strength test | `abs_diff_u8`, `min_max_u8` (planned) |
| HTJ2K 5/3 lifting | `add_i32_strided`, `shift_round` (planned) |
| 4×4 / 8×8 transpose | `transpose_4x4_i16`, `transpose_8x8_i16` (planned) |

When you need a primitive that's not in the list, add it (see "How to add a
new primitive" above) instead of writing inline `_mm256_*` in your codec
crate.  That's how the SIMD copy-paste happens that this crate is here to
prevent.

---

## Where to handed off

This crate will be picked up by a Windows agent for v0.2 work.  Read this
file top-to-bottom, then `src/prims_avx2.rs` to see the pattern in code.
Pick a primitive from the "planned" lines above, add it across all five
backends, write the cross-tier test, open a PR.
