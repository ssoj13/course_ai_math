/**
 * Lesson 0.4 (angles, radians and the unit circle): the store contract, the drag mappings and every derived number.
 * Pure and three-free: the entry, the scenes, the N-panel rows and the tests share it.
 *
 * One store, `unit-circle`, shared by all four cards (the views are linked: one angle everywhere):
 * - `th`: the angle in whole degrees, 0..360, counter-clockwise from the positive x axis.
 * - `roll`: 0..1, how far the UC-2 arc is unrolled onto the radian ruler (0 = on the circle, 1 = flat on the ruler).
 */
import { finite } from '../links';
import type { LessonState, Val } from '../store';
import { magnet } from '../scene/drag';

export const LESSON = 'unit-circle';

/** Ranges of the reader-writable keys; the N-panel rows, the formula scrubs and every drag clamp use them. */
export const SPEC = {
	th: { min: 0, max: 360, step: 1 },
	roll: { min: 0, max: 1, step: 0.05 },
} as const;

/** The special angles of the lesson table, in degrees (snap targets of the N-panel and the drag magnet). */
export const SPECIAL = [0, 30, 45, 60, 90, 120, 180] as const;

/**
 * Magnet targets of a dragged point: every multiple of 30 and 45 degrees around the whole circle, so the point sticks
 * on the special angles of every quadrant (120, 135, 210, 300, ...), not only of the first half turn.
 */
export const MAGNET = [...new Set([...range(0, 360, 30), ...range(0, 360, 45)])].sort((a, b) => a - b);

/** Half-width of the magnet zone in degrees: wide enough to catch a hand, narrow enough to leave 1-degree steps free. */
export const MAGNET_ZONE = 3;

/** Defaults: the worked example of the prose (60 degrees), the arc on the circle. */
export const INITIAL: Record<string, Val> = { th: 60, roll: 0 };

/**
 * UC-3 graph geometry: the graph's own vertical axis (theta = 0) sits at x = GRAPH_X0, and one radian of theta is one
 * world unit along x, so the horizontal axis of the graph is the same radian ruler as UC-2 (2 pi = 6.28 units wide).
 */
export const GRAPH_X0 = 1.6;

function range(from: number, to: number, step: number): number[] {
	const out: number[] = [];
	for (let x = from; x <= to; x += step) out.push(x);
	return out;
}

type Vals = LessonState['vals'];

/** A polyline vertex (z = 0: every 0.4 scene is 2D), the tuple drei's Line takes. */
export type P3 = [number, number, number];

const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`unit-circle: "${key}" must be a number`);
	finite(v);
	return v;
};

/** Degrees to radians. */
export const rad = (deg: number): number => (deg * Math.PI) / 180;

/**
 * cos and sin of an angle in degrees, exact on the quadrant angles: Math.cos(Math.PI / 2) is 6.1e-17, not 0, and a
 * reader who sets 90 degrees must read 0 (and see no leg drawn), not "0.00" from a tiny positive number.
 */
export function cosSin(deg: number): [number, number] {
	const d = ((deg % 360) + 360) % 360;
	if (d === 0) return [1, 0];
	if (d === 90) return [0, 1];
	if (d === 180) return [-1, 0];
	if (d === 270) return [0, -1];
	const r = rad(d);
	return [Math.cos(r), Math.sin(r)];
}

/** Sign of a coordinate as a word-free symbol: "+", "−" or "0". */
export const signOf = (v: number): '+' | '−' | '0' => (v > 0 ? '+' : v < 0 ? '−' : '0');

/**
 * Quadrant of the point: 1..4 counter-clockwise from the upper right, or 0 on an axis (0, 90, 180, 270, 360 degrees),
 * where one coordinate is zero and the point belongs to no quadrant.
 */
export function quadrant(deg: number): 0 | 1 | 2 | 3 | 4 {
	const d = ((deg % 360) + 360) % 360;
	if (d % 90 === 0) return 0;
	return (Math.floor(d / 90) + 1) as 1 | 2 | 3 | 4;
}

