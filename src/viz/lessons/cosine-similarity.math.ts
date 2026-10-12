import { fixed, int } from '../fmt';
import type { LessonState } from '../store';
import type { V } from '../geom';

/**
 * Lesson 1.7, cosine similarity and embeddings: the one model every scene, formula, N-panel row, widget and challenge
 * reads. Three-free, DOM-free and data-free (the real embedding table lives in cosine-similarity.data.ts), so the pure
 * tests feed it small hand-checkable vectors.
 *
 * Domain rule: a zero vector has no direction, so every cosine and angle involving it is undefined (NaN, shown as a
 * dash with a reason), while dot products and distances stay defined.
 */

export const LESSON = 'cosine-similarity';

/** The toy space of section 1: two hand-made features, "furry" (x) and "has wheels" (y). Not a real embedding. */
export const TOY = ['cat', 'dog', 'car'] as const;
export type Toy = (typeof TOY)[number];

/**
 * Initial store values. Toy arrows as in the prose; `sa`/`sb`/`s` = the scale-invariance scene (a = (3, 4),
 * b = (4, 3), the scale s on b); `pair` = the two word rows of the real table the heatmap selected.
 */
export const INITIAL = {
	cat: [0.9, 0.1],
	dog: [0.8, 0.2],
	car: [0.1, 0.9],
	sa: [3, 4],
	sb: [4, 3],
	s: 1,
	pair: [0, 1],
};

export const RANGE_TOY = { min: -1.2, max: 1.2, step: 0.05 };
export const RANGE_AB = { min: -5, max: 5, step: 0.1 };
export const RANGE_S = { min: 0, max: 2, step: 0.05 };

type Vals = LessonState['vals'];

/** Below this length a vector counts as zero: drags and fields land on multiples of 0.05, never this close. */
export const ZERO = 1e-9;

const DEG = 180 / Math.PI;

export const dot = (a: V, b: V): number => {
	if (a.length !== b.length) throw new Error(`dot: lengths ${a.length} and ${b.length} differ`);
	let s = 0;
	for (let i = 0; i < a.length; i++) s += a[i] * b[i];
	return s;
};
export const len = (a: V): number => Math.sqrt(dot(a, a));
export const dist = (a: V, b: V): number => len(a.map((x, i) => x - b[i]));

/** cos θ = a·b / (|a||b|); NaN when either vector is zero. Clamped to [−1, 1] against rounding. */
export function cosine(a: V, b: V): number {
	const n = len(a) * len(b);
	if (len(a) < ZERO || len(b) < ZERO) return NaN;
	return Math.max(-1, Math.min(1, dot(a, b) / n));
}

/** The angle between two vectors in degrees, NaN when either is zero. */
export const angleDeg = (a: V, b: V): number => Math.acos(cosine(a, b)) * DEG;

/** Each row divided by its length; a zero row stays zero (it has no direction to keep). */
export function unitRows(rows: readonly V[]): number[][] {
	return rows.map((r) => {
		const n = len(r);
		return n < ZERO ? r.map(() => 0) : r.map((x) => x / n);
	});
}

/** Componentwise products aᵢbᵢ: their sum is the dot product (lesson 1.4's table, 300 cells long). */
export const products = (a: V, b: V): number[] => {
	if (a.length !== b.length) throw new Error(`products: lengths ${a.length} and ${b.length} differ`);
	return a.map((x, i) => x * b[i]);
};

/** Cosine of every pair, by normalising the rows first (then each cell is one dot product). */
export function cosMatrix(rows: readonly V[]): number[][] {
	const u = unitRows(rows);
	const zero = rows.map((r) => len(r) < ZERO);
	return u.map((a, i) => u.map((b, j) => (zero[i] || zero[j] ? NaN : Math.max(-1, Math.min(1, dot(a, b))))));
}

/** Plain dot product of every pair (the raw score table attention starts from, before its learned matrices). */
export const dotMatrix = (rows: readonly V[]): number[][] => rows.map((a) => rows.map((b) => dot(a, b)));

/** Softmax of one row: e^(xᵢ − max) / Σ e^(xⱼ − max). Subtracting the max keeps exp from overflowing. */
export function softmax(row: readonly number[]): number[] {
	const m = Math.max(...row);
	const e = row.map((x) => Math.exp(x - m));
	const s = e.reduce((a, b) => a + b, 0);
	return e.map((x) => x / s);
}

/**
 * One attention head with the learned matrices left out (W_Q = W_K = identity): score of token i for token j is
 * xᵢ·xⱼ / √d, each row goes through softmax, so row i holds the weights token i gives to every token (summing to 1).
 */
export function attentionWeights(rows: readonly V[]): number[][] {
	const d = rows[0]?.length ?? 1;
	return dotMatrix(rows).map((r) => softmax(r.map((x) => x / Math.sqrt(d))));
}

export interface Hit {
	i: number;
	cos: number;
}

/**
 * The k rows most similar to `q` by cosine, best first, skipping the row indices in `skip`. A zero query has no
 * direction and returns nothing; zero rows are never hits. Ties keep table order.
 */
