//! Layered ("Sugiyama-style") layout engine.
//!
//! Pipeline (see crate-level docs for the rationale and the v2 backlog):
//! 1. **Axis abstraction** — work in *primary* (flow) / *cross* (perpendicular) coords;
//!    [`Direction`] maps them to screen x/y at the boundary.
//! 2. **Components** — weakly-connected components are laid out independently and stacked
//!    along the cross axis (deterministic order).
//! 3. **Cycle break** — DFS (deterministic) reverses back-edges to get a DAG; reversed edges
//!    are flagged `backward` in the output routes.
//! 4. **Ranking** — longest-path layering on the DAG; layer primary-coord = cumulative max
//!    node primary-size + `layer_gap` (size-aware columns).
//! 5. **Ordering** — barycenter sweeps reduce crossings; initial order seeded from current
//!    cross positions for incremental stability.
//! 6. **Cross placement** — each node is pulled toward the *mean* of ALL its neighbours' pin
//!    attachment points (predecessors and successors together, so connected pins line up and
//!    a multi-input node settles *between* its inputs), blended with the seed position by
//!    `stability`, then packed drift-free (pool-adjacent-violators) so **no two nodes
//!    overlap**. `pinned` nodes stay fixed and act as anchors.
//!
//! v1 scope: no dummy nodes / long-edge waypoints (we draw beziers), no full Brandes–Köpf,
//! no network-simplex — those live behind the same [`LayoutEngine`] trait later.

use std::collections::HashMap;

use super::{Direction, GraphLayout, LayoutEngine, LayoutGraph, LayoutParams};

/// Layered layout engine. See module docs.
#[derive(Debug, Clone, Copy, Default)]
pub struct SugiyamaLayout;

/// Per-node working data in axis-abstracted (primary/cross) space.
struct Work {
    id: u64,
    psize: f32,
    csize: f32,
    pseed: f32,
    cseed: f32,
    in_off: Vec<f32>,
    out_off: Vec<f32>,
    pinned: bool,
}

/// A directed edge in index space, with pin indices.
#[derive(Clone, Copy)]
struct E {
    /// DAG-direction endpoints (may be swapped vs. the original edge if reversed to break a
    /// cycle). Use [`orig_from`]/[`orig_to`] to recover the original source/target.
    u: usize,
    v: usize,
    /// Original source's OUTPUT socket index.
    fp: usize,
    /// Original target's INPUT socket index.
    tp: usize,
    /// `true` if this edge was reversed during cycle breaking.
    rev: bool,
}

impl LayoutEngine for SugiyamaLayout {
    fn layout(&self, graph: &LayoutGraph, params: &LayoutParams) -> GraphLayout {
        let n = graph.nodes.len();
        if n == 0 {
            return GraphLayout::default();
        }

        // --- 1. Axis extraction: (psize,csize,cseed) per node for the chosen direction. ---
        // Pins (`inputs`/`outputs`) are cross-axis offsets supplied by the host per direction.
        let lr = params.direction == Direction::LeftRight;
        let work: Vec<Work> = graph
            .nodes
            .iter()
            .map(|nd| {
                let (psize, csize) = if lr { (nd.size[0], nd.size[1]) } else { (nd.size[1], nd.size[0]) };
                let (pseed, cseed) = if lr { (nd.pos[0], nd.pos[1]) } else { (nd.pos[1], nd.pos[0]) };
                // Sanitize host input: a non-finite size/position would poison the whole
                // layout (NaN propagates through mean()/PAVA so blocks never merge → overlaps).
                let fin = |v: f32| if v.is_finite() { v } else { 0.0 };
                Work {
                    id: nd.id,
                    psize: fin(psize),
                    csize: fin(csize),
                    pseed: fin(pseed),
                    cseed: fin(cseed),
                    in_off: nd.inputs.clone(),
                    out_off: nd.outputs.clone(),
                    pinned: nd.pinned,
                }
            })
            .collect();

        // id -> index (built once; algorithm internals use indices for determinism + speed).
        let mut idx: HashMap<u64, usize> = HashMap::with_capacity(n);
        for (i, nd) in graph.nodes.iter().enumerate() {
            idx.insert(nd.id, i);
        }

        // --- 2. Edges in index space (drop self-loops + edges to unknown ids). ---
        let mut edges: Vec<E> = Vec::with_capacity(graph.edges.len());
        for e in &graph.edges {
            let (Some(&u), Some(&v)) = (idx.get(&e.from), idx.get(&e.to)) else {
                continue;
            };
            if u == v {
                continue;
            }
            edges.push(E { u, v, fp: e.from_pin, tp: e.to_pin, rev: false });
        }

        // --- 3. Weakly-connected components (union-find over undirected edges). ---
        let mut comp = Components::new(n);
        for e in &edges {
            comp.union(e.u, e.v);
        }
        // Group node indices by component root; deterministic order: by smallest node id.
        let mut by_root: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            by_root.entry(comp.find(i)).or_default().push(i);
        }
        let mut comps: Vec<Vec<usize>> = by_root.into_values().collect();
        for c in &mut comps {
            c.sort_by_key(|&i| work[i].id);
        }
        comps.sort_by_key(|c| work[c[0]].id);

