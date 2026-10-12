/**
 * Shared, pure pieces of the tensor lessons (3.1 road, 3.2 data, 3.3 memory): where each cell of a tensor is drawn
 * (nested layout by axis direction), how one cell looks (abstract cell, byte brightness, signed Hinton square), and
 * which part of a long memory strip is listed. Three-free and DOM-free: the scenes, the DOM strip and the tests share it.
 * Every stride and address comes from the course's tensor core (src/core); nothing here re-derives NumPy rules.
 */
import { cStrides, sizeOf } from '../../core';
import type { SceneRole } from '../palette';
import type { BoxRole, Frame } from '../dotunit/choreo';

// ---------------------------------------------------------------------------------------------------------------
// Directions and layout.

/** Where an axis is drawn: world axis 0 = x (right), 1 = y (down the screen), 2 = z (into the screen); level 1 = blocks. */
export interface Dir {
	axis: 0 | 1 | 2;
	level: 0 | 1;
}

export const D = (axis: 0 | 1 | 2, level: 0 | 1 = 0): Dir => ({ axis, level });

/**
 * Colour of an axis (HUD numbers, rulers, N-panel): Blender's x, y, z colours for cell axes along that world axis, the
 * vector roles for block axes.
 */
export const dirRole = (d: Dir): SceneRole => (d.level === 0 ? (['axisX', 'axisY', 'axisZ'] as const) : (['vecA', 'vecB', 'vecC'] as const))[d.axis];

/**
 * Directions that keep every cell facing the viewer (no axis goes into the screen), so each cell's number stays
 * readable: the last axis runs right, the one before it down, then blocks side by side, then rows of blocks. Rank <= 4.
 */
export function flatDirs(rank: number): Dir[] {
	if (rank > 4) throw new Error(`tensor-cells: a flat layout has at most 4 axes, not ${rank}`);
	const inner: Dir[] = [D(0), D(1), D(0, 1), D(1, 1)];
	return Array.from({ length: rank }, (_, a) => inner[rank - 1 - a]);
}

/** Gap between blocks (one empty cell), so a nesting level reads at a glance. */
export const BLOCK_GAP = 1;
/**
 * Gap between blocks that are one cell thick along their axis (flat sheets one behind another): one empty cell would
 * hide the back sheet behind the front one, three let it show from a three-quarter view.
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
export const SIGN = [1, -1, -1] as const;

/**
 * Nested layout of every cell. Cell 0 sits at the origin and the figure grows away from it, so the copy with index 0
 * along a new axis stays exactly where the previous, smaller tensor was (3.1's extrusion slides the other copies out).
 */
export function layout(shape: readonly number[], dirs: readonly Dir[]): Layout {
	if (shape.length !== dirs.length) throw new Error('tensor-cells: one direction per axis');
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
// The look of one cell.

/** How a cell is drawn: an abstract cell, a byte (grey or one colour channel), or a signed number (Hinton square). */
export type Look = 'cells' | 'gray' | 'rgb' | 'signed';

/** Edge of a cell box (pitch 1): the gap between neighbours keeps every cell readable. */
export const CELL = 0.8;
/** Colour of channel c of an RGB image: red, green, blue drawn in Blender's x, y, z colours. */
export const CHANNEL: readonly BoxRole[] = ['axisX', 'axisY', 'axisZ'];

/**
 * Writes cell `i` centred at `c` into the frame. `cells`: a neutral box. `gray`/`rgb`: a solid box whose brightness is
 * the byte (0 = background, 255 = the full colour of `role`). `signed`: a Hinton square whose edge grows with |v| /
 * maxAbs, green when positive, magenta and hatched when negative. `alpha` fades the whole cell (extrusion, ghosts).
 */
export function paintCell(f: Frame, i: number, c: ArrayLike<number>, look: Look, v: number, alpha: number, o: { maxAbs?: number; role?: BoxRole } = {}): void {
	if (look === 'signed') {
		const e = 0.15 + 0.7 * Math.abs(v) / Math.max(1e-9, o.maxAbs ?? 1);
		f.box(i, c[0] - e / 2, c[0] + e / 2, c[1] - e / 2, c[1] + e / 2, c[2] - e / 2, c[2] + e / 2);
		f.look(i, v < 0 ? 'vecB' : 'vecC', false, alpha, v < 0 ? 1 : 0, false);
		return;
	}
	const h = CELL / 2;
	f.box(i, c[0] - h, c[0] + h, c[1] - h, c[1] + h, c[2] - h, c[2] + h);
	if (look === 'cells') f.look(i, o.role ?? 'ink', false, alpha, 0, false);
	else f.look(i, look === 'gray' ? 'ink' : (o.role ?? 'ink'), true, alpha * (0.06 + 0.94 * v / 255), 0, false);
}

/** Largest |v| (the Hinton scale), at least a tiny positive number so an all-zero tensor draws minimal squares. */
export const maxAbsOf = (data: ArrayLike<number>): number => {
	let m = 1e-9;
	for (let i = 0; i < data.length; i++) m = Math.max(m, Math.abs(data[i]));
	return m;
};

// ---------------------------------------------------------------------------------------------------------------
// Memory strip.

/**
 * The part of an `n`-cell strip that is listed: at most `max` cells, the `center` cell in the middle when possible
 * (a 4096-cell buffer would not fit a page). Returns [from, to).
 */
export function stripWindow(n: number, center: number, max: number): { from: number; to: number } {
	const from = Math.max(0, Math.min(n - max, center - Math.floor(max / 2)));
	return { from, to: Math.min(n, from + max) };
}

/** Index tuple of the k-th element of a row-major walk over `shape` (NumPy's `unravel_index`). */
export function unravel(k: number, shape: readonly number[]): number[] {
	if (!Number.isInteger(k) || k < 0 || k >= sizeOf(shape)) throw new Error(`tensor-cells: element ${k} outside shape (${shape.join(', ')})`);
	const st = cStrides(shape);
	let r = k;
	return shape.map((_, a) => {
		const j = Math.floor(r / st[a]);
		r -= j * st[a];
		return j;
	});
}

/** Shape as NumPy prints it:"()" for a scalar, "(4,)" for one axis, "(2, 3, 4)" otherwise. */
export const fmtShape = (s: readonly number[]): string => (s.length === 1 ? `(${s[0]},)` : `(${s.join(', ')})`);
