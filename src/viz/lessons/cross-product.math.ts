import type { LessonState } from '../store';
import { add, cross, dot, norm, scale, sub, type V } from '../geom';

/**
 * Lesson 1.8, the cross product: the one model every CP scene, formula, table, N-panel row and challenge reads
 * (storyboard §2 1.8). Three-free and DOM-free, so the pure tests import it directly.
 *
 * Three scenes share one store:
 * - CP-1 (3D): arrows `a`, `b`; `swap` (0/1) shows b × a instead of a × b. The result is the arrow perpendicular to both,
 *   its length the area of the parallelogram they span.
 * - CP-2 (2D): arrows `a2`, `b2`; the z component of the 3D formula is the signed area, its sign the turn direction.
 * - CP-3 (3D): a triangle `tA`, `tB`, `tC`; `rev` (0/1) lists it A, C, B instead of A, B, C (the winding order), `cull`
 *   (0/1) is back-face culling. The face normal is the normalized cross product of the two edges from the first vertex;
 *   the face is lit by a fixed directional light with max(0, N·L).
 *
 * Domain rules: a zero arrow has no direction, so its angle is undefined (NaN, shown as a dash), while the cross product
 * and the area are always defined (both 0). Parallel or zero inputs give the zero vector, never an error; only the
 * normalized normal is undefined then (null).
 */

/** Lesson store id. */
export const LESSON = 'cross-product';

/** Component boxes of the inputs: 3D kept small enough that |a × b| (up to |a||b|) stays in view. */
export const RANGE3 = { min: -2.5, max: 2.5, step: 0.1 };
export const RANGE2 = { min: -4, max: 4, step: 0.1 };

/**
 * Initial store values. CP-1 starts with a and b lying almost flat, so a × b = (−1, 4, −1) stands up like the normal of
 * a floor (and the three-quarter home view frames it well); it is not the textbook x × y, which challenge 1 asks for. The triangle leans toward the default three-quarter camera (normal along (2, 2, 4)):
 * in the xy plane, as in the prose example, the camera saw it so obliquely that the face read as a sliver.
 */
export const INITIAL = {
	a: [0, 0.5, 2],
	b: [2, 0.5, 0],
	swap: 0,
	a2: [3, 1],
	b2: [1, 2],
	tA: [0, 0, 1],
	tB: [2, 0, 0],
	tC: [0, 2, 0],
	rev: 0,
	cull: 1,
};

/** Direction toward the CP-3 light (unit): up, right and toward the default camera, so the front face starts lit. */
export const LIGHT: readonly number[] = scale([1, 1.5, 2.5], 1 / Math.hypot(1, 1.5, 2.5));

type Vals = LessonState['vals'];

const vec = (v: Vals, key: string, dims: number): number[] => {
	const x = v[key];
	if (typeof x === 'number' || x === undefined || x.length !== dims) throw new Error(`cross-product: "${key}" must be a ${dims}-vector`);
	return [...x];
};
const flag = (v: Vals, key: string): boolean => {
	const x = v[key];
	if (x !== 0 && x !== 1) throw new Error(`cross-product: "${key}" must be 0 or 1`);
	return x === 1;
};

const DEG = 180 / Math.PI;

/**
 * Unsigned angle in degrees between two nonzero vectors, NaN when either is zero. atan2 of the sine and cosine parts
 * (Kahan) rather than acos of the cosine: near 0° and 180° acos loses half the digits, and sin(acos(1 − ε)) left an
 * "area" of 2e-7 for exactly parallel inputs. 2D vectors use their z cross component.
 */
export function angleDeg(a: V, b: V): number {
	if (norm(a) === 0 || norm(b) === 0) return NaN;
	const s = a.length === 2 ? Math.abs(a[0] * b[1] - a[1] * b[0]) : norm(cross(a, b));
	return Math.atan2(s, dot(a, b)) * DEG;
}

