/**
 * Lesson 3.1 (the tensor road, docs/design/backlog-tensor-road-scene.md): the store contract, the real-data examples,
 * the nested layout and every derived number (shape, strides, offset, the transposed walk). Pure and three-free: the
 * entry, the scene, the N-panel rows and the tests share it. Strides and offsets come from the course's tensor core
 * (src/core), the same NumPy model lesson 3.3 builds on.
 *
 * Store `tensor-road` (one link group, so every write leaves a consistent state):
 * - `rank` 0..6: the number of axes on the road slider.
 * - `g0`..`g5`: sizes of the abstract road by GROWTH LEVEL. Level 0 is the innermost axis (the last number of the shape,
 *   the columns), level 1 the rows, level 2 the layers, levels 3-5 repeat x, y, z one nesting level up (rows, sheets and
 *   blocks of blocks). Each new axis repeats the whole previous object, so it becomes the FIRST number of the shape:
 *   shape = (g[rank-1], ..., g[1], g[0]).
 * - `k0`..`k5`: the selected cell's index by growth level (clamped to the sizes in the same write).
 * - `data` 0/1: show the real-data example of the current rank (`pick` chooses among several) instead of the
 *   abstract road (whose values are 0, 1, 2, ... in memory order, NumPy's `arange(n).reshape(shape)`).
 * - `tr` 0/1: the memory strip reads the tensor through its transposed view (`.T`: reversed axes, reversed strides).
 */
import { cStrides, offsets, sizeOf, transpose, Tensor, isContiguous } from '../../core';
import type { LessonLinks, LinkGroup } from '../links';
import { finite } from '../links';
import type { SceneRole } from '../palette';
import type { LessonState, Val } from '../store';
import { ANIM, dusk, GLOVE8, GLOVE_DIMS, ICON, LOSS, luma, mirror, pan, QUAD, SENTENCE } from './tensor-road.data';

export const LESSON = 'tensor-road';
export const MAX_RANK = 6;

/** Ranges of the reader-writable keys. Sizes stop at 4 so the deepest road (4^6 = 4096 cells) stays one instanced draw. */
export const SPEC = {
	rank: { min: 0, max: MAX_RANK, step: 1 },
	size: { min: 1, max: 4, step: 1 },
} as const;

/** Default sizes by growth level: a row of 4, a 3 x 4 sheet, a 2 x 3 x 4 block, then 2, 3 and 2 copies of that. */
export const DEF_SIZES = [4, 3, 2, 2, 3, 2] as const;


export const gKey = (k: number) => 'g' + k;
export const kKey = (k: number) => 'k' + k;
const LEVELS = Array.from({ length: MAX_RANK }, (_, k) => k);
/** Keys of the road's link group. */
export const ROAD_KEYS = ['rank', 'data', 'pick', ...LEVELS.map(gKey), ...LEVELS.map(kKey)] as const;

/** Defaults: rank 3 (the block 2 x 3 x 4), first cell selected, abstract road, plain (not transposed) view. */
export const INITIAL: Record<string, Val> = {
	rank: 3,
	data: 0,
	pick: 0,
	tr: 0,
	...Object.fromEntries(LEVELS.map((k) => [gKey(k), DEF_SIZES[k]])),
	...Object.fromEntries(LEVELS.map((k) => [kKey(k), 0])),
};

type Vals = LessonState['vals'];

// ---------------------------------------------------------------------------------------------------------------
// Directions and layout.

/** Where an axis is drawn: world axis 0 = x (right), 1 = y (down the screen), 2 = z (into the screen); level 1 = blocks. */
export interface Dir {
	axis: 0 | 1 | 2;
	level: 0 | 1;
}

/** Directions of the abstract road: growth level k goes to world axis k mod 3, one nesting level up from k = 3. */
export function roadDirs(rank: number): Dir[] {
	return Array.from({ length: rank }, (_, a) => {
		const k = rank - 1 - a;
		return { axis: (k % 3) as 0 | 1 | 2, level: k >= 3 ? 1 : 0 };
	});
}

