/**
 * Lesson 0.2 (powers): the store contract, the shapes the scenes draw and every derived number (storyboard §7.6).
 * Pure and three-free: the entry, the scenes, the N-panel rows and the tests share it.
 *
 * One store, `powers`. x (side, [0, 4]), A = x² and V = x³ are one linked group (`powersLinks` in links.ts): a write
 * of any of the three resolves the other two in the same store update and records the input channel in `from`
 * (0 = x, 1 = A, 2 = V). `n` (the power of 0.2-2) and `k` (the ladder rung of 0.2-3) are integer keys mirrored by the
 * `power` and `ladder` timelines. `base` (the ladder's base), `d3` (0.2-4's 2D/3D view), `g` and `t` (0.2-5's curve)
 * are plain keys.
 */
import { powersLinks } from '../links';
import type { LessonLinks, LinkGroup } from '../links';
import { finite } from '../links';
import type { LessonState, Val } from '../store';
import { tileCount, type TileShape } from '../scene/tiles';

export const LESSON = 'powers';

/** Ranges of the reader-writable keys (storyboard §7.6). */
export const SPEC = {
	x: { min: 0, max: 4, step: 0.1 },
	A: { min: 0, max: 16, step: 0.25 },
	V: { min: 0, max: 64, step: 1 },
	n: { min: 1, max: 3, step: 1 },
	k: { min: -3, max: 3, step: 1 },
	base: { min: 1.5, max: 3, step: 0.5 },
	g: { min: 0.2, max: 32, step: 0.1 },
	t: { min: 0, max: 1, step: 0.01 },
} as const;

/** Ctrl-snap targets: half units for the side, perfect squares for the area, perfect cubes for the volume. */
export const SNAP_X = 0.5;
export const SNAP_A = [0, 1, 4, 9, 16] as const;
export const SNAP_V = [0, 1, 8, 27, 64] as const;

/** 0.2-5 presets: display decode 2.2, encode 1/2.2, two specular exponents. Set exactly, not rounded to the step. */
export const PRESETS = [
	{ id: 'decode', g: 2.2, label: '2.2' },
	{ id: 'encode', g: 1 / 2.2, label: '1/2.2' },
	{ id: 'spec8', g: 8, label: '8' },
	{ id: 'spec32', g: 32, label: '32' },
] as const;

/**
 * Coherent defaults: x = 3, A = 9, V = 27 (the links reject an incoherent triple), from = 0 (x); the ladder at the top
 * rung (k = kf = 3).
 */
export const INITIAL: Record<string, Val> = { x: 3, A: 9, V: 27, from: 0, n: 2, k: 3, kf: 3, base: 2, d3: 0, g: 2.2, t: 0.5 };

/** x/A/V and the n/k timeline adapters (links.ts), plus the eased ladder position (`ladderLink`). */
export function powersLessonLinks(): LessonLinks {
	const base = powersLinks();
	return { groups: [...(base.groups ?? []), ladderLink()], timelines: base.timelines };
}
export const LINKS: LessonLinks = powersLessonLinks();

type Vals = LessonState['vals'];
const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`powers: "${key}" must be a number`);
	finite(v);
	return v;
};

/** The square of side s in the xy plane (0.2-1, 0.2-4 2D): thin (display-only) z. */
export const squareBox = (s: number): TileShape => ({ kind: 'box', size: [s, s, 0], thin: [false, false, true] });

/** The cube of edge s (0.2-4 3D). */
export const cubeBox = (s: number): TileShape => ({ kind: 'box', size: [s, s, s], thin: [false, false, false] });

/**
 * 0.2-2: x^n as a body on the 3D floor (y up). n = 1: a stick along x (y and z thin), n = 2: a slab swept along z
 * (y thin), n = 3: the slab lifted along y. The thin axes are display-only and never counted, so the count of unit
 * pieces is x^n for each n.
 */
export function powerBox(n: number, x: number): TileShape {
	if (n === 1) return { kind: 'box', size: [x, 0, 0], thin: [false, true, true] };
	if (n === 2) return { kind: 'box', size: [x, 0, x], thin: [false, true, false] };
	if (n === 3) return cubeBox(x);
	throw new Error(`powers: n must be 1, 2 or 3, got ${n}`);
}

/** The seven exponents of the ladder, top to bottom. */
export const RUNGS = [3, 2, 1, 0, -1, -2, -3] as const;
/** Term id of rung j: p3, p2, p1, p0, pm1, pm2, pm3 (one id for the rung, its label and its N-panel row). */
export const rungId = (j: number): string => (j < 0 ? 'pm' + -j : 'p' + j);
/** Pieces per band of a ladder rung. */
export const WRAP = 8;
/**
 * Bands reserved per rung: the tallest rung of the largest base (3³ = 27 pieces = 4 bands of 8, 3² = 9 = 2 bands).
 * Fixed slots keep every rung in place while the base changes.
 */