        // --- Solve each component independently, then stack along the cross axis. ---
        let mut primary = vec![0.0f32; n]; // top-left primary coord
        let mut cross = vec![0.0f32; n]; // top-left cross coord
        let mut cross_cursor = 0.0f32;
        for c in &comps {
            solve_component(c, &edges, &work, params, &mut primary, &mut cross);
            let (mut cmin, mut cmax) = (f32::INFINITY, f32::NEG_INFINITY);
            for &i in c {
                cmin = cmin.min(cross[i]);
                cmax = cmax.max(cross[i] + work[i].csize);
            }
            if !cmin.is_finite() {
                continue;
            }
            // Components holding pinned (anchored) nodes keep their absolute cross — we must
            // not slide a local "tidy" component off its fixed anchors. Free components are
            // packed into successive cross bands so they never overlap.
            let has_pinned = c.iter().any(|&i| work[i].pinned);
            if has_pinned {
                cross_cursor = cross_cursor.max(cmax + params.layer_gap);
            } else {
                let shift = cross_cursor - cmin;
                for &i in c {
                    cross[i] += shift;
                }
                cross_cursor = cmax + shift + params.layer_gap;
            }
        }

        // --- Transform primary/cross back to screen x/y and emit positions. ---
        let mut positions = HashMap::with_capacity(n);
        for i in 0..n {
            let pos = if lr {
                [primary[i], cross[i]]
            } else {
                [cross[i], primary[i]]
            };
            positions.insert(work[i].id, pos);
        }
        // Pinned nodes are anchors: return their exact original position regardless of the
        // pipeline (movable nodes were placed around them).
        for nd in &graph.nodes {
            if nd.pinned {
                positions.insert(nd.id, nd.pos);
            }
        }
        GraphLayout {
            positions,
            routes: Vec::new(),
        }
    }
}

