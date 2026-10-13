# nodes-core — headless graph engine (CLAUDE working notes)

> Keep this file current. This crate has no wgpu/egui — it's the data model, cook/executor,
> and serialization every other crate builds on. A mistake here breaks everything downstream.

## What
`Subnet` (= `Graph`), `Node`, cook/executor, addressing (`OpPath`/`OpAddress`), JSON + Rhai
serialization, and the builtin node library (`builtins/*`). No rendering, no interaction —
`nodes-view`/`nodes-editor`/`nodes-egui` all sit on top of this.

## `NodeKind::{Dataflow, Job}` (2026-07-14) — READ BEFORE TOUCHING THE DRIVER OR SAVE/LOAD

The crate used ONE word — *cook* — for two categorically different things, and most of its workarounds
grew from that root:

- **Dataflow**: lazy, automatic, cheap. Output = a VALUE derived from inputs. Recompute is free AND
  equivalent.
- **Job**: imperative, explicit, side-effecting. Output = a REFERENCE to an effect that already happened
  on disk (a path to 40 min of GPU). Recompute is neither free nor equivalent — it RE-DOES the effect.
  Houdini's ROP / Nuke's Write. (colmap's `Mapper`/`DenseStereo`/`Splat`.)

The ROADMAP's *"cook is cheap, rendering dominates"* is true of the first and false of the second; with
no way to say which, the loader marked everything dirty and the editor's per-frame cook re-ran
reconstructions just because someone opened a graph. `NodeType::kind()` names the FACT (never a cost
LABEL — behaviour cannot be derived from a subjective judgement), and everything else follows:

- **The driver NEVER runs a Job.** ONE guard, in `eval_one` (`RunJobs::No`) — and since `eval_one` is the
  single funnel for top-level nodes AND subnet children (`run_staged`), a Job buried in a compound is
  covered for free. `cook`/`cook_dirty`/`cook_with`/`cook_dirty_with` all pass `No`.
- **`execute` / `execute_all` are the only doors** (`execute_with` / `execute_all_with` take `CookOpts`).
  `execute_with` cooks ONLY the job's upstream (`collect_upstream`) — not the whole graph, or its
  consumers would cook a moment before the job produced anything and log a true-but-useless "not
  executed" error — then runs the job (`RunJobs::Yes`), then re-dirties its downstream (`eval_one` cleans
  only the node it ran; consumers may be CLEAN from an earlier pass and would keep serving stale values).
  `execute_all_with` runs only STALE jobs — re-doing a completed one is exactly what the model prevents.
- **Persistence follows from the kind** (`serialize_node_data`): a Job's outputs are WRITTEN to the file,
  a Dataflow node's are blanked. The serializer therefore needs the REGISTRY — threaded into
  `subnet_to_file` / `extract_fragment` / `subnet_to_json_pretty` / `save_subnet_json` (breaking change;
  the load side already took a registry and ignored it). The kind is NOT copied onto node instances: a
  type fact duplicated into every instance is a fact that can go stale.
- **Dirtiness is DERIVED at load**, not stamped: `node_from_data` sets `dirty = !outputs_complete()`.
  A Job whose outputs were restored → clean → nothing runs. A Dataflow node → blanked → dirty → recooked
  (unchanged).
- **A Job's outputs are seeded `Null` at instantiation** (`Node::from_def`, via the `kind` copied onto the
  cached `NodeDef` at `put_entry`), NOT from the port's declared default. This is load-bearing: a job
  output declared `PortDef::string("sparse")` would otherwise be born holding `""`, so a never-run job
  would look complete — it would round-trip CLEAN (never running) and its consumers would compute on an
  empty string. Absence of a value IS the proof a job has not run, so it must be a true absence.
- **A node fed by a never-executed Job FAILS** (`unexecuted_job_input` → `fail_node`), instead of falling
  back to its stored literal as `resolve_inputs` would. A plausible number computed from nothing is worse
  than an error.
- **A stale Job is SHOWN, never auto-run**: `is_stale(node, registry)` = Job && dirty. `jobs_in_order`
  lists them topologically.

### The TWO TRAPS in the dirty rule (do not 'simplify' them away)
1. **A zero-output SINK must be DIRTY.** "No output is missing" is *vacuously true* for `BusWrite` /
   `SaveImage` (no outputs; they run for their side effect), so the naive rule would retire every sink in
   a loaded graph — `BusWrite` never re-publishes, every `BusRead` reads an empty bus, and the whole Data
   Bus feature dies silently across a save/load. `outputs_complete()` is `false` with no outputs: a sink
   carries no evidence it ever ran.
2. **Dirtiness must NOT become a derived GETTER.** `from_def` seeds a DATAFLOW node's outputs with their
   declared defaults (`PortDef::float("sum", 0.0)`), so a fresh `Add` already "has" a value and a derived
   getter would birth it clean and freeze it at 0.0. `view.dirty` stays the ONE source of truth (`dirty()`
   = `unwrap_or(true)`: absent = never cooked = dirty); the derivation is applied ONLY at the load site.

