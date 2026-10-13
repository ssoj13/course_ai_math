# Node graph model & addressing

Single source of truth for **scene structure**, **identity**, and **Houdini-style paths** in `nodes-core`.

Related: [DATA_BUS.md](./DATA_BUS.md), [STRUCTURE.md](./STRUCTURE.md), [ADDING_NODE_TYPES.md](./ADDING_NODE_TYPES.md).

---

## 1. Scene = one `Subnet` (flat graph, not nested child graphs)

> **Corrected 2026-07-05:** this section previously described compounds as owning a
> separate `child_subnet: Option<Subnet>` graph. That model does not exist in code (zero
> `child_subnet` references anywhere in `nodes-core`/`nodes-editor`). The actual, shipping
> model is a single **flat** `Subnet` for the whole scene; "nesting" is per-node `parent`/
> `children` fields, not a separate graph per compound. See
> `nodes-editor/src/navigation.rs` (`SubnetNavigation`) for the editor-side focus stack.

| Concept | Type | Notes |
|---------|------|--------|
| **Scene** | `Subnet` | The one graph the editor shows; **no fake root node**. |
| **Compound** | `NodeInstance` with `type_name == "Subnet"` | A normal node in the SAME `Subnet`; its children are other nodes whose `parent` field points at it — there is no separate owned graph. |
| **Navigation** | `SubnetNavigation` (editor-only, `compound_stack: Vec<NodeId>`) | Breadcrumb into compounds changes which `parent` the viewport filters by (`current_scope()`); the underlying data is always the one flat `Subnet` — diving in/out never swaps graphs. |

Cook, wires, bus, and JSON all operate on the single flat `Subnet` the same way regardless of nesting depth; cook recursion follows `parent`/`children` links (see `cook.rs::run_subnet_prestage`), not a tree of separate graphs.

---

## 2. Dual identity (fixed contract)

| Role | Mechanism | Used for |
|------|-----------|----------|
| **Stable graph id** | `NodeId` (`Uuid`) | Wires, pick, dirty flags, undo payloads |
| **Human / script id** | `name` (unique among siblings) | Paths, search, future expressions |
| **UI label** | `display_name` | Title bar; may duplicate; **not** used in paths |

**Rules (non‑negotiable):**

1. **Wires store `NodeId` only** — never path strings.
2. **Paths are derived** — computed from `name` segments; not a primary key in saved topology.
3. **Expressions / params** that reference other nodes should resolve path → `NodeId` at bind time, or fail loudly at cook (v2).
4. **Rename** changes path strings but not `NodeId`; undo/redo and files stay stable.

---

## 3. Node names

### Syntax

- Must match `^[A-Za-z_][A-Za-z0-9_]*$` (no `.` — dot starts parm tail).
- Unique **within one `Subnet`** (siblings only); `geo1/box1` and `other/box1` are allowed.

### Auto-naming

On `create_node`, assign `{type_lower}{n}` (`Float` → `float1`, `float2`, …) via `suggest_node_name`.

On **paste / duplicate** (`remap_ids`), uniquify against the target subnet (`float1` → `float2` if taken).

### Legacy files

`SerializedNode.name` optional in JSON. Missing on load → derive from sanitized `display_name` or `{type_lower}_{short_id}`, then `normalize_subnet_names`.

---

## 4. Path grammar

### Op path (node filesystem)

```text
[/][obj/]seg/seg/seg
```

| Token | Meaning |
|-------|---------|
| Leading `/` | Optional; absolute from scene root |
| `obj` | **Reserved** virtual context (v1: stripped, no-op). Future: `/out`, `/mat`. |
| `seg` | `NodeInstance.name` in current subnet |

Examples:

- `/geo1/box1` — node `box1` inside compound `geo1`
- `geo1/box1` — same (absolute when resolved from root)

Empty path `/` is **not** a node (error on resolve).

### Full address (`OpAddress`)

```text
<op-path> [ <tail> ]
```

