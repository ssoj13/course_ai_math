/**
 * Numbers of the DotUnit scenes (docs/design/intro-and-dotunit.md §1.3, §2, §3): presets as einsum specs, their default
 * or seeded values, and everything a scene, formula or table prints, computed ONCE by the tensor core (`src/core`).
 * Pure and three-free. No arithmetic on the numbers happens here beyond what the core returns: products, running sums and
 * results come from `explain` / `einsum` (same kernel, same accumulation), so a tile's area, the table cell and the
 * formula term are the same float.
 */
import { fixed, trim } from '../fmt';
import { einsum, explain, planEinsum, rand, tensor, type AccOpts, type Explain, type Nested, type Plan, type Tensor } from '../../core';
import { idxId, layoutEinsum, stackScale, type Layout, type LayoutOpts } from './layout';

export type PresetId = 'dot' | 'matvec' | 'matmul' | 'batched' | 'attention' | 'layer';

export interface Preset {
	id: PresetId;
	op: string;
	/** Default extent of every letter. */
	shapes: Readonly<Record<string, number>>;
	/** Values used while the extents are the defaults and the seed is 0; any other choice draws seeded values. */
	a: Nested;
	b: Nested;
}

/** a = (3, 1, 2), b = (1, 2, −1): every composition starts from them so the reader keeps seeing the same 3 (spec §2). */
export const A_DEF = [3, 1, 2] as const;
export const B_DEF = [1, 2, -1] as const;
const W = [[3, 1, 2], [1, 0, 2]];
const X = [1, 2, -1];
const B = [[1, 0], [2, 1], [-1, 2]];
const Q = [[1, 2], [0, 1]];
const K = [[3, 1], [1, 0], [2, 2]];

export const PRESETS: Readonly<Record<PresetId, Preset>> = {
	dot: { id: 'dot', op: 'i,i->', shapes: { i: 3 }, a: [...A_DEF], b: [...B_DEF] },
	matvec: { id: 'matvec', op: 'ij,j->i', shapes: { i: 2, j: 3 }, a: W, b: X },
	matmul: { id: 'matmul', op: 'ij,jk->ik', shapes: { i: 2, j: 3, k: 2 }, a: W, b: B },
	// Batch of two: A[1] is A[0] with the signs flipped, B is the same matrix in both (spec §3.4).
	batched: { id: 'batched', op: 'bij,bjk->bik', shapes: { b: 2, i: 2, j: 3, k: 2 }, a: [W, W.map((r) => r.map((x) => -x))], b: [B, B] },
	// Head 0 = the Q and K of the "How it connects" Python cell (scores [[5, 1, 6], [1, 0, 2]]); head 1 swaps them in time.
	attention: { id: 'attention', op: 'bhqd,bhkd->bhqk', shapes: { b: 1, h: 2, q: 2, k: 3, d: 2 }, a: [[Q, [Q[1], Q[0]]]], b: [[K, [K[2], K[1], K[0]]]] },
	// A layer y = x Wᵀ (+ bias, drawn by the ladder): tokens as rows of x, the weight rows of W = A (spec §3.7).
	layer: { id: 'layer', op: 'td,od->to', shapes: { t: 2, d: 3, o: 2 }, a: [[1, 2, -1], [0, 1, 1]], b: W },
};
export const PRESET_IDS = Object.keys(PRESETS) as PresetId[];
/** Bias of the layer preset (one per output neuron o). */
export const LAYER_BIAS = [0.5, -0.5] as const;

/** Seeded demo numbers: integers in [−3, 3] (spec §1.1 `range [-3, 3]`, `step 1`); b uses the next seed. */
export const RANGE: readonly [number, number] = [-3, 3];

const sameShapes = (a: Readonly<Record<string, number>>, b: Readonly<Record<string, number>>) =>
	Object.keys(a).length === Object.keys(b).length && Object.entries(a).every(([k, v]) => b[k] === v);

/** Operand shapes of a plan in the operand's own letter order. */
const shapesOf = (plan: Plan, o: number) => [...plan.inputs[o]].map((l) => plan.dims[l]);

