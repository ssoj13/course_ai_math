/**
 * Lesson 3.4 (elementwise ops and broadcasting): the store contract and every number the scenes, the alignment table
 * and the tests show. Pure and three-free. Shapes, strides, results and error decisions all come from the course's
 * tensor core (src/core: `broadcastShapes`, `broadcastTo`, `add`/`sub`/`mul`, `offsets`); this module only adds the
 * per-axis explanation of the rule and which drawn cells are real memory and which are stride-0 repeats.
 *
 * Store `broadcasting` (one link group over the integer keys, so every write leaves a consistent state):
 * - `ar` 1..3 / `br` 0..3: rank of A and of B.
 * - `a0 a1 a2` / `b0 b1 b2`: sizes 1..4 counted FROM THE RIGHT (a0 = last axis), the way broadcasting aligns shapes;
 *   sizes beyond the rank are kept for when the rank grows back.
 * - `op` 0..2: +, -, x.
 * - `tint` [r, g, b] 0..1.5: the colour multiplier of scene 3.4-2 (tint every pixel).
 */
import {
	add, arange, broadcastShapes, broadcastTo, flat, map, mul, offsets, ShapeError, sizeOf, sub, tensor, type Tensor,
} from '../../core';
import type { LessonLinks, LinkGroup } from '../links';
import { finite } from '../links';
import type { LessonState, Val } from '../store';
import { ICON } from './tensor-road.data';

export const LESSON = 'broadcasting';

export const SPEC = {
	ar: { min: 1, max: 3, step: 1 },
	br: { min: 0, max: 3, step: 1 },
	size: { min: 1, max: 4, step: 1 },
	op: { min: 0, max: 2, step: 1 },
	tint: { min: 0, max: 1.5, step: 0.05 },
} as const;

export const OPS = ['+', '−', '×'] as const;
export type Op = 0 | 1 | 2;

const LEVELS = [0, 1, 2] as const;
export const aKey = (k: number) => 'a' + k;
export const bKey = (k: number) => 'b' + k;
const INT_KEYS = ['ar', 'br', 'op', ...LEVELS.map(aKey), ...LEVELS.map(bKey)] as const;

/** Default: A (3, 4) plus a row B (4,): "add a bias to every row". */
export const INITIAL: Record<string, Val> = {
	ar: 2, a0: 4, a1: 3, a2: 2,
	br: 1, b0: 4, b1: 1, b2: 1,
	op: 0,
	tint: [1, 0.8, 0.5],
};

type Vals = LessonState['vals'];

const int = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`broadcasting: "${key}" must be a number`);
	finite(v);
	return Math.round(v);
};
const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

/** Rounds and clamps ranks, sizes and the operator in one write. */
function intLink(): LinkGroup {
	return {
		keys: INT_KEYS,
		canonical: 'op',
		resolve(input, current) {
			const get = (k: string) => int(Object.hasOwn(input, k) ? input[k] : current[k], k);
			const out: Record<string, Val> = {
				ar: clamp(get('ar'), SPEC.ar.min, SPEC.ar.max),
				br: clamp(get('br'), SPEC.br.min, SPEC.br.max),
				op: clamp(get('op'), SPEC.op.min, SPEC.op.max),
			};
			for (const k of LEVELS) {
				out[aKey(k)] = clamp(get(aKey(k)), SPEC.size.min, SPEC.size.max);
				out[bKey(k)] = clamp(get(bKey(k)), SPEC.size.min, SPEC.size.max);
			}
			return out;
		},
		shown: () => ({}),
	};
}

export const LINKS: LessonLinks = { groups: [intLink()] };

/** Shape (outermost first) of rank `r` from sizes counted from the right. */
export const shapeOf = (r: number, s: readonly number[]): number[] => Array.from({ length: r }, (_, i) => s[r - 1 - i]);

/** Store patch that sets both shapes (presets, challenges' "show me"). */
export function shapesPatch(a: readonly number[], b: readonly number[]): Record<string, number> {
	const out: Record<string, number> = { ar: a.length, br: b.length };
	[...a].reverse().forEach((n, k) => (out[aKey(k)] = n));
	[...b].reverse().forEach((n, k) => (out[bKey(k)] = n));
	return out;
}