/// Lay out one weakly-connected component into `primary`/`cross` (index-keyed, top-left).
fn solve_component(
    comp: &[usize],
    edges: &[E],
    work: &[Work],
    params: &LayoutParams,
    primary: &mut [f32],
    cross: &mut [f32],
) {
    if comp.is_empty() {
        return;
    }
    // Restrict edges to this component (it is weakly connected, so both ends are inside).
    let cset: std::collections::HashSet<usize> = comp.iter().copied().collect();
    let cedges: Vec<E> = edges.iter().copied().filter(|e| cset.contains(&e.u)).collect();

    // --- Cycle break: deterministic DFS, reverse back-edges to get a DAG. ---
    let dag = break_cycles(comp, &cedges);

    // Adjacency (DAG) in both directions.
    let mut succ: HashMap<usize, Vec<E>> = HashMap::new();
    let mut pred: HashMap<usize, Vec<E>> = HashMap::new();
    let mut indeg: HashMap<usize, usize> = comp.iter().map(|&i| (i, 0usize)).collect();
    for e in &dag {
        succ.entry(e.u).or_default().push(*e);
        pred.entry(e.v).or_default().push(*e);
        *indeg.get_mut(&e.v).unwrap() += 1;
    }

    // --- Ranking: longest path via Kahn's topological order. ---
    let mut rank: HashMap<usize, usize> = comp.iter().map(|&i| (i, 0usize)).collect();
    let mut queue: Vec<usize> = comp.iter().copied().filter(|i| indeg[i] == 0).collect();
    queue.sort_by_key(|&i| work[i].id); // deterministic
    let mut deg = indeg.clone();
    let mut qi = 0;
    while qi < queue.len() {
        let u = queue[qi];
        qi += 1;
        if let Some(es) = succ.get(&u) {
            // deterministic neighbour order
            let mut es2 = es.clone();
            es2.sort_by_key(|e| work[e.v].id);
            for e in es2 {
                let r = rank[&u] + 1;
                if r > rank[&e.v] {
                    rank.insert(e.v, r);
                }
                let d = deg.get_mut(&e.v).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push(e.v);
                }
            }
        }
    }

    // --- Layers: group component nodes by rank. ---
    let max_rank = comp.iter().map(|&i| rank[&i]).max().unwrap_or(0);
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for &i in comp {
        layers[rank[&i]].push(i);
    }
    // Initial order within each layer: by seed cross position, then id (incremental stability).
    for layer in &mut layers {
        layer.sort_by(|&a, &b| {
            work[a].cseed
                .total_cmp(&work[b].cseed)
                .then_with(|| work[a].id.cmp(&work[b].id))
        });
    }

    // --- Crossing reduction: barycenter sweeps (down using preds, up using succs). ---
    for _ in 0..4 {
        for r in 1..layers.len() {
            reorder_by_barycenter(&mut layers, r, &pred, |e| e.u, work);
        }
        for r in (0..layers.len().saturating_sub(1)).rev() {
            reorder_by_barycenter(&mut layers, r, &succ, |e| e.v, work);
        }
    }

    // --- Primary coords: cumulative max primary-size per layer + layer_gap. ---
    let mut layer_primary = vec![0.0f32; layers.len()];
    let mut acc = 0.0f32;
    for (r, layer) in layers.iter().enumerate() {
        layer_primary[r] = acc;
        let max_p = layer.iter().map(|&i| work[i].psize).fold(0.0f32, f32::max);
        acc += max_p + params.layer_gap;
    }
    for (r, layer) in layers.iter().enumerate() {
        for &i in layer {
            primary[i] = if work[i].pinned { work[i].pseed } else { layer_primary[r] };
        }
    }

    // --- Cross placement: pull each node toward the MEAN of ALL its neighbours' socket
    // attachment points (predecessors AND successors every pass), blend with the seed by
    // `stability`, then pack each layer drift-free so nodes never overlap.
    //
    // Both directions are considered together (rather than alternating pred-only / succ-only
    // passes) to kill a parity artefact: a one-directional final pass would straighten ONE
    // side of a multi-input node and strand the other (the far input dangling). The symmetric
    // mean, combined with drift-free packing, settles a node with inputs from different ranks
    // at the balance point *between* its inputs — so the bend it makes up on one socket is
    // cancelled by the bend down on the other.
    let s = params.stability.clamp(0.0, 1.0);
    // Seed cross from the seed positions.
    for &i in comp {
        cross[i] = work[i].cseed;
    }
    for pass in 0..8 {
        // Alternate sweep direction each pass for faster, symmetric convergence (Gauss-Seidel).
        let rng: Vec<usize> = if pass % 2 == 0 {
            (0..layers.len()).collect()
        } else {
            (0..layers.len()).rev().collect()
        };
        for r in rng {
            let order = &layers[r];
            // Desired cross per node = mean of every incident edge's socket-alignment target.
            let mut desired: Vec<f32> = Vec::with_capacity(order.len());
            for &i in order {
                // Align this node's socket to the neighbour's socket (reversal-safe):
                //   cross[self] + self_socket == cross[other] + other_socket
                let mut targets: Vec<f32> = Vec::new();
                if let Some(es) = pred.get(&i) {
                    for e in es {
                        targets.push(cross[e.u] + sock_off(work, e, e.u) - sock_off(work, e, i));
                    }
                }
                if let Some(es) = succ.get(&i) {
                    for e in es {
                        targets.push(cross[e.v] + sock_off(work, e, e.v) - sock_off(work, e, i));
                    }
                }
                let base = if targets.is_empty() { cross[i] } else { mean(&targets) };
                desired.push((1.0 - s) * base + s * work[i].cseed);
            }
            // Layers with `pinned` anchors use the anchor-respecting sweep; the common
            // (fully movable) case uses the drift-free centered pack.
            if order.iter().any(|&i| work[i].pinned) {
                pack_layer(order, &desired, work, params.node_gap, cross);
            } else {
                pack_layer_centered(order, &desired, work, params.node_gap, cross);
            }
        }
    }
}

