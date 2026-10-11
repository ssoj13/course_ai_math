import type { LessonState } from '../store';
import type { V } from '../geom';
import { add, sub, scale, dot } from '../geom';
import { SPRING_MS, springProgress, omegaFor } from '../scene/motion';
import { lcPiece } from '../scene/polygon';
import { magnet, snap } from '../scene/drag';
import type { Draggable } from '../scene/handles';

/**
 * Lesson 0.5, law of cosines: the one `derive()` every scene, formula, row and challenge shares (spec §0.1), the
 * two-bone IK solve of LC-3, the vertex drag mappings and the reader transport maths (pause / scrub of a piece
 * motion). Three-free and DOM-free, so the pure tests import it directly.
 */

/** Lesson store id. */
export const LESSON = 'law-of-cosines';

/**
 * Initial store values. `cut_*` and `proof_fade_*` are the two reader transports (one per animated timeline):
 * `position` is the fractional placement the scenes draw, `from` / `to` the integer steps of the running
 * transition, `progress` 0..1, `play` 0/1 and `phases` the number of chained springs (cut step 9 has three).
 */
export const INITIAL = {
	a: 3, b: 4, th: 60,
	ik_hand: [2.5, 1.5], ik_l1: 2, ik_l2: 1.5, ik_flip: 0,
	cut_position: 0, cut_from: 0, cut_to: 0, cut_progress: 1, cut_play: 0, cut_phases: 1,
	proof_fade_position: 0, proof_fade_from: 0, proof_fade_to: 0, proof_fade_progress: 1, proof_fade_play: 0, proof_fade_phases: 1,
};

/** Angles (degrees) the vertex drag of A sticks to (spec §0.3). */
export const MAGNETS = [0, 30, 45, 60, 90, 120, 135, 150, 180] as const;

type Vals = LessonState['vals'];

export const clamp = (x: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, x));

/** Lengths and areas: 2 decimals; `+x.toFixed(2)` first so that -0.001 prints 0.00, not -0.00 (spec §0.2). */
export const fmt = (x: number) => (Number.isFinite(x) ? (+x.toFixed(2)).toFixed(2) : '–');
/** Cosines: 3 decimals. */
export const fmtCos = (x: number) => (Number.isFinite(x) ? (+x.toFixed(3)).toFixed(3) : '–');
/** Angles: whole degrees when integer, otherwise one decimal (the suffix is added by the caller). */
export const fmtAngle = (x: number) => (Number.isInteger(x) ? String(x) : (+x.toFixed(1)).toFixed(1));

/** Clockwise quarter turn: the outward normal of a CCW polygon edge (spec §0.1). */
export const cw = (p: V): V => [p[1], -p[0]];

/**
 * Everything the triangle a, b, θ determines, signed and at full precision; formatted labels never feed geometry.
 * Names follow the spec: C at the origin, B on the x axis, A at angle θ; D and E are the feet of the shadows of
 * b on a and of a on b; F is the foot of the altitude from C onto c. `|cos| < 1e-12` is rounded to 0 so that
 * θ = 90° is exact (strips vanish exactly, not to 1e-16).
 */