/** The presets under scene 3.4-1, in button order. */
export const PRESETS = {
	same: { a: [3, 4], b: [3, 4] },
	row: { a: [3, 4], b: [4] },
	col: { a: [3, 4], b: [3, 1] },
	outer: { a: [3, 1], b: [1, 4] },
	scalar: { a: [3, 4], b: [] },
	block: { a: [2, 3, 4], b: [3, 1] },
	bad: { a: [3, 4], b: [3] },
} as const satisfies Record<string, { a: readonly number[]; b: readonly number[] }>;
export type PresetId = keyof typeof PRESETS;

// ---------------------------------------------------------------------------------------------------------------
// The rule, axis by axis.

/**
 * One column of the right-aligned shape table. `axis` counts from the right (-1 = last axis). `kind`:
 * same (equal sizes), stretchA / stretchB (that operand has size 1 there and is repeated), newA / newB (that operand
 * has no such axis: it counts as size 1 in front and is repeated), bad (two different sizes, neither 1).
 */
export type AlignKind = 'same' | 'stretchA' | 'stretchB' | 'newA' | 'newB' | 'bad';
export interface Align {
	axis: number;
	a: number | null;
	b: number | null;
	/** Size of the result along this axis (null where the rule fails). */
	out: number | null;
	kind: AlignKind;
}

/** The rule applied column by column, from the right. Left-most column first in the returned list (table order). */
export function align(sa: readonly number[], sb: readonly number[]): Align[] {
	const n = Math.max(sa.length, sb.length);
	const cols: Align[] = [];
	for (let k = 1; k <= n; k++) {
		const a = k <= sa.length ? sa[sa.length - k] : null;
		const b = k <= sb.length ? sb[sb.length - k] : null;
		let kind: AlignKind;
		if (a === null) kind = 'newA';
		else if (b === null) kind = 'newB';
		else if (a === b) kind = 'same';
		else if (a === 1) kind = 'stretchA';
		else if (b === 1) kind = 'stretchB';
		else kind = 'bad';
		const out = kind === 'bad' ? null : Math.max(a ?? 1, b ?? 1);
		cols.unshift({ axis: -k, a, b, out, kind });
	}
	return cols;
}

// ---------------------------------------------------------------------------------------------------------------
// One elementwise operation, drawn.

/** What the scene draws for one operand inside the result's shape. */
export interface Operand {
	t: Tensor;
	/** The read-only stride-0 view of the operand with the result's shape (core `broadcastTo`). */
	view: Tensor;
	/** Per result cell: the buffer element the view reads there (relative to the operand's offset). */
	src: Int32Array;
	/** Per result cell: true where this is the FIRST result cell reading that element (a real cell, not a repeat). */
	real: Uint8Array;
	/** Per result cell: the first result cell that reads the same element (where a repeat slides out from). */
	from: Int32Array;
	/** Repeats drawn (result cells minus real elements): what a copy would have to write, never written. */
	repeats: number;
}

export interface BModel {
	sa: number[];
	sb: number[];
	op: Op;
	A: Tensor;
	B: Tensor;
	cols: Align[];
	/** False when the shapes do not broadcast: the core threw, nothing is computed. */
	ok: boolean;
	/** The core's own message when it refused (the scene and the box print it as NumPy would). */
	coreMsg: string;
	/** Columns that broke the rule (axis from the right). */
	bad: number[];
	out: number[] | null;
	C: Tensor | null;
	opA: Operand | null;
	opB: Operand | null;
}

function operand(t: Tensor, shape: readonly number[]): Operand {
	const view = broadcastTo(t, shape);
	const src = offsets(view.shape, view.strides);
	const real = new Uint8Array(src.length);
	const from = new Int32Array(src.length);
	const first = new Map<number, number>();
	src.forEach((p, k) => {
		const f = first.get(p);
		if (f === undefined) {
			first.set(p, k);
			real[k] = 1;
			from[k] = k;
		} else from[k] = f;
	});
	return { t, view, src, real, from, repeats: src.length - first.size };
}

