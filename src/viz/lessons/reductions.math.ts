/**
 * Lesson 3.5 (reductions: sum, mean, max along an axis): the store contract and every number the scene, the readouts
 * and the tests show. Pure and three-free. Results come from the course's tensor core (src/core `sum`/`mean`/`max` with
 * `axis` and `keepdims`, the NumPy rules); this module adds which input cells collapse into which result cell, the
 * maxima (ties included) and the summation-order experiment of the machine layer (core `sum` with fp32 arithmetic).
 *
 * Store `reductions` (one link group over the integer keys):
 * - `src` 0..2: the data (a 3 x 4 sheet of small integers, the 2 x 3 x 4 block 0..23, the 8 x 8 grayscale icon).
 * - `op` 0..2: sum, mean, max.
 * - `ax` -3..2: the axis to reduce, NumPy-style (negative counts from the end); clamped to the data's rank.
 * - `all` 0/1: reduce every axis (NumPy `axis=None`). `kd` 0/1: keepdims.
 * - `sn` 0..3: how many numbers the machine-layer experiment adds.
 */
import { arange, full, max, mean, sum, tensor, type AccOpts, type Tensor } from '../../core';
import type { LessonLinks, LinkGroup } from '../links';
import { finite } from '../links';
import type { LessonState, Val } from '../store';
import { ICON, luma } from './tensor-road.data';
import { unravel } from './tensor-ops';

export const LESSON = 'reductions';

export const SPEC = {
	src: { min: 0, max: 2, step: 1 },
	op: { min: 0, max: 2, step: 1 },
	ax: { min: -3, max: 2, step: 1 },
	sn: { min: 0, max: 3, step: 1 },
} as const;

/** NumPy method names: the same in every language. */
export const OP_NAMES = ['sum', 'mean', 'max'] as const;
export type Op = 0 | 1 | 2;
export type SrcId = 'sheet' | 'block' | 'gray';
export const SOURCES: readonly SrcId[] = ['sheet', 'block', 'gray'];

/**
 * A 3 x 4 sheet: 3 samples (rows) of 4 features (columns). Small integers with negatives (hatched in the scene) and a
 * tie: row 1 holds its maximum 5 twice (columns 1 and 3).
 */
export const SHEET = [
	[2, -1, 4, 3],
	[0, 5, -2, 5],
	[1, 3, 2, -4],
] as const;

/** The data of each source, as tensors of the core (f64). */
export function source(id: SrcId): Tensor {
	if (id === 'sheet') return tensor(SHEET.map((r) => [...r]));
	if (id === 'block') return arange(24, [2, 3, 4]);
	return tensor(ICON.map((row) => row.map(luma)));
}

const INT_KEYS = ['src', 'op', 'ax', 'all', 'kd', 'sn'] as const;

/** Default: the row sums of the sheet, sum(axis=1) -> (3,). */
export const INITIAL: Record<string, Val> = { src: 0, op: 0, ax: 1, all: 0, kd: 0, sn: 1 };

type Vals = LessonState['vals'];

const int = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`reductions: "${key}" must be a number`);
	finite(v);
	return Math.round(v);
};
const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

/** Rounds and clamps every key; the axis is kept inside -rank..rank-1 of the chosen data. */
function intLink(): LinkGroup {
	return {
		keys: INT_KEYS,
		canonical: 'op',
		resolve(input, current) {
			const get = (k: string) => int(Object.hasOwn(input, k) ? input[k] : current[k], k);
			const src = clamp(get('src'), SPEC.src.min, SPEC.src.max);
			const rank = source(SOURCES[src]).ndim;
			return {
				src,
				op: clamp(get('op'), SPEC.op.min, SPEC.op.max),
				ax: clamp(get('ax'), -rank, rank - 1),
				all: get('all') === 1 ? 1 : 0,
				kd: get('kd') === 1 ? 1 : 0,
				sn: clamp(get('sn'), SPEC.sn.min, SPEC.sn.max),
			};
		},
		shown: () => ({}),
	};
}

export const LINKS: LessonLinks = { groups: [intLink()] };

export interface RModel {
	src: SrcId;
	op: Op;
	x: Tensor;
	/** The axis as the reader wrote it (may be negative), or null for axis=None (all axes). */
	axis: number | null;
	/** Reduced axes, normalized to 0..rank-1. */
	axes: number[];
	keepdims: boolean;
	/** Shape with every reduced axis set to 1 (keepdims=True), and the result's actual shape. */
	kept: number[];
	out: number[];
	/** The core's result (`sum`/`mean`/`max` with these options). Its values are in row-major order of `kept`. */
	r: Tensor;
	/** Per input cell (row-major): the result cell it collapses into. */
	group: Int32Array;
	/** Per result cell: the input cell where the collapse lands (index 0 along every reduced axis), and how many cells. */
	first: Int32Array;
	count: Int32Array;
	/** Per input cell: 1 if it holds its result cell's maximum (several = a tie). Only for max. */
	top: Uint8Array;
}

