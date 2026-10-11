import type { LessonState } from '../store';
import type { LessonLinks } from '../links';
import type { Move } from '../scene/handles';
import { add, dot, norm, scale, sub, type V } from '../geom';
import { snap } from '../scene/drag';
import { radialLink } from '../scene/radialLink';

/**
 * Lesson 1.4, the dot product: the one model every DP scene, formula, table, N-panel row and challenge reads (spec §0.1,
 * §2.1). Three-free and DOM-free, so the pure tests import it directly. Every geometric construction (squares on the
 * triangle, the signed rectangle |a| × shadow, the 3D display plane, the lamp's horizon circle) is computed here once;
 * the scenes only draw what these functions return.
 *
 * Domain rules (controller rulings on the DP preflight, 2026-10-11):
 * - a zero arrow has no direction: the angle, the cosine route |a||b| cos θ and the brightness are undefined (NaN,
 *   shown as a dash with a reason), while the component sum Σ aᵢbᵢ is always defined;
 * - with a ≠ 0 and b = 0 the shadow, the projection and the signed rectangle are 0 (the axis of a exists);
 *   with a = 0 there is no axis to project on, so shadow and rectangle are undefined;
 * - "perpendicular" needs two nonzero arrows AND a zero dot product;
 * - the rectangle's sign is the sign of the shadow (of the dot product), never the winding of its vertex list.
 */

/** Lesson store id. */
export const LESSON = 'dot-product';

/** Sphere radius of DP-4 and the lamp's minimum distance from the centre (radius + 0.3, spec §2.1). */
export const SPHERE_R = 1.5;
export const LAMP_MIN = SPHERE_R + 0.3;
/** Declared initial directions of the radial inputs (spec §2.1); also their zero-request fallback. */
export const LIGHT_INITIAL = [2, 2.5, 1.5] as const;
export const PROBE_INITIAL = [1, 0.6, 0.8] as const;

/** Lamp kept at least LAMP_MIN from the centre, probe kept on the sphere: one rule for fields, handles and init. */
export const lightLink = radialLink('nl_light', LAMP_MIN, 'outside', LIGHT_INITIAL);
export const probeLink = radialLink('nl_pt', SPHERE_R, 'surface', PROBE_INITIAL);
export const LINKS: LessonLinks = { groups: [lightLink, probeLink] };

/**
 * Initial store values (spec §2.1); `nl_pt` = normalize([1, .6, .8]) · 1.5, on the sphere as the radial link requires.
 * `strike` (0..2) and `fade_a` / `fade_b` (1 = full tile, 0 = outline) are the DP-1 cancellation state written by
 * the step-6 sequence (see `strikeTarget`); scenes read the eased `fade_*`, formulas and the table read `strike`.
 */
export const INITIAL = {
	a: [3, 1],
	b: [1, 2],
	a3: [2, 1, 1],
	b3: [1, 2, -1],
	nl_light: [...LIGHT_INITIAL],
	nl_pt: scale(PROBE_INITIAL, SPHERE_R / norm(PROBE_INITIAL)),
	nl_raw: 0,
	strike: 0,
	fade_a: 1,
	fade_b: 1,
};

/** Component box of the 2D inputs (both DP-1 and DP-2) and of the 3D inputs (controller ruling 3). */
export const RANGE2 = { min: -4.5, max: 4.5, step: 0.1 };
export const RANGE3 = { min: -3.5, max: 3.5, step: 0.1 };

type Vals = LessonState['vals'];

export const clamp = (x: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, x));
const DEG = 180 / Math.PI;

// ---------------------------------------------------------------------------------------------------------------
// Readout formatting (spec §0.2). DOM text uses a dash for undefined values; TeX gets `\text{--}` because the formula
// renderer rejects raw non-ASCII. `+x.toFixed(n)` first so that -0.001 prints 0.00, never -0.00.

const fixed = (x: number, n: number) => (+x.toFixed(n) + 0).toFixed(n);

/** Lengths, areas, dot products: 2 decimals. */
export const fmt = (x: number) => (Number.isFinite(x) ? fixed(x, 2) : '–');
/** Cosines, N·L, brightness: 3 decimals. */
export const fmtCos = (x: number) => (Number.isFinite(x) ? fixed(x, 3) : '–');
/**
 * Angles: whole degrees when the value is an integer up to float noise (acos of an exact 1/√2 gives 45.00000000000001,
 * which is 45°, not 45.0°), otherwise one decimal. No suffix: callers add `°` (DOM) or `^\circ` (TeX).
 */
