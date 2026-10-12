/**
 * Lesson 1.3 (length, Σ notation, normalization): the store contract and every derived number the LN scenes, formulas,
 * tables, N-panel rows and challenges read (storyboard §2 1.3). Pure and three-free, so the tests import it directly.
 *
 * Keys of the `length` store:
 * - `v` (2D) and `v3` (3D): the arrows whose length LN-1 / LN-1-3D unfold as a sum of component squares;
 * - `w` (2D) and `w3` (3D): the arrows LN-2 normalizes, `k` the blend 0..1 from w to w/|w|;
 * - `u`: the eight-component vector of LN-3 (bars).
 *
 * Domain rule: the zero vector has length 0 and no direction, so its normalization is undefined (NaN components,
 * shown as a dash with a reason); everything else (squares, sums, the length itself) is always defined.
 */
import { add, dot, scale, type V } from '../geom';
import { snap } from '../scene/drag';
import type { Move } from '../scene/handles';
import type { LessonState, Val } from '../store';

export const LESSON = 'length';

/** Component boxes of the reader-writable keys; `k` is the normalization blend. */
export const SPEC = {
	v: { min: -4, max: 4, step: 0.1 },
	v3: { min: -3, max: 3, step: 0.1 },
	w: { min: -4, max: 4, step: 0.1 },
	w3: { min: -3, max: 3, step: 0.1 },
	k: { min: 0, max: 1, step: 0.01 },
	u: { min: -4, max: 4, step: 0.1 },
} as const;

/** Number of components of LN-3's vector (the prose's 8-component example). */
export const N_BARS = 8;

/**
 * Defaults. v = (3, 4) and u are the prose's worked examples (|v| = 5, |u| = √30); w starts at (2, 1), not at (3, 4),
 * so the challenge "make v̂ = (0.6, 0.8)" is not solved on load; v3 = (2, 1, 2) has the whole length 3.
 */
export const INITIAL: Record<string, Val> = {
	v: [3, 4],
	v3: [2, 1, 2],
	w: [2, 1],
	w3: [2, 2, 1],
	k: 0,
	u: [2, 1, 2, 0, 4, 0, 1, 2],
};

type Vals = LessonState['vals'];

const vec = (v: Val | undefined, key: string): number[] => {
	if (!Array.isArray(v)) throw new Error(`length: "${key}" must be a vector`);
	return [...v];
};

// ---------------------------------------------------------------------------------------------------------------
// Formatting: fixed decimals so a live number never changes width by a trailing zero; never "-0.00".

const fixed = (x: number, n: number) => (+x.toFixed(n) + 0).toFixed(n);
/** Components (on the 0.1 grid): one decimal. */
export const fmt1 = (x: number) => (Number.isFinite(x) ? fixed(x, 1) : '–');
/** Squares, sums, lengths: two decimals. */
export const fmt2 = (x: number) => (Number.isFinite(x) ? fixed(x, 2) : '–');
/** Unit-vector components and their length: three decimals (0.707, 1.000). */
export const fmt3 = (x: number) => (Number.isFinite(x) ? fixed(x, 3) : '–');
/** A vector as "(a, b, c)" with `f` per component. */
export const fmtVec = (v: V, f: (x: number) => string = fmt1) => '(' + v.map(f).join(', ') + ')';
/** The same formats for `\s{}` values inside TeX: an undefined value is `\text{--}` (the renderer rejects "–"). */
export const texFmt = {
	c: (x: number) => (Number.isFinite(x) ? fixed(x, 1) : String.raw`\text{--}`),
	s: (x: number) => (Number.isFinite(x) ? fixed(x, 2) : String.raw`\text{--}`),
	u: (x: number) => (Number.isFinite(x) ? fixed(x, 3) : String.raw`\text{--}`),
};

// ---------------------------------------------------------------------------------------------------------------
// Length as a sum of squares.

/**
 * Everything the Σ of a vector determines: the squares v_i², the running sums after each component (the timeline's
 * "the sum grows one square at a time"), the total Σ v_i² and the length √Σ.
 */
export function sumOf(v: V) {
	if (!v.every(Number.isFinite)) throw new Error('length: components must be finite');
	const sq = v.map((x) => x * x);
	const run: number[] = [];
	sq.reduce((s, x) => (run.push(s + x), s + x), 0);
	const s2 = run[run.length - 1] ?? 0;
	return { comps: [...v], sq, run, s2, len: Math.sqrt(s2) };
}

/**
 * The walk along the axes that LN-1-3D draws, as in lesson 0.3 and the prose: along x, then along z (both on the
 * floor, y = 0, of the y-up world), then up along y. Corners c0 = O, c1 = (x, 0, 0), c2 = (x, 0, z), c3 = v; `d1` =
 * the floor diagonal O->c2 of the first right triangle. `order` = the component index (0-based) of each leg in walk
 * order: a sum does not depend on the order of its terms, so the Σ table and formula follow the walk.
 */
export const WALK = [0, 2, 1] as const;
export function walk3(v: V) {
	if (v.length !== 3) throw new Error('walk3 is 3D');
	const [x, y, z] = v;
	return { c0: [0, 0, 0], c1: [x, 0, 0], c2: [x, 0, z], c3: [x, y, z], d1: Math.hypot(x, z) };
}

/**
 * Plane normal for an AreaTile on edge a->b lying in the plane through a, b with normal `n`, chosen so that the tile
 * (on the side (b - a) × normal) faces AWAY from the triangle's third vertex `third`. When the third vertex is on the
 * line (a degenerate triangle) either side is fine and `n` is returned.
 */
