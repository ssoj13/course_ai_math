/**
 * Lesson 1.1 (a vector: an arrow and a list of numbers): the store contract, the drag mappings and every derived
 * number of its six scenes. Pure and three-free: the entry, the scenes, the N-panel rows and the tests share it.
 *
 * Scenes and their keys (each scene owns its keys, so a drag in one card never rewrites another card's readout):
 * - VE-1 arrow = list: `v`.
 * - VE-2 no address: `tail` (where the arrow is drawn), `w` (its components), `anchor` (0 = free direction,
 *   1 = position: drawn from the origin, so the tip is a point). One link group keeps the whole arrow on the board.
 * - VE-3 translate an object: `T` (the translation), `asPoint` (1 = the normals are wrongly moved like points).
 * - VE-4 3D arrow: `v3`.
 * - VE-5 B − A in 2D: `A`, `B`, `rev` (1 = show the trip back, A − B).
 * - VE-6 B − A in 3D: `A3`, `B3`.
 */
import type { LessonLinks, LinkGroup } from '../links';
import { finite } from '../links';
import type { LessonState, Val } from '../store';
import { add, norm, scale, sub, type V } from '../geom';
import { snap } from '../scene/drag';
import type { Move } from '../scene/handles';

export const LESSON = 'vector';

type Vals = LessonState['vals'];

/** A closed interval, used for boards (where a scene's points may go) and N-panel field ranges. */
export interface Box {
	min: number;
	max: number;
}

/** Component range of the free arrows VE-1 and the 3D arrow VE-4. */
export const RANGE_V = { min: -4.5, max: 4.5, step: 0.1 } as const;
export const RANGE_V3 = { min: -3.5, max: 3.5, step: 0.1 } as const;
/**
 * VE-2's board: the prose slides the arrow (3, 2) to start at (5, 5), so its tip reaches (8, 7). Both ends of the
 * arrow stay inside it; the components may then span the whole board in either direction.
 */
export const BOARD2: Box = { min: -3, max: 8 };
export const RANGE_TAIL = { min: BOARD2.min, max: BOARD2.max, step: 0.1 } as const;
export const RANGE_W = { min: BOARD2.min - BOARD2.max, max: BOARD2.max - BOARD2.min, step: 0.1 } as const;
/** VE-3's translation range: the object (left, below the origin) may move right and up across the board. */
export const RANGE_T = { min: -1, max: 5, step: 0.1 } as const;
/** VE-5's board (prose: A = (1, 2), B = (4, 6)) and VE-6's 3D box. */
export const BOARD5: Box = { min: -2, max: 7 };
export const RANGE_P = { min: BOARD5.min, max: BOARD5.max, step: 0.1 } as const;
export const BOARD6: Box = { min: -3, max: 3 };
export const RANGE_P3 = { min: BOARD6.min, max: BOARD6.max, step: 0.1 } as const;

/**
 * The VE-3 object: three vertices of a tilted floor strip (direction (1.5, −1)) and the one normal they share, the
 * unit vector perpendicular to the strip, (1, 1.5)/|(1, 1.5)| ≈ (0.555, 0.832). The strip runs down to the right, so
 * the translations the lesson asks for (up and to the right) carry it sideways, never along itself.
 */
export const STRIP: readonly V[] = [[-4, -0.5], [-2.5, -1.5], [-1, -2.5]];
export const NORMAL: V = scale([1, 1.5], 1 / Math.hypot(1, 1.5));

/** Defaults. Every challenge starts unsolved (checked in the tests). */
export const INITIAL: Record<string, Val> = {
	v: [3, 2],
	tail: [0, 0],
	w: [3, 2],
	anchor: 0,
	T: [2.5, 2],
	asPoint: 0,
	// Not the prose's (2, 1, 3): that arrow points almost straight at the home camera and reads as a dot.
	v3: [3, 2, -1],
	A: [1, 2],
	B: [4, 6],
	rev: 0,
	A3: [-1, 0, 1],
	B3: [2, 2, -1],
};

const clamp = (x: number, b: Box) => Math.min(b.max, Math.max(b.min, x));
const vec = (v: Val | undefined, key: string, dims?: number): readonly number[] => {
	if (typeof v === 'number' || v === undefined || (dims !== undefined && v.length !== dims)) throw new Error(`vector: "${key}" must be a ${dims ?? ''}-vector`);
	finite(v);
	return v;
};
const flag = (v: Val | undefined, key: string): 0 | 1 => {
	if (v !== 0 && v !== 1) throw new Error(`vector: "${key}" must be 0 or 1`);
	return v;
};