/** The two operand tensors: the preset's own numbers at its default extents and seed 0, else seeded values. */
export function operands(p: Preset, plan: Plan, seed: number): [Tensor, Tensor] {
	if (seed === 0 && sameShapes(p.shapes, plan.dims)) return [tensor(p.a), tensor(p.b)];
	return [rand(shapesOf(plan, 0), { seed, range: RANGE, step: 1 }), rand(shapesOf(plan, 1), { seed: seed + 1, range: RANGE, step: 1 })];
}

/**
 * Everything a generic einsum card draws: the layout, the operand element values in layout order, the products and
 * running sums of every drawn cell (LOD 1 only: the tiles), the results, and the stack scale.
 */
export interface SceneData {
	L: Layout;
	ts: readonly [Tensor, Tensor];
	/** Values of the drawn operand elements, in `L.operands[o]` order. */
	vals: readonly [Float64Array, Float64Array];
	/** Result of every drawn cell (einsum, fp64, sequential). */
	out: Float64Array;
	/** LOD 1: products and running sums per (cell, shaft position), `c * sumLen + k`. */
	prods: Float64Array | null;
	run: Float64Array | null;
	maxAbsP: number;
	maxAbsRun: number;
	maxAbsOut: number;
	/** World units per unit of value along the stacks. */
	hs: number;
	/** Whether term ids join indices with `_` (some extent above 10). */
	wide: boolean;
}

export function sceneData(plan: Plan, ts: readonly [Tensor, Tensor], o: LayoutOpts = {}): SceneData {
	const L = layoutEinsum(plan, o);
	const vals = L.operands.map((op, k) => {
		const v = new Float64Array(op.n), r = op.letters.length, at = new Array<number>(r);
		for (let e = 0; e < op.n; e++) {
			for (let d = 0; d < r; d++) at[d] = op.idx[e * r + d];
			v[e] = ts[k].get(at);
		}
		return v;
	}) as [Float64Array, Float64Array];
	const res = einsum(plan, ts[0], ts[1]);
	const rank = plan.kept.length;
	const cellIdx = (c: number) => Array.from(L.cells.idx.subarray(c * rank, c * rank + rank));
	const out = new Float64Array(L.cells.n);
	for (let c = 0; c < L.cells.n; c++) out[c] = res.data[L.cells.flat[c]];
	let prods: Float64Array | null = null, run: Float64Array | null = null, maxAbsP = 0, maxAbsRun = 0;
	if (L.lod === 1) {
		const n = plan.sumLen;
		prods = new Float64Array(L.cells.n * n);
		run = new Float64Array(L.cells.n * n);
		for (let c = 0; c < L.cells.n; c++) {
			const ex = explain(plan, cellIdx(c), ts);
			ex.terms.forEach((tm, k) => (prods![c * n + k] = tm.prod!));
			// Sequential order: merge k combines the running node with term k + 1 (dtype.sumSteps).
			run[c * n] = prods[c * n];
			ex.merges.forEach((m, k) => (run![c * n + k + 1] = m.value!));
		}
		for (const x of prods) maxAbsP = Math.max(maxAbsP, Math.abs(x));
		for (const x of run) maxAbsRun = Math.max(maxAbsRun, Math.abs(x));
	}
	let maxAbsOut = 0;
	for (const x of out) maxAbsOut = Math.max(maxAbsOut, Math.abs(x));
	const hs = stackScale(Math.max(maxAbsRun, maxAbsOut), L.room);
	const wide = Object.values(plan.dims).some((n) => n > 10);
	return { L, ts, vals, out, prods, run, maxAbsP, maxAbsRun, maxAbsOut, hs, wide };
}

/** Term ids of a generic scene: operand elements `a…`/`b…`, cells `c…`, tiles `p<cell><shaft>` (spec §3 ids). */
export const ids = {
	elem: (d: SceneData, o: 0 | 1, e: number) => (o === 0 ? 'a' : 'b') + idxId(d.L.operands[o].idx, e * d.L.operands[o].letters.length, d.L.operands[o].letters.length, d.wide),
	cell: (d: SceneData, c: number) => 'c' + idxId(d.L.cells.idx, c * d.L.plan.kept.length, d.L.plan.kept.length, d.wide),
	tile: (d: SceneData, c: number, k: number) => 'p' + idxId(d.L.cells.idx, c * d.L.plan.kept.length, d.L.plan.kept.length, d.wide) + (d.wide ? '_' : '') + k,
};