/**
 * The criss-cross of each component: c_i = plus_i − minus_i, cycling x → y → z → x
 * (x: u_y v_z − u_z v_y, y: u_z v_x − u_x v_z, z: u_x v_y − u_y v_x).
 */
export function terms(u: V, v: V): { plus: number[]; minus: number[] } {
	return {
		plus: [u[1] * v[2], u[2] * v[0], u[0] * v[1]],
		minus: [u[2] * v[1], u[0] * v[2], u[1] * v[0]],
	};
}

/** Relative test for "no area": exact zero inputs, or parallel inputs up to rounding noise. */
export const flat = (len: number, na: number, nb: number): boolean => len <= 1e-9 * na * nb;

/** CP-1: everything the 3D cross-product scene, its formula, table and rows show. */
export function derive(v: Vals) {
	const a = vec(v, 'a', 3), b = vec(v, 'b', 3), swap = flag(v, 'swap');
	// The pair in the order it is multiplied: (a, b) or (b, a).
	const [u, w] = swap ? [b, a] : [a, b];
	const c = cross(u, w);
	const { plus, minus } = terms(u, w);
	const na = norm(a), nb = norm(b), len = norm(c);
	const th = angleDeg(a, b);
	// The second route to the area; with a zero arrow it is 0 (one factor is 0) even though θ is undefined.
	const area = na === 0 || nb === 0 ? 0 : na * nb * Math.sin(th / DEG);
	return {
		a, b, swap, u, w, c, plus, minus, na, nb, len, th, area,
		/** Dot products of the result with the inputs: 0 up to rounding, the "perpendicular to both" check. */
		ca: dot(c, a), cb: dot(c, b),
		parallel: flat(len, na, nb),
		zero: na === 0 || nb === 0,
	};
}

export type Orientation = 'ccw' | 'cw' | 'line';

/** CP-2: the 2D signed area a_x b_y − a_y b_x and its orientation. */
export function derive2(v: Vals) {
	const a = vec(v, 'a2', 2), b = vec(v, 'b2', 2);
	const p = a[0] * b[1], q = a[1] * b[0], sa = p - q;
	const na = norm(a), nb = norm(b);
	const orient: Orientation = flat(Math.abs(sa), na, nb) ? 'line' : sa > 0 ? 'ccw' : 'cw';
	return { a, b, p, q, sa, na, nb, th: angleDeg(a, b), orient };
}

/**
 * CP-3: the triangle in its listed order P0, P1, P2 (A, B, C or A, C, B), the edges from P0, the cross product n, the
 * unit normal N (null when the points are on one line), the triangle's area |n| / 2 and the lighting max(0, N·L).
 */
export function tri(v: Vals) {
	const A = vec(v, 'tA', 3), B = vec(v, 'tB', 3), C = vec(v, 'tC', 3), rev = flag(v, 'rev'), cull = flag(v, 'cull');
	const P = rev ? [A, C, B] : [A, B, C];
	const e1 = sub(P[1], P[0]), e2 = sub(P[2], P[0]);
	const n = cross(e1, e2), len = norm(n);
	const N = flat(len, norm(e1), norm(e2)) ? null : scale(n, 1 / len);
	const ndotl = N ? dot(N, LIGHT) : NaN;
	const centroid = scale(add(add(A, B), C), 1 / 3);
	return { A, B, C, rev, cull, P, e1, e2, n, N, area: len / 2, ndotl, bright: N ? Math.max(0, ndotl) : NaN, centroid };
}

/**
 * Reference direction for a 3D angle arc: any vector off the plane of the two sides. The cross product itself when it
 * exists (the arc then lies in the plane of a and b), else the coordinate axis least parallel to `a`.
 */
export function arcNormal(a: V, c: V): number[] {
	if (norm(c) > 0) return [...c];
	const m = a.map(Math.abs);
	const i = m[0] <= m[1] && m[0] <= m[2] ? 0 : m[1] <= m[2] ? 1 : 2;
	return [0, 1, 2].map((j) => (j === i ? 1 : 0));
}