export const OP_FN = [add, sub, mul] as const;

/** A broadcast elementwise op of two given tensors, with every drawn fact. */
export function compute(A: Tensor, B: Tensor, op: Op): Pick<BModel, 'ok' | 'coreMsg' | 'out' | 'C' | 'opA' | 'opB' | 'cols' | 'bad'> {
	const cols = align(A.shape, B.shape);
	const bad = cols.filter((c) => c.kind === 'bad').map((c) => c.axis);
	try {
		const out = broadcastShapes(A.shape, B.shape);
		const C = OP_FN[op](A, B);
		return { ok: true, coreMsg: '', out, C, opA: operand(A, out), opB: operand(B, out), cols, bad };
	} catch (e) {
		if (!(e instanceof ShapeError)) throw e;
		return { ok: false, coreMsg: e.message, out: null, C: null, opA: null, opB: null, cols, bad };
	}
}

/**
 * Readable operands: A = 0, 1, 2, ... in memory order (NumPy `arange`), B = 10, 20, 30, ..., so in every result cell
 * the tens come from B and the ones from A, and the reader sees which elements met.
 */
export function operands(sa: readonly number[], sb: readonly number[]): { A: Tensor; B: Tensor } {
	return {
		A: arange(sizeOf(sa), sa),
		B: map(arange(sizeOf(sb), sb), (x) => 10 * (x + 1)),
	};
}

export function model(v: Vals): BModel {
	const ar = int(v.ar, 'ar'), br = int(v.br, 'br');
	const sa = shapeOf(ar, LEVELS.map((k) => int(v[aKey(k)], aKey(k))));
	const sb = shapeOf(br, LEVELS.map((k) => int(v[bKey(k)], bKey(k))));
	const op = clamp(int(v.op, 'op'), 0, 2) as Op;
	const { A, B } = operands(sa, sb);
	return { sa, sb, op, A, B, ...compute(A, B, op) };
}

// ---------------------------------------------------------------------------------------------------------------
// Scene 3.4-2: tint every pixel.

/** A 4 x 4 crop of lesson 3.1's icon (rows 2..5, columns 1..4: the roof over the wall and the door), uint8 RGB. */
export const CROP = ICON.slice(2, 6).map((row) => row.slice(1, 5));

/** The picture times the tint: (4, 4, 3) x (3,) -> (4, 4, 3). The tint is read from the eased (shown) values. */
export function tintModel(v: Vals): BModel {
	const tv = v.tint;
	if (!Array.isArray(tv) || tv.length !== 3) throw new Error('broadcasting: "tint" must be a 3-vector');
	finite(tv);
	const A = tensor(CROP.map((row) => row.map((p) => [...p])));
	const B = tensor([...tv]);
	return { sa: [...A.shape], sb: [3], op: 2, A, B, ...compute(A, B, 2) };
}

// ---------------------------------------------------------------------------------------------------------------
// Challenges.

const same = (s: readonly number[], t: readonly number[]) => s.length === t.length && s.every((n, i) => n === t[i]);

export const CHECKS = {
	/** A (3, 4) with a column B of shape (3, 1): each ROW gets its own number. */
	column: (v: Vals) => {
		const m = model(v);
		return same(m.sa, [3, 4]) && same(m.sb, [3, 1]);
	},
	/** Shapes that do not broadcast (the core refuses). */
	error: (v: Vals) => !model(v).ok,
	/** Both operands stretch and the result is (3, 4) from only 3 + 4 stored numbers (the outer pattern). */
	outer: (v: Vals) => {
		const m = model(v);
		return m.ok && same(m.out!, [3, 4]) && m.A.size + m.B.size === 7 && m.opA!.repeats > 0 && m.opB!.repeats > 0;
	},
	/** Red channel removed from the whole picture with one number. */
	noRed: (v: Vals) => {
		const tv = v.tint as number[];
		return Math.abs(tv[0]) < 1e-9 && tv[1] > 0 && tv[2] > 0;
	},
};

/** The values of a tensor in row-major order (for readouts and tests). */
export const values = (t: Tensor): number[] => flat(t);