export const fmtAngle = (x: number) => {
	if (!Number.isFinite(x)) return '–';
	const whole = Math.round(x);
	return Math.abs(x - whole) < 1e-9 ? String(whole + 0) : fixed(x, 1);
};
export const fmtDeg = (x: number) => (Number.isFinite(x) ? fmtAngle(x) + '°' : '–');

/** The same three formats for `\s{}` values inside TeX. */
export const tex = {
	num: (x: number) => (Number.isFinite(x) ? fixed(x, 2) : String.raw`\text{--}`),
	cos: (x: number) => (Number.isFinite(x) ? fixed(x, 3) : String.raw`\text{--}`),
	deg: (x: number) => (Number.isFinite(x) ? fmtAngle(x) + String.raw`^\circ` : String.raw`\text{--}`),
};

// ---------------------------------------------------------------------------------------------------------------
// Geometry helpers.

/** Counter-clockwise quarter turn of a 2D vector. */
export const ccw = (v: V): number[] => [-v[1], v[0]];

/**
 * Unit vector perpendicular to `u` (nonzero) inside the plane of `u` and `toward`, pointing AWAY from `toward`.
 * When `toward` is zero or parallel to `u` the plane is undefined; the display convention then uses the coordinate
 * axis least parallel to `u` (controller ruling 4): the result is still exactly perpendicular to `u`, so the rectangle
 * stays a rectangle and its plane normal u × h is perpendicular to u. 2D uses the in-plane perpendicular and only
 * picks the side.
 */
export function awayFrom(u: V, toward: V): number[] {
	const n = norm(u);
	if (n === 0) throw new Error('awayFrom needs a nonzero direction');
	const e = scale(u, 1 / n);
	if (u.length === 2) {
		const c = e[0] * toward[1] - e[1] * toward[0];
		// `toward` clockwise of u (c < 0): go counter-clockwise; otherwise (ccw side, or collinear by convention): clockwise.
		return c < 0 ? [-e[1], e[0]] : [e[1], -e[0]];
	}
	const rest = sub(toward, scale(e, dot(toward, e)));
	const r = norm(rest);
	if (r > 1e-9 * Math.max(1, norm(toward))) return scale(rest, -1 / r);
	return refPerp(e);
}

/** Coordinate axis least parallel to `e` (ties: x, then y, then z): the reference of the 3D display conventions. */
export function refAxis(e: V): number[] {
	const a = e.map(Math.abs);
	const i = a[0] <= a[1] && a[0] <= a[2] ? 0 : a[1] <= a[2] ? 1 : 2;
	return [0, 0, 0].map((_, j) => (j === i ? 1 : 0));
}

/** Unit vector perpendicular to the unit 3D vector `e`: its least parallel axis with the `e` component removed. */
export function refPerp(e: V): number[] {
	const ref = refAxis(e);
	const rest = sub(ref, scale(e, dot(ref, e)));
	return scale(rest, 1 / norm(rest));
}

/** Square built on the directed edge p -> q, on the side `n` (a unit perpendicular): p, q, q + n·len, p + n·len. */
function squareOn(p: V, q: V, n: V): V[] {
	const len = norm(sub(q, p));
	const s = scale(n, len);
	return [p, q, add(q, s), add(p, s)];
}

// ---------------------------------------------------------------------------------------------------------------
// The two arrows (2D DP-1/DP-2, and the same model in 3D for DP-3).

/**
 * Everything a and b determine (spec §2.1 "Derived"), at full precision. Undefined quantities are NaN; geometry that
 * does not exist is null. Works for 2D and 3D vectors alike; `rect` vertices and the display plane follow ruling 4.
 */