## `CancelToken` exposes its raw flag (2026-07-14)

`CancelToken::as_arc() -> Arc<AtomicBool>` / `CancelToken::from_arc(Arc<AtomicBool>)` (`cook.rs`). We
deliberately declined this at first ("leaks a representation detail for zero gain"); a real consumer
produced the gain within a day. **The `Arc<AtomicBool>` IS the contract, not an implementation detail:**
colmap's stages take `Option<Arc<AtomicBool>>` and poll it INSIDE their loops, so without the raw flag a
cook can only check cancellation at its boundaries and a 40-min splat train ignores Ctrl-C until it
finishes. `from_arc` adopts a flag the host already owns (a Ctrl-C handler's) so there is ONE flag rather
than two that drift. Ordering is unchanged (`Relaxed`, as `cancel`/`is_cancelled` always were).

## Per-port display levels (added 2026-07-08)

Each port has a DISPLAY LEVEL (min view mode to draw it): `0` = shown even collapsed (Minimal),
`1` = from Mid up, `2` = Full only. `NodeViewMode::level()` = 0/1/2. Stored per-INSTANCE in the box
(`view.in_level.<i>` / `view.out_level.<i>`); `Node::port_display_level(side,i)` /
`set_port_display_level(...)`. Unset ports default index-based: FIRST port on a side = 0 (a
collapsed node always shows its primary socket), the rest = 1.

- **`visible_pin_indices`** = ALL ports (layout POSITIONS every pin so wires never dangle; Minimal
  merges them to one point per side). UNCHANGED — do not filter it, or connections to hidden ports
  break.
- **`displayed_pin_indices`** = the SSOT for "which pins to DRAW" (socket + label): filters
  `visible_pin_indices` by `port_display_level(i) <= view.level()`. Used by `nodes-view`
  `shape_scene` (sockets) + `text_labels` (labels). In Minimal, callers still draw only the FIRST
  of these (layout merged all sockets to one point), which is why collapsed nodes stopped showing
  a STACK of overlapping port labels/sockets.
- TODO: no editor UI yet to set levels at runtime (API exists); a level-2 port hidden in Mid still
  reserves node height (cosmetic).

## Node data model (redesigned 2026-07-05 — read this before touching `Node`)

`Node.data` is one `RustBox`, organized into four top-level sections:

```text
type / name / display_name
inputs.<name>   : { id, value, source: null | {node, attr} }   -- authored/connectable
outputs.<name>  : { id, value }                                 -- computed results
input_order     : [name, ...]   -- declared/pin order (a BTreeMap can't preserve it)
output_order    : [name, ...]
compute.<name>  : transient cook-resolved override, ONE cook pass only — never serialized
tmp.<anything>  : transient scratch for other inspectable-but-not-persisted state — never
                  serialized (future: cook timing, mesh/preview buffers)
parent / children
view            : { pos, size, mode, bypass, display, full_h }
```

**`inputs` and `outputs` are separate namespaces.** Before 2026-07-05 there was ONE shared
`attrs.<name>` map for both — a node with the same name on both sides (e.g. a future
`mesh` -> `mesh` deform node) would collide, silently overwriting one port's attr with the
other's (there was a debug-only assert catching this, which only fires when a node is
actually *instantiated*, not at registration — the Debug builtin node hit this the hard way
by using symmetric names before the fix). If you're about to give an input and an output the
same name, you're fine now — that's exactly the case this redesign exists for.

**Everything that affects computation is a connectable input** (Maya-attribute-style) — there
is no separate "unconnectable config" concept. `BusRead`/`BusWrite`'s `slot` and
`MapGet`/`MapInsert`'s `key` used to be bare `data.params.slot`/`data.params.key` string
fields with no attr id (so they could never be wired); they're now regular declared input
ports like any other. If you're adding a node type with an internal "setting" (not just a
computed value), make it an input port, not a bespoke `data.*` field — it gets wireability,
inspector editing, and serialization for free that way.

### `Node` API — which accessor for which side

| Need | Method |
|---|---|
| Read an input's stored/authored value | `input_value(name)` |
| Read an output's last-cooked value | `output_value(name)` |
| Read effective value INSIDE a cook closure (resolves `compute` override first) | `cook_value(name)` / `in_f64`/`in_str`/etc. |
| Write an input's literal value (user edit) | `set_input_value(name, v)` |
| Write an output (cook closures) | `set_out(name, v)` |
| Attr uuid for wiring | `input_attr_id(name)` (consumer side) / `output_attr_id(name)` (source side) |
| Resolve a wire's source attr uuid back to a name | `attr_name_by_id` — searches **outputs only** (wires always source from an output) |
| Wire state | `source_of`/`is_connected`/`set_source`/`clear_source` — **input-only**, outputs are never "connected from" anything |

