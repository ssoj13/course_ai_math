/** Pure vector helpers for lesson scenes. Arrays in/out, any dimension unless noted; no DOM, no three. */
export type V = readonly number[];

/** Silent truncation by `zip` would hide a 2D/3D mix-up in a lesson, so mismatches are loud. */
function same(a: V, b: V): void {
	if (a.length !== b.length) throw new Error(`dimension mismatch: ${a.length} vs ${b.length}`);
}

export const add = (a: V, b: V): number[] => (same(a, b), a.map((x, i) => x + b[i]));
export const sub = (a: V, b: V): number[] => (same(a, b), a.map((x, i) => x - b[i]));
export const scale = (a: V, s: number): number[] => a.map((x) => x * s);
export const dot = (a: V, b: V): number => (same(a, b), a.reduce((s, x, i) => s + x * b[i], 0));
export const norm = (a: V): number => Math.hypot(...a);

/** Unit vector. Throws on zero length: a NaN direction would silently vanish a scene object. */
export function normalize(a: V): number[] {
	const n = norm(a);
	if (n === 0) throw new Error('cannot normalize a zero vector');
	return scale(a, 1 / n);
}

/** Angle in radians in [0, pi]. The cosine is clamped because rounding can push it just outside [-1, 1] (acos -> NaN). */
export function angleBetween(a: V, b: V): number {
	const d = norm(a) * norm(b);
	if (d === 0) throw new Error('angle with a zero vector is undefined');
	return Math.acos(Math.min(1, Math.max(-1, dot(a, b) / d)));
}

/** Vector projection of `a` onto `onto`. Throws if `onto` is zero. */
export function project(a: V, onto: V): number[] {
	const dd = dot(onto, onto);
	if (dd === 0) throw new Error('cannot project onto a zero vector');
	return scale(onto, dot(a, onto) / dd);
}

/**
 * Points of the shorter arc from direction `from` to direction `to`, radius `r` around `center`
 * (n segments -> n+1 points). Works in 2D and 3D through an orthonormal basis of the plane
 * spanned by the two rays (Gram-Schmidt), so it needs no cross product.
 * Throws when the plane is undefined: zero, parallel or antiparallel rays (an antiparallel
 * pair has infinitely many "shorter" arcs; callers decide how to draw a straight angle).
 */
export function arcPoints(center: V, from: V, to: V, r: number, n = 24): number[][] {
	same(center, from);
	same(from, to);
	if (!Number.isInteger(n) || n < 1) throw new Error(`arc needs a positive integer segment count, got ${n}`);
	const e1 = normalize(from);
	const rest = sub(to, scale(e1, dot(to, e1))); // component of `to` perpendicular to `from`
	// Relative threshold: tiny perpendicular residue is rounding, not a real plane.
	if (norm(rest) <= 1e-9 * norm(to)) throw new Error('arc rays are parallel or zero: plane is undefined');
	const e2 = normalize(rest);
	// e2 points toward `to`, so theta lies in (0, pi) and the sweep is automatically the short way.
	const theta = Math.atan2(dot(to, e2), dot(to, e1));
	return Array.from({ length: n + 1 }, (_, i) => {
		const t = (theta * i) / n;
		return add(center, add(scale(e1, r * Math.cos(t)), scale(e2, r * Math.sin(t))));
	});
}

/** z-component of the 2D cross product: signed parallelogram area, positive when b is counter-clockwise from a. */
export function signedArea2(a: V, b: V): number {
	if (a.length !== 2 || b.length !== 2) throw new Error('signedArea2 is 2D only');
	return a[0] * b[1] - a[1] * b[0];
}
