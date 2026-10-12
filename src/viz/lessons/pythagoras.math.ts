/**
 * Lesson 0.3 (distance and Pythagoras): the store contract and every derived number of its five scenes. Pure and
 * three-free: the entry, the scenes, the N-panel rows and the tests share it.
 *
 * One store, `pythagoras`:
 * - 0.3-1 (2D distance): P, Q.
 * - 0.3-2 (dissection proof): a, b and the timeline `dis`.
 * - 0.3-3 (3D, two Pythagoras steps): P3, Q3 (y-up, the floor is the xz plane) and the timeline `room`.
 * - 0.3-4 (n dimensions): n and the components p1..p8, q1..q8 (scalar keys, so each one is a scrubbable number).
 * - 0.3-5 (squared distance vs distance): X and r.
 */
import { fixed, trim } from '../fmt';
import type { LessonState, Val } from '../store';
import type { V } from '../geom';

export const LESSON = 'pythagoras';

/** Most dimensions the n-D scene offers (the store keeps all components; only the first n count). */
export const MAX_N = 8;
const IDX = Array.from({ length: MAX_N }, (_, i) => i + 1);

/** Ranges of the reader-writable keys. */
export const SPEC = {
	P: { min: -2, max: 8, step: 0.1 },
	/** Up to 4: the big square (side a + b <= 8) stays inside the framed view. */
	ab: { min: 0.5, max: 4, step: 0.1 },
	P3: { min: -3, max: 4, step: 0.1 },
	n: { min: 1, max: MAX_N, step: 1 },
	comp: { min: -5, max: 5, step: 0.5 },
	X: { min: -4.5, max: 4.5, step: 0.1 },
	r: { min: 0.5, max: 4, step: 0.1 },
} as const;

/**
 * Defaults = the prose's worked examples: (1, 2) -> (4, 6) is the 3-4-5 triangle, (0, 0, 0) -> (1, 2, 2) has legs 1, 2,
 * 2 and length 3, the 4D pair (0, 0, 0, 0) -> (1, 0, 2, 2) has length 3. The fifth q component (4) makes the 5D
 * length 5: one more dimension visibly changes the sum.
 */
const Q_ND = [1, 0, 2, 2, 4, 0, 2, 1];
export const INITIAL: Record<string, Val> = {
	P: [1, 2], Q: [4, 6],
	a: 3, b: 4,
	P3: [0, 0, 0], Q3: [1, 2, 2],
	n: 4,
	...Object.fromEntries(IDX.flatMap((i) => [['p' + i, 0], ['q' + i, Q_ND[i - 1]]])),
	X: [2, 1.5], r: 3,
};

type Vals = LessonState['vals'];

const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number' || !Number.isFinite(v)) throw new Error(`pythagoras: "${key}" must be a finite number`);
	return v;
};
const vec = (v: Val | undefined, key: string, dims: number): number[] => {
	if (typeof v === 'number' || !v || v.length !== dims || !v.every(Number.isFinite)) throw new Error(`pythagoras: "${key}" must be a finite ${dims}-vector`);
	return [...v];
};

/** Sum of squares of the differences q - p, then its root: the one recipe of every scene. */
export function distance(p: readonly number[], q: readonly number[]): { legs: number[]; squares: number[]; d2: number; d: number } {
	if (p.length !== q.length) throw new Error('distance: points of different dimension');
	const legs = q.map((x, i) => x - p[i]);
	const squares = legs.map((x) => x * x);
	const d2 = squares.reduce((s, x) => s + x, 0);
	return { legs, squares, d2, d: Math.sqrt(d2) };
}

/**
 * The naive norm (square, add, root) and the rescaled one (`hypot`): divide by the largest |component| first, so no
 * square overflows to infinity or underflows to zero; multiply back at the end. Both agree whenever the naive one is
 * representable. The machine layer of the page states this; the tests check it at 1e200 and 1e-200.
 */
export const naiveNorm = (v: readonly number[]): number => Math.sqrt(v.reduce((s, x) => s + x * x, 0));
export function scaledNorm(v: readonly number[]): number {
	const m = v.reduce((s, x) => Math.max(s, Math.abs(x)), 0);
	if (m === 0 || !Number.isFinite(m)) return m;
	return m * Math.sqrt(v.reduce((s, x) => s + (x / m) ** 2, 0));
}