There is no generic `attr_value`/`set_attr_value` anymore — every call site must know which
side it means. If you're staring at old code/docs mentioning `attrs.<name>` as one bucket,
it's stale; see the migration commit (`refactor(nodes-core): split node attrs into
inputs/outputs/compute/tmp`, 2026-07-05) for the shape of the fix.

### `compute` — replaces the old `cook_inputs` Rust field
Cook-time resolved overrides for connected inputs now live in `data.compute.<name>` (via
`Node::set_compute`/`insert_compute`/`clear_compute`), not an out-of-band `HashMap` on the
`Node` struct. Same lifetime/semantics as before (populated before a cook pass, cleared
after) — just inside the box so future code that walks `data` generically sees it too.

## Port row layout (`chrome.rs`, `flex-geom`, added 2026-07-05)
The pin/label/inline-widget geometry for one port row used to be computed independently in
three places (nodes-core's `chrome.rs`, nodes-view's `text_labels.rs`, nodes-editor's
`node_widgets.rs`) — they silently disagreed, and the inline widget started drawing over any
port label longer than ~2-3 characters. `chrome::port_row_geometry` is now the single
authoritative computation: given a pin's world position + the node's bounds + orientation, it
returns `{ label, widget }` rects via `flex_geom::arrange_hbox`/`arrange_vbox` — nodes-view and
nodes-editor both call it and can no longer drift apart (see `chrome.rs`'s
`label_and_widget_never_overlap_horizontal` test, which encodes the invariant directly).

`flex-geom` is a **separate crate** (git-ref to `wgpu-widgets-rs`, `package = "flex-geom"`) —
the same `arrange_hbox`/`FlexItem` primitives `wgpu-widgets` uses for its own widget layout,
extracted into a crate with only `glam` as a dependency specifically so this crate could use
them without violating "no wgpu, ever" (see this file's header). `wgpu-widgets-core` depends on
`flex-geom` too and re-exports the same types at the same paths, so nothing downstream broke.

Colors are NOT defined here: `category.rs` (`category_accent_rgba`/`category_header_bg_rgba`) now
delegates to the app-wide SSOT `ui-theme` crate (a pure glam+serde leaf, git-refed like
`flex-geom` — so nodes-core stays wgpu-free). Change category colors in `ui-theme`, not here.

`chrome::inline_field_rects(graph, node_id, layout)` (added 2026-07-08) is the SINGLE source of
truth for the inline value-field rects on a node (which input ports show an editable box, and
where). BOTH the editor's widget placement (`node_widgets::sync_ports`) and the GPU pick quads
(`nodes-view::shape_scene`, which stamps `PickTarget::NodeField{node,port}` into the id buffer)
derive from it, so a field's hit rect can never drift from the value box the user sees. It skips
frame nodes / Minimal view and non-widget kinds (Vec/Color/Map/...), and reports `editable=false`
for wire-connected inputs (read-only echo).

`PortRowParams` bundles the tunable knobs (label column width, gaps, widget size) as named
fields instead of scattered constants — override per call if a future node style needs it.
`Node::orientation_pref()`/`set_orientation_pref()` (`view.orientation`, default "auto") is a
real per-node override for horizontal vs. vertical layout, resolved via
`Node::resolved_orientation(w, h)` (falls back to aspect-based `chrome::pin_edge_layout` when
unset). No editor UI exposes this yet (no toggle button) — the mechanism is correct and tested
(including a vertical-orientation non-overlap test) but wiring a manual toggle is a fast
follow-up, not yet done.

## Node rotation SSOT — `NodeXform` (unified 2026-07-08)

One authoritative placement transform per node so the body, pins, wires, inline widgets and text
labels can NEVER desync when a node is rotated (the force sim's `rotate_nodes` writes
`Node::view_rotation`).

- **`LayoutNode.rotation`** (mirrored from `Node::view_rotation` in `build_layout`) + **`LayoutNode::xform()` → `NodeXform { center, rotation }`** = the single source. `NodeXform::point(uv)` rotates a
  graph-UV point about the center; `inverse_point` for hit-testing.
- **Authoring rule:** node-attached geometry is built in the node's UNROTATED LOCAL frame; the ONE
  rotation is applied once at the leaf:
  - `layout` keeps `pin_positions` LOCAL. The WORLD/placed pin position is **`LayoutCache::pin_world(node,side,idx)`** (= `xform.point(local)`) — the SSOT every world-space consumer
    (tube wires in `nodes-view::wire_scene`, `GraphRenderer::last_layout_pin` for the wire-drag
    preview) MUST use, or wires miss the sockets of rotated nodes. `cache.edges` is finalized the
    same way but is NOT read by the tube path (kept only for 2D-line consumers).
  - `nodes-view::shape_scene` stamps `rotation`+`pivot` (from `LayoutNode::xform`) on EVERY node
    instance incl. pins → `shape.wgsl` rotates the drawn quad, SDF stays local; GPU pick uses the
    same VS.
  - `nodes-editor::node_widgets` + `nodes-view::text_labels` place their surfaces at the local
    anchor then post-rotate via the shared `nodes_view::rotate_surface_about(base, pivot, normal,
    angle)` (widgets and labels share this ONE compose).
- **Do NOT** re-fetch `Node::view_rotation` or re-rotate pins in a consumer — always read
  `LayoutNode::xform()`. Adding a new node-attached visual? Author it local, apply the xform once.
- **`drawn_pin_indices(node, side)`** (view_mode.rs) is the SSOT for "which sockets/labels to
  DRAW" (= `displayed_pin_indices` + Minimal→first-only); `shape_scene` and `text_labels` both use
  it so a socket and its label never drift.

## Live layout simulation (`force_sim.rs` + `layout::build_layout_graph`, added 2026-07-08)

The editor's "live" view is now a real, stable **force simulation**, not a per-frame Sugiyama
re-solve + eased tween + clamp (that was a crutch that either exploded or froze).

- **`layout::build_layout_graph(graph, scope) -> (LayoutGraph, Vec<NodeId>)`** is the SSOT that
  turns the editor graph into `graph_layout` engine input (filter `parent()==scope`, non-frame;
  sorted by uuid; seed `pos = view_pos`; socket cross-offsets from `compute_static_layout_scoped`;
  edges via `index.in_edge`). Both the one-shot `auto_layout_subnet_with` (L / GPU) and the live
  `ForceSim` build from it, so "tidy" and "live" can never disagree about nodes/seed/edges. Caller
  must have a fresh `index` (`rebuild_index`).
- **`force_sim::ForceSim`** (pure math, no wgpu) retains node **pos + vel** across frames and
  advances a semi-implicit Euler integrator a few substeps/frame. Stable BY CONSTRUCTION via three
  independent guards, not a clamp: dissipation (`damping<1` → energy decreases), per-component
  speed cap `v_max`, and an annealing `temp` that cools the timestep to a floor (settles instead of
  orbiting; re-heats on disturbance). Force model matches `graph-layout-gpu` (inverse-square
  repulsion + Hooke springs + centroid gravity).
  - Pinning is REAL: `step(dt, graph, pins)` reads pin positions from the graph (a drag feeds in),
    holds their velocity at 0, and lets them anchor the rest — no snapshot/restore hack.
  - `sync(graph, scope)` rebuilds on topology/scope change while preserving survivors' pos/vel (no
    teleport when a node is added/removed). `write_back(graph)` writes only FREE nodes and does NOT
    set dirty (a position change never triggers a cook). `is_settled()` = converged.
  - CPU O(N²) is <1ms at editor scale and needs no GPU↔CPU readback stall; the GPU
    `graph-layout-gpu` engine stays for the one-shot L/GPU relayout and is reserved for >~1k nodes.
  - **`translate` (2026-08-08) decides whether POSITIONS may move at all.** With it off the sim is
    orientation-only: every force term, the integrator and the position write-back are skipped, so
    cards rotate along their wires while the layout the caller arranged stays exactly as it is.
    This is what makes "rotate nodes" and "keep relaxing the layout" two independent choices — they
    were one switch, and asking for rotation dragged the whole graph around. Settling accounts for
    rotation too (the arc a card's corner sweeps, in world units), or an orientation-only run would
    report converged on frame one because nothing translates.
    Covered by `orientation_only_rotates_without_moving_anything`.

## A node type is an OBJECT — `trait NodeType` + `NodeLibrary` (2026-07-14, Increment 2)

`node_type.rs` is the crate's extension point. A node type is no longer a `NodeDef` struct with a
closure stapled to it — it is an implementable trait, so an EXTERNAL compute crate defines its nodes
in its own code and a host injects them with one call.

```rust
pub trait NodeType: Send + Sync + 'static {
    fn def(&self) -> NodeDef;                       // shape. Called ONCE, CACHED. MUST be pure.
    fn value_types(&self) -> Vec<ValueTypeInfo>;    // custom port types this node needs (default: none)
    fn stage(&self) -> CookStage;                   // Main (default) | Late
    fn cook(&self, ctx: &mut CookCtx<'_>) -> CookResult<()>;
}
pub trait NodeLibrary { fn name(&self) -> &str; fn types(&self) -> Vec<Arc<dyn NodeType>>;
                        fn value_types(&self) -> Vec<ValueTypeInfo>; }

