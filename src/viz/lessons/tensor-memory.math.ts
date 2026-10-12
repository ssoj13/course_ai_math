/**
 * Lesson 3.3 (memory: flat buffer, strides, reshape / view / transpose / contiguous): the store contract and every
 * derived number. Pure and three-free (entry, scenes, rows and tests share it). Every view, copy and refusal is the
 * tensor core's (src/core): this module only chains core calls and reads their shape, strides and offset.
 *
 * Store `tensor-memory`:
 * - card 3.3-1 (the view chain over one buffer of 24 numbers, x = arange(24)):
 *   `shp` which shape x is read as, `ord` 0 = row-major (C) / 1 = column-major (F), `op` the view applied to x,
 *   `cont` 0/1 `ascontiguousarray` after it, `re` the reshape tried last, `sel` the selected cell of the result as its
 *   step in a row-major walk.
 * - card 3.3-2 (cache lines): `col` 0 = row walk / 1 = column walk, `lines` cache capacity in lines, `step` how far
 *   the walk has gone.
 */
import { contiguous, isContiguous, offsets, reshape, ShapeError, slice, Tensor, transpose, type SliceSpec } from '../../core';
import type { LessonLinks, LinkGroup } from '../links';
import { finite } from '../links';
import type { LessonState, Val } from '../store';
import { unravel } from './tensor-cells';

export const LESSON = 'tensor-memory';
type Vals = LessonState['vals'];

const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`tensor-memory: "${key}" must be a number`);
	finite(v);
	return v;
};
const int = (v: Val | undefined, key: string): number => Math.round(num(v, key));
const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

// ---------------------------------------------------------------------------------------------------------------
// 3.3-1: one buffer, many views.

/** Elements of the buffer x lives in. */
export const BUF = 24;
/** The buffer: 0, 1, ..., 23, so every value is also its own address (NumPy's arange). */
export const DATA = Float64Array.from({ length: BUF }, (_, i) => i);

/** Shapes x can be read as (all hold 24 numbers). */
export const SHAPES: readonly (readonly number[])[] = [[24], [4, 6], [6, 4], [2, 3, 4], [3, 2, 4], [2, 2, 6], [2, 2, 2, 3]];

export type OpId = 'none' | 'T' | 'step' | 'rev' | 'idx' | 'crop';
export interface Op {
	id: OpId;
	/** Python for this view of x (rank-independent spelling with `...`). */
	code: string;
	apply(x: Tensor): Tensor;
}

/** `[..., s]`: all of every axis but the last, `s` on the last (rank >= 1). */
const last = (x: Tensor, s: SliceSpec): SliceSpec[] => [...Array<SliceSpec>(x.ndim - 1).fill(null), s];

export const OPS: readonly Op[] = [
	{ id: 'none', code: 'y = x', apply: (x) => x },
	{ id: 'T', code: 'y = x.T', apply: (x) => transpose(x) },
	{ id: 'step', code: 'y = x[..., ::2]', apply: (x) => slice(x, last(x, [null, null, 2])) },
	{ id: 'rev', code: 'y = x[::-1]', apply: (x) => slice(x, [[null, null, -1]]) },
	{ id: 'idx', code: 'y = x[1]', apply: (x) => slice(x, [1]) },
	{ id: 'crop', code: 'y = x[..., 1:3]', apply: (x) => slice(x, last(x, [1, 3])) },
];

/** Reshapes tried at the end of the chain (0 = none); -1 = "whatever fits". (5, -1) never fits a size of 24, 12 or 8. */
export const RESHAPES: readonly (readonly number[] | null)[] = [null, [-1], [2, -1], [-1, 2], [5, -1]];

/** Why a reshape was refused: the numbers do not fit the new shape, or the memory cannot be read with strides. */
export type Refusal = 'size' | 'contig';

export interface Chain {
	/** x itself: the buffer read as SHAPES[shp] in C or F order. */
	x: Tensor;
	/** The view after the op (still x's buffer). */
	view: Tensor;
	/** True when `ascontiguousarray` made a new buffer (it returns the view itself when it already is contiguous). */
	copied: boolean;
	/** The final tensor (after the reshape, if it was accepted). */
	res: Tensor;
	refused: Refusal | null;
	/** The last tensor of the chain that still lives in x's buffer (what the x strip shows). */
	onX: Tensor;
	code: string[];
}

/** Python literal of a shape for the code lines: `(2, 3, 4)`, `-1`, `(2, -1)`. */
const pyShape = (s: readonly number[]): string => (s.length === 1 ? String(s[0]) : `(${s.join(', ')})`);

