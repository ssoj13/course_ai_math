/**
 * Lesson 2.2 (a matrix = images of the basis; M·v two ways; new rulers in 3D): the one model every MC scene, formula,
 * table, N-panel row and challenge reads. Pure, three-free and DOM-free, so the tests import it directly.
 *
 * One store, `mcols`, two groups of keys:
 * - MC-1 (2D): the columns `m1`, `m2` of M (where e₁ and e₂ land) and the input `x`; y = Mx is read two ways:
 *   rows (yᵢ = rowᵢ·x) and columns (y = x₁·col₁ + x₂·col₂). The products mᵢⱼxⱼ are the same four numbers in both.
 * - MC-2 (3D): the rulers `q1`, `q2`, `q3` = the ROWS of R, the world vector `p` (= v), the flag `ortho` (0/1, the
 *   lesson-1.6 Gram–Schmidt link) and the transfer `tr` (0..1). Rv lists the readings uᵢ·v. With B = Rᵀ (rulers as
 *   columns) the true coordinates c solve Bc = v; the ghost B(Rv) = Σ (uᵢ·v)uᵢ equals v only for orthonormal rulers.
 *   The Gram matrix G = RRᵀ (gᵢⱼ = uᵢ·uⱼ) is the identity exactly then.
 *
 * The transfer applies R(τ) to the whole picture: a rotation from I to R (quaternion slerp) when R is a rotation, the
 * straight blend (1 − τ)I + τR otherwise (a mirror frame passes through a flat picture: no rotation can flip it).
 */
import { fixed } from '../fmt';
import type { LessonLinks } from '../links';
import type { LessonState, Val } from '../store';
import { add, dot, norm, scale, sub, type V } from '../geom';
import { combine, det3, orthoLink, solve, TOL_SHOW } from './basis.math';

export const LESSON = 'mcols';

/** Component boxes of the inputs (N-panel ranges; every drag is clamped into them). */
export const RANGE_M = { min: -3, max: 3, step: 0.1 };
export const RANGE_X = { min: -3, max: 3, step: 0.1 };
export const RANGE_R = { min: -2.5, max: 2.5, step: 0.1 };
export const RANGE_P = { min: -5, max: 5, step: 0.1 };
export const SPEC_TR = { min: 0, max: 1, step: 0.05 };

/** MC-2 rulers at load (rows of R), the prose frame: u₁, u₂ = the x and y axes turned 53.13° in the xy-plane, u₃ = z. */
export const FRAME0: readonly V[] = [[0.6, 0.8, 0], [-0.8, 0.6, 0], [0, 0, 1]];

/** The ruler keys (rows of R) and the flag, shared with the lesson-1.6 orthonormal link. */
export const RULERS = ['q1', 'q2', 'q3'] as const;

/**
 * Initial values = the prose: M = [[2, 1], [1, 2]] (columns (2, 1), (1, 2)), x = (2, −1), Mx = (3, 0); the frame above
 * with v = (5, 0, 2) reads Rv = (3, −4, 2).
 */
export const INITIAL: Record<string, Val> = {
	m1: [2, 1],
	m2: [1, 2],
	x: [2, -1],
	q1: FRAME0[0],
	q2: FRAME0[1],
	q3: FRAME0[2],
	p: [5, 0, 2],
	ortho: 1,
	tr: 0,
};

export const LINKS: LessonLinks = { groups: [orthoLink(RULERS, 'ortho')] };

/** MC-2 timeline `rv`: overview, rows 1..3, transfer, column view. */
export const STEP_TRANSFER = 4;
export const STEP_COLUMNS = 5;
export const STEPS = 6;

/**
 * Ready-made frames for MC-2 (prose sections): the orthonormal start, oblique rulers (2.2 "Shadows are no longer
 * coordinates"), a unit but oblique ruler, a mirror (orthonormal, left-handed) and duplicate rulers (not a basis).
 */
export const PRESETS = {
	ortho: { q1: FRAME0[0], q2: FRAME0[1], q3: FRAME0[2], p: [5, 0, 2], ortho: 1 },
	oblique: { q1: [1, 0, 0], q2: [1, 1, 0], q3: [0, 0, 1], p: [2, 1, 3], ortho: 0 },
	unitOblique: { q1: [1, 0, 0], q2: [0.6, 0.8, 0], q3: [0, 0, 1], p: [2, 0.8, 3], ortho: 0 },
	mirror: { q1: [1, 0, 0], q2: [0, -1, 0], q3: [0, 0, 1], p: [2, 1, 3], ortho: 1 },
	duplicate: { q1: [1, 0, 0], q2: [1, 0, 0], q3: [0, 0, 1], p: [2, 1, 3], ortho: 0 },
} as const satisfies Record<string, Record<string, Val>>;
export type Preset = keyof typeof PRESETS;