pub fn fn_node(def, cook) -> Arc<dyn NodeType>                       // closure adapter (Main, no types)
pub fn fn_node_staged(def, stage, value_types, cook) -> Arc<dyn NodeType>

registry.add(Arc::new(MyNode), &mut value_types);     // ONE type  (+ its value types)
registry.add_library(&MyNodes, &mut value_types);     // a PACK    (+ its value types)
```

- **`registry.add` also registers the type's `value_types()`.** A host that adds a node with a
  `"Recon"` port gets the `"Recon"` value type (coercion + pin visual) without knowing it exists.
- **The registry is the ONE authority per type:** `RegEntry { def, ty }` — the cached shape and the
  behaviour. `get(name) -> Option<&NodeDef>` is unchanged (all data-driven UI reads it);
  `node_type(name) -> Option<&Arc<dyn NodeType>>` is what the cook driver calls; `entry(name)` gives
  both. **`NodeDef.cook` and `NodeDef.stage` are DELETED** — the cook used to live on the def while
  the def lived in the registry (two half-authorities), and a type had nowhere to keep its own state.
- **`stage` lives on the TRAIT**, not on `NodeDef` (only one of the two may exist). `NodeDef` is
  serialized as display data (`schema.rs`) and is what a *display-only* registry parses back; a cook
  ordering property has no business there, while a type that overrides `stage()` naturally sits
  beside its `cook()`. `split_stages` reads `registry.node_type(..).stage()`.
- **`NodeDef` names are `Arc<str>`** (`type_name`/`display_name`/`category`), built with
  `NodeDef::new(type_name, category).with_display(..).with_inputs(..).with_outputs(..)
  .with_schema(..)`. A type created at RUNTIME (plugin manifest, generated pipeline) cannot produce
  a `&'static str` without leaking — `schema.rs` used to `Box::leak` every parsed name, and that hack
  is now gone.