/** 0.3-1: P, Q, the corner R where the legs meet (walk east, then north), legs, squares, distance, grid walk. */
export function derive2(v: Vals) {
	const P = vec(v.P, 'P', 2), Q = vec(v.Q, 'Q', 2);
	const { legs: [dx, dy], d2, d } = distance(P, Q);
	return {
		P, Q, R: [Q[0], P[1]] as V,
		dx, dy, dx2: dx * dx, dy2: dy * dy, d2, d,
		/** The grid walk (Manhattan distance): the two legs end to end. */
		walk: Math.abs(dx) + Math.abs(dy),
		/** P -> R -> Q turns left (counter-clockwise) when dx·dy > 0; squares go outward on the right otherwise (flip). */
		ccw: dx * dy > 0,
	};
}

// ---------------------------------------------------------------------------------------------------------------
// 0.3-2: the four-triangle dissection.

/** Steps of the `dis` timeline: 0 tilted square, 1 count, 2 slide T3, 3 slide T2 and T4, 4 two squares. */
export const DIS_STEPS = 5;
/** First step at which each triangle sits in the second arrangement (T1 never moves). */
const MOVES_AT = [Infinity, 3, 2, 3] as const;

/**
 * Triangle k (0..3) of the dissection at a timeline step, inside the big square [0, s]² with s = a + b.
 *
 * Arrangement A (each right angle in a corner of the big square, the hypotenuses fence a tilted square of side c):
 *   T1 (0,0),(a,0),(0,b) · T2 (s,0),(s,a),(a,0) · T3 (s,s),(b,s),(s,a) · T4 (0,s),(0,b),(b,s).
 * T3 is T1 turned half a turn, T4 is T2 turned half a turn, so each pair closes into a rectangle by SLIDING only:
 * arrangement B moves T3 by (−b, −a) (rectangle a × b at the origin with T1), T2 by (0, b) and T4 by (a, 0)
 * (rectangle b × a in the opposite corner). Pure translations: no piece changes shape or area while it moves, which
 * is the whole argument of the proof. What B leaves uncovered is the square [0, a] × [b, s] (side a) and the square
 * [a, s] × [0, b] (side b).
 */
export function triangle(a: number, b: number, k: 0 | 1 | 2 | 3, step: number): V[] {
	const s = a + b;
	const A: V[][] = [
		[[0, 0], [a, 0], [0, b]],
		[[s, 0], [s, a], [a, 0]],
		[[s, s], [b, s], [s, a]],
		[[0, s], [0, b], [b, s]],
	];
	const shift: V[] = [[0, 0], [0, b], [-b, -a], [a, 0]];
	const moved = step >= MOVES_AT[k];
	return A[k].map(([x, y]) => (moved ? [x + shift[k][0], y + shift[k][1]] : [x, y]));
}

/** The uncovered regions: the tilted square (arrangement A) and the squares of side a and side b (arrangement B). */
export function leftovers(a: number, b: number) {
	const s = a + b;
	return {
		c2: [[a, 0], [s, a], [b, s], [0, b]] as V[],
		a2: [[0, b], [a, b], [a, s], [0, s]] as V[],
		b2: [[a, 0], [s, 0], [s, b], [a, b]] as V[],
	};
}

/** Shoelace area (signed: positive for counter-clockwise vertices). */
export function polyArea(pts: readonly V[]): number {
	let sum = 0;
	for (let i = 0; i < pts.length; i++) {
		const [x0, y0] = pts[i], [x1, y1] = pts[(i + 1) % pts.length];
		sum += x0 * y1 - x1 * y0;
	}
	return sum / 2;
}

export function deriveDis(v: Vals) {
	const a = num(v.a, 'a'), b = num(v.b, 'b');
	const c2 = a * a + b * b;
	return { a, b, s: a + b, big: (a + b) ** 2, tri: (a * b) / 2, tris: 2 * a * b, c2, c: Math.sqrt(c2), a2: a * a, b2: b * b };
}

// ---------------------------------------------------------------------------------------------------------------
// 0.3-3: 3D, y-up (Maya, Houdini): the floor is the xz plane, height is y.

export function derive3(v: Vals) {
	const P = vec(v.P3, 'P3', 3), Q = vec(v.Q3, 'Q3', 3);
	const [dx, dy, dz] = distance(P, Q).legs;
	const f2 = dx * dx + dz * dz;
	const d2 = f2 + dy * dy;
	return {
		P: P as V, Q: Q as V,
		/** Walk along x first (corner X), then along z to F, the point under (or over) Q on P's floor. */
		X: [Q[0], P[1], P[2]] as V,
		F: [Q[0], P[1], Q[2]] as V,
		dx, dy, dz, dx2: dx * dx, dy2: dy * dy, dz2: dz * dz,
		f2, f: Math.sqrt(f2), d2, d: Math.sqrt(d2),
	};
}

