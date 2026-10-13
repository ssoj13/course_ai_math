//! Continuous, stateful force-directed layout **simulation** for the editor's "live" view.
//!
//! ## Why this exists
//! The editor's live layout used to re-solve a *deterministic* layered (Sugiyama) layout every
//! frame and ease node positions toward the freshly-solved target, capped by a per-frame max-step
//! clamp. That is a crutch: the target can sit far from the current scatter, so without the clamp
//! nodes fling across the screen ("explosion"), and with it the motion is an arbitrary tween, not
//! a physical relaxation.
//!
//! [`ForceSim`] replaces that with a real, persistent physics simulation: it retains node
//! **position AND velocity** across frames and advances a semi-implicit Euler integrator a few
//! substeps per frame. Stability is guaranteed *by construction* — not by a clamp band-aid — via
//! three independent mechanisms:
//! 1. **Dissipation** — velocity is damped (`damping < 1`) each substep, so total kinetic energy
//!    monotonically decreases absent new forces.
//! 2. **Speed cap** — each velocity component is clamped to `v_max`, bounding any single step
//!    regardless of a force spike at a tiny inter-node distance (softened by `epsilon`).
//! 3. **Cooling** — an annealing "temperature" scales the timestep down to a floor over time, so
//!    the system settles instead of orbiting; a disturbance (drag / topology change) re-heats it.
//!
//! ## Model (size-aware, self-unfolding — inspired by Gource's directory layout)
//! Forces are computed on node CENTERS and respect each node's real footprint (its half-extents),
//! so wide cards don't stack:
//! - **Rectangular anti-overlap** — if two nodes' AABBs (+ a margin) overlap, push them apart along
//!   the minimum-translation axis. This is the near-field spacing that accounts for node size
//!   (Gource uses circular radii; a node editor's cards are rectangular, so we use AABBs).
//! - **Directional "flow" springs** — an edge a→b pulls the consumer `b` to sit one card-gap
//!   DOWNSTREAM of its source `a` (to a's right along +x) and near its height. Pulling toward a
//!   *directed* rest — rather than an isotropic distance — is what makes the DAG UNFOLD left→right
//!   and stay uncluttered (the analogue of Gource pushing a child out along its branch so the tree
//!   fans out instead of folding back on itself).
//! - **Mild long-range repulsion + weak centroid gravity** — spread disconnected clusters apart
//!   but keep the whole graph from drifting off to infinity.
//!
//! At editor scale (tens–hundreds of nodes) the O(N²) CPU cost is well under a millisecond per
//! frame and — unlike a per-frame GPU sim — needs no GPU↔CPU readback stall. A GPU variant
//! (`graph-layout-gpu`) stays available for the one-shot `L`/GPU relayout and is the right tool
//! only for very large graphs (>~1k nodes); it is intentionally NOT used here.
//!
//! ## Pinning
//! Selected (and actively dragged) nodes are **pins**: their positions are read from the graph
//! each step (so a drag feeds straight into the sim), their velocity is held at zero, and they act
//! as fixed anchors the rest of the graph arranges around — replacing the old snapshot/restore
//! hack. Moving a pin re-heats the sim so neighbours relax again.
//!
//! ## Ownership
//! The sim OWNS the positions of free (non-pinned) nodes and writes them back via
//! [`ForceSim::write_back`]; the editor owns pinned nodes. Topology (which nodes/edges exist) is
//! kept in sync via [`ForceSim::sync`], which rebuilds while preserving the positions/velocities of
//! surviving nodes so adding/removing a node never teleports the rest.

use std::collections::{HashMap, HashSet};

use crate::graph::NodeId;
use crate::graph_container::Graph;
use crate::layout::build_layout_graph;

/// How the anti-overlap separation treats a node's footprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeparationMode {
    /// Rectangular AABB, pushed apart along the minimum-translation axis. Crisp near-field spacing
    /// for axis-aligned cards, but NOT valid once a node rotates.
    Rect,
    /// Bounding circle (radius ≈ half the card diagonal). Cheaper "draft" dynamics and — crucially
    /// — ROTATION-INVARIANT, so it is used automatically when [`ForceSimConfig::rotate_nodes`] is
    /// on (a rotated card's AABB would be wrong).
    Sphere,
}

