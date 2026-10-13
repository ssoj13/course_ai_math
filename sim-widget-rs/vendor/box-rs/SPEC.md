# box-rs — specification (locked)

Standalone nested container inspired by [python-box](https://github.com/cdgriffith/Box), extended with **structural merge / diff / subtract**.

## Access modes (all supported)

| Mode | Example | Notes |
|------|---------|--------|
| Bracket, single key | `box["a"]` | Always one segment at current level |
| Bracket, path string | `box["a.b.c"]` | When `box_dots = true`, one string = full path |
| Bracket chain | `box["a"]["b"]` | Via `BoxMut` / `BoxRef` nested handles |
| Path API | `box.get_path("a.b[0].c")` | Always parses path; hot loops: `PathKey::parse` once |
| Attr chain | `box.attr("a").attr("b")` | Rust equivalent of `box.a.b` (no dynamic fields) |
| Typed getters | `get_f64`, `set_vec3`, … | Built on path engine |

**Not in Rust:** true `box.a.b` field syntax for arbitrary runtime keys — use `attr()` or `get_path()`.

### Path grammar

```
path     := segment ("." segment | "[" index "]")*
segment  := key | quoted_key
index    := uint
```

- `a.b.c`, `a.b[2].name`, `attrs["weird key"].value`
- Brackets inside keys: escaped or quoted (v1: `["key with dots.here"]` as single segment)

### Performance

- Parse path → `PathKey` (`Arc<[Segment]>` or `SmallVec`) once, reuse for many lookups
- Objects: `BTreeMap<String, BoxValue>` for stable merge/diff order
- No string concatenation per tree level during walk

---

## Box behavior flags (python-box parity)

| Flag | Default | Description |
|------|---------|-------------|
| `conversion_box` | `true` | Invalid identifier keys accessible via attr (mapped to safe form) |
| `box_safe_prefix` | `"x"` | Prefix for keys starting with digit or reserved name |
| `box_duplicates` | `Ignore` | Duplicate attr mappings: `Ignore` / `Warn` / `Error` |
| `default_box` | `false` | Missing keys on read create empty object (recursive) |
| `default_box_create_on_get` | `true` | Create on get vs only on set |
| `default_box_none_transform` | `true` | `null` value → default empty box on get |
| `default_box_no_key_error` | `true` | `pop`/`remove` on missing key: no error |
| `box_dots` | `false` | `box["a.b"]` traverses path, not literal key `"a.b"` |
| `camel_killer_box` | `false` | Store keys as snake_case; attr accepts snake_case for CamelCase keys |
| `frozen_box` | `false` | Immutable after build; lists→tuples semantics via frozen values |
| `box_recast` | `{}` | Per-key or per-path type coercion on set |
| `box_intact_types` | `[]` | Skip auto-conversion to nested `RustBox` for listed type tags |

Reserved attr names (always bracket-only): `get`, `set`, `merge`, `diff`, `keys`, …

---

## Merge — full policy set

`RustBox::merge(&self, other, MergeOptions) -> Result<RustBox, MergeError>`

### `ObjectMergePolicy`

| Variant | Behavior |
|---------|----------|
| `Deep` | Recurse into both objects key-by-key |
| `ShallowReplace` | Top-level keys from `other` replace; no recursion |
| `ReplaceSubtree` | On key conflict, entire right subtree replaces left |
| `MergeKeysPresentInLeft` | Only keys existing in left are updated |
| `MergeKeysPresentInRight` | Only keys from right are inserted/updated |
| `Union` | All keys from both; conflicts use scalar policy |

### `ArrayMergePolicy`

| Variant | Behavior |
|---------|----------|
| `Replace` | Right array replaces left entirely |
| `Concat` | Append right after left |
| `Prepend` | Prepend right before left |
| `MergeByIndex` | Zip by index; longer tail per `ZipTailPolicy` |
| `ZipStrict` | Lengths must match or error |
| `ZipLongest` | Pad shorter with `ZipPadPolicy` |
| `LongestCommonSubsequence` | Align elements by LCS then merge aligned pairs |
| `UnionByKey { key_path }` | Array of objects merged by key field |

### `ZipTailPolicy` / `ZipPadPolicy`

- `KeepLeft`, `KeepRight`, `Truncate`, `FillNull`, `FillDefaultBox`

### `ScalarConflictPolicy`

- `PreferLeft`, `PreferRight`, `Error`, `PreferTypeOfRight`, `PreferTypeOfLeft`, `CoerceToRight`, `CoerceToLeft`, `Custom` — set `MergeOptions.scalar_custom` resolver

### `NullMergePolicy`

- `Ignore`, `SetNull`, `DeleteKey`, `TreatAsMissing`

### `TypeConflictPolicy` (object vs array vs scalar)

- `Error`, `PreferLeft`, `PreferRight`, `ReplaceWithCoercion`, `WrapAsValue`

### `MissingKeyPolicy` (key in left, absent in right)

- `Keep`, `Remove`, `SetNull`

### `MergeMode`

- `Patch` — apply `other` as patch onto clone of `self`
- `Overlay` — `other` wins at roots, then policies apply
- `Symmetric` — 3-way merge; requires `MergeOptions.base` (ancestor) or `RustBox::merge_symmetric(base, other, …)`

---

## Diff — full policy set

`RustBox::diff(&self, other, DiffOptions) -> DiffReport`

### `DiffKind` (what to emit)

- `Added`, `Removed`, `Changed`, `TypeChanged`, `Moved` (array reorder), `Resized` (array length)

### `ScalarComparePolicy`

- `Strict`, `JsonCanonical`, `Numeric { epsilon }`, `StringTrimmed`

### `ArrayDiffPolicy`

- `ByIndex`, `LongestCommonSubsequence`, `HashBy { path_to_key }`

### `ObjectDiffPolicy`

- `Deep`, `KeysOnly`, `WithValues`

### `PathFormat`

- `Dot`, `JsonPointer`, `Segments`

### Output

```rust
DiffReport {
    changes: Vec<Change>,
    patch: RustBox,           // optional apply-able patch
}
Change { path, kind, old, new }
```

---

## Subtract — full policy set

`RustBox::subtract(&self, other, SubtractOptions) -> Result<RustBox, SubtractError>`

| `SubtractMode` | Behavior |
|----------------|----------|
| `KeyDifference` | Remove keys present in `other` (object) |
| `ValueMatch` | Remove keys where values equal per `ScalarComparePolicy` |
| `StructuralMinus` | `self` minus `other` via diff removals only |
| `AntiMerge` | Inverse of merge with same `MergeOptions` |

| `ArraySubtractPolicy` | Behavior |
|-----------------------|----------|
| `RemoveMatchingElements` | Drop elements equal to any in right ( multiset ) |
| `IndexMask` | Remove indices listed in right array |
| `ByDiff` | Use array diff policy, apply removals |

---

## Serde

- JSON round-trip preserves structure
- Optional `#[serde(transparent)]` on root value

## Versioning

Policy enums are **exhaustive** in this spec; new variants only with minor crate bump.
