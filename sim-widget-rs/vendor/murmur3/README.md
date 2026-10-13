# murmur3

MurmurHash3 family in pure Rust (`std` only, zero dependencies):

| Module | Variant | Used by |
|--------|---------|---------|
| [`x64_128`](src/x64_128.rs) | `MurmurHash3_x64_128` | Alembic array-sample digests |
| [`x86_32`](src/x86_32.rs) | `MurmurHash3_x86_32` | Cryptomatte object/material IDs |

## Why

- **Alembic** identifies array samples by hashing raw bytes with MurmurHash3
  x64_128 — digests must match C++ bit-for-bit.
- **Cryptomatte** IDs are MurmurHash3 x86_32 of UTF-8 name bytes (seed `0`),
  then stored as float bits (exponent clamp lives in Cryptomatte code).

One crate, both digests.

## API

```rust
use murmur3::{x64_128, x86_32};

// Alembic — seed None = seedless; pod_size None = no BE swap
let (h1, h2) = x64_128::hash(data, None, None);
let digest16 = x64_128::hash_bytes(data, None, None);

// Cryptomatte — seed 0
let id_u32 = x86_32::hash(name_utf8, 0);
let digest4 = x86_32::hash_bytes(name_utf8, 0);
```

**Breaking change in 0.2:** crate-root `hash128` / `hash128_bytes` moved to
`x64_128::hash` / `x64_128::hash_bytes`.

## Usage

```toml
[dependencies]
murmur3 = { git = "ssh://git@github.com/ssoj13/murmur3.git", branch = "main" }
```

Consumed by `alembic-rs` (`x64_128`) and intended for `cryptomatte-rs` (`x86_32`).

## Build

```bash
python bootstrap.py b   # build
python bootstrap.py t   # test
```

## License

MIT OR Apache-2.0