export function pair(a: V, b: V) {
	if (a.length !== b.length) throw new Error('pair: dimension mismatch');
	const na2 = dot(a, a), nb2 = dot(b, b), na = Math.sqrt(na2), nb = Math.sqrt(nb2);
	const zeroA = na === 0, zeroB = nb === 0;
	const amb = sub(a, b), namb2 = dot(amb, amb), d = dot(a, b);
	const cosRaw = zeroA || zeroB ? NaN : d / (na * nb);
	// The cosine is clamped before acos: rounding can give 1.0000000000000002 (acos -> NaN) for parallel arrows.
	const cosT = Number.isNaN(cosRaw) ? NaN : clamp(cosRaw, -1, 1);
	const th = Math.acos(cosT) * DEG;
	// The geometric route: lengths times the cosine of the measured angle (same number, other road; may differ in the
	// last bits, which the lesson mentions as a feature).
	const rhs = na * nb * Math.cos(th / DEG);
	const ahat = zeroA ? null : scale(a, 1 / na);
	const shadow = zeroA ? NaN : d / na;
	const rect = zeroA ? NaN : na * shadow;
	const perp = !zeroA && !zeroB && Math.abs(d) < 1e-9;
	const foot = ahat ? scale(ahat, shadow) : null;
	// Rectangle |a| × shadow: base from O along â of signed length `shadow`, height |a| on the side of a's line away
	// from b. Its area is |shadow|·|a| = |a·b|; hatched (negative) exactly when the shadow points backwards.
	const h = ahat ? awayFrom(a, b) : null;
	const rectPts = ahat && foot && h ? [a.map(() => 0), foot, add(foot, scale(h, na)), scale(h, na)] : null;
	return {
		a, b, na2, nb2, na, nb, zeroA, zeroB, amb, namb2, dot: d, cosT, th, rhs, ahat, shadow, rect, perp, foot, h, rectPts,
		bright: Number.isNaN(cosT) ? NaN : Math.max(0, cosT),
		/** Proj of b on a as a vector (the "vector projection", as opposed to the signed length `shadow`). */
		proj: foot,
	};
}

/** The 2D lesson values: `pair` plus the component table of DP-1 and the three squares of the triangle. */
export function derive(v: Vals) {
	const a = v.a as V, b = v.b as V;
	if (a.length !== 2 || b.length !== 2) throw new Error('dot-product: a and b are 2D');
	const z = pair(a, b);
	const [a1, a2] = a, [b1, b2] = b, d1 = a1 - b1, d2 = a2 - b2;
	const O = [0, 0];
	// Squares of the triangle O, a, b, each built outward (away from the third vertex). A zero side has no square.
	const sq_a = z.zeroA ? null : squareOn(O, a, awayFrom(a, b));
	const sq_b = z.zeroB ? null : squareOn(O, b, awayFrom(b, a));
	const sq_amb = z.namb2 === 0 ? null : squareOn(b, a, awayFrom(sub(a, b), sub(O, b)));
	return {
		...z, a1, a2, b1, b2, d1, d2,
		d1sq: d1 * d1, d2sq: d2 * d2, a1sq: a1 * a1, a2sq: a2 * a2, b1sq: b1 * b1, b2sq: b2 * b2, p1: a1 * b1, p2: a2 * b2,
		aa: z.na2, sq_a, sq_b, sq_amb,
	};
}

/** The 3D lesson values of DP-3 (`a3`, `b3`). */
export function derive3(v: Vals) {
	const a = v.a3 as V, b = v.b3 as V;
	if (a.length !== 3 || b.length !== 3) throw new Error('dot-product: a3 and b3 are 3D');
	const z = pair(a, b);
	// The arc of θ needs an in-plane direction for antiparallel arrows: the least parallel axis is never along a.
	const arcRef = z.ahat ? refAxis(z.ahat) : [0, 1, 0];
	return { ...z, p: [0, 1, 2].map((i) => a[i] * b[i]), arcRef };
}

// ---------------------------------------------------------------------------------------------------------------
// The lamp (DP-4).

/**
 * N·L at the probe P for a POINT light: N = P/|P| (sphere at the origin), L = normalize(light − P), so L changes from
 * surface point to surface point exactly as in the shader (`LitSphere` computes the same per fragment). The zero set
 * of N·L on the sphere is the horizon circle P·light = r² (centre r²/|light| along the light, radius
 * r√(1 − (r/|light|)²)): the lit region is a cap smaller than a hemisphere, which is only the directional limit.
 */
export function lamp(v: Vals) {
	const P = v.nl_pt as V, Lp = v.nl_light as V;
	const pn = norm(P), N = scale(P, 1 / pn), toL = sub(Lp, P), L = scale(toL, 1 / norm(toL));
	const ndotl = dot(N, L);
	const thNL = Math.acos(clamp(ndotl, -1, 1)) * DEG;
	const dist = norm(Lp), lhat = scale(Lp, 1 / dist), r = SPHERE_R;
	const term = { c: scale(lhat, (r * r) / dist), r: r * Math.sqrt(1 - (r / dist) ** 2), n: lhat };
	// Probe rectangle: base along N of signed length N·L, height |N| = 1 in the plane of N and L, away from L; the
	// drawing is scaled by RECT_NL (both sides), so its drawn area is 0.36·N·L while its logical area is N·L.
	const h = awayFrom(N, L);
	const k = RECT_NL;
	const rectPts = [P, add(P, scale(N, k * ndotl)), add(add(P, scale(N, k * ndotl)), scale(h, k)), add(P, scale(h, k))];
	return { P, N, L, ndotl, brightNL: Math.max(0, ndotl), thNL, term, rectPts, arcRef: refAxis(N), raw: v.nl_raw === 1 };
}