| Tail | Syntax | Resolves to |
|------|--------|-------------|
| **Node** | (none) | The node |
| **Parameter** | `.` + `PathKey` | `node.params` (rust-box dot path) |
| **Input port** | `.inputs[N]` or `.inputs.name` | Port index by def or name |
| **Output port** | `.outputs[N]` or `.outputs.name` | Port index |
| **Bus slot** | `@slot` | `Subnet.bus` (see §5) |

**Parsing rule for dot:** only the **last path segment** may contain `.`; split at the **first** `.` → node segment + parm tail.  
Example: `/geo1/box1.frequency` → path `/geo1/box1`, parm `frequency`.

Parm tail uses [`PathKey`](../../rust-box/src/path.rs) (`attr`, `inputs[0]`, nested keys).

### Relative paths (editor; v1 API)

Requires [`EditFocus`](../../src/address.rs):

| Syntax | Meaning |
|--------|---------|
| `name` | Child/sibling in **focus subnet** |
| `../name` | Parent subnet |
| `/abs/...` | Absolute from scene root |

Core resolver: **absolute only**. Relative = `resolve_relative(focus, …)` in `address` module.

---

## 5. Data bus & paths

- Each `Subnet` has its own [`DataBus`](../../src/bus.rs).
- **`@slot`** — bus of the **edit-focus subnet** (where the user is in the breadcrumb), not the root unless focus is root.
- v1: no `/geo1/@slot` cross-subnet bus paths (add in v2 if needed).
- Cook order: `BusRead` after other nodes (see DATA_BUS.md).

---

## 6. Compound boundaries

- From **outside** a compound, path stops at the container: `/geo1`, not internal nodes.
- From **inside** (focus in `geo1`), paths are relative to that inner `Subnet`.
- `SubnetInput` / `SubnetOutput` are normal nodes **inside** the child graph; visible in paths only when focus is inside that graph.

---

## 7. Resolution API (`address` module)

```rust
// Parse
parse_op_path(s) -> OpPath
parse_address(s) -> OpAddress

// Absolute (from scene root)
resolve_op(root, &OpPath) -> ResolvedLocation
resolve_address(root, focus_subnet, registry, &OpAddress) -> ResolvedTarget

// Inverse
op_path_of(root, node_id) -> OpPath

// Names
validate_node_name(s) -> Result<()>
suggest_node_name(subnet, type_name) -> String
subnet.rename_node(id, new_name) -> Result<()>
```

`ResolvedLocation` carries `node`, `node_id`, and `owning_subnet` (immediate parent subnet).

---

## 8. Serialization

- `SubnetFile.format_version` stays **1**; new field `SerializedNode.name`.
- Save always writes `name`.
- Load: missing `name` → infer + `normalize_subnet_names`.
- Wires unchanged (`NodeId`).

---

## 9. Diagram adapter (mermaid-rs)

`NodeGraph` (diagram) and `Subnet` (node editor) remain separate until explicitly mapped.

- Path addressing applies to **`nodes-core::Subnet`** first.
- Adapter layer (future): stable `diagram_id` ↔ `OpPath` when syncing mermaid topology.

Do not assume diagram node labels equal `name` without the adapter.

---

## 10. Implementation phases

| Phase | Deliverable | Status |
|-------|-------------|--------|
| **P0** | `name`, auto-name, serde, validate | this PR |
| **P1** | `OpPath` / `OpAddress`, absolute resolve, tests | this PR |
| **P2** | Inspector shows path; rename UI | **done** (`nodes-editor` inspector) |
| **P3** | `EditFocus`, breadcrumb, dive into compounds | **done** (`SubnetNavigation`, Esc/↑/breadcrumb) |
| **P4** | `/obj`/`/out` contexts, expr/ch() | later |

---

## 11. Anti-patterns

- Storing path strings in `Wire` or as sole reference in saved params.
- Using `display_name` in path segments.
- Allowing `.` in node `name`.
- Resolving `@bus` from root when user is dived into a compound (use focus subnet).