export function outward(a: V, b: V, third: V, n: V): number[] {
	const e = b.map((x, i) => x - a[i]);
	const r = [e[1] * n[2] - e[2] * n[1], e[2] * n[0] - e[0] * n[2], e[0] * n[1] - e[1] * n[0]];
	const toThird = third.map((x, i) => x - a[i]);
	return dot(r, toThird) > 0 ? scale(n, -1) : [...n];
}

/**
 * The plane of the second right triangle of the 3D walk (O, c2, v): the vertical plane through the floor diagonal
 * d = (x, 0, z), so its normal is d × y = (−z, 0, x)/|d|. With no diagonal (x = z = 0) the triangle is degenerate
 * and the x-y plane's normal z is used.
 */
export function upPlaneNormal(v: V): number[] {
	const [x, , z] = v;
	const d = Math.hypot(x, z);
	return d > 1e-12 ? [-z / d, 0, x / d] : [0, 0, 1];
}

/** 2D: the triangle O, (x, 0), v turns counter-clockwise when x·y > 0 (AreaTile `flip` follows it). */
export const ccw2 = (v: V) => v[0] * v[1] > 0;

// ---------------------------------------------------------------------------------------------------------------
// Normalization.

/** Below this length a vector counts as zero (only the exact zero is reachable on the 0.1 grid). */
export const EPS = 1e-12;

/**
 * v / |v|, or null for the zero vector (no direction to keep). The normalized length is 1 up to rounding, never
 * compared with `===` (lesson text: compare with a tolerance).
 */
export function unit(v: V): number[] | null {
	const n = Math.hypot(...v);
	return n > EPS ? scale(v, 1 / n) : null;
}

/**
 * The blend of LN-2: k = 0 is v itself, k = 1 is v̂, and in between the arrow keeps its direction while its length
 * moves linearly from |v| to 1 (the scalar (1 − k) + k/|v| is positive, so the direction never flips). Zero stays zero.
 */
export function blend(v: V, k: number): number[] {
	const n = Math.hypot(...v);
	if (!(n > EPS)) return v.map(() => 0);
	return scale(v, 1 - k + k / n);
}

/** Everything LN-2 shows for an arrow w and blend k; undefined quantities are NaN. */
export function norm2(w: V, k: number) {
	const n = Math.hypot(...w);
	const hat = unit(w);
	return {
		w: [...w], n, k, hat,
		hatC: hat ?? w.map(() => NaN),
		hatLen: hat ? Math.hypot(...hat) : NaN,
		mid: blend(w, k),
		midLen: hat ? (1 - k) * n + k : 0,
		zero: hat === null,
	};
}

// ---------------------------------------------------------------------------------------------------------------
// Everything the lesson derives from the store, in one place.

export function derive(vals: Vals) {
	const v = vec(vals.v, 'v'), v3 = vec(vals.v3, 'v3'), u = vec(vals.u, 'u');
	const k = vals.k;
	if (typeof k !== 'number') throw new Error('length: "k" must be a number');
	return {
		s: sumOf(v), s3: sumOf(v3), su: sumOf(u),
		n: norm2(vec(vals.w, 'w'), k), n3: norm2(vec(vals.w3, 'w3'), k),
	};
}

// ---------------------------------------------------------------------------------------------------------------
// LN-3 bar handles: bar i moves only component i, vertically, on the 0.1 grid, inside its box.

/** Drag/key mapping of bar i's handle: the vertical delta changes u_i; the horizontal delta is ignored. */
export function moveBar(i: number): (vals: Vals, raw: readonly number[]) => Move {
	return (vals, raw) => {
		const u = vec(vals.u, 'u');
		const next = snap(u[i] + (raw[1] ?? 0));
		u[i] = Math.min(SPEC.u.max, Math.max(SPEC.u.min, next));
		return { set: { u } };
	};
}

/** Centre x of bar i (bars stand at 0.5, 1.5, ... so the first sits right of the vertical axis). */
export const barX = (i: number) => i + 0.5;

// ---------------------------------------------------------------------------------------------------------------
// Challenge predicates (store values only, so the keyboard and typed values solve them too).

const near = (a: number, b: number, tol = 1e-9) => Math.abs(a - b) <= tol;

export const CHECKS = {
	/** LN-1: another 2D arrow of length exactly 5, i.e. not the starting (3, 4): (4, 3), (0, 5), (−3, 4)... */
	five: (x: Vals) => {
		const v = vec(x.v, 'v');
		return near(sumOf(v).s2, 25) && !(near(v[0], 3) && near(v[1], 4));
	},
	/** LN-1-3D: the arrow (1, 1, 1): length √3, not 3. */
	ones3: (x: Vals) => vec(x.v3, 'v3').every((c) => near(c, 1)),
	/** LN-2: a unit vector equal to (0.6, 0.8) (any positive multiple of (3, 4)). */
	sixEight: (x: Vals) => {
		const h = unit(vec(x.w, 'w'));
		return h !== null && near(h[0], 0.6, 5e-4) && near(h[1], 0.8, 5e-4);
	},
	/** LN-2: the zero vector, the one input normalization cannot handle. */
	zero: (x: Vals) => unit(vec(x.w, 'w')) === null,
	/** LN-2-3D: the blend at 1 (the tip on the unit sphere) for a vector longer than 2. */
	onSphere: (x: Vals) => x.k === 1 && Math.hypot(...vec(x.w3, 'w3')) > 2,
	/** LN-3: Σ u_i² = 16 exactly, so |u| = 4. */
	four: (x: Vals) => near(sumOf(vec(x.u, 'u')).s2, 16),
	/** LN-3: eight ones, |u| = √8 ≈ 2.83. */
	ones8: (x: Vals) => vec(x.u, 'u').every((c) => near(c, 1)),
};

/** Midpoint helper for labels. */
export const mid = (p: V, q: V) => scale(add(p, q), 0.5);
