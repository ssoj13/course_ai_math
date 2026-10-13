# Adding a node type

> This page is a **pointer**. The authoring documentation lives in two places, and both are kept in
> sync with the code:
>
> - **A builtin** (a type shipping inside `nodes-core`) —
>   [`docs/mdbook/src/dev-guide/adding-node-types.md`](../../../docs/mdbook/src/dev-guide/adding-node-types.md).
> - **A node pack from your own crate** (custom port types, the DataBus handle lane, errors,
>   cancellation, progress, injecting into a host, and the migration guide from the old
>   `NodeDef { cook }` API) — [`docs/custom-nodes.md`](../../../docs/custom-nodes.md), with a runnable
>   example: `cargo run -p nodes-editor --example custom_nodes`.

The 30-second version, for a builtin:

1. A node type is an object: `trait NodeType` (`def` / `value_types` / `stage` / `cook`), stored as
   `Arc<dyn NodeType>`. The terse `node!` macro builds one from a name, a category, port lists and a
   cook closure; a type with an inspector schema, a custom port type, a `Late` cook stage or state of
   its own builds its `NodeDef` explicitly and calls `fn_node` / `fn_node_staged`, or implements the
   trait on a struct.
2. Put it in the category's `types()` list in `src/builtins/<category>.rs`; `builtins/mod.rs`
   concatenates every module into the `Builtins` library, which is an ordinary `NodeLibrary`.
3. `cook` returns `CookResult<()>` — an `Err` becomes `NodeStatus::Error(msg)` on the node (red tint +
   toast in the editor). Never emit a sentinel value instead.
4. `def()` is called ONCE and cached: it must be pure. Type/port/attr names may not contain `.`, `[`
   or `]` (box-rs path grammar) — such a type is refused at registration.
5. Add a cook-chain or serialization round-trip test when the behaviour is non-trivial. To cook one
   type with no graph at all, use `CookCtx::for_test(..)`.