/// Reorder `layers[r]` by the barycenter of each node's neighbours in the adjacent layer.
fn reorder_by_barycenter(
    layers: &mut [Vec<usize>],
    r: usize,
    adj: &HashMap<usize, Vec<E>>,
    other: impl Fn(&E) -> usize,
    work: &[Work],
) {
    // Position of each node within its current layer (for barycenter).
    let mut pos: HashMap<usize, f32> = HashMap::new();
    for layer in layers.iter() {
        for (p, &i) in layer.iter().enumerate() {
            pos.insert(i, p as f32);
        }
    }
    let mut keyed: Vec<(f32, u64, usize)> = layers[r]
        .iter()
        .enumerate()
        .map(|(p, &i)| {
            let bary = match adj.get(&i) {
                Some(es) if !es.is_empty() => {
                    // Port-aware: bias each edge's contribution by the rank of the socket it
                    // uses on the FIXED neighbour, so a parent feeding the upper socket sorts
                    // above one feeding the lower socket. The fraction stays in (0,1) so it
                    // only breaks ties among edges to the SAME neighbour and never reorders
                    // distinct neighbours.
                    let sum: f32 = es
                        .iter()
                        .map(|e| {
                            let o = other(e);
                            pos.get(&o).copied().unwrap_or(0.0) + sock_frac(work, e, o)
                        })
                        .sum();
                    sum / es.len() as f32
                }
                // No neighbours: keep current position (stable).
                _ => p as f32,
            };
            (bary, work[i].id, i)
        })
        .collect();
    // Sort by barycenter, tie-break by id for determinism.
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    layers[r] = keyed.into_iter().map(|(_, _, i)| i).collect();
}

/// Place nodes of one layer (in `order`) so each sits as close to `desired` as possible while
/// never overlapping (size-aware gap), honoring `pinned` anchors fixed at their seed cross.
/// A single forward sweep guarantees the no-overlap invariant. Used for layers that contain a
/// `pinned` node; fully movable layers use [`pack_layer_centered`], which avoids the
/// directional drift a one-sided sweep accumulates across passes.
fn pack_layer(order: &[usize], desired: &[f32], work: &[Work], gap: f32, cross: &mut [f32]) {
    let mut lo = f32::NEG_INFINITY;
    for (k, &i) in order.iter().enumerate() {
        let c = if work[i].pinned {
            work[i].cseed
        } else {
            desired[k].max(lo)
        };
        cross[i] = c;
        lo = c + work[i].csize + gap;
    }
}

