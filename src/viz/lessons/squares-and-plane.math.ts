/**
 * Lesson 0.G (squares, roots and the plane): the store contract, the linked x <-> A write and every derived number
 * (storyboard §7.2). Pure and three-free: the entry, the scenes, the N-panel rows and the tests share it.
 *
 * One store, `squares-and-plane`: x (the side, may be negative), A (the area, never negative), P (a point).
 * x and A are one linked group: a write of x resolves A = x·x, a write of A resolves x = √A >= 0, so a negative x set in
 * 0.G-1 becomes the principal (positive) root the moment 0.G-3 writes A. Both are published in one store update; the
 * drawn values derive A from the drawn x, so an eased change keeps side and area geometrically coherent.
 */
import type { LessonLinks, LinkGroup } from '../links';
import { finite } from '../links';
import type { LessonState, Val } from '../store';
import { tileCount, type TileShape } from '../scene/tiles';

export const LESSON = 'squares-and-plane';

/** Ranges of the reader-writable keys (storyboard §7.2). */
export const SPEC = {
	x: { min: -4, max: 4, step: 0.1 },
	A: { min: 0, max: 16, step: 0.25 },
	P: { min: -4.5, max: 4.5, step: 0.1 },
} as const;

/** Ctrl-snap targets: half units for the side, the perfect squares for the area. */
export const SNAP_X = 0.5;
export const SNAP_A = [0, 1, 4, 9, 16] as const;

/** `moved` is 0 until the reader first moves P (see `pointLink`); the defaults are the prose's example point. */
export const INITIAL: Record<string, Val> = { x: 3, A: 9, P: [-2, 3], moved: 0 };

type Vals = LessonState['vals'];
const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`squares-and-plane: "${key}" must be a number`);
	finite(v);
	return v;
};
const clamp = (v: number, r: { min: number; max: number }) => Math.min(r.max, Math.max(r.min, v));

/**
 * x <-> A. Input x: clamped to [-4, 4], A = x·x. Input A: clamped to [0, 16], x = √A (the principal root; A keeps the
 * exact typed value, not √A·√A). Both at once (the initial snapshot, a checkpoint) must agree, else the patch is
 * rejected: a conflicting pair would publish a square whose side and area disagree.
 */
export function squareLink(): LinkGroup {
	return {
		keys: ['x', 'A'],
		canonical: 'x',
		resolve(input) {
			const hasX = Object.hasOwn(input, 'x');
			const hasA = Object.hasOwn(input, 'A');
			if (hasX) {
				const x = clamp(num(input.x, 'x'), SPEC.x);
				if (hasA) {
					const A = num(input.A, 'A');
					if (Math.abs(A - x * x) > 1e-9 * Math.max(1, A)) throw new Error('Conflicting linked inputs x and A');
					return { x, A };
				}
				return { x, A: x * x };
			}
			const A = clamp(num(input.A, 'A'), SPEC.A);
			return { x: Math.sqrt(A), A };
		},
		shown: (v) => {
			const x = v.x as number;
			return { A: x * x };
		},
	};
}

/**
 * P with its `moved` flag: any write of P that changes it marks the point as moved, in the same update. Challenge 1
 * ("put P at (-2, 3)") then needs the reader to bring P there, not the default that already sits there; the route
 * (typing, dragging, nudging) is free.
 */
export function pointLink(): LinkGroup {
	return {
		keys: ['P', 'moved'],
		canonical: 'P',
		resolve(input, current): Record<string, Val> {
			if (!Object.hasOwn(input, 'P')) return { moved: num(input.moved, 'moved') };
			const P = input.P;
			if (typeof P === 'number' || P.length !== 2) throw new Error('P must be a 2-vector');
			finite(P);
			const p = [clamp(P[0], SPEC.P), clamp(P[1], SPEC.P)];
			const old = current.P;
			const changed = Array.isArray(old) && (old[0] !== p[0] || old[1] !== p[1]);
			if (Object.hasOwn(input, 'moved')) return { P: p, moved: num(input.moved, 'moved') };
			return { P: p, moved: changed || current.moved === 1 ? 1 : 0 };
		},
		shown: () => ({}),
	};
}

export const LINKS: LessonLinks = { groups: [squareLink(), pointLink()] };

/** Box of the 0.G squares: side s on x and y, thin (display-only) z. */
export const squareBox = (s: number): TileShape => ({ kind: 'box', size: [s, s, 0], thin: [false, false, true] });

/** Every number the formulas, rows, readouts and challenges show (storyboard §7.2 "Derived"). */
export function derive(v: Vals) {
	const x = num(v.x, 'x');
	const A = num(v.A, 'A');
	const P = v.P;
	if (typeof P === 'number' || P.length !== 2) throw new Error('P must be a 2-vector');
	finite(P);
	// One count model with the drawn tiles: TileGrid's tileCount of the same boxes (the 0.G-1 square is mirrored).
	const c1 = tileCount(squareBox(x), { negative: 'mirror' });
	const sqrtA = Math.sqrt(A);
	const c3 = tileCount(squareBox(sqrtA));
	const [px, py] = P;
	return {
		x,
		x2: x * x,
		whole: c1.whole,
		partial: c1.partial,
		/** Clipped (orange) pieces of 0.G-1: pieces - whole. */
		partialPieces: c1.pieces - c1.whole,
		A,
		sqrtA,
		check: sqrtA * sqrtA,
		whole3: c3.whole,
		partial3: c3.partial,
		partialPieces3: c3.pieces - c3.whole,
		px,
		py,
		px2: px * px,
		py2: py * py,
		sum: px * px + py * py,
	};
}
export type Derived = ReturnType<typeof derive>;

/** Comparator of the challenges: finite and within rounding of a decimal step (drags and keys land on 0.1 grids). */
export const near = (a: number, b: number): boolean => Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) < 1e-9;

/** Challenge predicates per scene (§7.3-§7.5); store reads only, so typing in the N panel alone solves each. */
export const CHECKS = {
	sixteen: (v: Vals) => near(Math.abs(derive(v).x), 4),
	negative: (v: Vals) => derive(v).x < 0,
	pointTyped: (v: Vals) => v.moved === 1 && near(derive(v).px, -2) && near(derive(v).py, 3),
	sumOther: (v: Vals) => {
		const d = derive(v);
		return near(d.sum, 13) && !(near(d.px, -2) && near(d.py, 3));
	},
	wholeSide: (v: Vals) => {
		const d = derive(v);
		return d.A > 0 && !near(d.A, 9) && near(d.sqrtA, Math.round(d.sqrtA));
	},
	sideHalf: (v: Vals) => near(derive(v).A, 2.25),
};

/** One decimal for coordinates and sides ("x = -3.0"); a negative zero prints as 0. */
export const fmt1 = (v: number): string => (Math.abs(v) < 5e-10 ? 0 : v).toFixed(1);
/** Two decimals for areas ("9.00", "2.25"). */
export const fmt2 = (v: number): string => (Math.abs(v) < 5e-10 ? 0 : v).toFixed(2);
/** Four decimals for roots ("1.4142"). */
export const fmt4 = (v: number): string => (Math.abs(v) < 5e-10 ? 0 : v).toFixed(4);
/** Whole-tile counts are integers. */
export const fmtInt = (v: number): string => String(Math.round(v));
