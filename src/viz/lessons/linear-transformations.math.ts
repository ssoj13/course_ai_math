/**
 * Lesson 2.1 (linear transformations): the one model every LT scene, formula, N-panel row and challenge reads. Pure,
 * three-free and DOM-free, so the tests import it directly; the scenes only draw what these functions return.
 *
 * One store, `lintx`, two groups of keys:
 * - LT-1 "where the basis lands" (2D): the images `a1` = T(e₁) and `a2` = T(e₂) (the future columns of the matrix), the
 *   input `v` and the morph `t` (0 = identity, 1 = the whole map; every in-between map I + t(A − I) is linear too);
 * - LT-2 "linear or not?" (2D): `mode` (0 linear T(p) = Ap, 1 shifted F(p) = Ap + b, 2 bent S(x, y) = (x, y + x²/4)),
 *   a second input `w` and a scale `s` for the two tests T(v + w) = T(v) + T(w) and T(sv) = sT(v). LT-2 shares `a1`,
 *   `a2` and `v` with LT-1, so the matrix the reader built above is the one tested below.
 */
import { fixed } from '../fmt';
import type { LessonState, Val } from '../store';
import type { Move } from '../scene/handles';
import { add, norm, scale, sub, type V } from '../geom';
import { moved } from '../scene/drag';
import { det2 } from './basis.math';

export const LESSON = 'lintx';

/** Component boxes of the inputs (N-panel ranges; every drag is clamped into them). */
export const RANGE_A = { min: -3, max: 3, step: 0.1 };
export const RANGE_V = { min: -3, max: 3, step: 0.1 };
export const SPEC_T = { min: 0, max: 1, step: 0.05 };
export const SPEC_S = { min: -3, max: 3, step: 0.1 };
export const SPEC_MODE = { min: 0, max: 2, step: 1 };

/** The shift of the affine counterexample F(p) = Ap + b. */
export const SHIFT: V = [1, -2];
/** The bend of S(x, y) = (x, y + BEND·x²): origin fixed, vertical lines straight, horizontal lines bent. */
export const BEND = 0.25;
/** Below this morph the column handles are not draggable: a drag would have to divide a pointer move by t ≈ 0. */
export const T_DRAG = 0.2;
/** Half side of the square the grid lines of both scenes cover (input space). */
export const EXT = 4;

/**
 * Initial values = the prose: a₁ = (2, 1), a₂ = (1, 2), v = (2, −1) lands on (3, 0); w = (1, 3) lands on (5, 7) and
 * v + w = (3, 2) on (8, 7); s = −2 gives A(−2v) = (−6, 0).
 */
export const INITIAL: Record<string, Val> = {
	a1: [2, 1],
	a2: [1, 2],
	v: [2, -1],
	t: 1,
	w: [1, 3],
	s: -2,
	mode: 0,
};

type Vals = LessonState['vals'];

const vec = (v: Val | undefined, key: string): number[] => {
	if (typeof v === 'number' || !v || v.length !== 2 || !v.every(Number.isFinite)) throw new Error(`lintx: "${key}" must be a finite 2-vector`);
	return [...v];
};
const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number' || !Number.isFinite(v)) throw new Error(`lintx: "${key}" must be a finite number`);
	return v;
};

// ---------------------------------------------------------------------------------------------------------------
// Formatting (`fixed`: identical on server and client, never −0.0); TeX gets `\text{--}` for a missing value.

export const fmt1 = (x: number) => (Number.isFinite(x) ? fixed(x, 1) : '–');
export const fmt2 = (x: number) => (Number.isFinite(x) ? fixed(x, 2) : '–');
export const fmtV = (p: readonly number[], f = fmt1) => '(' + p.map(f).join(', ') + ')';
export const tex1 = (x: number) => (Number.isFinite(x) ? fixed(x, 1) : String.raw`\text{--}`);
export const tex2 = (x: number) => (Number.isFinite(x) ? fixed(x, 2) : String.raw`\text{--}`);

// ---------------------------------------------------------------------------------------------------------------
// The maps.

const E1: V = [1, 0];
const E2: V = [0, 1];

/** x·a₁ + y·a₂: the image of p = (x, y) under the linear map whose basis images are a₁, a₂ (the column rule). */
export const applyCols = (a1: V, a2: V, p: V): number[] => add(scale(a1, p[0]), scale(a2, p[1]));

/** The images of e₁, e₂ part-way: (1 − t)·eᵢ + t·aᵢ (the map I + t(A − I), linear for every t). */
export function morphCols(a1: V, a2: V, t: number): [number[], number[]] {
	return [add(scale(E1, 1 - t), scale(a1, t)), add(scale(E2, 1 - t), scale(a2, t))];
}

export type Mode = 0 | 1 | 2;

export function asMode(m: number): Mode {
	if (m !== 0 && m !== 1 && m !== 2) throw new Error('lintx: "mode" is 0, 1 or 2');
	return m;
}

/** The map of LT-2 in `mode`: linear T(p) = Ap, shifted F(p) = Ap + b, bent S(x, y) = (x, y + x²/4). */
export function mapPt(mode: Mode, a1: V, a2: V, p: V): number[] {
	if (mode === 0) return applyCols(a1, a2, p);
	if (mode === 1) return add(applyCols(a1, a2, p), SHIFT);
	return [p[0], p[1] + BEND * p[0] * p[0]];
}

// ---------------------------------------------------------------------------------------------------------------
// LT-1: where the basis lands.

