/**
 * Lesson 1.5 (projection: shadows on an axis): the store contract and every derived number the PJ scenes, formulas,
 * tables, N-panel rows and challenges read (storyboard §2 1.5). Pure and three-free, so the tests import it directly.
 *
 * Keys of the `projection` store:
 * - `a`, `b` (2D): the arrow and the axis of PJ-1 (signed shadow) and PJ-2 (vector projection, remainder, tiles);
 * - `c` (2D), `phi`, `gap` (degrees): PJ-3, the arrow and an axis pair u₁ = (cos φ, sin φ), u₂ at angle `gap` from u₁;
 * - `a3`, `n3` (3D): PJ-4, the arrow and the unit normal of a plane through O (`n3` is kept on the unit sphere).
 *
 * Domain rules:
 * - a zero axis b has no direction: the shadow, the projection and the remainder are undefined (NaN / null), while
 *   a·b itself is 0 and always defined;
 * - a zero a has a shadow of 0 on any nonzero axis;
 * - "coordinate = dot product with a unit axis" holds only for perpendicular unit axes (orthonormal); PJ-3 computes the
 *   real coordinates (solving s·u₁ + t·u₂ = c) and the rebuild from shadows separately, so the gap is visible.
 */
import { add, cross, dot, planeBasis, scale, sub, type V } from '../geom';
import type { LessonLinks } from '../links';
import { radialLink } from '../scene/radialLink';
import type { LessonState, Val } from '../store';

export const LESSON = 'projection';

export const SPEC = {
	a: { min: -4.5, max: 4.5, step: 0.1 },
	b: { min: -4.5, max: 4.5, step: 0.1 },
	c: { min: -4.5, max: 4.5, step: 0.1 },
	phi: { min: -90, max: 90, step: 1 },
	gap: { min: 30, max: 150, step: 1 },
	a3: { min: -3.5, max: 3.5, step: 0.1 },
	n3: { min: -1, max: 1, step: 0.05 },
} as const;

/** Declared initial direction of the plane normal (the floor) and its zero-request fallback. */
export const N_INITIAL = [0, 1, 0] as const;
/** The plane normal is a unit vector: one rule for its field, its handle and the initial value. */
export const normalLink = radialLink('n3', 1, 'surface', N_INITIAL);
export const LINKS: LessonLinks = { groups: [normalLink] };

/**
 * Defaults. a = (1, 3) onto b = (3, 1): the shadow 6/√10 ≈ 1.90 is positive and not a round number, so the reader sees
 * the general case first; c = (3, 4) with the standard axes is the prose's "coordinates are shadows"; a3 = (2, −3, 1)
 * on the floor is the prose's collision example (slide (2, 0, 1)).
 */
export const INITIAL: Record<string, Val> = {
	a: [1, 3],
	b: [3, 1],
	c: [3, 4],
	phi: 0,
	gap: 90,
	a3: [2, -3, 1],
	n3: [...N_INITIAL],
};

type Vals = LessonState['vals'];

const vec = (v: Val | undefined, key: string): number[] => {
	if (!Array.isArray(v)) throw new Error(`projection: "${key}" must be a vector`);
	return [...v];
};
const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number' || !Number.isFinite(v)) throw new Error(`projection: "${key}" must be a finite number`);
	return v;
};

// ---------------------------------------------------------------------------------------------------------------
// Formatting (fixed decimals: a live number never changes width by a trailing zero; never "-0.00").

const fixed = (x: number, n: number) => (+x.toFixed(n) + 0).toFixed(n);
export const fmt1 = (x: number) => (Number.isFinite(x) ? fixed(x, 1) : '–');
export const fmt2 = (x: number) => (Number.isFinite(x) ? fixed(x, 2) : '–');
export const fmtVec = (v: V | null, f: (x: number) => string = fmt2) => (v ? '(' + v.map(f).join(', ') + ')' : '–');
/** Whole degrees for the PJ-3 angles (the fields step by 1°). */
export const fmtDeg = (x: number) => (Number.isFinite(x) ? String(Math.round(x) + 0) + '°' : '–');
export const texFmt = {
	n: (x: number) => (Number.isFinite(x) ? fixed(x, 2) : String.raw`\text{--}`),
	c: (x: number) => (Number.isFinite(x) ? fixed(x, 1) : String.raw`\text{--}`),
	deg: (x: number) => (Number.isFinite(x) ? String(Math.round(x) + 0) + String.raw`^\circ` : String.raw`\text{--}`),
};