/// Place nodes of one layer (in `order`) at their `desired` cross positions while keeping them
/// ordered and non-overlapping (size-aware gap), **minimizing total squared displacement**
/// from `desired` (pool-adjacent-violators / isotonic-with-separation).
///
/// Unlike [`pack_layer`]'s one-sided sweep — which can only push nodes one way and therefore
/// makes a layer creep in that direction across the alternating placement passes — this
/// centers each maximal run of colliding nodes on the mean of its members' desired positions.
/// That keeps a long-edge leaf (e.g. a far input feeding a sink) at its natural height instead
/// of being shoved away, which is what lets multi-input nodes settle *between* their inputs.
///
/// Assumes no `pinned` nodes in `order` (pinned layers use [`pack_layer`]). Empty ⇒ no-op.
fn pack_layer_centered(order: &[usize], desired: &[f32], work: &[Work], gap: f32, cross: &mut [f32]) {
    /// A maximal run of nodes forced together by the no-overlap constraint. `sum_adj / count`
    /// is the optimal top-left of the run's FIRST member (mean over members of
    /// `desired - intra`, where `intra` is a member's offset from the first member). `span` is
    /// the run's extent: first member's top-left → the next free slot (Σ of `csize + gap`).
    struct Block {
        first: usize, // index into `order` of the block's first member
        len: usize,   // number of members (contiguous in `order`)
        sum_adj: f32, // Σ (desired[k] - intra[k]) over members
        count: f32,
        span: f32,
    }
    let mut blocks: Vec<Block> = Vec::with_capacity(order.len());
    for (k, &i) in order.iter().enumerate() {
        let mut b = Block {
            first: k,
            len: 1,
            sum_adj: desired[k],
            count: 1.0,
            span: work[i].csize + gap,
        };
        // Merge with the previous block while this one would start above its free slot.
        while let Some(prev) = blocks.last() {
            if b.sum_adj / b.count < prev.sum_adj / prev.count + prev.span {
                let prev = blocks.pop().expect("checked by while-let");
                // `b`'s members shift down by `prev.span`, so their intra grows by `prev.span`
                // (subtract `b.count * prev.span` from the running sum).
                b = Block {
                    first: prev.first,
                    len: prev.len + b.len,
                    sum_adj: prev.sum_adj + b.sum_adj - b.count * prev.span,
                    count: prev.count + b.count,
                    span: prev.span + b.span,
                };
            } else {
                break;
            }
        }
        blocks.push(b);
    }
    // Emit: walk each block's members in order, accumulating intra from member sizes.
    for b in &blocks {
        let mut p = b.sum_adj / b.count; // top-left of the block's first member
        for k in b.first..b.first + b.len {
            let i = order[k];
            cross[i] = p;
            p += work[i].csize + gap;
        }
    }
}

/// Pin cross-offset from a node's top-left; falls back to the node's cross-center.
fn pin_off(offsets: &[f32], pin: usize, csize: f32) -> f32 {
    offsets.get(pin).copied().unwrap_or(csize * 0.5)
}

/// Original source / target of an edge, regardless of cycle-break reversal.
fn orig_from(e: &E) -> usize {
    if e.rev { e.v } else { e.u }
}
fn orig_to(e: &E) -> usize {
    if e.rev { e.u } else { e.v }
}

/// Cross-offset of the socket edge `e` uses on `node`: the original source's output socket
/// if `node` is that source, else the original target's input socket. Reversal-safe.
fn sock_off(work: &[Work], e: &E, node: usize) -> f32 {
    if node == orig_from(e) {
        pin_off(&work[node].out_off, e.fp, work[node].csize)
    } else {
        debug_assert!(node == orig_to(e), "sock_off: node is neither endpoint of e");
        pin_off(&work[node].in_off, e.tp, work[node].csize)
    }
}

/// Vertical rank of `e`'s socket on `node` as a fraction in `(0, 1)` for port-aware
/// ordering. Empty socket lists ⇒ `0.5` (no tie-break; falls back to id order).
fn sock_frac(work: &[Work], e: &E, node: usize) -> f32 {
    let (offsets, rank) = if node == orig_from(e) {
        (&work[node].out_off, e.fp)
    } else {
        (&work[node].in_off, e.tp)
    };
    if offsets.is_empty() {
        0.5
    } else {
        rank.min(offsets.len() - 1) as f32 / (offsets.len() as f32 + 1.0)
    }
}