const SLOT_BANDS: Readonly<Record<number, number>> = { 3: 4, 2: 2, 1: 1, 0: 1, [-1]: 1, [-2]: 1, [-3]: 1 };
/** Vertical gap between two rung slots, in world units. */
export const RUNG_GAP = 0.5;
/** World y of the top edge of the ladder (rung 3). */
export const LADDER_TOP = 7;

/** Top edge (world y) of rung j's slot: slots stack downward from LADDER_TOP, RUNG_GAP apart. */
export function rungTop(j: number): number {
	let y = LADDER_TOP;
	for (const r of RUNGS) {
		if (r === j) return y;
		y -= SLOT_BANDS[r] + RUNG_GAP;
	}
	throw new Error(`powers: no ladder rung ${j}`);
}

/** Bands rung j actually fills at this base (a fraction rung is one band). */
export const rungBands = (base: number, j: number): number => (j > 0 ? Math.max(1, Math.ceil(Math.ceil(base ** j) / WRAP)) : 1);

/** World y of the centre of band b (0 = top) of rung j. */
export const bandMid = (j: number, b: number): number => rungTop(j) - b - 0.5;

/**
 * Rung j at base b with the ladder standing at p (the drawn, possibly eased, ladder position `kf`): a row of b^j unit
 * pieces, or for an amount of at most one tile a unit outline filled to it (b^0 = 1 is the full outline). A rung below
 * the position (p <= j - 1, not reached yet) is empty: its slot and its exponent stay, its value is the answer the next
 * step reveals.
 *
 * The step down to rung j (p gliding from j + 1 to j) is the division made visible: the rung starts as a copy of the
 * rung above (capped to its own slot: 3³ = 27 pieces would not fit the two bands of rung 2) and shrinks to b^j, so
 * the pieces beyond b^j disappear from the end and, below one tile, the filled part of the tile shrinks. Stepping up
 * plays it backwards.
 */
export function rungShape(base: number, j: number, p: number): { shape: TileShape; fill: number | undefined } {
	if (p >= j + 1) return { shape: { kind: 'row', len: 0, wrap: WRAP }, fill: undefined };
	const to = base ** j;
	const from = Math.min(base ** (j + 1), SLOT_BANDS[j] * WRAP);
	const v = p > j ? to + (from - to) * (p - j) : to;
	if (v > 1) return { shape: { kind: 'row', len: v, wrap: WRAP }, fill: undefined };
	return { shape: { kind: 'row', len: 1, wrap: WRAP }, fill: v };
}

/**
 * The drawn ladder position: `kf` is written with every write of `k` (the authoritative rung, an integer the
 * timeline mirrors) and is the canonical key of this group, so a timeline or field step eases it while k itself cuts.
 * The rungs are laid out from the eased `kf` (`rungShape`), which is what animates the division between two rungs.
 */
export function ladderLink(): LinkGroup {
	const clampK = (v: Val | undefined, key: string): number => Math.max(SPEC.k.min, Math.min(SPEC.k.max, Math.trunc(num(v, key))));
	return {
		keys: ['k', 'kf'],
		canonical: 'kf',
		resolve(input) {
			if (!Object.hasOwn(input, 'k')) throw new Error('powers: the ladder position follows k; write k');
			const k = clampK(input.k, 'k');
			if (Object.hasOwn(input, 'kf') && num(input.kf, 'kf') !== k) throw new Error('powers: conflicting k and kf');
			return { k, kf: k };
		},
		shown: () => ({}),
	};
}

/** Every number the formulas, rows, labels, readouts and challenges show (storyboard §7.6 "Derived"). */
export function derive(v: Vals) {
	const x = num(v.x, 'x');
	const A = num(v.A, 'A');
	const V = num(v.V, 'V');
	const n = num(v.n, 'n');
	const k = num(v.k, 'k');
	const base = num(v.base, 'base');
	const g = num(v.g, 'g');
	const t = num(v.t, 't');
	// One count model with the drawn tiles: TileGrid's tileCount of the same shapes.
	const sq = tileCount(squareBox(x));
	const pw = tileCount(powerBox(n, x));
	const sqrtA = Math.sqrt(A);
	const cbrtV = Math.cbrt(V);
	const root2 = tileCount(squareBox(sqrtA));
	const root3 = tileCount(cubeBox(cbrtV));
	const y = t ** g;
	return {
		x, A, V, n, k, base, g, t,
		from: num(v.from, 'from'),
		d3: num(v.d3, 'd3'),
		x2: x * x,
		x3: x * x * x,
		xn: x ** n,
		/** 0.2-1 square: whole tiles, cut area, cut pieces. */
		whole: sq.whole,
		partial: sq.partial,
		partialPieces: sq.pieces - sq.whole,
		/** 0.2-2 body of the current power. */
		wholeN: pw.whole,
		partialN: pw.partial,
		partialPiecesN: pw.pieces - pw.whole,
		layers: Math.floor(x),
		sqrtA,
		check2: sqrtA * sqrtA,
		cbrtV,
		check3: cbrtV * cbrtV * cbrtV,
		whole2: root2.whole,
		partial2: root2.partial,
		whole3: root3.whole,
		partial3: root3.partial,
		p3: base ** 3, p2: base ** 2, p1: base, p0: 1, pm1: base ** -1, pm2: base ** -2, pm3: base ** -3,
		cur: base ** k,
		prevval: base ** (k + 1),
		next: base ** (k - 1),
		y,
		yinv: y ** (1 / g),
	};
}
export type Derived = ReturnType<typeof derive>;