// ---------------------------------------------------------------------------------------------------------------
// The DotUnit (`i,i->`): both faces of one number.

export interface DotData {
	plan: Plan;
	a: readonly number[];
	b: readonly number[];
	/** aᵢ·bᵢ in shaft order and the running sums S₀ … Sₙ₋₁ (fp64, sequential), from `explain`: the GEOMETRY. */
	prods: readonly number[];
	run: readonly number[];
	/** a·b in fp64 (= run[n−1], bit for bit the einsum value). */
	dot: number;
	/** |a| = √(a·a), the shadow of b on a's line (a·b/|a|; NaN when a = 0) and |a|·shadow. */
	lenA: number;
	shadow: number;
	proj: number;
	/**
	 * The PRINTED computation (spec §1.3): the products and additions in the chosen arithmetic (fp64 or fp32) and order
	 * (sequential or pairwise), exactly as the core's `explain` executes them; `err` = exact (fp64) − printed. Geometry
	 * never uses it: a rounding error is a number in the panel, not a visibly wrong height.
	 */
	acc: { opts: AccOpts; prods: readonly number[]; merges: Explain['merges']; value: number; err: number };
}

/** The DotUnit's numbers for `a`, `b` (same length n >= 1); `o` picks the printed arithmetic and order. */
export function dotData(a: readonly number[], b: readonly number[], o: AccOpts = {}): DotData {
	if (a.length !== b.length || a.length < 1) throw new Error(`DotUnit needs two vectors of one length >= 1, got ${a.length} and ${b.length}`);
	const plan = planEinsum('i,i->', { i: a.length });
	const ta = tensor([...a]), tb = tensor([...b]);
	const ex = explain(plan, [], [ta, tb]);
	const prods = ex.terms.map((tm) => tm.prod!);
	const run = [prods[0], ...ex.merges.map((m) => m.value!)];
	const dot = ex.value!;
	const lenA = Math.sqrt(einsum('i,i->', ta, ta).item());
	const shadow = lenA > 0 ? dot / lenA : NaN;
	const ax = explain(plan, [], [ta, tb], o);
	const acc = { opts: o, prods: ax.terms.map((tm) => tm.prod!), merges: ax.merges, value: ax.value!, err: dot - ax.value! };
	return { plan, a, b, prods, run, dot, lenA, shadow, proj: lenA > 0 ? lenA * shadow : NaN, acc };
}
/** Readout format: two decimals, ASCII minus, never "-0.00"; a dash for undefined values. */
export const fmt = (x: number): string => (Number.isFinite(x) ? fixed(x, 2) : '–');
/** Short value of a tile or element: integers bare, else up to two decimals. */
export const fmtShort = (x: number): string => (Number.isFinite(x) ? trim(x, 2) : '–');

/**
 * Matrix x vector, column view (spec §3.2): y = Σⱼ xⱼ · W[:, j]. The scaled columns are the same products the row view
 * stacks (cell i, shaft position j), and the head-to-tail points are the same running sums, so both views end on the
 * same y by construction (one `explain` per output cell).
 */
export function columnView(plan: Plan, ts: readonly [Tensor, Tensor]): { cols: number[][]; heads: number[][]; y: number[] } {
	if (plan.op !== 'ij,j->i') throw new Error(`column view needs "ij,j->i", got "${plan.op}"`);
	const I = plan.dims.i, J = plan.dims.j;
	const ex = Array.from({ length: I }, (_, i) => explain(plan, [i], ts));
	const cols = Array.from({ length: J }, (_, j) => ex.map((e) => e.terms[j].prod!));
	const heads = Array.from({ length: J }, (_, j) => ex.map((e) => (j === 0 ? e.terms[0].prod! : e.merges[j - 1].value!)));
	return { cols, heads, y: ex.map((e) => e.value!) };
}