/**
 * Builds the chain for the store values. Column-major x = the transposed view of the row-major read of the reversed
 * shape: exactly what `order='F'` stores, with no stride rule of our own.
 */
export function chain(v: Vals): Chain {
	const shape = SHAPES[clamp(int(v.shp, 'shp'), 0, SHAPES.length - 1)];
	const F = int(v.ord, 'ord') === 1;
	const x = F ? transpose(new Tensor(DATA, [...shape].reverse())) : new Tensor(DATA, shape);
	const op = OPS[clamp(int(v.op, 'op'), 0, OPS.length - 1)];
	const view = op.apply(x);
	const after = int(v.cont, 'cont') === 1 ? contiguous(view) : view;
	const copied = after !== view;
	const target = RESHAPES[clamp(int(v.re, 're'), 0, RESHAPES.length - 1)];
	let res = after, refused: Refusal | null = null;
	if (target) {
		try {
			res = reshape(after, target);
		} catch (e) {
			if (!(e instanceof ShapeError)) throw e;
			refused = why(after, target);
		}
	}
	const code = [`x = np.arange(24).reshape(${pyShape(shape)}${F ? ", order='F'" : ''})`, op.code];
	if (int(v.cont, 'cont') === 1) code.push('y = np.ascontiguousarray(y)');
	if (target) code.push(`y = y.reshape(${pyShape(target)})`);
	return { x, view, copied, res, refused, onX: copied ? view : res, code };
}

/**
 * The core refuses a reshape for one of two reasons; ask the core which: if the same reshape of a contiguous copy
 * works, contiguity was the problem, otherwise the sizes do not fit.
 */
function why(t: Tensor, target: readonly number[]): Refusal {
	if (isContiguous(t)) return 'size';
	try {
		reshape(contiguous(t), target);
		return 'contig';
	} catch (e) {
		if (!(e instanceof ShapeError)) throw e;
		return 'size';
	}
}

export interface Walk {
	/** pos[k] = buffer position of the k-th cell of a row-major walk over `t`. */
	pos: Int32Array;
	/** step[p] = the walk step that reads buffer position p, or -1 if the view does not reach p. */
	step: Int32Array;
}

/** The row-major walk of a view over its buffer (core `offsets` plus the view's offset). */
export function walkOf(t: Tensor): Walk {
	const pos = offsets(t.shape, t.strides).map((r) => r + t.offset);
	const step = new Int32Array(t.data.length).fill(-1);
	pos.forEach((p, k) => (step[p] = k));
	return { pos, step };
}

export interface Selected {
	k: number;
	idx: number[];
	/** Buffer position of the selected cell (in the result's buffer). */
	addr: number;
	value: number;
}

export function selected(res: Tensor, sel: number): Selected {
	const k = clamp(sel, 0, res.size - 1);
	const idx = unravel(k, res.shape);
	const addr = res.pos(idx);
	return { k, idx, addr, value: res.data[addr] };
}

// ---------------------------------------------------------------------------------------------------------------
// 3.3-2: cache lines.

/** The matrix walked: N x N float32, so one row is 16 · 4 = 64 bytes = one cache line of a current x86 CPU. */
export const N = 16;
export const ITEM = 4;
export const LINE_BYTES = 64;
export const PER_LINE = LINE_BYTES / ITEM;

/** Element of the matrix the k-th step reads: row by row, or column by column. */
export const cellAt = (k: number, col: boolean): [number, number] => (col ? [k % N, Math.floor(k / N)] : [Math.floor(k / N), k % N]);
/** Cache line of element (r, c) of the row-major matrix (address 16 r + c elements, 4 bytes each). */
export const lineOf = (r: number, c: number): number => Math.floor(((r * N + c) * ITEM) / LINE_BYTES);

export interface CacheRun {
	/** hit[k]: step k found its line already in the cache. */
	hit: Uint8Array;
	/** misses[k]: misses in steps 0..k. */
	misses: Int32Array;
}

/**
 * A toy cache: fully associative, `lines` lines, least recently used line evicted, no prefetching. Real CPUs have
 * several levels, sets and prefetchers (which make the row walk even cheaper); the toy keeps only what makes the
 * difference visible: memory arrives a whole line at a time.
 */
