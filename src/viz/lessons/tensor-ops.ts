/**
 * Shared pure helpers of lessons 3.4 (broadcasting) and 3.5 (reductions): where the cells of a tensor sit in a
 * front-view scene and how its numbers print. Three-free and DOM-free (the tests import it). The cell layout itself is
 * the tensor road's (`layout` in tensor-road.math.ts): the same nesting rule, pitch and gaps, so a (3, 4) sheet looks the
 * same in 3.1, 3.4 and 3.5. Only the choice of directions differs: these lessons are read from the front, so every
 * axis is laid in the screen plane (no axis into the screen).
 */
import { layout, type Dir, type Layout } from './tensor-road.math';

/** Highest rank the 3.4/3.5 scenes draw (shapes are edited per axis in the N-panel). */
export const OPS_MAX_RANK = 3;

/**
 * Depth (z) of a cell tile. The figures are read from the front, so cells are thin tiles, not cubes: a cube's side
 * walls show in perspective near the frame edges and read as extra lines between the numbers.
 */
export const TILE_DEPTH = 0.1;

const D = (axis: 0 | 1, level: 0 | 1 = 0): Dir => ({ axis, level });

/**
 * Front-view directions for a shape of rank 0..3, outermost axis first: the last axis runs right (columns), the one
 * before it down (rows), the third-last stacks whole sheets DOWN with a gap (blocks), so every cell faces the viewer
 * and the figure stays taller than wide enough for three tensors side by side.
 */
export function flatDirs(rank: number): Dir[] {
	if (!Number.isInteger(rank) || rank < 0 || rank > OPS_MAX_RANK) throw new Error(`tensor-ops: rank ${rank} outside 0..${OPS_MAX_RANK}`);
	return [D(1, 1), D(1), D(0)].slice(OPS_MAX_RANK - rank);
}

/**
 * An image H x W x C read from the front: rows down, columns right, and the three channel images stacked DOWN one
 * under another (so red, green and blue of every pixel are all visible, as in the tensor road's image presets).
 */
export const IMAGE_DIRS: readonly Dir[] = [D(1), D(0), D(1, 1)];

/** A placed tensor: its cell centres (C order, world units, cell pitch 1) moved to `origin`, and its extent. */
export interface Placed {
	pos: Float32Array;
	/** World width (x) and height (y) of the figure, cell edges included. */
	w: number;
	h: number;
	/** Top-left cell centre. */
	origin: readonly [number, number];
}

/** Places `shape` laid out along `dirs` with its first cell at `origin` (x right, y up: rows grow downwards). */
export function place(shape: readonly number[], dirs: readonly Dir[], origin: readonly [number, number]): Placed {
	const L: Layout = layout(shape, dirs);
	const pos = new Float32Array(L.pos.length);
	for (let i = 0; i < pos.length; i += 3) {
		pos[i] = L.pos[i] + origin[0];
		pos[i + 1] = L.pos[i + 1] + origin[1];
		pos[i + 2] = L.pos[i + 2];
	}
	return { pos, w: L.max[0] + 1, h: L.max[1] + 1, origin };
}

/** Index tuple (shape order) of row-major position `k`. */
export function unravel(shape: readonly number[], k: number): number[] {
	const idx = new Array<number>(shape.length);
	for (let a = shape.length - 1; a >= 0; a--) {
		idx[a] = k % shape[a];
		k = Math.floor(k / shape[a]);
	}
	return idx;
}

/** A value as the lessons print it: integers plainly, everything else with up to `dec` decimals (no "-0"). */
export function fmtNum(x: number, dec = 2): string {
	if (Number.isNaN(x)) return 'nan';
	const r = Number(x.toFixed(dec));
	return String(Object.is(r, -0) ? 0 : r);
}

/** Shapes print as in lesson 3.1 (NumPy's way): "()", "(4,)", "(2, 3, 4)". */
export { fmtShape } from './tensor-road.math';