/** Steps of the `room` timeline: 0 floor triangle, 1 upright triangle, 2 the whole recipe. */
export const ROOM_STEPS = 3;

/** Unit normal of the triangle a, b, c taken counter-clockwise (its squares face outward with this normal); null if flat. */
export function triNormal(a: V, b: V, c: V): V | null {
	const u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]], w = [c[0] - b[0], c[1] - b[1], c[2] - b[2]];
	const n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
	const len = Math.hypot(n[0], n[1], n[2]);
	return len > 1e-9 ? n.map((x) => x / len) : null;
}

// ---------------------------------------------------------------------------------------------------------------
// 0.3-4: n dimensions.

export const pKey = (i: number) => 'p' + i;
export const qKey = (i: number) => 'q' + i;

export function deriveN(v: Vals) {
	const n = num(v.n, 'n');
	if (!Number.isInteger(n) || n < 1 || n > MAX_N) throw new Error(`pythagoras: n = ${n} outside 1..${MAX_N}`);
	const p = IDX.slice(0, n).map((i) => num(v[pKey(i)], pKey(i)));
	const q = IDX.slice(0, n).map((i) => num(v[qKey(i)], qKey(i)));
	return { n, p, q, ...distance(p, q) };
}

// ---------------------------------------------------------------------------------------------------------------
// 0.3-5: inside the radius, compared without a root.

export function deriveR(v: Vals) {
	const X = vec(v.X, 'X', 2), r = num(v.r, 'r');
	const d2 = X[0] * X[0] + X[1] * X[1];
	const r2 = r * r;
	return {
		x: X[0], y: X[1], x2: X[0] * X[0], y2: X[1] * X[1], d2, d: Math.sqrt(d2), r, r2,
		/** Which side, from the squares alone (the same answer as d vs r, since both are non-negative). */
		side: (near(d2, r2) ? 'on' : d2 < r2 ? 'in' : 'out') as 'in' | 'on' | 'out',
	};
}

/** Comparator of the challenges: drags and keys land on 0.1 grids, so allow rounding only. */
export const near = (a: number, b: number): boolean => Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) < 1e-9;

/** Challenge predicates; store reads only, so typing in an N panel alone solves each. */
export const CHECKS = {
	/** 0.3-1: a 3-4-5 triangle somewhere else than the default. */
	fiveElsewhere: (v: Vals) => {
		const z = derive2(v);
		return near(z.d2, 25) && !(near(z.P[0], 1) && near(z.P[1], 2) && near(z.Q[0], 4) && near(z.Q[1], 6));
	},
	/** 0.3-1: a straight path exactly as long as the grid walk (one leg zero). */
	walkEqual: (v: Vals) => { const z = derive2(v); return z.d > 0 && near(z.d, z.walk); },
	/** 0.3-2: a whole-number long side from other sides than 3 and 4 (e.g. 1.8 and 2.4 give 3). */
	wholeC: (v: Vals) => {
		const z = deriveDis(v);
		const default34 = (near(z.a, 3) && near(z.b, 4)) || (near(z.a, 4) && near(z.b, 3));
		return !default34 && Math.abs(z.c - Math.round(z.c)) < 1e-9;
	},
	/** 0.3-2: watched the slide to the end. */
	slid: (_v: Vals, step: Readonly<Record<string, number>>) => (step['dis:max'] ?? 0) >= DIS_STEPS - 1,
	/** 0.3-3: Q straight above P (floor diagonal zero: the distance is just the height). */
	above: (v: Vals) => { const z = derive3(v); return near(z.f2, 0) && z.d > 0; },
	/** 0.3-3: a box diagonal of length exactly 3 other than the default box. */
	threeElsewhere: (v: Vals) => {
		const z = derive3(v);
		return near(z.d2, 9) && !(near(Math.abs(z.dx), 1) && near(Math.abs(z.dy), 2) && near(Math.abs(z.dz), 2));
	},
	/** 0.3-4: eight dimensions with distance 5. */
	eightFive: (v: Vals) => { const z = deriveN(v); return z.n === MAX_N && near(z.d2, 25); },
	/** 0.3-5: X exactly on the circle. */
	onCircle: (v: Vals) => deriveR(v).side === 'on',
};

/** One decimal for coordinates and legs; a negative zero prints as 0. */
export const fmt1 = (v: number): string => fixed(v, 1);
/** Two decimals for squares, sums and lengths. */
export const fmt2 = (v: number): string => fixed(v, 2);
/** Integers stay integers ("4"), other values get up to 2 decimals ("2.5", "0.25"): table cells. */
export const fmtCell = (v: number): string => trim(v, 2);