export function runCache(col: boolean, lines: number): CacheRun {
	if (!Number.isInteger(lines) || lines < 1) throw new Error(`tensor-memory: cache of ${lines} lines`);
	const total = N * N;
	const hit = new Uint8Array(total), misses = new Int32Array(total);
	const lru: number[] = [];
	let m = 0;
	for (let k = 0; k < total; k++) {
		const [r, c] = cellAt(k, col);
		const line = lineOf(r, c);
		const at = lru.indexOf(line);
		if (at >= 0) {
			hit[k] = 1;
			lru.splice(at, 1);
		} else {
			m++;
			if (lru.length === lines) lru.pop();
		}
		lru.unshift(line);
		misses[k] = m;
	}
	return { hit, misses };
}

/** Lines held after step k (most recently used first): replays the run up to k. */
export function cacheAt(col: boolean, lines: number, k: number): number[] {
	const lru: number[] = [];
	for (let j = 0; j <= k; j++) {
		const line = lineOf(...cellAt(j, col));
		const at = lru.indexOf(line);
		if (at >= 0) lru.splice(at, 1);
		else if (lru.length === lines) lru.pop();
		lru.unshift(line);
	}
	return lru;
}

export interface CacheModel {
	col: boolean;
	lines: number;
	step: number;
	run: CacheRun;
	held: number[];
	cell: [number, number];
}

export function cacheModel(v: Vals): CacheModel {
	const col = int(v.col, 'col') === 1;
	const lines = clamp(int(v.lines, 'lines'), 1, N);
	const step = clamp(int(v.step, 'step'), 0, N * N - 1);
	return { col, lines, step, run: runCache(col, lines), held: cacheAt(col, lines, step), cell: cellAt(step, col) };
}

// ---------------------------------------------------------------------------------------------------------------
// The store.

export const INITIAL: Record<string, Val> = { shp: 3, ord: 0, op: 1, cont: 0, re: 2, sel: 1, col: 1, lines: 4, step: 20 };

function group(keys: readonly string[], canonical: string, fix: (get: (k: string) => number) => Record<string, Val>): LinkGroup {
	return {
		keys,
		canonical,
		resolve(input, current) {
			return fix((k) => num(Object.hasOwn(input, k) ? input[k] : current[k], k));
		},
		shown: () => ({}),
	};
}

export const LINKS: LessonLinks = {
	groups: [
		group(['shp', 'ord', 'op', 'cont', 're', 'sel'], 'sel', (get) => {
			const out = {
				shp: clamp(Math.round(get('shp')), 0, SHAPES.length - 1),
				ord: Math.round(get('ord')) === 1 ? 1 : 0,
				op: clamp(Math.round(get('op')), 0, OPS.length - 1),
				cont: Math.round(get('cont')) === 1 ? 1 : 0,
				re: clamp(Math.round(get('re')), 0, RESHAPES.length - 1),
				sel: 0,
			};
			// The selection is clamped to the new result, so a smaller view never leaves it outside.
			out.sel = clamp(Math.round(get('sel')), 0, chain(out).res.size - 1);
			return out;
		}),
		group(['col', 'lines', 'step'], 'step', (get) => ({
			col: Math.round(get('col')) === 1 ? 1 : 0,
			lines: clamp(Math.round(get('lines')), 1, N),
			step: clamp(Math.round(get('step')), 0, N * N - 1),
		})),
	],
};

export const fmtInt = (x: number): string => String(Math.round(x));

export const CHECKS = {
	/** x.T made contiguous: a copy, after which reshape(2, -1) is accepted. */
	copyThenView: (v: Vals) => {
		const c = chain(v);
		return OPS[int(v.op, 'op')].id === 'T' && c.copied && RESHAPES[int(v.re, 're')] !== null && c.refused === null;
	},
	/** A view that reaches the buffer backwards (a negative stride) is selected and its cell at address 0 chosen. */
	negativeStride: (v: Vals) => {
		const c = chain(v);
		return !c.copied && c.res.strides.some((s) => s < 0) && selected(c.res, int(v.sel, 'sel')).addr === 0;
	},
	/** Column-major x read with `.T` is contiguous (the transpose of an F-order array is C-order). */
	fThenT: (v: Vals) => {
		const c = chain(v);
		return int(v.ord, 'ord') === 1 && OPS[int(v.op, 'op')].id === 'T' && c.x.ndim >= 2 && isContiguous(c.view);
	},
	/** The column walk with the smallest cache that misses only once per line. */
	columnFits: (v: Vals) => {
		const m = cacheModel(v);
		return m.col && m.lines === N;
	},
};