/** Comparator of the challenges: finite and within rounding of a decimal step (drags and keys land on 0.1 grids). */
export const near = (a: number, b: number): boolean => Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) < 1e-9;

/** Challenge predicates per scene (§7.7-§7.11); store reads only, so typing in the N panel alone solves each. */
export const CHECKS = {
	fourTiles: (v: Vals) => near(derive(v).x, 2),
	typedRoot: (v: Vals) => {
		const d = derive(v);
		return d.from === 1 && d.x > 0 && Math.abs(d.x - Math.round(d.x)) > 1e-6;
	},
	notTyped: (v: Vals) => {
		const d = derive(v);
		return near(d.A, 6.25) && d.from === 0;
	},
	cube27: (v: Vals) => {
		const d = derive(v);
		return d.n === 3 && near(d.x, 3);
	},
	cubeSmaller: (v: Vals) => {
		const d = derive(v);
		return d.n === 3 && d.x > 0 && d.x < 1;
	},
	twoLayers: (v: Vals) => {
		const d = derive(v);
		return d.n === 3 && near(d.x, 2);
	},
	oneTile: (v: Vals) => derive(v).k === 0,
	quarter: (v: Vals) => {
		const d = derive(v);
		return near(d.base, 2) && d.k === -2;
	},
	otherBase: (v: Vals) => {
		const d = derive(v);
		return d.k === 0 && !near(d.base, 2);
	},
	wholeEdge: (v: Vals) => {
		const d = derive(v);
		return d.d3 === 1 && d.V > 0 && !near(d.V, 27) && Math.abs(d.cbrtV - Math.round(d.cbrtV)) < 1e-9;
	},
	edgeTwo: (v: Vals) => {
		const d = derive(v);
		return d.d3 === 1 && near(d.A, 4);
	},
	diagonal: (v: Vals) => near(derive(v).g, 1),
	quarterOut: (v: Vals) => {
		const d = derive(v);
		return near(d.g, 2) && near(d.t, 0.5);
	},
	highlight: (v: Vals) => {
		const d = derive(v);
		return near(d.g, 32) && near(d.t, 0.5);
	},
};

const clean = (v: number): number => (Math.abs(v) < 5e-10 ? 0 : v);
/** Two decimals ("0.50" for the curve input t). */
export const fmt2 = (v: number): string => clean(v).toFixed(2);
/**
 * The side x: at least one decimal, up to four ("3.0", "2.5", "1.4142"). A side written from a typed area or volume
 * is a root (√2, ∛2); one decimal would print 1.3 for ∛2 = 1.26, whose square is not the 1.59 shown next to it.
 */
export const fmtX = (v: number): string => {
	const s = String(Number(clean(v).toFixed(4)));
	return s.includes('.') ? s : s + '.0';
};
/** Four decimals for roots and curve values ("1.4142", "0.2176"). */
export const fmt4 = (v: number): string => clean(v).toFixed(4);
/** Counts are integers. */
export const fmtInt = (v: number): string => String(Math.round(v));
/** Ladder values and bases: up to three decimals, no trailing zeros ("8", "0.5", "15.625", "0.667"). */
export const fmtVal = (v: number): string => String(Number(clean(v).toFixed(3)));
/**
 * Lengths, areas and volumes of the side x: up to four decimals, no trailing zeros ("9", "6.25", "15.625",
 * "1.5874"), the precision of the side itself, so x¹ of x = 1.2599 reads 1.2599, not 1.26.
 */
export const fmtAmt = (v: number): string => String(Number(clean(v).toFixed(4)));
/** Exponent g: up to three decimals, no trailing zeros ("2.2", "0.455", "32"). */
export const fmtG = fmtVal;
/** Scientific form for a curve value too small for four decimals (2.3e-10 is not zero). */
export const fmtSci = (v: number): string => (v !== 0 && Math.abs(v) < 5e-5 ? v.toExponential(1) : fmt4(v));

const SUP: Readonly<Record<string, string>> = { '-': '⁻', 0: '⁰', 1: '¹', 2: '²', 3: '³' };
/** Unicode superscript of a small integer exponent (-3..3): "³", "⁰", "⁻²". */
export const sup = (j: number): string => [...String(j)].map((c) => SUP[c] ?? c).join('');
