/**
 * Procedural layout of an einsum scene (docs/design/intro-and-dotunit.md §1.2, §1.4): `scene = f(spec, values)`.
 * Pure, three-free and DOM-free, so the unit tests import it directly. It takes the tensor core's `Plan` (one plan
 * drives the numbers AND the placement, nothing is parsed twice) and returns where every operand element, output cell
 * and tile sits, which axis the shaft runs along and which way the stacks grow.
 *
 * Index-to-axis rule (spec §1.2):
 * 1. a kept letter's role comes from its position in the FIRST operand containing it, counted from the end: last =
 *    column (x), second-last = row (y; row 0 on top), earlier = batch. A rank-1 operand's only letter is a column;
 * 2. two kept letters wanting one axis: the first in output order keeps it, the next takes the first free of x, y, z;
 * 3. the summed letters flatten (row-major, last innermost) into ONE shaft on the first free axis after the kept ones,
 *    with a small extra gap between outer groups;
 * 4. batch letters in output order: the first is a stack of blocks along y (index 0 at the bottom), the second a row of
 *    stacks along x, the rest are slice selectors (one index drawn);
 * 5. output cells sit on the output face at the end of the shaft; stacks grow +z (shaft z), +y (shaft x), +x (shaft y).
 *
 * Units: one tile = 1, pitch PITCH between neighbours. Coordinates: x right, y up, z toward the viewer; x indices grow
 * right, y and z indices grow down / into depth (so index 0 is top / front, the reading order of a matrix).
 */
import { ShapeError, type Plan } from '../../core';

export type Axis = 0 | 1 | 2;
export type Lod = 1 | 2 | 3;
/** Role of an index letter in the picture. */
export type Place = 'x' | 'y' | 'z' | 'stack' | 'row' | 'slice' | 'shaft';

/** Distance between neighbouring tiles (tile 1 + gap 0.25). */
export const PITCH = 1.25;
/** Gap from a block to an operand face drawn outside it, and from the shaft's end to the output face. */
export const FACE = 1.25;
/** Gaps between batch blocks (stack along y) and between stacks of a row (heads along x). */
export const GAP_STACK = 1.5;
export const GAP_ROW = 2.5;
/** Extra gap between outer groups of a flattened shaft (two or more summed letters). */
export const GROUP_GAP = 0.5;
/** World units a stack may reach on each side of its output face when its direction runs along a kept axis. */
export const ROOM_KEPT = 1.2;
/** ... and when it runs along an axis nothing else uses (z of the matmul cube). */
export const ROOM_FREE = 3;
/** Largest extent of any index: beyond it the scene refuses (spec §1.4). */
export const CAP = 64;
/** LOD limits per level: largest extent of a drawn axis and number of drawn output cells. */
export const LOD_LIMITS: readonly { ext: number; cells: number }[] = [
	{ ext: 8, cells: 512 },
	{ ext: 32, cells: 4096 },
	{ ext: CAP, cells: 20000 },
];

const AXES: readonly Place[] = ['x', 'y', 'z'];
const axisOf = (p: Place): Axis => AXES.indexOf(p) as Axis;

/**
 * Level of detail of a picture whose largest drawn extent is `ext` and that draws `cells` output cells: 1 = every tile
 * with numbers, 2 = one bar per cell, 3 = a heatmap face. Chosen from the spec only (never from the device), so the
 * same spec always gives the same picture. Beyond level 3 it throws.
 */
export function lodOf(ext: number, cells: number): Lod {
	for (let i = 0; i < LOD_LIMITS.length; i++) if (ext <= LOD_LIMITS[i].ext && cells <= LOD_LIMITS[i].cells) return (i + 1) as Lod;
	throw new ShapeError(`picture too large: extent ${ext}, ${cells} cells (at most ${CAP} per index and ${LOD_LIMITS[2].cells} cells)`);
}