/// Tunable knobs for [`ForceSim`]. Defaults mirror the GPU force engine's tuned constants,
/// re-expressed for this integrator's unitless substep timestep (`h_base`).
#[derive(Clone, Copy, Debug)]
pub struct ForceSimConfig {
    /// Inverse-square repulsion strength for LONG-RANGE spreading (acts on node CENTERS). Kept
    /// mild — near-field spacing / no-overlap is handled by the rectangular separation term below,
    /// which is what actually accounts for real node sizes.
    pub repulsion: f32,
    /// Stiffness of the DIRECTIONAL flow spring: pulls a consumer to sit one `edge_gap` downstream
    /// (to the right, +x) of its source and near its height, so the DAG unfolds left→right.
    pub attraction: f32,
    /// How strongly the flow spring aligns a consumer's HEIGHT to its source (`0` = free vertical,
    /// let separation/repulsion spread branches; `1` = snap to the source's row). Middle values
    /// straighten chains while still letting parallel branches fan out.
    pub flow_align: f32,
    /// Extra gap (world units) added BEYOND the two nodes' half-widths for the flow spring's
    /// downstream rest distance, so connected nodes settle a comfortable card-gap apart.
    pub edge_gap: f32,
    /// Extra margin (world units) around each node's AABB for the anti-overlap separation.
    pub separation_margin: f32,
    /// Stiffness of the anti-overlap separation (size-aware; keeps footprints from overlapping).
    pub separation_k: f32,
    /// Footprint model for the separation term ([`SeparationMode`]). Forced to `Sphere` internally
    /// while `rotate_nodes` is on.
    pub separation_mode: SeparationMode,
    /// When `true`, each node also carries an ORIENTATION the sim relaxes so the node's local flow
    /// axis (+x, input→output) aligns with the average direction of its wires — the card rotates to
    /// "unfold" along its connection springs. Off = nodes stay axis-aligned (angle held at 0).
    pub rotate_nodes: bool,
    /// Whether POSITIONS may move. `false` freezes every node where the caller put it and runs the
    /// sim for orientations only — forces and the integrator are skipped entirely.
    ///
    /// This is what makes "rotate cards along their wires" and "keep relaxing the layout" two
    /// independent choices. They used to be fused, so a host that asked for rotation also got its
    /// arranged graph drifting apart under it. nodes-editor maps this to its live-layout toggle
    /// (`App::live_layout`), and rotation to `RenderSettings::node_rotation`.
    pub translate: bool,
    /// Torque stiffness pulling a node's angle toward its incident-edge direction (only when
    /// `rotate_nodes`).
    pub rot_stiffness: f32,
    /// Per-substep angular-velocity damping in `0..1` (rotational dissipation → angles settle).
    pub rot_damping: f32,
    /// Hard cap on angular speed (radians per substep) — the rotational analogue of `v_max`.
    pub rot_max: f32,
    /// Weak pull of every node toward the centroid, so disconnected components stay together.
    pub gravity: f32,
    /// Per-substep velocity damping in `0..1` (energy dissipation; `<1` guarantees decay).
    pub damping: f32,
    /// Hard per-component velocity cap (anti-explosion; independent of force magnitude).
    pub v_max: f32,
    /// Softening added to squared distance so coincident nodes don't produce infinite force.
    pub epsilon: f32,
    /// Base substep timestep (scaled by the annealing temperature).
    pub h_base: f32,
    /// Temperature multiplier applied each substep (cooling / simulated annealing), `<1`.
    pub cool: f32,
    /// Lowest temperature the cooling anneals toward (never fully freezes mid-relax).
    pub temp_floor: f32,
    /// Max per-frame node displacement (world units) still counted as "still" for settling.
    pub settle_disp: f32,
    /// Consecutive still frames before [`ForceSim::is_settled`] reports converged.
    pub settle_frames: u32,
}

impl Default for ForceSimConfig {
    fn default() -> Self {
        Self {
            repulsion: 2400.0,
            attraction: 0.08,
            flow_align: 0.5,
            // Downstream rest distance beyond the two half-widths — bumped so chains breathe
            // horizontally instead of hugging (connections read as longer, less tight).
            edge_gap: 170.0,
            // A touch more breathing room around each node's footprint.
            separation_margin: 40.0,
            separation_k: 0.40,
            separation_mode: SeparationMode::Rect,
            rotate_nodes: false,
            // Positions move by default: the plain force-layout sim is the common case, and an
            // orientation-only run is the deliberate opt-out.
            translate: true,
            rot_stiffness: 0.15,
            rot_damping: 0.85,
            rot_max: 0.08,
            gravity: 0.008,
            damping: 0.90,
            v_max: 60.0,
            epsilon: 1.0,
            h_base: 0.5,
            cool: 0.99,
            temp_floor: 0.15,
            settle_disp: 0.2,
            settle_frames: 24,
        }
    }
}