/** A letter F in input space (second quadrant, asymmetric, so a mirror image is visible at once). */
export const FIG: readonly V[] = [
	[-1.6, 0.4], [-1.6, 2], [-0.6, 2], [-0.6, 1.6], [-1.2, 1.6], [-1.2, 1.2], [-0.8, 1.2], [-0.8, 0.8], [-1.2, 0.8], [-1.2, 0.4],
];

export function derive1(vals: Vals) {
	const a1 = vec(vals.a1, 'a1');
	const a2 = vec(vals.a2, 'a2');
	const v = vec(vals.v, 'v');
	const t = num(vals.t, 't');
	const [m1, m2] = morphCols(a1, a2, t);
	/** The full image Av, and the image drawn at the current morph. */
	const av = applyCols(a1, a2, v);
	const avT = applyCols(m1, m2, v);
	const det = det2(a1, a2);
	const detT = det2(m1, m2);
	return {
		a1, a2, v, t, m1, m2, av, avT, det, detT,
		/** The walk to the image: v₁ copies of the first column, then v₂ copies of the second. */
		leg: scale(m1, v[0]),
		fig: FIG.map((p) => applyCols(m1, m2, p)),
		/** The image of the unit square has zero area: the plane is squashed onto a line (or a point). */
		flat: Math.abs(detT) < 1e-9,
	};
}

/** Handle of column `k` (1 or 2) at the current morph: (1 − t)·e_k + t·a_k. */
export function handleCol(k: 1 | 2): (vals: Vals) => number[] {
	return (vals) => morphCols(vec(vals.a1, 'a1'), vec(vals.a2, 'a2'), num(vals.t, 't'))[k - 1];
}

/**
 * Drag/key mapping of column handle `k`: the handle sits at e_k + t(a_k − e_k), so a pointer travel `raw` moves a_k by
 * raw / t (the handle stays under the pointer), snapped to the 0.1 grid. Never called below T_DRAG (the scene turns
 * the handle off there).
 */
export function moveCol(k: 1 | 2): (vals: Vals, raw: readonly number[]) => Move {
	const key = 'a' + k;
	return (vals, raw) => {
		const t = Math.max(num(vals.t, 't'), T_DRAG);
		return { set: moved(vals, [key], raw.map((x) => x / t)) };
	};
}

// ---------------------------------------------------------------------------------------------------------------
// LT-2: linear or not?

/** Samples per grid line: enough for the bent parabolas to look smooth at the scene's size. */
export const SAMPLES = 32;

/**
 * Images of the grid lines x = k and y = k (k = −EXT..EXT) under `mode`, sampled so a bent line shows as a curve; each
 * line is a list of segment end-point pairs. `vertical` = images of x = k, `horizontal` = images of y = k.
 */
export function mappedGrid(mode: Mode, a1: V, a2: V): { vertical: number[][][]; horizontal: number[][][] } {
	const line = (at: (u: number) => V): number[][][] => {
		const pts = Array.from({ length: SAMPLES + 1 }, (_, i) => mapPt(mode, a1, a2, at(-EXT + (2 * EXT * i) / SAMPLES)));
		return pts.slice(1).map((p, i) => [pts[i], p]);
	};
	const ks = Array.from({ length: 2 * EXT + 1 }, (_, i) => i - EXT);
	return {
		vertical: ks.flatMap((k) => line((u) => [k, u])),
		horizontal: ks.flatMap((k) => line((u) => [u, k])),
	};
}

export function derive2(vals: Vals) {
	const mode = asMode(num(vals.mode, 'mode'));
	const a1 = vec(vals.a1, 'a1');
	const a2 = vec(vals.a2, 'a2');
	const v = vec(vals.v, 'v');
	const w = vec(vals.w, 'w');
	const s = num(vals.s, 's');
	const f = (p: V) => mapPt(mode, a1, a2, p);
	const tv = f(v);
	const tw = f(w);
	const tvw = f(add(v, w));
	/** "Transform first, then add": the images added as arrows from the origin. */
	const sum = add(tv, tw);
	const tsv = f(scale(v, s));
	const stv = scale(tv, s);
	const origin = f([0, 0]);
	const gapAdd = norm(sub(tvw, sum));
	const gapScale = norm(sub(tsv, stv));
	return {
		mode, a1, a2, v, w, s, tv, tw, tvw, sum, tsv, stv, origin, gapAdd, gapScale,
		/** Both tests pass (to float noise) and the origin stays put. */
		addOk: gapAdd < 1e-9,
		scaleOk: gapScale < 1e-9,
		originOk: norm(origin) < 1e-12,
	};
}

// ---------------------------------------------------------------------------------------------------------------
// Challenges: store reads only, so typing in the N-panel alone solves each.

export const CHECKS = {
	/** LT-1: the F comes out mirrored (the image of the unit square turned over: negative signed area). */
	mirror: (v: Vals) => { const z = derive1(v); return z.t === 1 && z.det < -1e-9; },
	/** LT-1: both images non-zero, yet the whole plane lands on one line. */
	squash: (v: Vals) => { const z = derive1(v); return z.t === 1 && Math.abs(z.det) < 1e-9 && norm(z.a1) > 0 && norm(z.a2) > 0; },
	/** LT-2: the bent map passes the sum test for one pair, though it is not linear (luck, not proof). */
	lucky: (v: Vals) => { const z = derive2(v); return z.mode === 2 && z.addOk && norm(z.v) > 0 && norm(z.w) > 0; },
	/** LT-2: the shifted map passes the scaling test: only s = 1 does, the scaling that changes nothing. */
	shiftScale: (v: Vals) => { const z = derive2(v); return z.mode === 1 && z.scaleOk && norm(z.v) > 0; },
};