/// Arithmetic mean of a slice. Empty ⇒ 0. Used to balance a node between *all* its neighbour
/// socket-alignment targets (so a multi-input node centers between its inputs).
fn mean(v: &[f32]) -> f32 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f32>() / v.len() as f32
    }
}

/// Break directed cycles within a component via deterministic DFS; back-edges are reversed.
/// Returns the resulting DAG edge list (reversed edges have `u`/`v` swapped; pins kept with
/// their original endpoints so cross-alignment stays geometrically correct).
fn break_cycles(comp: &[usize], edges: &[E]) -> Vec<E> {
    let mut out_adj: HashMap<usize, Vec<E>> = HashMap::new();
    for e in edges {
        out_adj.entry(e.u).or_default().push(*e);
    }
    for es in out_adj.values_mut() {
        es.sort_by_key(|e| e.v); // deterministic DFS order
    }
    // 0 = white, 1 = gray (on stack), 2 = black.
    let mut color: HashMap<usize, u8> = comp.iter().map(|&i| (i, 0u8)).collect();
    let mut dag: Vec<E> = Vec::with_capacity(edges.len());
    // Iterative DFS to avoid stack overflow on big graphs.
    let mut roots: Vec<usize> = comp.to_vec();
    roots.sort();
    for &root in &roots {
        if color[&root] != 0 {
            continue;
        }
        // stack of (node, child-iter-position)
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        color.insert(root, 1);
        while let Some(&(node, ci)) = stack.last() {
            let children = out_adj.get(&node);
            if let Some(es) = children {
                if ci < es.len() {
                    stack.last_mut().unwrap().1 += 1;
                    let e = es[ci];
                    match color[&e.v] {
                        0 => {
                            color.insert(e.v, 1);
                            dag.push(e);
                            stack.push((e.v, 0));
                        }
                        1 => {
                            // Back-edge → reverse to keep the graph acyclic. Endpoints swap
                            // but fp/tp still name the ORIGINAL output/input sockets; `rev`
                            // toggles so socket sides stay correct downstream.
                            dag.push(E { u: e.v, v: e.u, fp: e.fp, tp: e.tp, rev: !e.rev });
                        }
                        _ => {
                            dag.push(e); // forward/cross edge: keep
                        }
                    }
                    continue;
                }
            }
            color.insert(node, 2);
            stack.pop();
        }
    }
    dag
}