/** Where every letter of `plan` goes (rules 1-4), the shaft axis and the stack direction (rule 5). */
export interface Axes {
	place: Readonly<Record<string, Place>>;
	shaft: Axis;
	stackDir: Axis;
	/** Kept letter on each spatial axis (or null). */
	onAxis: readonly [string | null, string | null, string | null];
	/** Batch letters in output order: [stack, row, ...slices]. */
	batch: readonly string[];
}

export function assignAxes(plan: Plan): Axes {
	const place: Record<string, Place> = {};
	const onAxis: [string | null, string | null, string | null] = [null, null, null];
	const batch: string[] = [];
	for (const l of plan.kept) {
		const term = plan.inputs.find((t) => t.includes(l))!;
		const fromEnd = term.length - 1 - term.indexOf(l);
		if (fromEnd > 1) {
			batch.push(l);
			continue;
		}
		let a: Axis = fromEnd === 0 ? 0 : 1;
		if (onAxis[a] !== null) {
			const free = onAxis.indexOf(null);
			if (free < 0) throw new ShapeError(`"${plan.op}": no free axis for index "${l}"`);
			a = free as Axis;
		}
		onAxis[a] = l;
		place[l] = AXES[a];
	}
	const free = onAxis.indexOf(null);
	if (free < 0) throw new ShapeError(`"${plan.op}": no free axis left for the shaft`);
	const shaft = free as Axis;
	for (const l of plan.summed) place[l] = 'shaft';
	batch.forEach((l, i) => (place[l] = i === 0 ? 'stack' : i === 1 ? 'row' : 'slice'));
	const stackDir: Axis = shaft === 2 ? 2 : shaft === 0 ? 1 : 0;
	return { place, shaft, stackDir, onAxis, batch };
}

/** Coordinate of index `t` on spatial axis `a` (x grows right; y and z grow down / into depth). */
const coord = (a: Axis, t: number, pitch: number) => (a === 0 ? t * pitch : -t * pitch);

export interface OperandPlace {
	/** Letters of the operand, its element count drawn (selectors fixed) and per element its multi-index. */
	letters: string;
	n: number;
	idx: Int32Array;
	/** Element centre (x, y, z) per element. */
	pos: Float64Array;
	/** Axis the face looks along (its normal), and the face's side: -1 low (A), +1 high (B), 0 in the block plane. */
	normal: Axis;
	side: -1 | 0 | 1;
}

export interface Layout {
	plan: Plan;
	axes: Axes;
	lod: Lod;
	/** Selected index of every slice letter. */
	slices: Readonly<Record<string, number>>;
	/** Pitch of each spatial axis (the stack axis gets room for the stacks when it is a kept axis). */
	pitch: readonly [number, number, number];
	/** World units a stack may reach each way from the output face (scene scales values into it). */
	room: number;
	/** Shaft coordinate (along `axes.shaft`, block-local) of every shaft position, in shaft order. */
	shaftAt: Float64Array;
	/** Coordinate of the output face on the shaft axis (block-local). */
	outFace: number;
	/** Drawn output cells: flat index into the output, multi-index, anchor on the output face. */
	cells: { n: number; flat: Int32Array; idx: Int32Array; pos: Float64Array };
	operands: readonly [OperandPlace, OperandPlace];
	/** Bounds of everything placed (operands, tiles, cells and stack room). */
	lo: [number, number, number];
	hi: [number, number, number];
}

export interface LayoutOpts {
	/** Index drawn for each slice letter (default 0). */
	slices?: Readonly<Record<string, number>>;
	/** Stack room override (the DotUnit draws true heights: Infinity). */
	room?: number;
}

/**
 * Lays out a two-operand plan (rules 1-5 above). Throws a ShapeError for an extent above CAP, a picture above the
 * last LOD, an op without a free axis, or a slice index out of range: nothing is silently clamped.
 */