- **Builtins are just a library** (`builtins::Builtins: NodeLibrary`, `register_builtins(reg, types)`
  = `add_library(&Builtins, types)`), and they declare the builtin VALUE types as the library's
  `value_types()`. There is no privileged in-crate registration path. Each `builtins/*` module
  exposes `types() -> Vec<Arc<dyn NodeType>>`.
- **`node!` macro** now BUILDS a type (`node!("Add", "Math", [ins] => [outs], |ctx| ...)` →
  `Arc<dyn NodeType>`) instead of registering into a registry. It covers the schema-less case; a type
  with inspector schema / a custom stage builds its `NodeDef` explicitly and calls `fn_node` /
  `fn_node_staged` (see `builtins/math.rs`, `builtins/bus.rs`).
- **`set_schema` survives** on the registry as the HOST escape hatch (re-skin someone else's type
  without reimplementing it); builtins now declare schema in their own `def()`.
- `CookFn` (the type alias) is **deleted**; `passthrough_cook` is a plain `fn(&mut CookCtx)`.

## The cook contract (REWRITTEN 2026-07-13)

Everything a cook may touch hangs off `CookCtx` (pub fields — a closure routinely needs `ctx.node`
AND `ctx.bus` at once; `#[non_exhaustive]`, so no out-of-crate struct literal):

| field | what |
|---|---|
| `node: &mut Node` | the cooking node (detached from `graph.nodes` for the duration) |
| `graph: GraphView` | read-only `&nodes` + `&index` of everything else |
| `registry` / `types` | node-type + value-type registries |
| `bus: &mut DataBus` | slots **and** the heavy `Arc` handle lane (`ctx.handle::<T>(key)`) |
| `cancel: &CancelToken` | `ctx.check_cancel()?` inside a long loop |
| `progress: &dyn ProgressSink` | `ctx.phase(name, done, total)` |

- **Failure is reported, not swallowed.** `CookError::{Math, Cancelled, Custom}` (+`CookError::msg(e)`
  to wrap any `Display` error). The driver stamps `NodeStatus::{Idle, Cooking, Done{elapsed_s},
  Error(msg)}` into the node's transient `tmp.status` (`Node::status()`, never serialized). A
  **panic** is caught and becomes `Error("panicked: …")` — not the old log-and-pretend.
- **ONLY a node that SUCCEEDED is stamped fresh + clean.** Cancelled, errored and panicked nodes
  produced nothing, so they keep `dirty` and are NOT given a `cooked_gen`: a stamp would lie to the
  M7 freshness signal live previews consume, and a clean flag would stop `cook_dirty` from ever
  retrying (the node would sit stale until the user happened to edit something).
- **`CookCtx::phase(name, done, total)` is the live progress channel** — it writes
  `NodeStatus::Cooking { progress, msg }` into the node's own `tmp.status` AND forwards the report,
  tagged with the node's `NodeId`, to the host's `ProgressSink` (that node id is what lets a host
  draw a PER-NODE progress bar while the node is detached from the graph mid-cook).
- **Staging is declared on the type**, not sniffed from its name: `NodeType::stage() -> CookStage::
  {Main, Late}`. `BusRead` is `Late` (it must see every `BusWrite` of the main pass). The driver's old
  `type_name() == "BusRead"` split is gone — a host bus-reader can now opt in. The Main→Late split
  runs in **every scope**: `run_subnet_prestage` stages the subnet interior through the same
  `run_staged`, so a `BusRead` parented into a compound is not silently one pass stale (it was,
  before 2026-07-14 — `late_stage_is_honoured_inside_a_subnet` pins it).
- **Unit-testing a node type's `cook` out of crate:** `CookCtx::for_test(node, registry, types, bus, cancel)`
  (+`with_graph`/`with_progress`). `#[non_exhaustive]` blocks a struct literal from another crate,
  which would otherwise force a host to register the type and drive a whole `cook()` just to test
  its own closure.