/// Union-find for weakly-connected components.
struct Components {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl Components {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.parent[r] != r {
            r = self.parent[r];
        }
        // path compression
        let mut c = x;
        while self.parent[c] != r {
            let next = self.parent[c];
            self.parent[c] = r;
            c = next;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        if self.rank[ra] < self.rank[rb] {
            self.parent[ra] = rb;
        } else if self.rank[ra] > self.rank[rb] {
            self.parent[rb] = ra;
        } else {
            self.parent[rb] = ra;
            self.rank[ra] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutEdge, LayoutNode};

    fn node(id: u64) -> LayoutNode {
        LayoutNode::new(id, [100.0, 40.0], [0.0, 0.0])
    }

    /// No two node boxes may overlap after layout (the core invariant).
    fn assert_no_overlap(g: &LayoutGraph, out: &GraphLayout) {
        for a in &g.nodes {
            for b in &g.nodes {
                if a.id >= b.id {
                    continue;
                }
                let pa = out.pos(a.id).unwrap();
                let pb = out.pos(b.id).unwrap();
                let sep_x = pa[0] + a.size[0] <= pb[0] + 0.01 || pb[0] + b.size[0] <= pa[0] + 0.01;
                let sep_y = pa[1] + a.size[1] <= pb[1] + 0.01 || pb[1] + b.size[1] <= pa[1] + 0.01;
                assert!(sep_x || sep_y, "nodes {} and {} overlap: {pa:?} {pb:?}", a.id, b.id);
            }
        }
    }

    #[test]
    fn chain_ranks_advance_in_x() {
        let g = LayoutGraph {
            nodes: vec![node(1), node(2), node(3)],
            edges: vec![LayoutEdge::simple(1, 2), LayoutEdge::simple(2, 3)],
        };
        let out = SugiyamaLayout.layout(&g, &LayoutParams::default());
        let (a, b, c) = (out.pos(1).unwrap(), out.pos(2).unwrap(), out.pos(3).unwrap());
        assert!(b[0] > a[0] && c[0] > b[0], "ranks should advance +x: {a:?} {b:?} {c:?}");
        assert_no_overlap(&g, &out);
    }

    #[test]
    fn siblings_spread_in_cross() {
        // Two parents feeding one child: parents share a rank, must not collapse onto one line.
        let g = LayoutGraph {
            nodes: vec![node(1), node(2), node(3)],
            edges: vec![LayoutEdge::simple(1, 3), LayoutEdge::simple(2, 3)],
        };
        let out = SugiyamaLayout.layout(&g, &LayoutParams::default());
        let (a, b) = (out.pos(1).unwrap(), out.pos(2).unwrap());
        assert!((a[0] - b[0]).abs() < 1.0, "parents share rank (same x)");
        assert!((a[1] - b[1]).abs() >= 40.0, "parents spread along cross");
        assert_no_overlap(&g, &out);
    }

    #[test]
    fn top_bottom_direction_advances_in_y() {
        let g = LayoutGraph {
            nodes: vec![node(1), node(2)],
            edges: vec![LayoutEdge::simple(1, 2)],
        };
        let params = LayoutParams { direction: Direction::TopBottom, ..Default::default() };
        let out = SugiyamaLayout.layout(&g, &params);
        let (a, b) = (out.pos(1).unwrap(), out.pos(2).unwrap());
        assert!(b[1] > a[1], "TB ranks advance +y: {a:?} {b:?}");
    }

    #[test]
    fn cycle_does_not_hang_or_overlap() {
        let g = LayoutGraph {
            nodes: vec![node(1), node(2), node(3)],
            edges: vec![LayoutEdge::simple(1, 2), LayoutEdge::simple(2, 3), LayoutEdge::simple(3, 1)],
        };
        let out = SugiyamaLayout.layout(&g, &LayoutParams::default());
        assert_eq!(out.positions.len(), 3);
        assert_no_overlap(&g, &out);
    }

    #[test]
    fn deterministic_bitwise() {
        let g = LayoutGraph {
            nodes: vec![node(1), node(2), node(3), node(4)],
            edges: vec![
                LayoutEdge::simple(1, 3),
                LayoutEdge::simple(2, 3),
                LayoutEdge::simple(3, 4),
            ],
        };
        let a = SugiyamaLayout.layout(&g, &LayoutParams::default());
        let b = SugiyamaLayout.layout(&g, &LayoutParams::default());
        for n in &g.nodes {
            assert_eq!(a.pos(n.id), b.pos(n.id), "layout must be deterministic");
        }
    }

    #[test]
    fn disconnected_components_do_not_overlap() {
        let g = LayoutGraph {
            nodes: vec![node(1), node(2), node(3), node(4)],
            edges: vec![LayoutEdge::simple(1, 2), LayoutEdge::simple(3, 4)],
        };
        let out = SugiyamaLayout.layout(&g, &LayoutParams::default());
        assert_no_overlap(&g, &out);
    }

    #[test]
    fn pinned_node_stays_put() {
        let mut n2 = node(2);
        n2.pinned = true;
        n2.pos = [500.0, 333.0];
        let g = LayoutGraph {
            nodes: vec![node(1), n2],
            edges: vec![LayoutEdge::simple(1, 2)],
        };
        let out = SugiyamaLayout.layout(&g, &LayoutParams::default());
        assert_eq!(out.pos(2).unwrap()[1], 333.0, "pinned node keeps its cross position");
    }

    /// The screenshot bug: two parents feed sockets a(top)/b(bottom) of one child. The parent
    /// feeding the UPPER socket must be placed above the other so the wires don't cross — even
    /// when node-id order would put them the other way.
    #[test]
    fn parents_ordered_by_target_socket() {
        let mut child = LayoutNode::new(3, [100.0, 40.0], [300.0, 0.0]);
        child.inputs = vec![8.0, 28.0]; // socket a at y+8 (top), b at y+28 (lower)
        let mut pa = LayoutNode::new(1, [100.0, 40.0], [0.0, 0.0]);
        pa.outputs = vec![20.0];
        let mut pb = LayoutNode::new(2, [100.0, 40.0], [0.0, 0.0]);
        pb.outputs = vec![20.0];
        // Deliberately "wrong" by id: id 1 → b (lower), id 2 → a (upper).
        let g = LayoutGraph {
            nodes: vec![pa, pb, child],
            edges: vec![
                LayoutEdge { from: 1, from_pin: 0, to: 3, to_pin: 1 },
                LayoutEdge { from: 2, from_pin: 0, to: 3, to_pin: 0 },
            ],
        };
        let out = SugiyamaLayout.layout(&g, &LayoutParams::default());
        assert!(
            out.pos(2).unwrap()[1] < out.pos(1).unwrap()[1],
            "parent feeding the upper socket (id 2) must sit above the other (id 1): {:?} {:?}",
            out.pos(2),
            out.pos(1)
        );
    }

    /// The "organic centering" request (screenshot): a sink Add (id 5) has two inputs from
    /// DIFFERENT ranks — a short edge from the previous Add (id 4) and a long edge from a leaf
    /// Float (id 3) two ranks back. The sink must settle BETWEEN its inputs (the bend up on
    /// one socket cancels the bend down on the other) instead of snapping flush to the nearer
    /// input and stranding the far one (the old one-sided-pack behaviour).
    #[test]
    fn sink_centers_between_inputs() {
        let sz = [120.0, 90.0];
        let out52 = vec![52.0]; // single output socket near node center
        let in_ab = vec![40.0, 64.0]; // two input sockets a (upper) / b (lower)
        let leaf = |id: u64| {
            let mut n = LayoutNode::new(id, sz, [0.0, 0.0]);
            n.outputs = out52.clone();
            n
        };
        let adder = |id: u64| {
            let mut n = LayoutNode::new(id, sz, [0.0, 0.0]);
            n.inputs = in_ab.clone();
            n.outputs = out52.clone();
            n
        };
        let g = LayoutGraph {
            nodes: vec![leaf(1), leaf(2), leaf(3), adder(4), adder(5)],
            edges: vec![
                LayoutEdge { from: 1, from_pin: 0, to: 4, to_pin: 0 }, // F1 -> Add1.a
                LayoutEdge { from: 2, from_pin: 0, to: 4, to_pin: 1 }, // F2 -> Add1.b
                LayoutEdge { from: 4, from_pin: 0, to: 5, to_pin: 0 }, // Add1 -> Add2.a (short)
                LayoutEdge { from: 3, from_pin: 0, to: 5, to_pin: 1 }, // F3   -> Add2.b (long)
            ],
        };
        let out = SugiyamaLayout.layout(&g, &LayoutParams::default());
        let a1_result = out.pos(4).unwrap()[1] + 52.0; // Add1 output socket y
        let f3_out = out.pos(3).unwrap()[1] + 52.0; // Float3 output socket y
        let sink_y = out.pos(5).unwrap()[1];
        let d_top = a1_result - (sink_y + 40.0); // signed gap at socket a
        let d_bot = f3_out - (sink_y + 64.0); // signed gap at socket b
        assert!(
            (d_top + d_bot).abs() < 20.0,
            "sink should sit between its inputs (balanced bends), got d_top={d_top}, d_bot={d_bot}"
        );
        // And it is genuinely bending both ways — not a degenerate flush alignment.
        assert!(d_bot.abs() > 10.0, "the far (long-edge) input should actually bend: d_bot={d_bot}");
        assert_no_overlap(&g, &out);
    }
}