const DEG = Math.PI / 180;
/** A shadow this close to 0 counts as zero (exact zeros are reachable on the 0.1 grid; rounding leaves ~1e-16). */
export const ZERO = 1e-9;

// ---------------------------------------------------------------------------------------------------------------
// The projection of a onto the line of b.

/**
 * Everything the shadow of `a` on the line of `b` determines, in 2D or 3D. Undefined (b = 0) quantities are NaN and
 * missing geometry is null. Two routes to the vector projection are computed independently: through the unit vector
 * b̂ (`proj`) and without normalizing, (a·b)/(b·b)·b (`projAlt`); the lesson shows they agree.
 */
export function shadow(a: V, b: V) {
	if (a.length !== b.length) throw new Error('shadow: dimension mismatch');
	const ab = dot(a, b), bb = dot(b, b), nb = Math.sqrt(bb), na = Math.hypot(...a);
	const zeroB = !(nb > 0);
	const bh = zeroB ? null : scale(b, 1 / nb);
	const s = bh ? dot(a, bh) : NaN;
	const proj = bh ? scale(bh, s) : null;
	const projAlt = zeroB ? null : scale(b, ab / bb);
	const perp = proj ? sub(a, proj) : null;
	const sign: 'pos' | 'neg' | 'zero' | 'none' = zeroB ? 'none' : s > ZERO ? 'pos' : s < -ZERO ? 'neg' : 'zero';
	const cosT = zeroB || !(na > 0) ? NaN : Math.max(-1, Math.min(1, ab / (na * nb)));
	return {
		a: [...a], b: [...b], ab, bb, nb, na, zeroB, bh, s, proj, projAlt, perp, sign,
		th: Math.acos(cosT) / DEG,
		/** |proj|², |perp|², and their sum, which equals |a|² (Pythagoras of the right triangle O, proj, a). */
		proj2: proj ? dot(proj, proj) : NaN,
		perp2: perp ? dot(perp, perp) : NaN,
		na2: dot(a, a),
		/** perp·b: 0 up to rounding whenever b ≠ 0 (the remainder is perpendicular to the axis). */
		perpDotB: perp ? dot(perp, b) : NaN,
	};
}

// ---------------------------------------------------------------------------------------------------------------
// PJ-3: coordinates as shadows on an axis pair.

/** The axis pair of PJ-3: u₁ at angle φ, u₂ at angle φ + gap (degrees), both unit length. */
export function axes(phi: number, gap: number): [number[], number[]] {
	const p = phi * DEG, q = (phi + gap) * DEG;
	return [[Math.cos(p), Math.sin(p)], [Math.cos(q), Math.sin(q)]];
}

/**
 * The shadows c·u₁, c·u₂, the real coordinates (s, t) with s·u₁ + t·u₂ = c (Cramer's rule; the axes are never
 * parallel because gap ∈ [30°, 150°]), and the rebuild from shadows (c·u₁)u₁ + (c·u₂)u₂ with its miss |rebuild − c|.
 * Orthonormal axes (gap = 90°): shadows = coordinates and the miss is 0.
 */
export function coords(c: V, phi: number, gap: number) {
	const [u1, u2] = axes(phi, gap);
	const sh = [dot(c, u1), dot(c, u2)];
	const det = u1[0] * u2[1] - u1[1] * u2[0];
	const co = [(c[0] * u2[1] - c[1] * u2[0]) / det, (u1[0] * c[1] - u1[1] * c[0]) / det];
	const rebuild = add(scale(u1, sh[0]), scale(u2, sh[1]));
	const miss = Math.hypot(...sub(rebuild, c));
	return { c: [...c], u1, u2, sh, co, rebuild, miss, ortho: Math.abs(gap - 90) < 1e-9, feet: [scale(u1, sh[0]), scale(u2, sh[1])] };
}