/** Display scale of the probe rectangle (spec DP-4 `rectNL`, ruling 6). */
export const RECT_NL = 0.6;

/** N·L at an arbitrary surface point for the current light: the same formula the shader evaluates per fragment. */
export function ndotlAt(P: V, light: V): number {
	const N = scale(P, 1 / norm(P)), d = sub(light, P);
	return dot(N, scale(d, 1 / norm(d)));
}

// ---------------------------------------------------------------------------------------------------------------
// Handle mappings.

/** sin 3°: inside this |cos θ| band, DP-2's b is caught exactly perpendicular to a (spec DP-2 `b`). */
export const PERP_ZONE = Math.sin(3 / DEG);

/**
 * DP-2 tip of b: grid-snapped like any tip, then caught exactly perpendicular to a when |cos θ| < sin 3°, keeping its
 * current length and its own side of a's line (the counter-clockwise perpendicular for b counter-clockwise of a). The
 * catch is written off-grid (no snap) and listed in `ease`, so the glyph glides onto the line while the pointer is not
 * fought. A key step is caught only when it moves toward the line (one-way magnet), so the keyboard can always leave.
 */
export function moveB(v: Vals, raw: readonly number[], src: 'drag' | 'key'): Move {
	const a = v.a as V, b0 = v.b as V;
	const b = [snap(b0[0] + (raw[0] ?? 0)), snap(b0[1] + (raw[1] ?? 0))];
	const na = norm(a), nb = norm(b);
	if (!na || !nb) return { set: { b } };
	const cos = dot(a, b) / (na * nb);
	const was = norm(b0) ? Math.abs(dot(a, b0) / (na * norm(b0))) : Infinity;
	if (Math.abs(cos) >= PERP_ZONE || (src === 'key' && Math.abs(cos) >= was)) return { set: { b } };
	const side = a[0] * b[1] - a[1] * b[0] >= 0 ? 1 : -1;
	return { set: { b: scale(ccw(scale(a, 1 / na)), side * nb) }, ease: ['b'] };
}

/** DP-4 lamp: grid-snapped, then kept at least LAMP_MIN from the centre along its own direction (radial link). */
export function moveLight(v: Vals, raw: readonly number[]): Move {
	const l = (v.nl_light as V).map((x, i) => snap(x + (raw[i] ?? 0)));
	return { set: lightLink.resolve({ nl_light: l }, v) };
}

/** DP-4 probe: free (the sphere is not on the grid), then projected back onto the surface (radial link). */
export function moveP(v: Vals, raw: readonly number[]): Move {
	const q = (v.nl_pt as V).map((x, i) => x + (raw[i] ?? 0));
	return { set: probeLink.resolve({ nl_pt: q }, v) };
}

// ---------------------------------------------------------------------------------------------------------------
// DP-1 derivation timeline: reveals, marks and the step-6 cancellation.

/** Number of DP-1 steps (0..8). */
export const STEPS = 9;

/** Delay between the a² pair and the b² pair being struck at step 6 (spec DP-1 step 6). */
export const STRIKE_GAP_MS = 350;

/**
 * Where the cancellation stands for a timeline step, before the reader sees any motion: 0 = nothing struck,
 * 1 = the |a|² pair, 2 = both pairs. Step 6 starts at 1 and the clock moves it to 2 after STRIKE_GAP_MS (reduced
 * motion: 2 at once); a later step is fully struck; an earlier one has nothing struck. Leaving step 6 before the gap
 * cancels the pending write, because the target of any other step is already final.
 */
export function strikeTarget(step: number, reduced: boolean): { now: number; later: number | null } {
	if (step < 6) return { now: 0, later: null };
	if (step > 6 || reduced) return { now: 2, later: null };
	return { now: 1, later: 2 };
}

/** Columns of the DP-1 component table and the step that reveals each (spec "NumTable comp"). */
export const COMP_COLS = [
	{ id: 'a', from: 0 },
	{ id: 'b', from: 0 },
	{ id: 'd', from: 1 },
	{ id: 'dsq', from: 1 },
	{ id: 'asq', from: 1 },
	{ id: 'bsq', from: 1 },
	{ id: 'p', from: 3 },
] as const;

/** The rightmost column a step reveals (scrolled into view at 360 px), or null when the step reveals none. */
export function newestColumn(step: number): string | null {
	const fresh = COMP_COLS.filter((c) => c.from === step);
	return fresh.length ? fresh[fresh.length - 1].id : null;
}