/// A persistent force-directed layout simulation over one edit scope (see module docs).
#[derive(Clone, Debug)]
pub struct ForceSim {
    cfg: ForceSimConfig,
    /// Engine index → node id (parallel to [`Self::pos`]/[`Self::vel`]).
    ids: Vec<NodeId>,
    /// Fast reverse lookup id → index, so [`Self::sync`] can preserve survivors' state.
    id_index: HashMap<NodeId, usize>,
    /// Edges as `(from_index, to_index)` pairs (spring endpoints).
    edges: Vec<(usize, usize)>,
    /// Connected-component id (union-find root index) per node, parallel to [`Self::ids`]. Nodes in
    /// DIFFERENT components exert NO force on each other (no cross-component repulsion/separation,
    /// and each component gravitates to its OWN centroid) — so dragging a disconnected node or a
    /// separate sub-graph never shoves an unrelated one. Recomputed on every topology change.
    comp: Vec<usize>,
    /// Current node positions (graph-UV top-left), index-parallel to [`Self::ids`].
    pos: Vec<[f32; 2]>,
    /// Current node velocities, index-parallel to [`Self::ids`].
    vel: Vec<[f32; 2]>,
    /// Half-extents `[w/2, h/2]` per node (from `compute_node_size` via `build_layout_graph`), so
    /// forces respect real node footprints (anti-overlap + size-aware spring rest). Refreshed from
    /// the layout every [`Self::sync`] since a view-mode change resizes a node without changing
    /// topology.
    half: Vec<[f32; 2]>,
    /// Per-node orientation (radians), index-parallel. Always tracked but only evolves/written when
    /// [`ForceSimConfig::rotate_nodes`] is on; otherwise held at 0 (axis-aligned).
    angle: Vec<f32>,
    /// Per-node angular velocity (radians per substep), index-parallel.
    ang_vel: Vec<f32>,
    /// Whether each node was pinned on the last [`Self::step`] (used by [`Self::write_back`]).
    pinned: Vec<bool>,
    /// Annealing temperature in `[temp_floor, 1]`; reset to 1 on disturbance, decays each substep.
    temp: f32,
    /// Consecutive frames the max displacement stayed below `settle_disp`.
    settle_frames: u32,
}

impl ForceSim {
    /// Create a fresh simulation seeded from the current positions of the nodes in `scope`.
    pub fn new(graph: &Graph, scope: Option<NodeId>, cfg: ForceSimConfig) -> Self {
        let mut sim = Self {
            cfg,
            ids: Vec::new(),
            id_index: HashMap::new(),
            edges: Vec::new(),
            comp: Vec::new(),
            pos: Vec::new(),
            vel: Vec::new(),
            half: Vec::new(),
            angle: Vec::new(),
            ang_vel: Vec::new(),
            pinned: Vec::new(),
            temp: 1.0,
            settle_frames: 0,
        };
        sim.rebuild(graph, scope);
        sim
    }

    /// Re-sync topology if the node set or edge set changed (e.g. a node/edge was added or
    /// removed, or the edit scope changed). Cheap no-op when nothing changed. Preserves the
    /// positions/velocities of surviving nodes so the rest of the graph never teleports.
    pub fn sync(&mut self, graph: &Graph, scope: Option<NodeId>) {
        let (lg, ids) = build_layout_graph(graph, scope);
        let edges: Vec<(usize, usize)> = lg
            .edges
            .iter()
            .map(|e| (e.from as usize, e.to as usize))
            .collect();
        // Half-extents are refreshed EVERY frame (a view-mode change resizes a node without a
        // topology change), and `lg` is already built here so it is free.
        let half: Vec<[f32; 2]> = lg
            .nodes
            .iter()
            .map(|n| [n.size[0] * 0.5, n.size[1] * 0.5])
            .collect();
        if ids == self.ids && edges == self.edges {
            self.half = half;
            return;
        }
        // Preserve survivors' pos/vel/angle by id; seed new nodes from their current view position.
        let old_index = std::mem::take(&mut self.id_index);
        let old_pos = std::mem::take(&mut self.pos);
        let old_vel = std::mem::take(&mut self.vel);
        let old_angle = std::mem::take(&mut self.angle);
        let old_ang_vel = std::mem::take(&mut self.ang_vel);
        let mut pos = Vec::with_capacity(ids.len());
        let mut vel = Vec::with_capacity(ids.len());
        let mut angle = Vec::with_capacity(ids.len());
        let mut ang_vel = Vec::with_capacity(ids.len());
        for (i, id) in ids.iter().enumerate() {
            if let Some(&old) = old_index.get(id) {
                pos.push(old_pos[old]);
                vel.push(old_vel[old]);
                angle.push(old_angle.get(old).copied().unwrap_or(0.0));
                ang_vel.push(old_ang_vel.get(old).copied().unwrap_or(0.0));
            } else {
                pos.push(lg.nodes[i].pos);
                vel.push([0.0, 0.0]);
                angle.push(0.0);
                ang_vel.push(0.0);
            }
        }
        self.ids = ids;
        self.edges = edges;
        self.comp = components(self.ids.len(), &self.edges);
        self.pos = pos;
        self.vel = vel;
        self.half = half;
        self.angle = angle;
        self.ang_vel = ang_vel;
        self.pinned = vec![false; self.ids.len()];
        self.rebuild_index();
        // A topology change is a disturbance — re-heat so the new arrangement relaxes.
        self.temp = 1.0;
        self.settle_frames = 0;
    }