export function nearest(rows: readonly V[], q: V, k: number, skip: ReadonlySet<number> = new Set()): Hit[] {
	if (len(q) < ZERO) return [];
	const hits: Hit[] = [];
	rows.forEach((r, i) => {
		if (skip.has(i)) return;
		const c = cosine(r, q);
		if (Number.isFinite(c)) hits.push({ i, cos: c });
	});
	hits.sort((x, y) => y.cos - x.cos || x.i - y.i);
	return hits.slice(0, k);
}

/** a − b + c, componentwise (the analogy query "a is to b as ? is to c"). */
export const analogyQuery = (a: V, b: V, c: V): number[] => a.map((x, i) => x - b[i] + c[i]);

/**
 * Words nearest to a − b + c. `exclude` drops the three inputs from the answers, as every published analogy test
 * does; without it the answer is usually one of the inputs (the honest failure case of the lesson).
 */
export function analogy(rows: readonly V[], a: number, b: number, c: number, k: number, exclude: boolean): Hit[] {
	const q = analogyQuery(rows[a], rows[b], rows[c]);
	return nearest(rows, q, k, exclude ? new Set([a, b, c]) : new Set());
}

/** Documents ranked by cosine with the query, best first (all of them: the widget shows the whole ranking). */
export const rank = (docs: readonly V[], q: V): Hit[] => nearest(docs, q, docs.length);

// ---------------------------------------------------------------------------------------------------------------
// Scene models.

const vec = (v: Vals, key: string): V => v[key] as V;

/** COS-1, the toy space: the three pairwise cosines and the cat–dog / cat–car angles. */
export function toy(v: Vals) {
	const cat = vec(v, 'cat'), dog = vec(v, 'dog'), car = vec(v, 'car');
	return {
		cosCD: cosine(cat, dog),
		cosCC: cosine(cat, car),
		cosDC: cosine(dog, car),
		thCD: angleDeg(cat, dog),
		thCC: angleDeg(cat, car),
		dotCD: dot(cat, dog),
		nCat: len(cat),
		nDog: len(dog),
		zero: TOY.filter((k) => len(vec(v, k)) < ZERO),
	};
}

/** COS-2, scale invariance: b is scaled by s; the cosine with a ignores s (while s > 0), the distance does not. */
export function scaled(v: Vals) {
	const a = vec(v, 'sa'), b = vec(v, 'sb'), s = v.s as number;
	const sb = b.map((x) => x * s);
	return {
		sb,
		na: len(a),
		nb: len(b),
		nsb: len(sb),
		dot: dot(a, sb),
		cos: cosine(a, sb),
		cos1: cosine(a, b),
		th: angleDeg(a, sb),
		dist: dist(a, sb),
	};
}

// ---------------------------------------------------------------------------------------------------------------
// Formatting: a dash for undefined values; `fixed` (shared, identical on server and client) never prints −0.00.

const minus = (s: string) => s.replace('-', '−');

/** Lengths, distances, dot products: 2 decimals. */
export const fmt2 = (x: number) => (Number.isFinite(x) ? minus(fixed(x, 2)) : '–');
/** Cosines: 3 decimals. */
export const fmtCos = (x: number) => (Number.isFinite(x) ? minus(fixed(x, 3)) : '–');
/** Angles: whole degrees (embedding angles are never exact; one decimal would be false precision). */
export const fmtDeg = (x: number) => (Number.isFinite(x) ? int(x) + '°' : '–');
/** Percent with one decimal. */
export const fmtPct = (x: number) => (Number.isFinite(x) ? fixed(x * 100, 1) + '%' : '–');
/** Grouped thousands with a thin space: 400 001 → "400 001" (same in both languages). */
export const fmtInt = (x: number) => int(x).replace(/\B(?=(\d{3})+(?!\d))/g, ' ');

/** The same formats for `\s{}` values inside TeX (`\text{--}` for undefined: TeX rejects raw non-ASCII). */
export const tex = {
	num: (x: number) => (Number.isFinite(x) ? fixed(x, 2) : String.raw`\text{--}`),
	cos: (x: number) => (Number.isFinite(x) ? fixed(x, 3) : String.raw`\text{--}`),
	deg: (x: number) => (Number.isFinite(x) ? int(x) + String.raw`^\circ` : String.raw`\text{--}`),
};

// ---------------------------------------------------------------------------------------------------------------
// Challenges (store values only, so keyboard and typed values solve them too).

export const CHECKS = {
	/** COS-1: make cat and car agree (cosine above 0.95) by moving one of them. */
	catCar: (v: Vals) => toy(v).cosCC > 0.95,
	/** COS-1: make dog unrelated to cat (cosine 0 up to the 0.05 grid). */
	perp: (v: Vals) => Math.abs(toy(v).cosCD) < 0.02,
	/** COS-2: a distance at least twice the starting one while the cosine still equals its starting 0.96. */
	farSame: (v: Vals) => {
		const z = scaled(v);
		return z.dist >= 2 * Math.SQRT2 && Math.abs(z.cos - 0.96) < 1e-9;
	},
	/** COS-2: s = 0 makes s·b the zero vector, whose cosine is undefined. */
	zero: (v: Vals) => (v.s as number) === 0,
};