export function layoutEinsum(plan: Plan, o: LayoutOpts = {}): Layout {
	if (plan.inputs.length !== 2) throw new ShapeError(`"${plan.op}": a DotUnit scene takes exactly two operands`);
	for (const [l, n] of Object.entries(plan.dims)) {
		if (n > CAP) throw new ShapeError(`index "${l}" has extent ${n}; at most ${CAP} per index`);
		if (n < 1) throw new ShapeError(`index "${l}" has extent ${n}; a scene needs at least 1`);
	}
	const axes = assignAxes(plan);
	const { place, shaft, stackDir } = axes;
	const slices: Record<string, number> = {};
	for (const l of axes.batch.slice(2)) {
		const i = o.slices?.[l] ?? 0;
		if (!Number.isInteger(i) || i < 0 || i >= plan.dims[l]) throw new ShapeError(`slice ${i} of index "${l}" out of range (${plan.dims[l]})`);
		slices[l] = i;
	}
	for (const l of Object.keys(o.slices ?? {})) if (!(l in slices)) throw new ShapeError(`index "${l}" is not a slice selector of "${plan.op}"`);

	// Drawn cells and LOD: slice letters contribute one index.
	const drawnExt = (l: string) => (place[l] === 'slice' ? 1 : plan.dims[l]);
	const cellCount = plan.kept.reduce((p, l) => p * drawnExt(l), 1);
	const maxExt = Math.max(1, plan.sumLen, ...plan.kept.filter((l) => place[l] !== 'slice').map((l) => plan.dims[l]));
	const lod = lodOf(maxExt, cellCount);

	const used = [false, false, false];
	for (const l of plan.kept) if (place[l] === 'x' || place[l] === 'y' || place[l] === 'z') used[axisOf(place[l])] = true;
	used[shaft] = true;
	const stackKept = used[stackDir];
	const room = o.room ?? (stackKept ? ROOM_KEPT : ROOM_FREE);
	const pitch: [number, number, number] = [PITCH, PITCH, PITCH];
	// A stack running along a kept axis (matrix x vector: rows down y, stacks up y) needs its room between rows.
	if (stackKept && Number.isFinite(room)) pitch[stackDir] = PITCH + 2 * room;

	// Shaft: flattened summed letters, row-major; an extra gap after each complete inner run.
	const inner = plan.sumShape.length ? plan.sumShape[plan.sumShape.length - 1] : 1;
	const shaftAt = new Float64Array(plan.sumLen);
	for (let k = 0; k < plan.sumLen; k++) {
		const g = plan.sumShape.length > 1 ? Math.floor(k / inner) : 0;
		const d = k * pitch[shaft] + g * GROUP_GAP;
		shaftAt[k] = shaft === 0 ? d : -d;
	}
	const shaftEnd = plan.sumLen ? shaftAt[plan.sumLen - 1] : 0;
	// Output face on the shaft axis: right of the shaft (x), below it (y), in front of it (z).
	const outFace = shaft === 0 ? shaftEnd + FACE : shaft === 1 ? shaftEnd - FACE : FACE;

	// Block offsets of the batch letters: stack (y, index 0 at the bottom) and row of stacks (x).
	const stackL = axes.batch[0], rowL = axes.batch[1];
	const blockLocal = blockBounds(plan, axes, pitch, shaftAt, outFace, room);
	const blockH = blockLocal.hi[1] - blockLocal.lo[1], blockW = blockLocal.hi[0] - blockLocal.lo[0];
	const blockOff = (idxOf: (l: string) => number, out: number[]) => {
		out[0] = rowL ? idxOf(rowL) * (blockW + GAP_ROW) : 0;
		out[1] = stackL ? idxOf(stackL) * (blockH + GAP_STACK) : 0;
		out[2] = 0;
	};

	// Output cells.
	const kept = plan.kept;
	const cells = { n: cellCount, flat: new Int32Array(cellCount), idx: new Int32Array(cellCount * kept.length), pos: new Float64Array(cellCount * 3) };
	const off = [0, 0, 0];
	{
		const cur = kept.map((l) => (place[l] === 'slice' ? slices[l] : 0));
		for (let c = 0; c < cellCount; c++) {
			let flat = 0;
			kept.forEach((l, d) => (flat = flat * plan.dims[l] + cur[d]));
			cells.flat[c] = flat;
			cells.idx.set(cur, c * kept.length);
			const val = (l: string) => cur[kept.indexOf(l)];
			blockOff(val, off);
			const p = [off[0], off[1], off[2]];
			kept.forEach((l, d) => {
				const pl = place[l];
				if (pl === 'x' || pl === 'y' || pl === 'z') p[axisOf(pl)] += coord(axisOf(pl), cur[d], pitch[axisOf(pl)]);
			});
			p[shaft] += outFace;
			cells.pos.set(p, 3 * c);
			// Next multi-index (row-major), skipping slice letters (fixed).
			for (let d = kept.length - 1; d >= 0; d--) {
				if (place[kept[d]] === 'slice') continue;
				if (++cur[d] < plan.dims[kept[d]]) break;
				cur[d] = 0;
			}
		}
	}

	const operands = [0, 1].map((o2) => operandPlace(plan, axes, o2 as 0 | 1, pitch, shaftAt, slices, blockLocal, blockOff, used)) as [OperandPlace, OperandPlace];

	// Overall bounds: block bounds offset by the last block.
	const lo: [number, number, number] = [...blockLocal.lo];
	const hi: [number, number, number] = [...blockLocal.hi];
	const nRow = rowL ? plan.dims[rowL] : 1, nStack = stackL ? plan.dims[stackL] : 1;
	hi[0] += (nRow - 1) * (blockW + GAP_ROW);
	hi[1] += (nStack - 1) * (blockH + GAP_STACK);
	return { plan, axes, lod, slices, pitch, room, shaftAt, outFace, cells, operands, lo, hi };
}