- **`BusRead`/`BusWrite` are ORDINARY cook closures** using `ctx.bus` (`builtins/bus.rs`). They used
  to be hardcoded in the driver, which meant no out-of-crate node type could touch the bus at all.
  A `BusWrite` with **no value** (its `value` attr absent — hand-built node / older JSON) writes
  NOTHING and warns: publishing `Null` there would destroy a slot another node had already filled.
- **The bus is reached by disjoint field borrows** (`let Graph { nodes, index, bus, .. } = &mut
  *graph;`), never `mem::take`: an empty bus is impossible by type and a panicking cook cannot lose
  it. `Graph.bus` is therefore **private** — use `Graph::bus()` / `Graph::bus_mut()`.
- Host entry points: `cook_with` / `cook_dirty_with` take `CookOpts { cancel, progress, observer }`.
  `cook` / `cook_dirty` are thin default-opts wrappers. **`cook_observed` is DELETED** — its job is
  `CookOpts::observer` (`StatusObserver = &mut dyn FnMut(&Node, &NodeStatus)`), which fires on
  start AND finish and carries the real status, not just a name.
- The `node!` macro's cook form is `|ctx| { … Ok(()) }` (one binding, not four). `?` works in a body
  (it sits in a `Result`-returning closure).

## `ValueTypes` is the type SSOT (the §0 fix, 2026-07-13)

`convert.rs` is **DELETED** (with the free `convert::convert` / `convert::compatible` /
`port::type_compatible`). The conversion kernel now lives on the type itself:

```rust
ValueTypeInfo { name: Arc<str>, visual: PinVisual, default: BoxValue, coerce: Option<CoerceFn> }
//   builders: ValueTypeInfo::new(name, visual).with_default(v).with_coerce(f)
ValueTypes::convert(&self, v: &BoxValue, from: &str, to: &str) -> Option<BoxValue>  // the SOURCE type is part of the decision
ValueTypes::compatible(&self, from: &str, to: &str) -> bool
ValueTypes::make_default(&self, ty: &str) -> BoxValue   // reads the REGISTERED default
ValueTypes::port_def(&self, name: &str, ty: &str) -> PortDef  // port seeded from it
```

**The bug this fixes:** `convert()` was a hardcoded `match` on the type NAME ending in `_ => None`,
while `compatible()` passed anything with `from == to`. So a HOST-registered port type (e.g.
`"Recon"`) **connected fine but had its value silently DROPPED on every cook** (`resolve_inputs` →
`convert` → `None` → `log::warn` → the consumer kept its own literal). `"Image"` only worked because
someone hardcoded an arm for it in this crate.

- `coerce: Some(f)` — `f(v)` decides (and performs) the cross-type conversion. All builtins carry
  their exact previous semantics as closures in `register_builtin_types` (Map/Image stay
  identity-only, String accepts everything, …).
- `coerce: None` — **identity type**: its own values pass through unchanged and NOTHING else
  converts into it. The right default for an opaque host payload (a mesh, a reconstruction handle).
- `"Any"` is short-circuited before the lookup (accepts + forwards everything). An **unregistered**
  target type converts to nothing — fail loud, never silently forward.
- **`convert` MUST know the SOURCE type**, and this hole bit us TWICE. First: the old `convert.rs`
  matched hardcoded type NAMES and ended `_ => None` (the bug above). Then, with the kernel moved but
  the signature still `convert(v, to)`, an identity type (`coerce: None`) returned `Some(v.clone())` for
  ANY value — so an `Any`-output node wired into a custom port smuggled a Float in wearing the custom
  type's name. An `Any` SOURCE is now judged by the VALUE's shape against the target type's registered
  prototype (`ValueTypeInfo::default`), which is exactly why that prototype must be REAL and never
  `Null` (`reseed_port_defaults` guarantees it). Do not let `convert` and `compatible` drift apart again.
- **A type's DEFAULT is part of the type** (`ValueTypeInfo.default`, 2026-07-14), not a hardcoded
  name-match. `compatible` probes `from`'s registered default through `to`'s coerce closure — the
  same kernel cook runs on the real value. While the default was still the free
  `values::default_value` match, every HOST type got `Null`, so `compatible("Recon","Mesh")` probed
  `Null`, `Mesh`'s kernel rejected it, and the wire was **refused** although every real value would
  have flowed — the mirror image of the value-drop bug. Give a host type a REPRESENTATIVE default
  (`.with_default(...)`), not `Null`, unless its values genuinely are absent-until-cooked.
  - `values.rs` survives only as the **crate-private builtin seed table**
    (`values::builtin_default`); there is no public `nodes_core::default_value` anymore. Use
    `ValueTypes::make_default(ty)`, and `ValueTypes::port_def(name, ty)` to declare a port of a HOST
    type (`PortDef::typed` has no registry and can only know builtin defaults).
- `Graph::connect` / `connect_idx` therefore take **`&ValueTypes` as a new last argument** (they
  gate on `ValueTypes::compatible`), and `resolved_input_value` takes one too. `ValueTypeInfo.name`
  is `Arc<str>` (was `&'static str`) and `PortDef::new` takes `&str` (was `&'static str`), so a type
  name can come from a plugin manifest at runtime.