    /// Advance the simulation using a frame-rate-independent number of fixed substeps derived from
    /// the real frame `dt`. `pins` = node ids that must not move (their positions are read from
    /// `graph` so a live drag feeds in; they act as anchors). Returns the max per-frame node
    /// displacement (world units) — small when converged.
    pub fn step(&mut self, dt: f32, graph: &Graph, pins: &HashSet<NodeId>) -> f32 {
        if self.ids.is_empty() {
            return 0.0;
        }
        // Refresh the pinned mask + pull pin positions from the graph (drag authoritative).
        let mut disturbed = false;
        for (i, id) in self.ids.iter().enumerate() {
            let is_pin = pins.contains(id);
            self.pinned[i] = is_pin;
            if is_pin && let Some(n) = graph.nodes.get(id) {
                let p = n.view_pos();
                let d = [p[0] - self.pos[i][0], p[1] - self.pos[i][1]];
                if d[0] * d[0] + d[1] * d[1] > 1.0e-4 {
                    disturbed = true;
                }
                self.pos[i] = p;
                self.vel[i] = [0.0, 0.0];
            }
        }
        if disturbed {
            self.temp = 1.0;
            self.settle_frames = 0;
        }

        // Frame-rate independence: ~60 Hz baseline, 1–4 substeps per frame.
        let substeps = ((dt * 60.0).round() as i32).clamp(1, 4) as u32;
        let mut max_disp = 0.0_f32;
        for _ in 0..substeps {
            max_disp = max_disp.max(self.substep());
        }

        if max_disp < self.cfg.settle_disp {
            self.settle_frames = self.settle_frames.saturating_add(1);
        } else {
            self.settle_frames = 0;
        }
        max_disp
    }

    /// Write the simulated positions of every FREE (non-pinned) node back into the graph. Pinned
    /// nodes are owned by the editor and left untouched. Does not mark nodes dirty (a position
    /// change never triggers a cook).
    pub fn write_back(&self, graph: &mut Graph) {
        for (i, id) in self.ids.iter().enumerate() {
            if self.pinned.get(i).copied().unwrap_or(false) {
                continue;
            }
            if let Some(n) = graph.nodes.get_mut(id) {
                // Each channel is written only when its own mode is on, so an orientation-only run
                // cannot nudge a position and a translate-only run cannot spin a card.
                if self.cfg.translate {
                    n.set_view_pos(self.pos[i]);
                }
                if self.cfg.rotate_nodes {
                    n.set_view_rotation(self.angle[i]);
                }
            }
        }
    }

    /// Current simulated orientation (radians) of a node by id, if present. `0` unless
    /// [`ForceSimConfig::rotate_nodes`] is on.
    pub fn angle_of(&self, id: NodeId) -> Option<f32> {
        self.id_index
            .get(&id)
            .and_then(|&i| self.angle.get(i).copied())
    }

    /// True once the graph has stayed effectively still for `settle_frames` frames — the caller
    /// can stop driving redraws until the next disturbance.
    pub fn is_settled(&self) -> bool {
        self.settle_frames >= self.cfg.settle_frames
    }

    /// Number of nodes currently simulated.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether the simulation has no nodes.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    // ── internals ──────────────────────────────────────────────────────────────────────────

