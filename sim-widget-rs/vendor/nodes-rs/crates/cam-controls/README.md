# cam-controls

Host-agnostic camera rigs: orbit (Houdini turntable), fit/framing, release inertia,
animated transitions, and FPS / spaceflight fly modes. Right-handed, Y-up, `glam`.
Copied from `gitnexus-rs`'s cam-controls.

`nodes-view`'s `Camera` is a facade over this; the reference editor's F1/F2/F3 modes and
the `H` top-down transition are driven by these rigs. Sits on `cam-viewport` for the
pixel/NDC/ray math.

Book: [`../../docs/mdbook`](../../docs/mdbook) → Whitepaper › Camera & navigation.