## Cook / executor (`cook.rs`)
- `cook` (full graph), `cook_dirty` (dirty + downstream only — editor hot path), `mark_dirty`.
- Nested subnets: flat single `Graph`, nesting via per-node `parent`/`children` — **not**
  separate child graphs (an older design doc said otherwise; the code is the source of truth
  and is the best-tested subsystem in the repo — see `tests/nested_subnet.rs`).
  `run_subnet_prestage` seeds `SubnetInput`, topo-cooks children, reads `SubnetOutput`.
- **`Subnet` constructor** (`builtins::subnet::create_subnet`, re-exported as
  `nodes_core::create_subnet`, added 2026-07-08): creates a `Subnet` node ALREADY populated with
  a wired `SubnetInput -> SubnetOutput` pass-through pair (Houdini-style), so a fresh compound
  forwards data out of the box instead of cooking to an empty bag. The raw `Graph::create_node`
  primitive stays able to make a BARE subnet (paste/scripting/low-level tests build the interior
  by hand) — the "seed default I/O" knowledge lives in the subnet builtin module, not the generic
  container. The `Subnet` node type sits in category `"Subnet"` (with its I/O helpers) in the TAB
  menu.
- Incompatible-type wire: `ValueTypes::convert` returns `None` → `log::warn!`, keeps the stored
  literal (never silently forwards an incompatible value). A `Null` source output is ABSENT data,
  not a mismatch — it is skipped without a warning.

## What a tick costs, and the rules that keep it there (2026-08-06)

`tests/cook_cost.rs` measures the driver against a chain of nodes that only
forward their input, so nearly all of what it reports IS the driver. Current
reading: **~11.6 us per node** — ~9.6 us bookkeeping, ~2 us port access. Payload
size does not affect it (a 100x bigger payload costs ~1.0x); that ratio is
asserted, so a stray deep copy fails the suite instead of quietly costing speed.

It was 24.6 us before this. Two rules got it there, and they are rules, not
one-off edits:

- **A node receives its inputs once and cooks once per tick, so the driver must
  touch its box a fixed small number of times.** `set_status` used to make four
  separate path writes, and the driver calls it twice per node — eight walks from
  the root to record four scalars. It now navigates to `tmp.status` once and
  writes the fields into the map it lands on. Anything writing several siblings
  should do the same.
- **A constant path must not be formatted and parsed on a hot path.** Parsing
  allocates a `String` per segment plus the segment array, outweighing the lookup
  it precedes. Use `PathKey::from_keys` (infallible — no `unwrap` on a literal)
  with `set_path_key` / `get_path_key`. `node.rs` keeps `P_STATUS`,
  `P_COOKED_GEN`, `P_VIEW_DIRTY`, `P_COMPUTE` as statics; `port_path(section,
  name, field)` builds the dynamic ones.

**Do not "optimise" further by indexing the section map directly.** The node box
runs with `conversion_box: true`, so a path-hostile port name (`0`) is stored as
`x0` and read back through that fallback — a raw `map.get(name)` would miss
exactly the ports needing the transform. A pre-built `PathKey` gets the speed
without the semantics change.

The payload is cheap to move because box-rs 0.3 shares `Array`/`Object`
interiors copy-on-write: a clone is a refcount bump. Consequence worth knowing:
`as_object_mut` on a value the source node still holds **does** copy (that is
what keeps a clone independent), so a node that mutates its input pays for one
copy where a pass-through pays nothing.

## Addressing (`address.rs`)
`OpAddress` parses `.inputs[N]`/`.inputs.name` and `.outputs[N]`/`.outputs.name` as distinct
tail variants (`AddressTail::Input`/`Output`) — this already matched the inputs/outputs split
conceptually before the storage was actually split; see `docs/NODES.md` for the full grammar.

## Serialization
- JSON (`serialization.rs`): fully generic (`RustBox::to_json_value`/`from_json_value`), no
  per-field code — `serialize_node_data` only special-cases blanking output *values* and
  stripping `view.dirty`/`compute`/`tmp` before save (all transient).
- Rhai (`script.rs`): NOT generic — a deterministic script emits explicit builder calls
  (`node`, `view`, `attr_in`/`attr_out`, `input`/`output`, `set_source`, `parent`, `bus`).
  Adding a new top-level `data` field means adding a builder call here too, or it silently
  won't round-trip through Rhai (JSON will still be fine since it's generic — this is the
  one place that needs a matching manual update).

