/**
 * Motion rules of docs/design/visual-language.md, as pure math (no DOM, no three): direct manipulation is 1:1 and
 * never animated; indirect changes (keyboard steps, snaps, timeline steps, Reset view, gizmo align) move with ONE
 * kind of motion, a critically damped spring, so nothing in the course overshoots or wobbles. The camera's smooth
 * view and the first-view draw-in are the same spring driven as a 0..1 progress. `prefers-reduced-motion` turns all
 * of it into hard cuts (callers check `reducedMotion()`).
 */

/** Settle time of indirect value changes (visual-language.md, ~180 ms). */
export const SPRING_MS = 180;
/** Reset view / gizmo align, like Blender's smooth view (~250 ms). */
export const VIEW_MS = 250;
/** The one orchestrated first-view draw-in of a lesson (600 ms). */
export const DRAW_MS = 600;

/** Residual (1 + x) e^-x of a critically damped spring released from rest, x = omega * t. */
const residual = (x: number): number => (1 + x) * Math.exp(-x);

/**
 * Angular frequency of a critically damped spring that, released from rest, is within `tol` of its target exactly
 * `ms` milliseconds later. Solves (1 + x) e^-x = tol by Newton's method (the residual is convex and decreasing).
 */
export function omegaFor(ms: number, tol = 0.01): number {
	let x = 5;
	for (let i = 0; i < 50; i++) {
		const f = residual(x) - tol;
		const df = -x * Math.exp(-x);
		const nx = x - f / df;
		if (Math.abs(nx - x) < 1e-13) break;
		x = nx;
	}
	return x / (ms / 1000);
}

export interface SpringState {
	x: number;
	v: number;
}

/**
 * Advances a critically damped spring by `dt` seconds with the exact solution x(t) = target + (c1 + c2 t) e^(-wt),
 * so the result does not depend on the frame rate (one 180 ms step equals 45 steps of 4 ms). Critical damping is the
 * fastest motion that never overshoots: from rest, x approaches the target monotonically.
 */
export function springStep(s: SpringState, target: number, omega: number, dt: number): SpringState {
	const c1 = s.x - target;
	const c2 = s.v + omega * c1;
	const e = Math.exp(-omega * dt);
	return { x: target + (c1 + c2 * dt) * e, v: (c2 - omega * (c1 + c2 * dt)) * e };
}

/** Residual below which a progress counts as arrived (sub-pixel for any on-screen motion). */
const DONE = 1e-3;
// springProgress runs per frame for a handful of fixed durations; the Newton solve is done once per duration.
const omegas = new Map<number, number>();

/**
 * Progress 0..1 of a spring released from rest at time 0 toward 1, with `ms` the 1% settle time: monotone, no
 * overshoot, >= 0.99 at `ms`, and exactly 1 once the residual is below 0.1% (so animations end on the exact target
 * and stop requesting frames).
 */
export function springProgress(tMs: number, ms: number): number {
	if (!(ms > 0)) return 1;
	if (tMs <= 0) return 0;
	let w = omegas.get(ms);
	if (w === undefined) omegas.set(ms, (w = omegaFor(ms)));
	const r = residual(w * (tMs / 1000));
	return r < DONE ? 1 : 1 - r;
}

/** True when the reader asked for reduced motion: every animation becomes a cut. Browser-only callers. */
export function reducedMotion(): boolean {
	return typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Camera state of an orbit view, in three.js spherical terms (y up): the camera sits at
 * target + dist * (sin(polar) sin(azimuth), cos(polar), sin(polar) cos(azimuth)), looking at `target`.
 * `zoom` is the camera zoom (orthographic cameras; perspective ones keep 1).
 */
export interface Pose {
	target: readonly [number, number, number];
	polar: number;
	azimuth: number;
	dist: number;
	zoom: number;
}

type P3 = readonly [number, number, number];

/** Pose of a camera at `pos` looking at `target`. */
export function poseOf(pos: P3, target: P3, zoom = 1): Pose {
	const d = [pos[0] - target[0], pos[1] - target[1], pos[2] - target[2]];
	const dist = Math.hypot(d[0], d[1], d[2]);
	if (dist === 0) throw new Error('camera position equals its target');
	return { target: [...target], polar: Math.acos(Math.min(1, Math.max(-1, d[1] / dist))), azimuth: Math.atan2(d[0], d[2]), dist, zoom };
}

/** Camera position of a pose. */
export function positionOf(p: Pose): [number, number, number] {
	const s = Math.sin(p.polar);
	return [p.target[0] + p.dist * s * Math.sin(p.azimuth), p.target[1] + p.dist * Math.cos(p.polar), p.target[2] + p.dist * s * Math.cos(p.azimuth)];
}

const lerp = (a: number, b: number, s: number) => a + (b - a) * s;

/**
 * Pose between `a` (s = 0) and `b` (s = 1): the camera swings around the target on the sphere (polar and azimuth,
 * azimuth the short way round) instead of cutting through it, the target and distance glide, and zoom changes
 * geometrically so zooming in and out by the same factor take the same time.
 */
export function lerpPose(a: Pose, b: Pose, s: number): Pose {
	if (s <= 0) return a;
	let dAz = (b.azimuth - a.azimuth) % (2 * Math.PI);
	if (dAz > Math.PI) dAz -= 2 * Math.PI;
	if (dAz < -Math.PI) dAz += 2 * Math.PI;
	return {
		target: [lerp(a.target[0], b.target[0], s), lerp(a.target[1], b.target[1], s), lerp(a.target[2], b.target[2], s)],
		polar: lerp(a.polar, b.polar, s),
		azimuth: a.azimuth + dAz * s,
		dist: lerp(a.dist, b.dist, s),
		zoom: a.zoom * (b.zoom / a.zoom) ** s,
	};
}

/**
 * Polar angle just off the poles: OrbitControls keeps the camera's up = +y and clamps the polar angle away from 0 and
 * pi, so a top/bottom view is approached as closely as the controls allow (the image is the exact axis view).
 */
const POLE = 1e-4;

/**
 * Pose looking along axis `axis` (0 = x, 1 = y, 2 = z) from its `sign` side (the camera sits on +x for "+X"), with
 * the target, distance and zoom of `base`. Top and bottom views use azimuth 0, so screen right is +x as in Blender.
 */
export function axisPose(axis: 0 | 1 | 2, sign: 1 | -1, base: Pose): Pose {
	const at = (polar: number, azimuth: number): Pose => ({ ...base, polar, azimuth });
	if (axis === 1) return at(sign > 0 ? POLE : Math.PI - POLE, 0);
	if (axis === 0) return at(Math.PI / 2, (sign * Math.PI) / 2);
	return at(Math.PI / 2, sign > 0 ? 0 : Math.PI);
}
