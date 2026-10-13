# nodes-core

Headless computational node graph — the single source of truth for graph **data**,
**evaluation** (`cook`), and **layout in graph-UV**. No wgpu, no egui.

A scene is one flat `Subnet` (`HashMap<Uuid, Node>`); each `Node` stores all its data
(inputs/outputs namespaces, connections, view state) in a `box_rs::RustBox`. Provides
`cook`/`cook_dirty`, a `LayoutCache` (node sizes, pin positions, wire endpoints), the
`NodeTypeRegistry` + builtins, and a stable continuous `ForceSim` for live layout.

Consumed headless by `mermaid-rs` and `vfx-rs`, and by every crate above it here.

## Custom node types

A node type is an object — `trait NodeType` (`def` / `value_types` / `stage` / `kind` / `cook`) — and
a `NodeLibrary` is a pack of them: **the unit of injection**. Your crate defines its own nodes, its
own port types (pin colour + coercion rule) and passes heavy payloads through the DataBus handle
lane, then any host registers the pack with one call:

```rust
let (mut registry, mut value_types) = nodes_core::default_registry();  // the builtins
registry.add_library(&MyNodes, &mut value_types);                      // your pack
```

The builtins are themselves a `NodeLibrary` (`Builtins`) — there is no privileged in-crate
registration path. Full guide (+ a migration guide from the old `NodeDef { cook }` API):
[`docs/custom-nodes.md`](../../docs/custom-nodes.md); runnable example:
`cargo run -p nodes-editor --example custom_nodes`.

### Dataflow or Job?

The first thing a type declares (`NodeType::kind()`): a **`Dataflow`** node is a pure function of its
inputs — cheap, and the driver recomputes it automatically. A **`Job`** (`fn_job`) is imperative and
side-effecting; its output is a REFERENCE to work that already happened on disk (a Houdini ROP, a Nuke
Write, a 40-minute reconstruction). **The cook driver never runs a Job** — only `execute` /
`execute_all` do — and a Job's outputs are persisted on save, so a saved graph re-loads clean and
nothing re-runs when someone merely opens it.

## More

- Design notes: [`docs/`](docs/) (`STRUCTURE`, `NODES`, `DATA_BUS`, `ROADMAP`) and `CLAUDE.md`.
- Book: [`../../docs/mdbook`](../../docs/mdbook) → Whitepaper › Data model & cook, and
  Developer Guide › Custom node libraries.