/** Every number the formulas, rows, readouts, labels and challenges show. */
export function derive(v: Vals) {
	const th = num(v.th, 'th');
	const roll = num(v.roll, 'roll');
	const [cos, sin] = cosSin(th);
	// The 45-degree mirror of the point: (x, y) -> (y, x), which is the point at 90 - theta (kept in 0..360).
	const mir = (((90 - th) % 360) + 360) % 360;
	return {
		th,
		rad: rad(th),
		cos,
		sin,
		cos2: cos * cos,
		sin2: sin * sin,
		one: cos * cos + sin * sin,
		quad: quadrant(th),
		/** Fraction of a full turn: theta / 360 (the arc as a share of the circumference 2 pi). */
		turn: th / 360,
		roll,
		mir,
	};
}
export type Derived = ReturnType<typeof derive>;

/**
 * The angle in whole degrees of a point dragged around the circle. atan2 gives -180..180; the result is unwrapped next
 * to the previous angle, so crossing the positive x axis continues smoothly (359 -> 360 counter-clockwise, 0 -> 359
 * clockwise wraps) instead of jumping a full turn. Outside 0..360 the plain 0..360 angle is used: the range has no
 * second turn. A point at the origin has no direction: the previous angle stays.
 */
export function angleOf(x: number, y: number, prev: number): number {
	if (Math.hypot(x, y) < 1e-9) return prev;
	const a = (Math.atan2(y, x) * 180) / Math.PI;
	const base = ((a % 360) + 360) % 360;
	const near = base + 360 * Math.round((prev - base) / 360);
	const deg = near >= SPEC.th.min && near <= SPEC.th.max ? near : base;
	return Math.round(deg);
}

/**
 * Snap an angle to the magnet targets (MAGNET, plus 360). `from` is the angle before a keyboard step, so a key step
 * leaves a target instead of being pulled back onto it (see `magnet` in drag.ts).
 */
export function snapAngle(deg: number, from?: number): number {
	return magnet(deg, [...MAGNET, 360], MAGNET_ZONE, from);
}

/**
 * The point dragged around the circle (UC-1, UC-2 on the circle, UC-4): the angle of the pointer seen from the
 * origin, magnet-caught on the special angles. A drag measures raw from the press, so the pointer sits at
 * press point + raw; a key step moves the point by raw from where it is now.
 */
export function moveOnCircle(vals: Vals, raw: readonly number[], src: 'drag' | 'key'): { set: Record<string, Val>; ease?: string[] } {
	const th0 = num(vals.th, 'th');
	const [c, s] = cosSin(th0);
	const deg = angleOf(c + (raw[0] ?? 0), s + (raw[1] ?? 0), th0);
	const caught = snapAngle(deg, src === 'key' ? th0 : undefined);
	return { set: { th: caught }, ease: caught !== deg ? ['th'] : [] };
}

/**
 * The point dragged along a horizontal radian axis (UC-2 on the ruler, UC-3 on the graph): one world unit = one
 * radian, so a horizontal travel of raw[0] adds raw[0] radians. Whole degrees, magnet as on the circle.
 */
export function moveAlong(vals: Vals, raw: readonly number[], src: 'drag' | 'key'): { set: Record<string, Val>; ease?: string[] } {
	const th0 = num(vals.th, 'th');
	const deg = Math.round(th0 + ((raw[0] ?? 0) * 180) / Math.PI);
	const caught = snapAngle(deg, src === 'key' ? th0 : undefined);
	return { set: { th: caught }, ease: caught !== deg ? ['th'] : [] };
}

/** Number of polyline segments per full turn of an arc or a curve (smooth at any card size). */
const PER_TURN = 192;

/** Segments for an arc or curve spanning `deg` degrees: at least 2, proportional to the span. */
const segs = (deg: number): number => Math.max(2, Math.ceil((Math.abs(deg) / 360) * PER_TURN));

/**
 * An arc of radius r around `c`, from angle a0 to a1 in degrees (either direction, any span up to a full turn; the
 * engine's AngleArc takes the shorter arc only, but theta here runs to 360). Null for a zero span.
 */
export function arc(c: readonly number[], r: number, a0: number, a1: number): P3[] | null {
	if (a0 === a1 || !(r > 0)) return null;
	const n = segs(a1 - a0);
	const out: P3[] = [];
	for (let i = 0; i <= n; i++) {
		const [x, y] = cosSin(a0 + ((a1 - a0) * i) / n);
		out.push([c[0] + r * x, c[1] + r * y, 0]);
	}
	return out;
}