// ---------------------------------------------------------------------------------------------------------------
// PJ-4: projection onto a plane through O with unit normal n.

/**
 * The ground part a − (a·n)n, the normal part (a·n)n, the reflection a − 2(a·n)n, and the same ground part rebuilt
 * from two shadows on an orthonormal pair (e₁, e₂) of the plane: (a·e₁)e₁ + (a·e₂)e₂ (the "two projections").
 */
export function plane(a: V, n: V) {
	const len = Math.hypot(...n);
	if (!(Math.abs(len - 1) < 1e-9)) throw new Error('plane: the normal must be a unit vector (radial link)');
	const an = dot(a, n);
	const along = scale(n, an);
	const ground = sub(a, along);
	const refl = sub(a, scale(n, 2 * an));
	const basis = planeBasis(n);
	if (!basis) throw new Error('plane: zero normal');
	const [e1, e2] = basis;
	const sh = [dot(a, e1), dot(a, e2)];
	const legs = [scale(e1, sh[0]), scale(e2, sh[1])];
	return { a: [...a], n: [...n], an, along, ground, refl, e1, e2, sh, legs, rebuild: add(legs[0], legs[1]), side: cross(e1, e2) };
}

// ---------------------------------------------------------------------------------------------------------------

export function derive(vals: Vals) {
	return {
		p: shadow(vec(vals.a, 'a'), vec(vals.b, 'b')),
		q: coords(vec(vals.c, 'c'), num(vals.phi, 'phi'), num(vals.gap, 'gap')),
		g: plane(vec(vals.a3, 'a3'), vec(vals.n3, 'n3')),
	};
}

/** The 2D triangle O, foot, a turns counter-clockwise (AreaTile `flip` follows it; see the 1.5 scenes). */
export function ccw(foot: V, a: V): boolean {
	return foot[0] * a[1] - foot[1] * a[0] > 0;
}

// ---------------------------------------------------------------------------------------------------------------
// Challenge predicates (store values only).

const near = (x: number, y: number, tol = 1e-9) => Math.abs(x - y) <= tol;

export const CHECKS = {
	/** PJ-1: a negative shadow (a leans backwards from b). */
	negative: (x: Vals) => shadow(vec(x.a, 'a'), vec(x.b, 'b')).sign === 'neg',
	/** PJ-1: a shadow of exactly 0 with both arrows nonzero (a ⟂ b). */
	zero: (x: Vals) => {
		const z = shadow(vec(x.a, 'a'), vec(x.b, 'b'));
		return z.sign === 'zero' && z.na > 0;
	},
	/** PJ-1: the shadow as long as a itself (a on the line of b, pointing the same way). */
	full: (x: Vals) => {
		const z = shadow(vec(x.a, 'a'), vec(x.b, 'b'));
		return z.na > 0 && near(z.s, z.na, 1e-9);
	},
	/** PJ-2: the two tiles equal, |proj|² = |perp|² (θ = 45° or 135°), with a ≠ 0. */
	equal: (x: Vals) => {
		const z = shadow(vec(x.a, 'a'), vec(x.b, 'b'));
		return !z.zeroB && z.na > 0 && near(z.proj2, z.perp2, 1e-9);
	},
	/** PJ-3: axes turned to 45°, still perpendicular: new shadows, and they still rebuild c. */
	turned: (x: Vals) => x.phi === 45 && x.gap === 90,
	/** PJ-3: non-perpendicular axes: the shadows no longer rebuild c. */
	oblique: (x: Vals) => coords(vec(x.c, 'c'), num(x.phi, 'phi'), num(x.gap, 'gap')).miss > 0.05,
	/** PJ-4: a vector that lies in the plane (no normal part) and is not zero. */
	inPlane: (x: Vals) => {
		const g = plane(vec(x.a3, 'a3'), vec(x.n3, 'n3'));
		return Math.abs(g.an) < 1e-9 && Math.hypot(...g.a) > 0;
	},
};
