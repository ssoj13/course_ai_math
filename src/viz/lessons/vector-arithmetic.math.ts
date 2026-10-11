/**
 * Lesson 1.2 (adding, scaling and linear combinations): the store contract and every derived number of its five
 * scenes. Pure and three-free: the entry, the scenes, the N-panel rows and the tests share it.
 *
 * Scenes and their keys (each scene owns its keys, so a drag in one card never rewrites another card's readout):
 * - VA-1 head-to-tail sum: `a`, `b`.
 * - VA-2 scaling: `c` (the arrow), `s` (the scalar).
 * - VA-3 linear combination: `la`, `lb`, `ls`, `lt`, `paint` (1 = stamp every reached tip).
 * - VA-4 three arrows in 3D: `a3`, `b3`, `c3`, `s3`, `t3`, `u3`.
 * - VA-5 blend shapes: `w1`, `w2` (the weights of the smile and jaw targets).
 */
import type { LessonState, Val } from '../store';
import { finite } from '../links';
import { add, norm, signedArea2, scale, type V } from '../geom';

export const LESSON = 'vector-arithmetic';

type Vals = LessonState['vals'];

export const RANGE_AB = { min: -4, max: 4, step: 0.1 } as const;
export const RANGE_C = { min: -2, max: 2, step: 0.1 } as const;
export const RANGE_S = { min: -3, max: 3, step: 0.1 } as const;
export const RANGE_L = { min: -3, max: 3, step: 0.1 } as const;
export const RANGE_ST = { min: -2, max: 2, step: 0.1 } as const;
export const RANGE_3 = { min: -2.5, max: 2.5, step: 0.1 } as const;
export const RANGE_W = { min: -1, max: 1.5, step: 0.05 } as const;

/**
 * VA-5's face: a mouth of six vertices (left corner, upper lip left/right, right corner, lower lip right/left) and the
 * two blend-shape targets stored as deltas per vertex. Vertex 3, the right corner, is the prose's worked example:
 * base (1.0, 0.5), smile delta (0.2, 0.3), jaw delta (0.0, −0.4).
 */
export const MOUTH: readonly V[] = [[-1, 0.5], [-0.4, 0.8], [0.4, 0.8], [1, 0.5], [0.4, 0.2], [-0.4, 0.2]];
export const SMILE: readonly V[] = [[-0.2, 0.3], [0, 0.05], [0, 0.05], [0.2, 0.3], [0, 0.1], [0, 0.1]];
export const JAW: readonly V[] = [[0, -0.4], [0, 0], [0, 0], [0, -0.4], [0, -0.8], [0, -0.8]];
/** The vertex the formula and the label follow. */
export const CORNER = 3;
/** Face furniture around the mouth (not blended): the head outline and the eyes. */
export const HEAD = { center: [0, 1.1] as V, r: 2.2 };
export const EYES: readonly V[] = [[-0.8, 1.9], [0.8, 1.9]];

/** Defaults: the prose's numbers where a challenge does not ask for them. Every challenge starts unsolved. */
export const INITIAL: Record<string, Val> = {
	a: [1, 2],
	b: [3, -1],
	c: [2, -1],
	s: 3,
	la: [1, 2],
	lb: [3, -1],
	ls: 1,
	lt: 1,
	paint: 0,
	a3: [2, 0, 0.5],
	b3: [-0.5, 0, 2],
	c3: [0, 1.5, 0],
	s3: 1,
	t3: 1,
	u3: 1,
	w1: 0.5,
	w2: 0.25,
};

const vec = (v: Val | undefined, key: string, dims: number): readonly number[] => {
	if (typeof v === 'number' || v === undefined || v.length !== dims) throw new Error(`vector-arithmetic: "${key}" must be a ${dims}-vector`);
	finite(v);
	return v;
};
const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`vector-arithmetic: "${key}" must be a number`);
	finite(v);
	return v;
};

/** Below this an arrow counts as zero (values sit on a 0.1 grid, so anything nonzero is far above it). */
const EPS = 1e-9;

/** VA-1: both walks end at the same tip; the triangle inequality |a + b| ≤ |a| + |b| is an equality only when aligned. */
export function sum(v: Vals) {
	const a = vec(v.a, 'a', 2), b = vec(v.b, 'b', 2);
	const ab = add(a, b);
	const na = norm(a), nb = norm(b), nab = norm(ab);
	return { a, b, ab, na, nb, nab, aligned: na > EPS && nb > EPS && Math.abs(na + nb - nab) < 1e-9 };
}

/** VA-2: s·c, its length factor |s| and whether the direction is kept (s > 0), reversed (s < 0) or lost (s = 0). */
export function scaled(v: Vals) {
	const c = vec(v.c, 'c', 2), s = num(v.s, 's');
	const sc = scale(c, s);
	return { c, s, sc, factor: Math.abs(s), sense: s > 0 ? 1 : s < 0 ? -1 : 0 };
}