    /// Rebuild the whole sim from scratch for `scope` (used by [`Self::new`]).
    fn rebuild(&mut self, graph: &Graph, scope: Option<NodeId>) {
        let (lg, ids) = build_layout_graph(graph, scope);
        self.edges = lg
            .edges
            .iter()
            .map(|e| (e.from as usize, e.to as usize))
            .collect();
        self.pos = lg.nodes.iter().map(|n| n.pos).collect();
        self.vel = vec![[0.0, 0.0]; ids.len()];
        self.half = lg
            .nodes
            .iter()
            .map(|n| [n.size[0] * 0.5, n.size[1] * 0.5])
            .collect();
        self.angle = vec![0.0; ids.len()];
        self.ang_vel = vec![0.0; ids.len()];
        self.pinned = vec![false; ids.len()];
        self.ids = ids;
        self.comp = components(self.ids.len(), &self.edges);
        self.rebuild_index();
        self.temp = 1.0;
        self.settle_frames = 0;
    }

    fn rebuild_index(&mut self) {
        self.id_index = self
            .ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();
    }

    /// One semi-implicit Euler substep. Returns the largest motion any node made this substep, in
    /// world units — a translation, or the arc a rotating card's corner swept, whichever is bigger.
    /// Both count so [`ForceSim::is_settled`] stays honest when only one of the two is enabled.
    fn substep(&mut self) -> f32 {
        let n = self.pos.len();
        if n == 0 {
            return 0.0;
        }
        if self.half.len() != n || self.comp.len() != n {
            return 0.0; // half-extents / components out of sync (shouldn't happen); skip vs panic
        }
        let cfg = self.cfg;
        let h = cfg.h_base * self.temp;

        // Work in CENTERS so node sizes are respected (positions are stored top-left; a center
        // displacement equals a top-left displacement since half-extents are constant per frame).
        let centers: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                [
                    self.pos[i][0] + self.half[i][0],
                    self.pos[i][1] + self.half[i][1],
                ]
            })
            .collect();

        // Per-COMPONENT centroid: each connected component gravitates to its OWN centre, so moving
        // one disconnected node / sub-graph never tugs another. Indexed by the union-find root
        // (`self.comp[i]`), holding `[sum_x, sum_y, count]`.
        let mut csum = vec![[0.0_f32, 0.0_f32, 0.0_f32]; n];
        for (&c, ctr) in self.comp.iter().zip(&centers) {
            csum[c][0] += ctr[0];
            csum[c][1] += ctr[1];
            csum[c][2] += 1.0;
        }

        // Footprint model for separation. Sphere (bounding circle) is ROTATION-INVARIANT so it is
        // used whenever nodes may rotate; it is also the cheaper "draft" option. Radii precomputed
        // once (half the card diagonal) to avoid a sqrt in the O(N²) inner loop.
        let sphere_mode = cfg.separation_mode == SeparationMode::Sphere || cfg.rotate_nodes;
        let radius: Vec<f32> = if sphere_mode {
            self.half
                .iter()
                .map(|hh| (hh[0] * hh[0] + hh[1] * hh[1]).sqrt())
                .collect()
        } else {
            Vec::new()
        };

        // Everything below that moves a node is gated on `translate`. With it off this is an
        // orientation-only pass: no forces, no integrator, positions stay exactly as laid out.
        let mut max_disp = 0.0_f32;
        if cfg.translate {
            // Accumulate forces on FREE nodes (pinned stay fixed but still repel/anchor others).
            let mut forces = vec![[0.0_f32, 0.0_f32]; n];
            for i in 0..n {
                if self.pinned[i] {
                    continue;
                }
                let ci = centers[i];
                let mut f = [0.0_f32, 0.0_f32];
                for j in 0..n {
                    if i == j {
                        continue;
                    }
                    // Only nodes in the SAME connected component interact — a separate node/graph is
                    // never shoved by this one's separation/repulsion.
                    if self.comp[i] != self.comp[j] {
                        continue;
                    }
                    let cj = centers[j];
                    let dx = ci[0] - cj[0];
                    let dy = ci[1] - cj[1];
                    // Anti-overlap (SIZE-AWARE). Sphere = bounding-circle penetration along the center
                    // line (rotation-invariant, draft). Rect = AABB minimum-translation axis (crisp for
                    // axis-aligned cards). Either way it accounts for each node's real footprint so
                    // wide cards stop stacking.
                    if sphere_mode {
                        let min_d = radius[i] + radius[j] + cfg.separation_margin;
                        let d = (dx * dx + dy * dy).sqrt();
                        if d > 1.0e-4 && d < min_d {
                            let push = (min_d - d) / d * cfg.separation_k;
                            f[0] += dx * push;
                            f[1] += dy * push;
                        }
                    } else {
                        let sx = self.half[i][0] + self.half[j][0] + cfg.separation_margin;
                        let sy = self.half[i][1] + self.half[j][1] + cfg.separation_margin;
                        let ox = sx - dx.abs();
                        let oy = sy - dy.abs();
                        if ox > 0.0 && oy > 0.0 {
                            if ox <= oy {
                                let s = if dx < 0.0 { -1.0 } else { 1.0 };
                                f[0] += s * ox * cfg.separation_k;
                            } else {
                                let s = if dy < 0.0 { -1.0 } else { 1.0 };
                                f[1] += s * oy * cfg.separation_k;
                            }
                        }
                    }
                    // Mild long-range repulsion so disconnected clusters spread out.
                    let d2 = (dx * dx + dy * dy).max(cfg.epsilon);
                    let inv = 1.0 / d2.sqrt();
                    let m = cfg.repulsion / d2 * inv;
                    f[0] += dx * m;
                    f[1] += dy * m;
                }
                // Gravity toward this node's OWN component centroid (keeps a cluster together without
                // coupling separate ones; a lone node's centroid is itself → no pull).
                let c = self.comp[i];
                let inv_cnt = 1.0 / csum[c][2].max(1.0);
                f[0] += (csum[c][0] * inv_cnt - ci[0]) * cfg.gravity;
                f[1] += (csum[c][1] * inv_cnt - ci[1]) * cfg.gravity;
                forces[i] = f;
            }

            // Directional "flow" springs (Gource-inspired unfolding). An edge a→b (a = source/output,
            // b = consumer/input) wants b to sit ONE card-gap DOWNSTREAM of a: to a's right along the
            // +x flow axis (`rest_x`) AND at a's height. Pulling toward that DIRECTED rest — not an
            // isotropic distance — makes the DAG unfold left→right and stay uncluttered instead of
            // folding back on itself, while the rectangular separation above keeps parallel branches
            // from stacking. Applied equal-and-opposite so momentum is conserved.
            for &(a, b) in &self.edges {
                if a >= n || b >= n {
                    continue;
                }
                let rest_x = self.half[a][0] + self.half[b][0] + cfg.edge_gap;
                // Positional error of b relative to where it "should" sit downstream of a.
                let err = [
                    (centers[a][0] + rest_x) - centers[b][0],
                    (centers[a][1] - centers[b][1]) * cfg.flow_align,
                ];
                let fb = [err[0] * cfg.attraction, err[1] * cfg.attraction];
                if !self.pinned[b] {
                    forces[b][0] += fb[0];
                    forces[b][1] += fb[1];
                }
                if !self.pinned[a] {
                    forces[a][0] -= fb[0];
                    forces[a][1] -= fb[1];
                }
            }

            // Integrate free nodes (semi-implicit Euler + damping + speed cap + finite guard).
            // `forces` was built with exactly `n` entries, so `enumerate` walks the same `0..n` the
            // sibling per-node slices (`pinned`/`vel`/`pos`) are indexed by.
            for (i, f) in forces.iter().enumerate() {
                if self.pinned[i] {
                    continue;
                }
                let mut v = [self.vel[i][0] + f[0] * h, self.vel[i][1] + f[1] * h];
                v[0] *= cfg.damping;
                v[1] *= cfg.damping;
                v[0] = v[0].clamp(-cfg.v_max, cfg.v_max);
                v[1] = v[1].clamp(-cfg.v_max, cfg.v_max);
                if !(v[0].is_finite() && v[1].is_finite()) {
                    v = [0.0, 0.0];
                }
                let step = [v[0] * h, v[1] * h];
                self.pos[i][0] += step[0];
                self.pos[i][1] += step[1];
                self.vel[i] = v;
                let disp = (step[0] * step[0] + step[1] * step[1]).sqrt();
                if disp > max_disp {
                    max_disp = disp;
                }
            }
        }

        // Rotational dynamics (optional): relax each node's angle so its local +x flow axis aligns
        // with the AVERAGE direction of its incident wires — the "unfold with the springs" effect.
        // A node with no edges keeps its angle (torque-free → damps in place). Uses the same
        // dissipation + speed-cap discipline as the linear integrator, so it is stable.
        if cfg.rotate_nodes {
            let mut dirsum = vec![[0.0_f32, 0.0_f32]; n];
            for &(a, b) in &self.edges {
                if a >= n || b >= n {
                    continue;
                }
                let d = [centers[b][0] - centers[a][0], centers[b][1] - centers[a][1]];
                dirsum[a][0] += d[0];
                dirsum[a][1] += d[1];
                dirsum[b][0] += d[0];
                dirsum[b][1] += d[1];
            }
            for (i, &ds) in dirsum.iter().enumerate() {
                if self.pinned[i] {
                    continue;
                }
                if ds[0] * ds[0] + ds[1] * ds[1] < 1.0e-6 {
                    continue; // no incident wire direction → leave angle as-is
                }
                let target = ds[1].atan2(ds[0]);
                let diff = wrap_pi(target - self.angle[i]);
                let mut w = (self.ang_vel[i] + diff * cfg.rot_stiffness) * cfg.rot_damping;
                w = w.clamp(-cfg.rot_max, cfg.rot_max);
                if !w.is_finite() {
                    w = 0.0;
                }
                self.angle[i] += w;
                self.ang_vel[i] = w;
                // Settling has to see rotation too, or an orientation-only run reports "converged"
                // on frame one because nothing ever translates. Charge the arc the card's corner
                // sweeps, which is in world units and so comparable to `settle_disp`.
                let tip = w.abs() * self.half[i][0].hypot(self.half[i][1]);
                if tip > max_disp {
                    max_disp = tip;
                }
            }
        }

        // Anneal toward the floor so the system settles instead of orbiting forever.
        self.temp = (self.temp * cfg.cool).max(cfg.temp_floor);
        max_disp
    }
}