/**
 * Colour of an axis (HUD numbers, rulers, N-panel): Blender's x, y, z colours for cell axes along that world axis, the
 * vector roles for block axes. The abstract road's growth levels 0..5 thus read red, green, blue, then vecA, vecB, vecC.
 */
export const dirRole = (d: Dir): SceneRole => (d.level === 0 ? (['axisX', 'axisY', 'axisZ'] as const) : (['vecA', 'vecB', 'vecC'] as const))[d.axis];

/** Gap between blocks (one empty cell), so a nesting level reads at a glance. */
export const BLOCK_GAP = 1;
/**
 * Gap between blocks that are one cell thick along their axis (the clips of the video stand one behind another as flat
 * sheets): one empty cell would hide the back sheet behind the front one, three let it show from the home view.
 */
export const SHEET_GAP = 3;

export interface Layout {
	/** Cell centres in C order (index i = memory offset of the contiguous tensor), world units, cell pitch 1. */
	pos: Float32Array;
	/** Pitch of each shape axis along its world axis (1 for cells, block size + gap for blocks). */
	pitch: number[];
	/** Largest coordinate per world axis (cells sit at 0..max, signed by the world direction). */
	max: [number, number, number];
}

/** World sign per axis: x grows right, rows grow DOWN (row 0 on top), layers grow AWAY from the viewer. */
const SIGN = [1, -1, -1] as const;

/**
 * Nested layout of every cell. Cell 0 sits at the origin and the figure grows away from it, so the copy with index 0
 * along a new axis stays exactly where the previous, smaller tensor was (the extrusion slides the other copies out).
 */
export function layout(shape: readonly number[], dirs: readonly Dir[]): Layout {
	if (shape.length !== dirs.length) throw new Error('tensor-road: one direction per axis');
	const cellSpan: [number, number, number] = [1, 1, 1];
	for (let a = 0; a < shape.length; a++) if (dirs[a].level === 0) cellSpan[dirs[a].axis] = shape[a];
	const pitch = dirs.map((d) => (d.level === 0 ? 1 : cellSpan[d.axis] + (cellSpan[d.axis] === 1 ? SHEET_GAP : BLOCK_GAP)));
	const n = sizeOf(shape);
	const pos = new Float32Array(3 * n);
	const max: [number, number, number] = [0, 0, 0];
	for (let a = 0; a < shape.length; a++) max[dirs[a].axis] += (shape[a] - 1) * pitch[a];
	const st = cStrides(shape);
	for (let i = 0; i < n; i++) {
		let r = i;
		for (let a = 0; a < shape.length; a++) {
			const j = Math.floor(r / st[a]);
			r -= j * st[a];
			pos[3 * i + dirs[a].axis] += SIGN[dirs[a].axis] * j * pitch[a];
		}
	}
	return { pos, pitch, max };
}

/**
 * Extrusion of axis 0 (the newest axis): at progress u the copy with index j along it sits at u of the way from the
 * index-0 copy to its place, and fades in with u. Writes the centre into `out` and returns the alpha.
 */
export function grow(L: Layout, shape: readonly number[], i: number, u: number, out: number[]): number {
	if (shape.length === 0 || u >= 1) {
		out[0] = L.pos[3 * i]; out[1] = L.pos[3 * i + 1]; out[2] = L.pos[3 * i + 2];
		return 1;
	}
	const s0 = sizeOf(shape) / shape[0];
	const j = Math.floor(i / s0), base = i - j * s0;
	for (let w = 0; w < 3; w++) out[w] = L.pos[3 * base + w] + (L.pos[3 * i + w] - L.pos[3 * base + w]) * u;
	return j === 0 ? 1 : u;
}

// ---------------------------------------------------------------------------------------------------------------
// Real data.

/** How the cells of an example are drawn: abstract cells, grey levels, colour channels, or signed numbers (Hinton). */
export type Look = 'cells' | 'gray' | 'rgb' | 'signed';
export type ExampleId = 'loss' | 'pixel' | 'word' | 'gray' | 'verts' | 'rgb' | 'text' | 'anim' | 'batch' | 'video';