## Backdrop nodes — `Frame` + `Note` (`is_backdrop`, Note added 2026-07-12)
Two bare passthrough node types (no sockets, user-resizable box): `Frame` (translucent backdrop,
hollow-body pick) and `Note` (a sticky note — solid card that draws its own multi-line text).
`is_frame_node` / `is_note_node` are the per-type predicates; **`is_backdrop`** (frame||note) is used
at every SHARED site — no pins (`visible_pin_indices` / `pin_row_count`), user size in
`compute_node_size`, no view-mode ladder (`set_view_mode_and_resize`), excluded from
`build_layout_graph` / the force sim, no inline fields (`chrome::inline_field_rects`). Fill / pick /
text still branch per CONCRETE type in nodes-view (a Frame is hollow+translucent, a Note is solid and
draws text). The Note's free text is **`Node::note_text`** (`data.note_text`, a plain string):
generic JSON serialize is free, but rhai is NOT generic — `script.rs` has a matching `note(id,text)`
builder or the text drops on a rhai round-trip (`rhai_preserves_note_text` guards it). `create_node`
seeds `DEFAULT_NOTE_W/H` + a placeholder. On-board text is rendered + edited by the editor's widget
layer via `wgpu_widgets::MultilineEdit` — see nodes-view / nodes-editor CLAUDE.

## A worker that dies must be visible (`worker.rs`, 2026-08-08)

`WorkerSet` keys by `NodeId` and stops a worker when its node is deleted — correct, and unchanged.
What was missing was any way to SEE one fail: the slot stays installed, `has(id)` keeps answering
`true`, and a poller whose socket died draws exactly like a healthy node.

The worker is the only thing that knows, so it is the one asked: `NodeWorker::health() ->
Result<(), String>`, defaulted to `Ok` so existing workers are untouched, and `WorkerSet::failed()`
for a host to poll and tint from. Whether to RESTART a broken worker is left to the driver on
purpose — otherwise every node author writes the same retry loop.

**What is NOT a `NodeWorker`:** a worker with a second consumer, or one whose lifetime is not a
node's, is a HOST SERVICE. Own it beside the graph and hand the node what it needs at cook time. Do
not widen the map to a name-keyed one to make those fit — the `NodeId` key is exactly what stops
unrelated nodes from sharing a thread by accident. Both shapes showed up in cryptobot (an inference
engine serving a node AND a node-less chat; a venue order-watch following the connection).

## Bypass is PASS-THROUGH (2026-08-08)

A bypassed node copies its inputs to its outputs; it does not simply skip running.

It used to be `if bypass { return; }`, which left the outputs holding the last real cook's result.
For a node that TRANSFORMS a value — one `map` in, one `map` out, which is every stage in a
pipeline like cryptobot's — that is not "switch this step off", it is "freeze this step's last
answer and keep republishing it". Downstream then rebuilds from a snapshot of the past on every
tick and looks like it is running while never advancing: the same failure the driver already logs
loudly for an errored node, but silent.

- **Pairing is by TYPE, in order**: each output takes the first unused input of the same port type.
  One-in-one-out (nearly everything) is just "in becomes out".
- An output with **no matching input is left alone** — there is nothing honest to put there, and a
  default would be a fabricated value rather than a missing one.
- Values come from `resolve_inputs`, i.e. what arrived ON THE WIRES this tick, not the node's
  stored literals — republishing those was the original bug in another form.
- The node is stamped `Done` and clean, because for everything downstream it is: its outputs are
  current.

Covered by `cook::tests::a_bypassed_node_passes_its_input_through`, which pins the difference by
making the two answers different numbers (`a + b` cooked, `a` passed through).

## Gotchas
- `BoxValue::Object` is a `BTreeMap` — **sorted, not insertion order**. That's exactly why
  `input_order`/`output_order` exist as separate arrays; don't try to derive port order from
  iterating `inputs`/`outputs` directly.
- `RustBox::set_path` auto-vivifies missing intermediate objects (`navigate_mut(..., create:
  true)` in box-rs) — you don't need to pre-initialize a nested section before writing into
  it, though `Node::from_def` does anyway for clarity/consistency with the rest of its fields.
- Never reuse `let _ = fallible_call()` to swallow a `Result` you actually care about — this
  bit us twice this session (output-value blanking on save silently broke after a path rename
  because the error was discarded; a bare `let _ =` on `connect_idx` in the editor hid wire
  rejections from the user for a long time).

## TODO
- [ ] **`SaveImage` is a `Dataflow` node that writes a file** (`builtins/image.rs`, registered with
      `fn_node`; its own comment says "it runs for its side effect during cook"). That is precisely the
      shape `NodeKind::Job` exists for, and it interacts badly with the zero-output-sink dirty rule: a
      sink is always dirty on load, so opening a graph containing a wired-up `SaveImage` lets the
      editor's per-frame cook re-write the file uninvited. `fn_job` is the obvious fix (a human would
      then press Execute), but it is a BEHAVIOUR change for anyone relying on save-on-cook — decide
      deliberately, do not just flip it. `BusWrite`, the other sink, is genuinely fine as Dataflow
      (re-publishing to the bus is idempotent and is what the dirty rule exists to preserve).
- [ ] No builtin node actually exercises a same-named input/output yet (the split just makes
      it *possible* now) — a `mesh -> mesh` style node would be the first real user.
- [ ] Parameter expressions (rhai `expr.rs`/`script.rs` exist) aren't surfaced in the
      `nodes-editor` inspector yet — inputs only take literals or wires today.
