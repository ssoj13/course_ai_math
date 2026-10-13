# box-rs

Standalone nested data container inspired by [python-box](https://github.com/cdgriffith/Box), extended with **structural merge / diff / subtract**.

The main type is `RustBox`, which stores a tree of `BoxValue` and provides a unified API for:

- path-based access (`get_path`, `set_path`, typed getters/setters)
- bracket/attr-style navigation (`box["a"]["b"]`, `box.attr("a").attr("b")`)
- deep merge/diff/subtract with a policy set (see `SPEC.md`)

## Quick start

```rust
use box_rs::{BoxValue, RustBox};

let mut b = RustBox::new();

b.set_path("a.b[0].c", BoxValue::from(1)).unwrap();
b.set_path("count", BoxValue::from(42)).unwrap();

assert_eq!(b.get_i64("a.b[0].c"), Some(1));
assert_eq!(b.get_i64("count"), Some(42));
```

Bracket-chain navigation:

```rust
use box_rs::{BoxValue, RustBox};

let mut b = RustBox::new();
b.set_path("a.b.c", BoxValue::from(42)).unwrap();

assert_eq!(b["a"]["b"]["c"].as_i64(), Some(42));
```

Attr-chain navigation (Rust equivalent of `box.a.b`):

```rust
use box_rs::{BoxValue, RustBox};

let mut b = RustBox::new();
b.set_path("a.b.c", BoxValue::from(7)).unwrap();

let c = b.attr("a").attr("b").attr("c").get();
assert_eq!(c.as_i64(), Some(7));
```

## Paths (path grammar)

Paths support:

- key segments via dots: `a.b.c`
- array indices via brackets: `a.b[2].name`
- quoted keys as a single segment: `attrs["weird key"].value` (internally `it looks like: ["weird key"]`)

In Rust, use:

```rust
use box_rs::RustBox;

let key = RustBox::path_key("a.b[0].c").unwrap();
// If you are in a hot loop, reusing a prepared PathKey avoids repeated parsing.
```

## Configuration (`BoxConfig`)

`RustBox` behavior is controlled by `BoxConfig` (defaults follow python-box-like semantics).

Common flags:

- `conversion_box` (default `true`): enables safe key mapping for attr-style access
- `box_dots` (default `false`): treat `box["a.b"]` as a path, not a literal key
- `camel_killer_box` (default `false`): allow camelCase/snake_case aliasing on reads
- `default_box` (default `false`): create empty objects on missing reads
- `default_box_create_on_get` (default `true`): create missing nodes during navigation/mutation
- `default_box_none_transform` (default `true`): `null` reads transform to an empty box
- `default_box_no_key_error` (default `true`): missing `pop/remove` is not an error
- `frozen_box` (default `false`): immutable box; writes return `BoxError::Frozen`

`BoxConfig` also contains a coercion map `box_recast`.

## Recast (coercion on `set_path`)

Use `box_recast` to coerce incoming values by key/path:

```rust
use box_rs::{BoxConfig, BoxValue, RecastRule, RustBox};

let mut cfg = BoxConfig::default();
cfg.box_recast.insert("count".into(), RecastRule::I64);

let mut b = RustBox::with_config(cfg);
b.set_path("count", BoxValue::from("42")).unwrap();

assert_eq!(b.get_i64("count"), Some(42));
```

## Merge

`RustBox::merge(&self, other, MergeOptions) -> Result<RustBox, MergeError>`

Example:

```rust
use box_rs::{
    MergeOptions, ObjectMergePolicy, ScalarConflictPolicy, RustBox,
};

let mut left = RustBox::new();
left.set_path("a.x", 1.into()).unwrap();
left.set_path("a.y", 2.into()).unwrap();

let mut right = RustBox::new();
right.set_path("a.y", 9.into()).unwrap();
right.set_path("b", 3.into()).unwrap();

let merged = left
    .merge(
        &right,
        MergeOptions {
            object: ObjectMergePolicy::Deep,
            scalar: ScalarConflictPolicy::PreferRight,
            ..Default::default()
        },
    )
    .unwrap();

assert_eq!(merged.get_i64("a.x"), Some(1));
assert_eq!(merged.get_i64("a.y"), Some(9));
assert_eq!(merged.get_i64("b"), Some(3));
```

Merge modes:

- `MergeMode::Patch`: apply `other` as a patch
- `MergeMode::Overlay`: `other` wins at roots, then policies apply
- `MergeMode::Symmetric`: 3-way merge (requires `base`), via `merge_symmetric`

## Diff

`RustBox::diff(&self, other, DiffOptions) -> DiffReport`

`DiffReport` can be applied back to a `RustBox`:

```rust
use box_rs::{DiffOptions, RustBox};

let mut a = RustBox::new();
a.set_path("x", 1.into()).unwrap();

let mut b = RustBox::new();
b.set_path("x", 2.into()).unwrap();
b.set_path("y", 3.into()).unwrap();

let report = a.diff(&b, DiffOptions::default());
report.apply_to(&mut a).unwrap();

assert_eq!(a.get_i64("x"), Some(2));
assert_eq!(a.get_i64("y"), Some(3));
```

## Subtract

`RustBox::subtract(&self, other, SubtractOptions) -> Result<RustBox, SubtractError>`

Typical modes:

- `SubtractMode::KeyDifference`
- `SubtractMode::ValueMatch`
- `SubtractMode::StructuralMinus`
- `SubtractMode::AntiMerge`

## Binary buffers (`BoxValue::Buffer`)

Bulk numeric payloads (point clouds, keypoints, descriptors, dense/splat data) do
NOT belong as `Array(Vec<BoxValue>)` — millions of boxed tree nodes. Store them as
a single **typed binary buffer**:

```rust
use box_rs::{BoxValue, DType, RustBox};

let xyz: Vec<f32> = vec![1.0, 2.0, 3.0,  4.0, 5.0, 6.0]; // 2 points
let bytes: Vec<u8> = xyz.iter().flat_map(|f| f.to_le_bytes()).collect();

let mut scene = RustBox::new();
scene.set_path("points.xyz", BoxValue::buffer(DType::F32, vec![2, 3], bytes)).unwrap();

// Read back (box-rs stays dtype-agnostic; cast the bytes on your side).
let (dtype, shape, raw) = scene.get_path("points.xyz").unwrap().as_buffer().unwrap();
assert_eq!((dtype, shape), (DType::F32, &[2usize, 3][..]));
```

Bytes are opaque + immutable + `Arc`-shared: `clone()` / `deep_freeze_clone` share
the allocation (no MB copy). `DType` mirrors the safetensors dtype set (`U8`, `F32`,
`F64`, `I32`, `F16`, …); `box-rs` never interprets the bytes.

## Serialization — two backends over one `RustBox`

Both are lossless and round-trip-tested; pick per task.

**1. JSON** (always available) — human-readable, full fidelity.

```rust
let json = scene.to_json_value();               // serde_json::Value
let back = RustBox::from_json_value(json, Default::default());
```

A buffer serializes as `{"$buffer":{"dtype":"f32","shape":[2,3],"data":[u8,…]}}`
(a native u8 array — no base64 dep). Great for small scenes / metadata / debugging;
**verbose for large buffers** (each byte is a JSON number), so prefer safetensors for
big point clouds.

**2. safetensors** (feature `safetensors`) — compact binary, mmap-friendly, one file.

```toml
box-rs = { git = "…", package = "box-rs", features = ["safetensors"] }
```
```rust
box_rs::save_safetensors(&scene, "scene.safetensors")?;
let loaded = box_rs::load_safetensors("scene.safetensors")?;
```

Each `Buffer` becomes a real safetensors tensor (typed, 8-byte aligned, inspectable
by numpy/HF); the rest of the box TREE — buffers replaced by `{"$bufref":"<path>"}`
— rides as a JSON string in the safetensors `__metadata__["box"]`. So ONE
`.safetensors` file is a complete, tool-openable scene/checkpoint (→ restart-from-any-
step). Scales to large volumes; the header (metadata) is capped at 100 MB but only
holds the structure — the bulk rides as tensors.

## Consumers (reused across `cglibs`)

`box-rs` is a shared foundational value model. Known consumers:

| Project | Crate(s) | Reference | Use |
|---------|----------|-----------|-----|
| `nodes-rs` | `nodes-core` (+ `nodes-editor`) | git `branch=main` | THE value model of the node graph (port values). |
| `mermaid-rs` | `diagram-adapter` | git `branch=main` | diagram → node-subnet conversion. |
| `otio-rs` | `otio-core`, `otio-schema` | path dep | timeline document/schema values. |
| `vfx-rs` | `usd-shade-graph` | transitive | USD shade-graph parameter values (`Vec3`/`Color`). |
| `colmap-rs` | live scene / checkpoints | git `branch=main` | SfM scene store + `.safetensors` checkpoints (uses the `Buffer` value + `safetensors` feature). |

> **Adding a `BoxValue` variant is a cross-project change.** Consumers that
> `match` on `BoxValue`/`ValueKind` exhaustively (no `_` arm) must gain the new
> arm before they update their pinned `box-rs` rev — notably
> `nodes-rs/crates/nodes-editor/src/inspector_wgpu.rs` (two `match value.kind()`).
> Keep `value.rs` round-trip tests green (see `CLAUDE.md`).

## Full policy specification

All merge/diff/subtract policies, enum variants, and access semantics are exhaustively defined in `SPEC.md`.