// ---------------------------------------------------------------------------------------------------------------
// Formatting. `+x.toFixed(n) + 0` first, so −0.001 prints 0.00, never −0.00; ASCII minus like every other readout.

const fixed = (x: number, n: number) => (+x.toFixed(n) + 0).toFixed(n);
/** Components, lengths, areas: 2 decimals; undefined values read as a dash. */
export const fmt = (x: number) => (Number.isFinite(x) ? fixed(x, 2) : '–');
/** Unit-vector components at one decimal: "(0.0, 0.0, 1.0)" fits the 200 px N panel. */
export const fmt1 = (x: number) => (Number.isFinite(x) ? fixed(x, 1) : '–');
/** Angles: whole degrees when integral up to float noise, otherwise one decimal; callers add the degree sign. */
export const fmtAngle = (x: number) => {
	if (!Number.isFinite(x)) return '–';
	const whole = Math.round(x);
	return Math.abs(x - whole) < 1e-9 ? String(whole + 0) : fixed(x, 1);
};
export const fmtDeg = (x: number) => (Number.isFinite(x) ? fmtAngle(x) + '°' : '–');
/** A vector as "(x, y, z)" at 2 decimals. */
export const fmtVec = (x: V) => '(' + x.map(fmt).join(', ') + ')';
/** The same formats for `\s{}` values inside TeX (the renderer rejects raw non-ASCII, so a dash is `\text{--}`). */
export const tex = {
	num: (x: number) => (Number.isFinite(x) ? fixed(x, 2) : String.raw`\text{--}`),
	deg: (x: number) => (Number.isFinite(x) ? fmtAngle(x) + String.raw`^\circ` : String.raw`\text{--}`),
};

// ---------------------------------------------------------------------------------------------------------------
// Challenges (store reads only, so typing in the N panel alone solves each).

const near = (x: number, y: number, tol = 1e-9) => Number.isFinite(x) && Math.abs(x - y) <= tol;

export const CHECKS = {
	/** x × y = +z: the result points straight along +z (any lengths, any order that gives +z). */
	plusZ: (v: Vals) => {
		const z = derive(v);
		return z.len > 0 && near(z.c[0], 0) && near(z.c[1], 0) && z.c[2] > 0 && !z.swap && near(z.a[1], 0) && near(z.a[2], 0) && z.a[0] > 0 && near(z.b[0], 0) && near(z.b[2], 0) && z.b[1] > 0;
	},
	/** Parallel, both nonzero: the parallelogram is flat. */
	parallel: (v: Vals) => {
		const z = derive(v);
		return !z.zero && z.parallel;
	},
	/** Area 6 (values land on 0.1 grids, so the root of a sum of squares needs a display tolerance). */
	areaSix: (v: Vals) => near(derive(v).len, 6, 0.005),
	/** The arrow turned over without touching a or b. */
	swapped: (v: Vals) => derive(v).swap && derive(v).len > 0,
	negative: (v: Vals) => derive2(v).orient === 'cw',
	six: (v: Vals) => {
		const z = derive2(v);
		return near(z.a[0], 2) && near(z.a[1], 0) && near(z.sa, 6);
	},
	line: (v: Vals) => {
		const z = derive2(v);
		return z.na > 0 && z.nb > 0 && z.orient === 'line';
	},
	/** Reversed winding, culling off: drawn, but N·L < 0, so black. */
	black: (v: Vals) => {
		const z = tri(v);
		return z.rev && !z.cull && z.N !== null && z.ndotl < 0;
	},
	/** The original order, but a vertex moved so the face turns away from the light. */
	turnAway: (v: Vals) => {
		const z = tri(v);
		return !z.rev && z.N !== null && z.ndotl < 0;
	},
};