/** Row-major position of `idx` in `shape`. */
export function ravel(shape: readonly number[], idx: readonly number[]): number {
	let k = 0;
	for (let a = 0; a < shape.length; a++) k = k * shape[a] + idx[a];
	return k;
}

export function compute(x: Tensor, op: Op, axis: number | null, keepdims: boolean): Omit<RModel, 'src'> {
	const rank = x.ndim;
	const axes = axis === null ? x.shape.map((_, d) => d) : [axis < 0 ? axis + rank : axis];
	const opts = axis === null ? { keepdims } : { axis, keepdims };
	const r = [sum, mean, max][op](x, opts);
	const kept = x.shape.map((n, d) => (axes.includes(d) ? 1 : n));
	const cells = r.size;
	const group = new Int32Array(x.size);
	const first = new Int32Array(cells).fill(-1);
	const count = new Int32Array(cells);
	for (let k = 0; k < x.size; k++) {
		const idx = unravel(x.shape, k);
		for (const d of axes) idx[d] = 0;
		const c = ravel(kept, idx);
		group[k] = c;
		count[c]++;
		if (first[c] < 0) first[c] = k;
	}
	const top = new Uint8Array(x.size);
	if (op === 2) for (let k = 0; k < x.size; k++) top[k] = x.data[k] === r.data[group[k]] ? 1 : 0;
	return { op, x, axis, axes, keepdims, kept, out: [...r.shape], r, group, first, count, top };
}

export function model(v: Vals): RModel {
	const src = SOURCES[clamp(int(v.src, 'src'), 0, 2)];
	const x = source(src);
	const all = int(v.all, 'all') === 1;
	const ax = clamp(int(v.ax, 'ax'), -x.ndim, x.ndim - 1);
	return { src, ...compute(x, clamp(int(v.op, 'op'), 0, 2) as Op, all ? null : ax, int(v.kd, 'kd') === 1) };
}

/** The NumPy call that computes what the scene shows, e.g. `x.sum(axis=1)`, `x.max(axis=None, keepdims=True)`. */
export function code(m: Pick<RModel, 'op' | 'axis' | 'keepdims'>): string {
	return `x.${OP_NAMES[m.op]}(axis=${m.axis === null ? 'None' : m.axis}${m.keepdims ? ', keepdims=True' : ''})`;
}

/** The first result cell whose maximum is held by more than one input cell, with those cells (row-major), or null. */
export function firstTie(m: RModel): { cell: number; cells: number[] } | null {
	if (m.op !== 2) return null;
	for (let c = 0; c < m.r.size; c++) {
		const cells: number[] = [];
		for (let k = 0; k < m.x.size; k++) if (m.group[k] === c && m.top[k]) cells.push(k);
		if (cells.length > 1) return { cell: c, cells };
	}
	return null;
}

// ---------------------------------------------------------------------------------------------------------------
// Machine layer: the same sum, three ways (core `sum` on float32 storage).

/** How many copies of 0.1 the experiment adds, by `sn`. */
export const SUM_NS = [10, 1000, 100_000, 1_000_000] as const;

/** One way of adding: its sum and its mean (sum / n in the same arithmetic), both as float32 results. */
export interface Way {
	sum: number;
	mean: number;
}

export interface OrderRun {
	/** The exact sum of the stored numbers (n times float32(0.1)), the reference. */
	exact: number;
	/** fp32 arithmetic, one running total left to right (a naive loop, a single GPU thread). */
	seq: Way;
	/** fp32 arithmetic, pairs of pairs (a tree: how GPUs and BLAS reduce). */
	tree: Way;
	/** fp64 accumulator, then stored as float32 (the result keeps the input's dtype). */
	wide: Way;
}

export function orderRun(n: number): OrderRun {
	const x = full([n], 0.1, 'f32');
	const way = (o: AccOpts): Way => ({ sum: sum(x, o).item(), mean: mean(x, o).item() });
	return {
		exact: n * Math.fround(0.1),
		seq: way({ arith: 'fp32', order: 'sequential' }),
		tree: way({ arith: 'fp32', order: 'pairwise' }),
		wide: way({}),
	};
}

// ---------------------------------------------------------------------------------------------------------------
// Challenges.

const same = (s: readonly number[], t: readonly number[]) => s.length === t.length && s.every((n, i) => n === t[i]);

export const CHECKS = {
	/** Column sums of the sheet: shape (4,). */
	colSums: (v: Vals) => {
		const m = model(v);
		return m.src === 'sheet' && m.op === 0 && same(m.out, [4]);
	},
	/** The sheet's rows reduced but the axis kept: shape (3, 1). */
	keep: (v: Vals) => {
		const m = model(v);
		return m.src === 'sheet' && same(m.out, [3, 1]);
	},
	/** The brightest pixel of the image: max over every axis. */
	brightest: (v: Vals) => {
		const m = model(v);
		return m.src === 'gray' && m.op === 2 && m.axis === null;
	},
	/** The block reduced over its last axis, written with a negative axis. */
	negative: (v: Vals) => {
		const m = model(v);
		return m.src === 'block' && m.axis === -1;
	},
};