/** Bounds of one block (batch offsets zero): kept extents, the shaft, both operand faces, output face plus stack room. */
function blockBounds(plan: Plan, axes: Axes, pitch: readonly number[], shaftAt: Float64Array, outFace: number, room: number) {
	const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
	const span = (a: number, x0: number, x1: number) => {
		lo[a] = Math.min(lo[a], x0, x1);
		hi[a] = Math.max(hi[a], x0, x1);
	};
	const half = 0.5;
	const range = (a: Axis): [number, number] => {
		const l = axes.onAxis[a];
		if (!l) return a === axes.shaft ? [outFace, outFace] : [0, 0];
		const c0 = coord(a, 0, pitch[a]), c1 = coord(a, plan.dims[l] - 1, pitch[a]);
		return [Math.min(c0, c1), Math.max(c0, c1)];
	};
	for (const a of [0, 1, 2] as const) if (a !== axes.shaft) span(a, range(a)[0] - half, range(a)[1] + half);
	const s0 = shaftAt[0], s1 = shaftAt[plan.sumLen - 1];
	span(axes.shaft, Math.min(s0, s1) - half, Math.max(s0, s1) + half);
	span(axes.shaft, outFace, outFace);
	// Operand faces outside the block (one face gap) and the stack room around the output face.
	for (let a = 0; a < 3; a++) {
		lo[a] -= a === axes.shaft ? 0 : FACE;
		hi[a] += a === axes.shaft ? 0 : FACE;
	}
	const r = Number.isFinite(room) ? room : 0;
	const [a0, a1] = range(axes.stackDir);
	span(axes.stackDir, a0 - r, a1 + r);
	return { lo: lo as [number, number, number], hi: hi as [number, number, number] };
}

/**
 * Placement of operand `o`: its letters' coordinates (kept spatial letters on their axis, summed letters on the shaft,
 * batch letters as block offsets). The face sits outside the block on the operand's first missing USED axis (A on the
 * low side, B on the high side: the matmul cube's left and top faces). An operand that misses only axes nothing uses
 * lies in the block plane (matrix x vector: W under its tiles), unless both operands have the same axes (the dot
 * product: a below the shaft, b above it).
 */