/**
 * The UC-2 arc being unrolled. Each point keeps its arc length s (0..theta in radians): on the circle it sits at
 * (cos s, sin s); on the ruler, which starts at the arc's start (1, 0) and runs along the x axis to the right, it sits
 * at (1 + s, 0). `roll` (0..1) blends the two. The ends are exact (s on the circle, s on the ruler); in between the
 * shape is a straight blend, a picture of the motion only. Null for a zero angle (no arc).
 */
export function unrolled(deg: number, roll: number): P3[] | null {
	if (deg <= 0) return null;
	const n = segs(deg);
	const total = rad(deg);
	const out: P3[] = [];
	for (let i = 0; i <= n; i++) {
		const s = (total * i) / n;
		out.push(unrollAt(s, roll));
	}
	return out;
}

/** One point of the unrolling arc at arc length s (radians) and blend `roll` (see `unrolled`). */
export function unrollAt(s: number, roll: number): P3 {
	const x0 = Math.cos(s), y0 = Math.sin(s);
	return [x0 + (1 + s - x0) * roll, y0 * (1 - roll), 0];
}

/**
 * Points of y = f(theta) on the UC-3 graph for theta in [d0, d1] degrees: x = GRAPH_X0 + theta in radians. Null for an
 * empty span (the trace before the angle has moved).
 */
export function graph(f: 'cos' | 'sin', d0: number, d1: number): P3[] | null {
	if (d1 <= d0) return null;
	const n = segs(d1 - d0);
	const out: P3[] = [];
	for (let i = 0; i <= n; i++) {
		const d = d0 + ((d1 - d0) * i) / n;
		const [c, s] = cosSin(d);
		out.push([GRAPH_X0 + rad(d), f === 'cos' ? c : s, 0]);
	}
	return out;
}

/** Comparator of the challenges: within rounding (cos and sin of whole degrees are not exact decimals). */
export const near = (a: number, b: number, tol = 1e-9): boolean => Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) < tol;

/** Challenge predicates; store reads only, so typing in the N panel alone solves each. */
export const CHECKS = {
	/** UC-1: a shadow of -0.5 on the x axis (120 or 240 degrees). */
	cosHalfNeg: (v: Vals) => near(derive(v).cos, -0.5, 1e-6),
	/** UC-1: straight down, (0, -1). */
	down: (v: Vals) => derive(v).th === 270,
	/** UC-1: the other angle with the cos of 60 degrees: 300 (same x shadow, mirrored below the x axis). */
	cosTwin: (v: Vals) => derive(v).th === 300,
	/** UC-2: a full turn laid flat on the ruler: 2 pi. */
	fullTurnFlat: (v: Vals) => derive(v).th === 360 && derive(v).roll === 1,
	/** UC-2: the arc as long as the radius, to the nearest whole degree (1 radian = 57.3 degrees). */
	oneRadian: (v: Vals) => derive(v).th === 57,
	/** UC-3: the first crossing of the two curves (cos = sin). */
	crossFirst: (v: Vals) => derive(v).th === 45,
	/** UC-3: the second crossing, both negative. */
	crossSecond: (v: Vals) => derive(v).th === 225,
	/** UC-4: the point on the mirror line, so it is its own mirror image. */
	onMirror: (v: Vals) => derive(v).th === 45 || derive(v).th === 225,
};

const clean = (v: number): number => (Math.abs(v) < 5e-10 ? 0 : v);

/** Whole degrees ("60°"), the angle in the N-panel and labels. */
export const fmtDeg = (v: number): string => String(Math.round(v)) + '°';
/** The same angle in TeX for the formulas ("60^\circ"). */
export const texDeg = (v: number): string => String(Math.round(v)) + '^\\circ';
/** cos and sin: two decimals in the scene labels ("0.50", "−0.87"), the minus as a real minus sign. */
export const fmt2 = (v: number): string => clean(v).toFixed(2).replace('-', '−');
/** Three decimals for radians and arc lengths ("1.047"). */
export const fmt3 = (v: number): string => clean(v).toFixed(3).replace('-', '−');
/** TeX variants keep the ASCII minus (KaTeX typesets it as a minus). */
export const tex2 = (v: number): string => clean(v).toFixed(2);
export const tex3 = (v: number): string => clean(v).toFixed(3);