export function derive(v: Vals) {
	const a = v.a as number, b = v.b as number, th = v.th as number, rad = th * Math.PI / 180;
	const cos = Math.abs(Math.cos(rad)) < 1e-12 ? 0 : Math.cos(rad);
	const sin = Math.abs(Math.sin(rad)) < 1e-12 ? 0 : Math.sin(rad);
	const C: V = [0, 0], B: V = [a, 0], u: V = [cos, sin], A = scale(u, b), D: V = [b * cos, 0], E = scale(u, a * cos);
	const c = Math.hypot(A[0] - a, A[1]), degenerate = c < 1e-9;
	// Direction B -> A along side c and its outward normal; a degenerate triangle (A on B) has no c direction,
	// so a fallback keeps every downstream value finite (nothing divides by c).
	const w: V = degenerate ? [0, 1] : scale(sub(A, B), 1 / c), m = cw(w);
	const FB = degenerate ? 0 : dot(sub(C, B), w), AF = degenerate ? 0 : c - FB, F = add(B, scale(w, FB));
	const p = b * cos, q = a * cos, h = b * sin, DB = a - p, EA = b - q;
	return {
		a, b, th, rad, cos, sin, C, B, A, D, E, F, u, w, m, c, degenerate, p, q, h, DB, EA, FB, AF,
		a2: a * a, b2: b * b, c2: c * c, pyth: a * a + b * b, corr: -2 * a * b * cos,
		// Areas: the two strips (equal by a·b·cos θ), the two leftovers and their images inside the square on c.
		strip_a: a * p, strip_b: b * q, ab: a * p, R1: a * DB, R2: b * EA, R1c: c * FB, R2c: c * AF,
		// LC-2 proof tiles: the legs of the right triangle ADB and the pieces of the square on b.
		rest: DB, rest2: DB * DB, rise: h, rise2: h * h, bcos2: p * p, bsin2: h * h,
	};
}

/**
 * Two-bone IK (LC-3): three lengths in, the elbow angle out. The target can be unreachable: the cosine is clamped
 * and the bones keep their exact lengths in every branch (out of reach: straight arm stopping short; too close:
 * forearm folded back). `ik_flip` is the pole-vector bit, choosing the side of the shoulder-target line.
 */
export function solveIK(v: Vals) {
	const T = v.ik_hand as V, l1 = v.ik_l1 as number, l2 = v.ik_l2 as number, d = Math.hypot(...T);
	const reachMin = Math.abs(l1 - l2), reachMax = l1 + l2;
	// A target on the shoulder has no direction: +x by convention (the scene says so).
	const u: V = d < 1e-9 ? [1, 0] : scale(T, 1 / d);
	const cosraw = (l1 * l1 + l2 * l2 - d * d) / (2 * l1 * l2), cosc = clamp(cosraw, -1, 1);
	const phi = Math.acos(cosc) * 180 / Math.PI;
	// Shoulder offset from the shoulder-target line: the same law with the sides relabelled.
	const cosb = d < 1e-9 ? 1 : clamp((l1 * l1 + d * d - l2 * l2) / (2 * l1 * d), -1, 1);
	const beta = Math.acos(cosb) * (v.ik_flip ? -1 : 1), cb = Math.cos(beta), sb = Math.sin(beta);
	const S: V = [0, 0], E: V = [l1 * (u[0] * cb - u[1] * sb), l1 * (u[0] * sb + u[1] * cb)];
	const delta = sub(T, E), n = Math.hypot(...delta);
	const W = add(E, scale(n < 1e-9 ? scale(u, -1) : scale(delta, 1 / n), l2));
	return {
		S, E, W, T, u, l1, l2, d, cosraw, cosc, phi, beta, reachMin, reachMax,
		zero: d < 1e-9, outside: d > reachMax + 1e-9, inside: d < reachMin - 1e-9,
	};
}

/**
 * Vertex A drag: polar, writes b (radius, grid snapped) and θ (angle, magnet). θ is an unsigned interior angle, so a
 * pointer below the line CB (y < 0) clamps to the nearer end, 180° for x < 0 and 0° otherwise: A stays continuous
 * under the cursor at both ends instead of flipping the triangle on a one-pixel overshoot (controller ruling on
 * audit M-3). A keyboard step is smaller than the magnet zone, so for `src === 'key'` the current θ is passed as
 * `from`: the magnet then only catches steps that move toward a target (AGENTS.md "Magnet" trap). A caught θ is
 * listed in `ease` so the glyph glides onto it.
 */