/**
 * The largest shift of both ends `p` and `q` by (up to) `d` that keeps both inside `b`, component by component.
 * Translating an arrow must never change its components, so instead of clamping each end on its own (which would
 * squash the arrow at the edge) the shift itself stops at the edge.
 */
export function clampShift(p: V, q: V, d: V, b: Box): number[] {
	return d.map((x, i) => {
		const lo = b.min - Math.min(p[i], q[i]);
		const hi = b.max - Math.max(p[i], q[i]);
		return Math.min(hi, Math.max(lo, x));
	});
}

/**
 * VE-2's free arrow {tail, w, anchor}. Rules, in order: a free arrow keeps both ends on BOARD2; an anchored one is
 * drawn from the origin, so its components are the tip itself and stay on the board. A new `w` (tip drag, typed
 * components) keeps the tail and stops the tip at the board edge; a new `tail` alone (whole-arrow drag, typed start)
 * keeps the components and stops the tail where the tip would leave the board. Toggling `anchor` never changes `w`
 * of a free arrow; anchoring clamps it to the board (the tip becomes a point on it).
 */
export function freeLink(): LinkGroup {
	return {
		keys: ['w', 'tail', 'anchor'],
		canonical: 'w',
		resolve(input, current) {
			const anchor = flag(Object.hasOwn(input, 'anchor') ? input.anchor : current.anchor, 'anchor');
			let w = vec(Object.hasOwn(input, 'w') ? input.w : current.w, 'w', 2);
			let tail = vec(Object.hasOwn(input, 'tail') ? input.tail : current.tail, 'tail', 2).map((x) => clamp(x, BOARD2));
			if (anchor) {
				w = w.map((x) => clamp(x, BOARD2));
			} else if (Object.hasOwn(input, 'w')) {
				w = add(tail, w).map((x) => clamp(x, BOARD2)).map((x, i) => x - tail[i]);
			} else {
				w = w.map((x) => clamp(x, RANGE_W));
				tail = tail.map((x, i) => clamp(x, { min: BOARD2.min - Math.min(0, w[i]), max: BOARD2.max - Math.max(0, w[i]) }));
			}
			return { w, tail, anchor };
		},
		shown: () => ({}),
	};
}

export const LINKS: LessonLinks = { groups: [freeLink()] };

/** Where VE-2 draws its arrow: from the stored tail, or from the origin while anchored (a position vector). */
export function arrow2(v: Vals): { from: number[]; to: number[]; w: readonly number[]; anchor: 0 | 1 } {
	const w = vec(v.w, 'w', 2);
	const anchor = flag(v.anchor, 'anchor');
	const from = anchor ? [0, 0] : [...vec(v.tail, 'tail', 2)];
	return { from, to: add(from, w), w, anchor };
}

/** VE-2 tip drag: the tip follows the pointer on the 0.1 grid; the link stops it at the board edge. */
export const moveTip2 = (v: Vals, raw: readonly number[]): Move => {
	const a = arrow2(v);
	return { set: { w: sub(a.to.map((x, i) => snap(x + (raw[i] ?? 0))), a.from) } };
};

/**
 * VE-2 whole-arrow drag: a free arrow slides (tail moves, components stay); an anchored one cannot leave the origin,
 * so dragging it moves its tip, the point, and the numbers change: the lesson's point-versus-direction contrast.
 */
export const moveWhole2 = (v: Vals, raw: readonly number[]): Move => {
	const a = arrow2(v);
	if (a.anchor) return moveTip2(v, raw);
	const d = clampShift(a.from, a.to, a.from.map((x, i) => snap(x + (raw[i] ?? 0)) - x), BOARD2);
	return { set: { tail: add(a.from, d) } };
};

/**
 * Whole-arrow drag of B − A (VE-5, VE-6): both points move by the same snapped shift, stopped at the board edge, so
 * B − A never changes while the arrow slides.
 */
export function moveBoth(aKey: string, bKey: string, board: Box) {
	return (v: Vals, raw: readonly number[]): Move => {
		const a = vec(v[aKey], aKey), b = vec(v[bKey], bKey);
		const d = clampShift(a, b, a.map((x, i) => snap(x + (raw[i] ?? 0)) - x), board);
		return { set: { [aKey]: add(a, d), [bKey]: add(b, d) } };
	};
}

/** VE-3 object drag: the translation follows the pointer on the 0.1 grid (the card clamps it to RANGE_T). */
export const moveT = (v: Vals, raw: readonly number[]): Move => ({ set: { T: vec(v.T, 'T', 2).map((x, i) => snap(x + (raw[i] ?? 0))) } });