/** VA-3: the two scaled pieces chained head to tail and their sum; `parallel` = a and b on one line (both nonzero). */
export function combo(v: Vals) {
	const a = vec(v.la, 'la', 2), b = vec(v.lb, 'lb', 2), s = num(v.ls, 'ls'), t = num(v.lt, 'lt');
	const sa = scale(a, s), tb = scale(b, t);
	const r = add(sa, tb);
	const zero = norm(a) < EPS || norm(b) < EPS;
	return { a, b, s, t, sa, tb, r, parallel: !zero && Math.abs(signedArea2(a, b)) < 1e-9, zero };
}

/** VA-3's lattice: the lines s = k (along b) and t = k (along a) for whole k across the scrub range. */
export function lattice(a: V, b: V): [number[], number[]][] {
	const out: [number[], number[]][] = [];
	const lo = RANGE_ST.min, hi = RANGE_ST.max;
	for (let k = Math.ceil(lo); k <= hi; k++) {
		out.push([add(scale(a, k), scale(b, lo)), add(scale(a, k), scale(b, hi))]);
		out.push([add(scale(a, lo), scale(b, k)), add(scale(a, hi), scale(b, k))]);
	}
	return out;
}

/** VA-4: three scaled arrows chained head to tail; `onPlane` = u is 0, so r stays on the plane of a and b. */
export function combo3(v: Vals) {
	const a = vec(v.a3, 'a3', 3), b = vec(v.b3, 'b3', 3), c = vec(v.c3, 'c3', 3);
	const s = num(v.s3, 's3'), t = num(v.t3, 't3'), u = num(v.u3, 'u3');
	const sa = scale(a, s), tb = scale(b, t), uc = scale(c, u);
	const ab = add(sa, tb);
	return { a, b, c, s, t, u, sa, tb, uc, ab, r: add(ab, uc), onPlane: Math.abs(u) < EPS };
}

/** VA-5: every vertex = base + w1·smile + w2·jaw, with the two weighted deltas kept for drawing. */
export function blend(v: Vals) {
	const w1 = num(v.w1, 'w1'), w2 = num(v.w2, 'w2');
	const d1 = SMILE.map((d) => scale(d, w1));
	const d2 = JAW.map((d) => scale(d, w2));
	const mid = MOUTH.map((p, i) => add(p, d1[i]));
	const out = mid.map((p, i) => add(p, d2[i]));
	return { w1, w2, d1, d2, mid, out, corner: out[CORNER] };
}

export const near = (a: number, b: number): boolean => Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) < 1e-9;
const nearV = (a: V, b: V) => a.length === b.length && a.every((x, i) => near(x, b[i]));

/** Challenge predicates (store reads only, so typing in the N-panel alone solves each). */
export const CHECKS = {
	/** VA-1: b undoes a. */
	cancel: (v: Vals) => { const z = sum(v); return z.na > EPS && nearV(z.ab, [0, 0]); },
	/** VA-1: lengths add only for aligned arrows. */
	aligned: (v: Vals) => sum(v).aligned,
	/** VA-2: turned around, same length. */
	flip: (v: Vals) => near(scaled(v).s, -1),
	/** VA-2: the zero vector. */
	zero: (v: Vals) => near(scaled(v).s, 0),
	/** VA-3: (5, 3) = 2·(1, 2) + 1·(3, −1) with the default arrows; any combination landing there counts. */
	reach: (v: Vals) => nearV(combo(v).r, [5, 3]),
	/** VA-3: paint with a and b on one line: the stamps stay on that line. */
	line: (v: Vals) => v.paint === 1 && combo(v).parallel,
	/** VA-4: straight up to (0, 3, 0). */
	up: (v: Vals) => nearV(combo3(v).r, [0, 3, 0]),
	/** VA-4: on the plane of a and b, away from the origin. */
	plane: (v: Vals) => { const z = combo3(v); return z.onPlane && norm(z.ab) > EPS; },
	/** VA-5: full smile and the jaw opened until the corner is back at its base height (0.3 = 0.4·0.75). */
	level: (v: Vals) => { const z = blend(v); return near(z.w1, 1) && near(z.corner[1], MOUTH[CORNER][1]); },
	/** VA-5: a negative smile weight is a frown. */
	frown: (v: Vals) => blend(v).w1 < 0,
};

const fixed = (x: number, n: number) => (+x.toFixed(n) + 0).toFixed(n);
/** Components: one decimal (drags, keys and scrubs land on a 0.1 grid). */
export const fmt1 = (x: number): string => fixed(x, 1);
/** Lengths, weights and blended positions: two decimals. */
export const fmt2 = (x: number): string => fixed(x, 2);
/** "(3.0, −1.0)" with a typographic minus, for labels and readouts. */
export const fmtV = (p: V, f: (x: number) => string = fmt1): string => '(' + p.map((x) => f(x).replace('-', '−')).join(', ') + ')';