function operandPlace(
	plan: Plan, axes: Axes, o: 0 | 1, pitch: readonly number[], shaftAt: Float64Array, slices: Readonly<Record<string, number>>,
	block: { lo: number[]; hi: number[] }, blockOff: (idxOf: (l: string) => number, out: number[]) => void, used: readonly boolean[],
): OperandPlace {
	const letters = plan.inputs[o];
	const { place } = axes;
	const axesOf = (term: string) => {
		const s = new Set<number>();
		for (const l of term) {
			const p = place[l];
			if (p === 'x' || p === 'y' || p === 'z') s.add(axisOf(p));
			else if (p === 'shaft') s.add(axes.shaft);
		}
		return s;
	};
	const mine = axesOf(letters), other = axesOf(plan.inputs[1 - o]);
	const missing = [0, 1, 2].filter((a) => !mine.has(a));
	const missUsed = missing.filter((a) => used[a]);
	const same = mine.size === other.size && [...mine].every((a) => other.has(a));
	let normal: Axis, side: -1 | 0 | 1;
	if (missUsed.length) {
		normal = missUsed[0] as Axis;
		side = o === 0 ? -1 : 1;
	} else {
		normal = (missing[0] ?? 2) as Axis;
		side = same ? (o === 0 ? -1 : 1) : 0;
	}
	// The face plane: outside the block on its normal (or just behind the tiles for an in-plane operand).
	const facePos = side === -1 ? block.lo[normal] + 0.5 : side === 1 ? block.hi[normal] - 0.5 : -0.05;

	const dims = [...letters].map((l) => (place[l] === 'slice' ? 1 : plan.dims[l]));
	const n = dims.reduce((p, x) => p * x, 1);
	const idx = new Int32Array(n * letters.length), pos = new Float64Array(n * 3);
	const cur = [...letters].map((l) => (place[l] === 'slice' ? slices[l] : 0));
	const off = [0, 0, 0];
	// Multi-index of the summed letters this operand holds, flattened like the shaft (missing summed letters = 0).
	const sumStride = plan.summed.map((_, i) => plan.sumShape.slice(i + 1).reduce((p, x) => p * x, 1));
	for (let e = 0; e < n; e++) {
		idx.set(cur, e * letters.length);
		const val = (l: string) => cur[letters.indexOf(l)] ?? 0;
		blockOff(val, off);
		const p = [off[0], off[1], off[2]];
		p[normal] = facePos + (normal === 1 ? off[1] : normal === 0 ? off[0] : 0);
		let k = 0;
		[...letters].forEach((l, d) => {
			const pl = place[l];
			if (pl === 'x' || pl === 'y' || pl === 'z') p[axisOf(pl)] += coord(axisOf(pl), cur[d], pitch[axisOf(pl)]);
			else if (pl === 'shaft') k += cur[d] * sumStride[plan.summed.indexOf(l)];
		});
		if (mine.has(axes.shaft)) p[axes.shaft] += shaftAt[k] ?? 0;
		pos.set(p, 3 * e);
		for (let d = letters.length - 1; d >= 0; d--) {
			if (place[letters[d]] === 'slice') continue;
			if (++cur[d] < plan.dims[letters[d]]) break;
			cur[d] = 0;
		}
	}
	return { letters, n, idx, pos, normal, side };
}

/** Tile position of cell `c` at shaft position `k`: the cell's anchor with its shaft coordinate moved onto the shaft. */
export function tileAt(L: Layout, c: number, k: number, out: number[]): number[] {
	const s = L.axes.shaft;
	out[0] = L.cells.pos[3 * c];
	out[1] = L.cells.pos[3 * c + 1];
	out[2] = L.cells.pos[3 * c + 2];
	out[s] += L.shaftAt[k] - L.outFace;
	return out;
}

/** Stack scale: true heights while the largest |running sum| fits `room`, else shrunk so it does. */
export function stackScale(maxAbs: number, room: number): number {
	if (!Number.isFinite(room) || !(maxAbs > room)) return 1;
	return room / maxAbs;
}

/** Term-id suffix of a multi-index: digits run together while every extent is <= 10 (`c01`), else joined by `_`. */
export function idxId(idx: ArrayLike<number>, at: number, len: number, wide: boolean): string {
	let s = '';
	for (let d = 0; d < len; d++) s += (d && wide ? '_' : '') + idx[at + d];
	return s;
}