export interface Example {
	id: ExampleId;
	shape: number[];
	/** Axis letters in shape order (H, W, C ...). */
	names: string[];
	dirs: Dir[];
	look: Look;
	/** Values in memory (C) order. */
	data: Float64Array;
	/** Element type the real data is stored in. */
	dtype: 'uint8' | 'float32';
	/** Shape axis holding the colour channel (rgb look). */
	channel?: number;
	/** Shape of the full tensor when the drawn one is its first slice along the last axis (GloVe: 300 numbers). */
	of?: number[];
	/** Decimals of the printed values. */
	dec: number;
}

const D = (axis: 0 | 1 | 2, level: 0 | 1 = 0): Dir => ({ axis, level });
const flatten = (v: unknown): number[] => (Array.isArray(v) ? v.flatMap(flatten) : [v as number]);
/**
 * Images: rows down (y), columns right (x), and the channel axis lays the red, green and blue images SIDE BY SIDE (x,
 * one nesting level up), so the three channels of every pixel are visible at once instead of hiding behind each other.
 */
const IMG = { names: ['H', 'W', 'C'], dirs: [D(1), D(0), D(0, 1)] };

function example(id: ExampleId, shape: number[], names: string[], dirs: Dir[], look: Look, values: unknown, extra: Partial<Example> = {}): Example {
	const data = Float64Array.from(flatten(values));
	if (data.length !== sizeOf(shape)) throw new Error(`tensor-road example ${id}: ${data.length} values for shape (${shape.join(', ')})`);
	return { id, shape, names, dirs, look, data, dtype: look === 'gray' || look === 'rgb' ? 'uint8' : 'float32', dec: look === 'signed' ? 2 : 0, ...extra };
}

/** The examples of each rank, in button order (rank 6 has none: the scene says so and keeps the abstract road). */
export const EXAMPLES: readonly (readonly Example[])[] = [
	[example('loss', [], [], [], 'signed', LOSS)],
	[
		// The sun pixel of the icon: one RGB triple.
		example('pixel', [3], ['C'], [D(0)], 'rgb', ICON[0][6], { channel: 0 }),
		example('word', [8], ['D'], [D(0)], 'signed', GLOVE8.cat, { of: [GLOVE_DIMS], dec: 3 }),
	],
	[
		example('gray', [8, 8], ['H', 'W'], [D(1), D(0)], 'gray', ICON.map((row) => row.map(luma))),
		example('verts', [4, 3], ['N', 'xyz'], [D(1), D(0)], 'signed', QUAD),
	],
	[
		example('rgb', [8, 8, 3], IMG.names, IMG.dirs, 'rgb', ICON, { channel: 2 }),
		example('text', [1, 6, 8], ['B', 'T', 'D'], [D(2), D(1), D(0)], 'signed', [SENTENCE.map((w) => GLOVE8[w])], { of: [1, 6, GLOVE_DIMS], dec: 3 }),
		example('anim', [4, 2, 3], ['F', 'J', 'xyz'], [D(2), D(1), D(0)], 'signed', ANIM),
	],
	[example('batch', [3, 8, 8, 3], ['B', ...IMG.names], [D(1, 1), ...IMG.dirs], 'rgb', [ICON, mirror(ICON), dusk(ICON)], { channel: 3 })],
	[
		example('video', [2, 3, 8, 8, 3], ['B', 'T', ...IMG.names], [D(2, 1), D(1, 1), ...IMG.dirs], 'rgb',
			[[0, 1, 2].map((t) => pan(ICON, t)), [0, 1, 2].map((t) => pan(dusk(ICON), t))], { channel: 4 }),
	],
	[],
];
for (const [r, list] of EXAMPLES.entries()) for (const e of list) if (e.shape.length !== r) throw new Error(`tensor-road example ${e.id} is not of rank ${r}`);

// ---------------------------------------------------------------------------------------------------------------
// The store and its one link group.