/**
 * VE-3: the object before and after the translation. Points get the translation (w = 1); the normal does not
 * (w = 0). With `asPoint` the normal is (wrongly) moved like a point: N + T, which turns and stretches it.
 */
export function translate(v: Vals) {
	const T = vec(v.T, 'T', 2);
	const asPoint = flag(v.asPoint, 'asPoint');
	const moved = STRIP.map((p) => add(p, T));
	const N2 = asPoint ? add(NORMAL, T) : [...NORMAL];
	return { T, asPoint, before: STRIP, after: moved, N: NORMAL, N2, lenN2: norm(N2) };
}

/** VE-5 / VE-6: the trip from A to B (or back, with `rev`), its components and its length (the distance). */
export function trip(v: Vals, three = false) {
	const A = vec(v[three ? 'A3' : 'A'], 'A', three ? 3 : 2);
	const B = vec(v[three ? 'B3' : 'B'], 'B', three ? 3 : 2);
	const rev = three ? 0 : flag(v.rev, 'rev');
	const [start, end] = rev ? [B, A] : [A, B];
	const d = sub(end, start);
	return { A, B, rev, start, end, d, len: norm(d) };
}

/** Comparator of the challenges: values sit on 0.1 grids, so equality is up to float noise. */
export const near = (a: number, b: number): boolean => Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) < 1e-9;
const nearV = (a: V, b: V) => a.length === b.length && a.every((x, i) => near(x, b[i]));

/** Challenge predicates (store reads only, so typing in the N-panel alone solves each). */
export const CHECKS = {
	/** VE-1: the arrow (−2, 3) of lesson 0.G's point. */
	v1: (v: Vals) => nearV(vec(v.v, 'v', 2), [-2, 3]),
	/** VE-1: straight down: no x, negative y. */
	down: (v: Vals) => { const [x, y] = vec(v.v, 'v', 2); return near(x, 0) && y < 0; },
	/** VE-2: the prose's slide, start (5, 5), with the components untouched. */
	slide: (v: Vals) => { const a = arrow2(v); return !a.anchor && nearV(a.from, [5, 5]) && nearV(a.w, [3, 2]); },
	/** VE-2: as a position, the tip is the point (5, 5): now the numbers are the address. */
	position: (v: Vals) => { const a = arrow2(v); return a.anchor === 1 && nearV(a.w, [5, 5]); },
	/** VE-3: the prose-style move, three right and three up. */
	move33: (v: Vals) => nearV(vec(v.T, 'T', 2), [3, 3]),
	/** VE-3: the bug made visible: normals moved like points. */
	bug: (v: Vals) => flag(v.asPoint, 'asPoint') === 1 && norm(vec(v.T, 'T', 2)) > 0,
	/** VE-4: straight up the y axis (y is up in the 3D view), length 2. */
	up: (v: Vals) => nearV(vec(v.v3, 'v3', 3), [0, 2, 0]),
	/** VE-4: lying on the floor: no y component, not the zero vector. */
	flat: (v: Vals) => { const p = vec(v.v3, 'v3', 3); return near(p[1], 0) && norm(p) > 0; },
	/** VE-5: the trip back, (−3, −4), with B − A itself (not just the toggle). */
	back: (v: Vals) => nearV(trip(v).d, [-3, -4]) && trip(v).rev === 0,
	/** VE-5: A at the origin while B − A is still (3, 4): then B is the arrow's tip, a position vector. */
	origin: (v: Vals) => { const z = trip(v); return nearV(z.A, [0, 0]) && nearV(sub(z.B, z.A), [3, 4]); },
	/** VE-6: the exercise's 3D trip (3, 4, 0): flat in a horizontal plane. */
	trip3: (v: Vals) => nearV(trip(v, true).d, [3, 4, 0]),
};

const fixed = (x: number, n: number) => (+x.toFixed(n) + 0).toFixed(n);
/** Components and coordinates: one decimal (drags and keys land on a 0.1 grid); a negative zero prints as 0.0. */
export const fmt1 = (x: number): string => fixed(x, 1);
/** Normals and lengths: two decimals. */
export const fmt2 = (x: number): string => fixed(x, 2);
/** A vector as "(3.0, 2.0)" with a typographic minus, for labels and readouts. */
export const fmtV = (p: V, f: (x: number) => string = fmt1): string => '(' + p.map((x) => f(x).replace('-', '−')).join(', ') + ')';