type Vals = LessonState['vals'];

const vec = (v: Val | undefined, key: string, n: number): number[] => {
	if (typeof v === 'number' || !v || v.length !== n || !v.every(Number.isFinite)) throw new Error(`mcols: "${key}" must be a finite ${n}-vector`);
	return [...v];
};
const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number' || !Number.isFinite(v)) throw new Error(`mcols: "${key}" must be a finite number`);
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
// Small matrix helpers (rows of numbers; tiny and explicit).

export type Mat = readonly (readonly number[])[];

/** M·x read by rows: one dot product per row. */
export const byRows = (m: Mat, x: V): number[] => m.map((r) => dot(r, x));
/** M·x read by columns: x₁·col₁ + x₂·col₂ + … (the same numbers, other grouping). */
export const byCols = (m: Mat, x: V): number[] =>
	x.reduce<number[]>((s, xj, j) => add(s, scale(m.map((r) => r[j]), xj)), m.map(() => 0));
/** The products mᵢⱼxⱼ: rows add up to the row readings, columns are the scaled columns. */
export const products = (m: Mat, x: V): number[][] => m.map((r) => r.map((mij, j) => mij * x[j]));
export const transpose = (m: Mat): number[][] => m[0].map((_, j) => m.map((r) => r[j]));
export const mul = (a: Mat, b: Mat): number[][] => a.map((r) => transpose(b).map((c) => dot(r, c)));

// ---------------------------------------------------------------------------------------------------------------
// MC-1: columns are images of the basis; Mx two ways.

export function derive1(vals: Vals) {
	const m1 = vec(vals.m1, 'm1', 2);
	const m2 = vec(vals.m2, 'm2', 2);
	const x = vec(vals.x, 'x', 2);
	/** M by rows: row i = (m1[i], m2[i]). */
	const M = [[m1[0], m2[0]], [m1[1], m2[1]]];
	const yRows = byRows(M, x);
	const yCols = byCols(M, x);
	return {
		m1, m2, x, M, yRows, yCols,
		y: yCols,
		rows: M as number[][],
		prods: products(M, x),
		/** The column walk: x₁ copies of col₁ from the origin, then x₂ copies of col₂. */
		leg: scale(m1, x[0]),
	};
}

// ---------------------------------------------------------------------------------------------------------------
// MC-2: new rulers in 3D.

/** Unit quaternion [w, x, y, z] of a rotation matrix (rows; acts on column vectors), Shepperd's branch choice. */
export function quatOf(m: Mat): [number, number, number, number] {
	const [[a, b, c], [d, e, f], [g, h, k]] = m;
	const tr = a + e + k;
	let q: [number, number, number, number];
	if (tr > 0) {
		const s = Math.sqrt(tr + 1) * 2;
		q = [s / 4, (h - f) / s, (c - g) / s, (d - b) / s];
	} else if (a > e && a > k) {
		const s = Math.sqrt(1 + a - e - k) * 2;
		q = [(h - f) / s, s / 4, (b + d) / s, (c + g) / s];
	} else if (e > k) {
		const s = Math.sqrt(1 + e - a - k) * 2;
		q = [(c - g) / s, (b + d) / s, s / 4, (f + h) / s];
	} else {
		const s = Math.sqrt(1 + k - a - e) * 2;
		q = [(d - b) / s, (c + g) / s, (f + h) / s, s / 4];
	}
	// The short way round: w ≥ 0 (q and −q are the same rotation).
	return q[0] < 0 ? [-q[0], -q[1], -q[2], -q[3]] : q;
}

/** Rotation matrix (rows) of a unit quaternion. */
export function matOfQuat([w, x, y, z]: readonly number[]): number[][] {
	return [
		[1 - 2 * (y * y + z * z), 2 * (x * y - w * z), 2 * (x * z + w * y)],
		[2 * (x * y + w * z), 1 - 2 * (x * x + z * z), 2 * (y * z - w * x)],
		[2 * (x * z - w * y), 2 * (y * z + w * x), 1 - 2 * (x * x + y * y)],
	];
}

/** The rotation a fraction τ of the way from I to the rotation `q` (slerp from the identity quaternion). */
export function slerpFromId(q: readonly number[], tau: number): number[][] {
	const half = Math.acos(Math.min(1, Math.max(-1, q[0])));
	const s = Math.sin(half);
	if (s < 1e-12) return matOfQuat([1, 0, 0, 0]);
	const k = Math.sin(tau * half) / s;
	return matOfQuat([Math.cos(tau * half), q[1] * k, q[2] * k, q[3] * k]);
}