const int = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`tensor-road: "${key}" must be a number`);
	finite(v);
	return Math.round(v);
};
const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

/** The example shown for these values, or null for the abstract road. */
export function exampleOf(rank: number, data: number, pick: number): Example | null {
	if (data !== 1) return null;
	const list = EXAMPLES[rank];
	return list.length ? list[clamp(pick, 0, list.length - 1)] : null;
}

/** Shape (outermost axis first) of the abstract road at `rank` from the level sizes. */
export const roadShape = (rank: number, g: readonly number[]): number[] => Array.from({ length: rank }, (_, a) => g[rank - 1 - a]);

/**
 * Rank, sizes and data choice are rounded and clamped; the selected index is clamped to the resulting shape, so a
 * smaller size or another example never leaves the selection outside the tensor.
 */
export function roadLink(): LinkGroup {
	return {
		keys: ROAD_KEYS,
		// Every other key of a group is written as a cut; data (a checkbox, never eased) keeps rank, sizes and index
		// integers on screen too (the scene animates the rank change itself).
		canonical: 'data',
		resolve(input, current) {
			const get = (k: string) => int(Object.hasOwn(input, k) ? input[k] : current[k], k);
			const rank = clamp(get('rank'), SPEC.rank.min, SPEC.rank.max);
			const data = get('data') === 1 ? 1 : 0;
			const pick = clamp(get('pick'), 0, 2);
			const g = LEVELS.map((k) => clamp(get(gKey(k)), SPEC.size.min, SPEC.size.max));
			const shape = exampleOf(rank, data, pick)?.shape ?? roadShape(rank, g);
			const out: Record<string, Val> = { rank, data, pick };
			for (const k of LEVELS) {
				out[gKey(k)] = g[k];
				// Level k is shape axis rank-1-k; levels beyond the rank keep their index for when the rank grows back.
				const n = k < shape.length ? shape[shape.length - 1 - k] : SPEC.size.max;
				out[kKey(k)] = clamp(get(kKey(k)), 0, n - 1);
			}
			return out;
		},
		shown: () => ({}),
	};
}

export const LINKS: LessonLinks = { groups: [roadLink()] };

// ---------------------------------------------------------------------------------------------------------------
// Derived numbers.

export interface Model {
	rank: number;
	ex: Example | null;
	/** True when data mode is on but the rank has no example (rank 6). */
	noExample: boolean;
	shape: number[];
	/** Axis letters (examples) or null (abstract road: "axis 0", "axis 1", ...). */
	names: string[] | null;
	dirs: Dir[];
	/** Colour of each shape axis. */
	role: SceneRole[];
	size: number;
	strides: number[];
	/** Level sizes of the abstract road (the store's g0..g5), also while an example is shown. */
	sizes: number[];
	/** Selected index in shape order. */
	idx: number[];
	offset: number;
	/** Values in memory order: the example's data, or 0, 1, 2, ... for the abstract road. */
	data: Float64Array;
	value: number;
}

/** Sizes of the road by level from the store. */
export const sizesOf = (v: Vals): number[] => LEVELS.map((k) => int(v[gKey(k)], gKey(k)));

export function model(v: Vals): Model {
	const rank = int(v.rank, 'rank');
	const ex = exampleOf(rank, int(v.data, 'data'), int(v.pick, 'pick'));
	const sizes = sizesOf(v);
	const shape = ex ? ex.shape : roadShape(rank, sizes);
	const size = sizeOf(shape);
	const strides = cStrides(shape);
	const idx = shape.map((_, a) => int(v[kKey(rank - 1 - a)], kKey(rank - 1 - a)));
	idx.forEach((i, a) => {
		if (i < 0 || i >= shape[a]) throw new Error(`tensor-road: index ${i} outside axis ${a} of size ${shape[a]}`);
	});
	const offset = idx.reduce((s, i, a) => s + i * strides[a], 0);
	const data = ex ? ex.data : Float64Array.from({ length: size }, (_, i) => i);
	const dirs = ex ? ex.dirs : roadDirs(rank);
	return {
		rank, ex, noExample: !ex && v.data === 1, shape, names: ex ? ex.names : null, dirs,
		role: dirs.map(dirRole), sizes, size, strides, idx, offset, data, value: data[offset],
	};
}