/// Connected-component root index per node, via union-find over the UNDIRECTED edge set. Two nodes
/// with different roots are in different components and (by [`ForceSim::substep`]) never force each
/// other. O(n + e·α). The returned value is the representative index, not a compacted 0..k id —
/// callers only compare for equality and index a per-node scratch array by it.
fn components(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]]; // path halving
            x = parent[x];
        }
        x
    }
    let mut parent: Vec<usize> = (0..n).collect();
    for &(a, b) in edges {
        if a < n && b < n {
            let ra = find(&mut parent, a);
            let rb = find(&mut parent, b);
            if ra != rb {
                parent[ra] = rb;
            }
        }
    }
    (0..n).map(|i| find(&mut parent, i)).collect()
}

/// Wrap an angle (radians) into `[-π, π]` for shortest-arc torque.
fn wrap_pi(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let a = a % TAU;
    if a > PI {
        a - TAU
    } else if a < -PI {
        a + TAU
    } else {
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Graph, NodeTypeRegistry, ValueTypes, register_builtins};

    // `register_builtins` registers the builtin VALUE types too (the `Builtins` library declares
    // them), so a separate `register_builtin_types` call would register each one twice.
    fn fixtures() -> (NodeTypeRegistry, ValueTypes) {
        let mut types = ValueTypes::new();
        let mut reg = NodeTypeRegistry::new();
        register_builtins(&mut reg, &mut types);
        (reg, types)
    }

    /// Orientation-only mode (`translate: false`) relaxes the angles and moves nothing. This is the
    /// split that lets a host ask for "unfold the cards along their wires" without the layout it
    /// arranged drifting apart underneath it — the two used to be one switch.
    #[test]
    fn orientation_only_rotates_without_moving_anything() {
        let (reg, types) = fixtures();
        let mut g = Graph::new();
        let a = g.create_node("Float", [0.0, 0.0], &reg, &types).unwrap();
        let b = g.create_node("Add", [900.0, 600.0], &reg, &types).unwrap();
        g.connect_idx(a, 0, b, 0, &reg, &types).unwrap();
        g.rebuild_index();

        // Read positions back by id, so the assert does not depend on map iteration order.
        let ids: Vec<_> = g.nodes.keys().copied().collect();
        let pos_of = |g: &Graph| -> Vec<[f32; 2]> {
            ids.iter()
                .map(|id| g.nodes.get(id).map(|n| n.view_pos()).unwrap_or_default())
                .collect()
        };
        let before = pos_of(&g);

        let cfg = ForceSimConfig {
            rotate_nodes: true,
            translate: false,
            ..Default::default()
        };
        let mut sim = ForceSim::new(&g, None, cfg);
        let no_pins = HashSet::new();
        for _ in 0..120 {
            sim.step(1.0 / 60.0, &g, &no_pins);
        }
        sim.write_back(&mut g);

        assert_eq!(
            before,
            pos_of(&g),
            "orientation-only mode must not move a node"
        );
        assert!(
            g.nodes.values().any(|n| n.view_rotation().abs() > 1.0e-3),
            "angles should have relaxed toward the wire direction"
        );
    }

    /// Two connected nodes seeded far apart converge, and the sim reports settled after enough
    /// still frames — with displacement always finite/bounded (dissipative, never exploding).
    #[test]
    fn converges_and_settles() {
        let (reg, types) = fixtures();
        let mut g = Graph::new();
        let a = g.create_node("Float", [0.0, 0.0], &reg, &types).unwrap();
        let b = g
            .create_node("Add", [4000.0, 3000.0], &reg, &types)
            .unwrap();
        g.connect_idx(a, 0, b, 0, &reg, &types).unwrap();
        g.rebuild_index();

        let mut sim = ForceSim::new(&g, None, ForceSimConfig::default());
        let no_pins = HashSet::new();
        let mut settled = false;
        for frame in 0..5000 {
            let disp = sim.step(1.0 / 60.0, &g, &no_pins);
            assert!(disp.is_finite(), "non-finite displacement at frame {frame}");
            if sim.is_settled() {
                settled = true;
                break;
            }
        }
        assert!(settled, "force sim never settled");
    }

    /// A pinned node never moves, regardless of the forces around it.
    #[test]
    fn pinned_node_never_moves() {
        let (reg, types) = fixtures();
        let mut g = Graph::new();
        let a = g
            .create_node("Float", [100.0, 100.0], &reg, &types)
            .unwrap();
        let b = g.create_node("Add", [110.0, 100.0], &reg, &types).unwrap();
        let c = g.create_node("Add", [120.0, 100.0], &reg, &types).unwrap();
        let _ = (a, c);
        g.rebuild_index();

        let mut sim = ForceSim::new(&g, None, ForceSimConfig::default());
        let mut pins = HashSet::new();
        pins.insert(b);
        let before = g.nodes.get(&b).unwrap().view_pos();
        for _ in 0..200 {
            sim.step(1.0 / 60.0, &g, &pins);
        }
        sim.write_back(&mut g);
        let after = g.nodes.get(&b).unwrap().view_pos();
        assert_eq!(before, after, "pinned node moved");
    }

    /// A disconnected node (its own component) is NOT pushed by a separate connected sub-graph:
    /// forces act only within a component, so unrelated networks don't shove each other.
    #[test]
    fn disconnected_components_are_independent() {
        let (reg, types) = fixtures();
        let mut g = Graph::new();
        // Component 1: a connected pair seeded far apart (will relax toward each other).
        let a = g.create_node("Float", [0.0, 0.0], &reg, &types).unwrap();
        let b = g
            .create_node("Add", [4000.0, 3000.0], &reg, &types)
            .unwrap();
        g.connect_idx(a, 0, b, 0, &reg, &types).unwrap();
        // Component 2: a lone node, no edges, sitting close to component 1's cluster.
        let lone = g.create_node("Add", [50.0, 50.0], &reg, &types).unwrap();
        g.rebuild_index();

        let mut sim = ForceSim::new(&g, None, ForceSimConfig::default());
        let no_pins = HashSet::new();
        let lone_start = sim.pos[sim.id_index[&lone]];
        for _ in 0..400 {
            sim.step(1.0 / 60.0, &g, &no_pins);
        }
        let lone_end = sim.pos[sim.id_index[&lone]];
        // The lone node forms its own component (centroid = itself → zero gravity) and gets no
        // cross-component repulsion from the a–b pair, so it must not have moved.
        let moved =
            ((lone_end[0] - lone_start[0]).powi(2) + (lone_end[1] - lone_start[1]).powi(2)).sqrt();
        assert!(
            moved < 1.0e-3,
            "lone node moved {moved} despite being disconnected"
        );
    }

    /// Adding a node preserves the existing nodes' simulated positions (no teleport on sync).
    #[test]
    fn sync_preserves_survivor_positions() {
        let (reg, types) = fixtures();
        let mut g = Graph::new();
        let a = g.create_node("Float", [0.0, 0.0], &reg, &types).unwrap();
        g.rebuild_index();
        let mut sim = ForceSim::new(&g, None, ForceSimConfig::default());
        let no_pins = HashSet::new();
        for _ in 0..30 {
            sim.step(1.0 / 60.0, &g, &no_pins);
        }
        sim.write_back(&mut g);
        let a_pos = g.nodes.get(&a).unwrap().view_pos();

        // Add a second node and re-sync: `a` must keep its simulated position.
        let _b = g.create_node("Add", [500.0, 500.0], &reg, &types).unwrap();
        g.rebuild_index();
        sim.sync(&g, None);
        let idx_a = sim.id_index[&a];
        assert_eq!(
            sim.pos[idx_a], a_pos,
            "survivor position not preserved on sync"
        );
    }
}