export const moveA: NonNullable<Draggable['move']> = (v, raw, src) => {
	const A = derive(v).A, x = A[0] + raw[0], y = A[1] + raw[1];
	const angle = y < 0 ? (x < 0 ? 180 : 0) : clamp(Math.atan2(y, x) * 180 / Math.PI, 0, 180);
	const th = magnet(angle, MAGNETS, 2.5, src === 'key' ? v.th as number : undefined);
	return { set: { b: clamp(snap(Math.hypot(x, y)), .5, 4), th }, ease: th !== angle ? ['th'] : [] };
};

/** Vertex B drag: along the x axis only, writes a. */
export const moveB: NonNullable<Draggable['move']> = (v, raw) => ({ set: { a: clamp(snap((v.a as number) + raw[0]), .5, 4) } });

/**
 * Placement of a moving leftover piece at a (possibly fractional) cut step. Piece 1 moves one phase per authored
 * step 6/7/8 (lean, turn, lean); piece 2 runs all three phases inside step 9, so its placement is 3 × the
 * fraction of step 9 reached.
 */
export function placement(step: number, piece: 1 | 2) {
	return piece === 1 ? clamp(step - 5, 0, 3) : 3 * clamp(step - 8, 0, 1);
}

/** The piece polygon at a cut step (engine `lcPiece`: area-preserving shear, rigid turn, shear). */
export function pieceAt(v: Vals, piece: 1 | 2, step: number): V[] {
	return lcPiece(v.a as number, v.b as number, v.th as number, piece, placement(step, piece));
}

/** The square built outward (cw normal) on the directed edge a -> b. */
export function square(a: V, b: V): V[] {
	const n = cw(sub(b, a));
	return [a, b, add(b, n), add(a, n)];
}

/** The strip cut off a square: width = the shadow (C to D, or C to E), height = the side. */
export function strip(v: Vals, piece: 1 | 2): V[] {
	const z = derive(v), end = piece === 1 ? z.D : z.E, n: V = piece === 1 ? [0, -z.a] : scale([-z.sin, z.cos], z.b);
	return [z.C, end, add(end, n), n];
}

/**
 * Progress 0..1 of a transition after `elapsed` ms: one clock for all paper pieces, `phases` chained springs that
 * settle in turn (each takes SPRING_MS). Normalised so that a settled spring reaches exactly 1.
 */
export function cutProgress(elapsed: number, phases: number) {
	if (elapsed >= SPRING_MS * phases) return 1;
	const n = Math.max(0, elapsed) / SPRING_MS, i = Math.floor(n);
	return (i + springProgress((n - i) * SPRING_MS, SPRING_MS) / springProgress(SPRING_MS, SPRING_MS)) / phases;
}

/**
 * Inverse of `cutProgress` (bisection on the critically damped spring), used only when resuming a paused or
 * reader-scrubbed position so playback continues from there instead of restarting.
 */
export function cutElapsed(progress: number, phases: number) {
	const p = clamp(progress, 0, 1);
	if (p === 1) return SPRING_MS * phases;
	const n = p * phases, i = Math.floor(n), fraction = (n - i) * springProgress(SPRING_MS, SPRING_MS), omega = omegaFor(SPRING_MS) / 1000;
	let lo = 0, hi = SPRING_MS;
	for (let k = 0; k < 48; k++) {
		const m = (lo + hi) / 2, q = 1 - (1 + omega * m) * Math.exp(-omega * m);
		if (q < fraction) lo = m; else hi = m;
	}
	return i * SPRING_MS + (lo + hi) / 2;
}

/**
 * Whether going from cut step `from` to `to` moves anything on screen (a piece placement, or a strip fill-in at
 * steps 2/3), so the transport controls are shown only for steps that animate.
 */
export function cutMoves(from: number, to: number, step: number) {
	if (step >= 4 && (placement(from, 1) !== placement(to, 1) || placement(from, 2) !== placement(to, 2))) return true;
	return (step >= 2 && clamp(from - 1, 0, 1) !== clamp(to - 1, 0, 1)) || (step >= 3 && clamp(from - 2, 0, 1) !== clamp(to - 2, 0, 1));
}