/** Store patch selecting the cell at memory offset `off` (a click on a cell or on the memory strip). */
export function selectPatch(m: Pick<Model, 'shape' | 'strides' | 'rank'>, off: number): Record<string, number> {
	const out: Record<string, number> = {};
	let r = off;
	m.shape.forEach((_, a) => {
		const j = Math.floor(r / m.strides[a]);
		r -= j * m.strides[a];
		out[kKey(m.rank - 1 - a)] = j;
	});
	return out;
}

/** Index tuple of memory offset `off` (shape order). */
export function indexOf(m: Pick<Model, 'shape' | 'strides'>, off: number): number[] {
	let r = off;
	return m.shape.map((_, a) => {
		const j = Math.floor(r / m.strides[a]);
		r -= j * m.strides[a];
		return j;
	});
}

/** The memory view the strip shows: the tensor itself, or its transpose (same buffer, reversed shape and strides). */
export interface MemView {
	shape: readonly number[];
	strides: readonly number[];
	contiguous: boolean;
	/** walk[k] = buffer position the k-th step of a row-major walk over the view reads. */
	walk: Int32Array;
	/** step[p] = the walk step that reads buffer position p. */
	step: Int32Array;
	/** The selected cell's index in this view. */
	idx: number[];
}

export function memView(m: Pick<Model, 'shape' | 'idx' | 'size'>, tr: boolean): MemView {
	const base = new Tensor(new Float64Array(Math.max(1, m.size)), m.shape);
	const view = tr ? transpose(base) : base;
	const walk = offsets(view.shape, view.strides);
	const step = new Int32Array(walk.length);
	walk.forEach((p, k) => (step[p] = k));
	return { shape: view.shape, strides: view.strides, contiguous: isContiguous(view), walk, step, idx: tr ? [...m.idx].reverse() : [...m.idx] };
}

// ---------------------------------------------------------------------------------------------------------------
// Formatting and challenges.

/** Shape as NumPy prints it: "()" for a scalar, "(4,)" for one axis, "(2, 3, 4)" otherwise. */
export const fmtShape = (s: readonly number[]): string => (s.length === 1 ? `(${s[0]},)` : `(${s.join(', ')})`);
export const fmtInt = (x: number): string => String(Math.round(x));
/** A value as its example prints it (uint8 integers, two or three decimals for floats); the road prints integers. */
export const fmtVal = (m: Pick<Model, 'ex'>, x: number): string => (m.ex ? (Math.abs(x) < 5e-10 ? 0 : x).toFixed(m.ex.dec) : fmtInt(x));

/** The walk step 1 of the transposed view: the second cell `.T` reads (challenge 4). */
export const secondOfTranspose = (m: Pick<Model, 'shape' | 'idx' | 'size'>): number => memView(m, true).walk[1];

export const CHECKS = {
	/** Rank 4, abstract road, 24 elements. */
	rank4of24: (v: Vals) => {
		const m = model(v);
		return !m.ex && !m.noExample && m.rank === 4 && m.size === 24;
	},
	/** A cell at memory address 17 is selected (abstract road). */
	offset17: (v: Vals) => {
		const m = model(v);
		return !m.ex && m.offset === 17;
	},
	/** Green channel of the pixel in row 2, column 5 of the RGB image. */
	green25: (v: Vals) => {
		const m = model(v);
		return m.ex?.id === 'rgb' && m.idx.join() === '2,5,1';
	},
	/** Transposed view on, the cell its walk reads second is selected (a tensor of rank 2 or more). */
	transposeSecond: (v: Vals) => {
		const m = model(v);
		return v.tr === 1 && m.rank >= 2 && m.size > 1 && m.offset === secondOfTranspose(m);
	},
};