const IDENTITY: Mat = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];

/**
 * The picture's matrix at transfer τ: the slerped rotation when R is a rotation (orthonormal, det +1), otherwise the
 * straight blend (1 − τ)I + τR. Either way it is I at τ = 0 and R at τ = 1.
 */
export function transferMat(R: Mat, rotation: boolean, tau: number): number[][] {
	if (rotation) return slerpFromId(quatOf(R), tau);
	return R.map((r, i) => r.map((x, j) => (1 - tau) * IDENTITY[i][j] + tau * x));
}

export function derive2(vals: Vals) {
	const u = RULERS.map((k) => vec(vals[k], k, 3));
	const v = vec(vals.p, 'p', 3);
	const tau = Math.min(1, Math.max(0, num(vals.tr, 'tr')));
	const ortho = vals.ortho === 1;
	/** R by rows = the rulers; its columns colⱼ = (u₁ⱼ, u₂ⱼ, u₃ⱼ). */
	const R = u;
	const y = byRows(R, v);
	const cols = transpose(R);
	const g = mul(R, transpose(R));
	const lens = u.map(norm);
	const vol = det3(u[0], u[1], u[2]);
	/** True coordinates: B c = v with the rulers as the columns of B (null for dependent rulers). */
	const c = solve(u, v);
	const ghost = combine(u, y);
	const gap = norm(sub(ghost, v));
	const unit = lens.map((l) => Math.abs(l - 1) < TOL_SHOW);
	const perp = [[0, 1], [0, 2], [1, 2]].map(([i, j]) => Math.abs(g[i][j]) < TOL_SHOW);
	const orthonormal = unit.every(Boolean) && perp.every(Boolean);
	const rotation = orthonormal && vol > 0;
	const T = transferMat(R, rotation, tau);
	const at = (p: V) => byRows(T, p);
	return {
		u, v, tau, ortho, R, y, cols, g, lens, vol, c, ghost, gap, unit, perp, orthonormal, rotation,
		prods: products(R, v),
		/** +1 right-handed, −1 left-handed, 0 flat (dependent rulers). */
		hand: c === null ? 0 : Math.sign(vol),
		/** Perpendicular feet of v on each ruler line (null for a zero ruler: no direction to drop onto). */
		feet: u.map((ui) => (norm(ui) === 0 ? null : scale(ui, dot(v, ui) / dot(ui, ui)))),
		/** The oblique walk to v along the rulers by the true coordinates: O → c₁u₁ → c₁u₁ + c₂u₂ → v. */
		walk: c ? [[0, 0, 0], scale(u[0], c[0]), add(scale(u[0], c[0]), scale(u[1], c[1])), [...v]] : null,
		/** The column walk to Rv: v₁·col₁, then v₂·col₂, then v₃·col₃ (drawn after the transfer). */
		colWalk: [[0, 0, 0], scale(cols[0], v[0]), add(scale(cols[0], v[0]), scale(cols[1], v[1])), [...y]],
		/** Picture at the current transfer: the matrix and a point mapper. */
		T, at,
	};
}

/** Is the 3×3 Gram matrix the identity (within the ✓ tolerance)? */
export const isIdentity = (g: Mat) => g.every((r, i) => r.every((x, j) => Math.abs(x - (i === j ? 1 : 0)) < TOL_SHOW));

// ---------------------------------------------------------------------------------------------------------------
// Challenges: store reads only, so typing in the N-panel alone solves each.

export const CHECKS = {
	/** MC-1: an input whose image is exactly the second column (x = (0, 1)) is the definition of a column. */
	column2: (v: Vals) => { const z = derive1(v); return z.x[0] === 0 && z.x[1] === 1; },
	/** MC-1: y = Mx lands on the origin although x is not zero (M squashes x away). */
	lost: (v: Vals) => { const z = derive1(v); return norm(z.y) < 1e-9 && norm(z.x) > 0; },
	/** MC-2: rulers bent (flag off) and the ghost misses v by more than 1. */
	ghostMiss: (v: Vals) => { const z = derive2(v); return !z.ortho && z.c !== null && z.gap > 1; },
	/** MC-2: an orthonormal mirror frame (left-handed): the transpose still undoes R, yet no rotation reaches it. */
	mirror: (v: Vals) => { const z = derive2(v); return z.orthonormal && z.vol < 0; },
};
