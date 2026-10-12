/**
 * Lesson 3.5 choreography as pure functions (three-free; the tests check exactly what the renderer draws). Drawn with
 * the DotUnit's instanced `Boxes` renderer.
 *
 * Instances: the input cells (`x<k>`, they stay as a faint record of the input), a moving copy of every input cell
 * (same id: hovering it is hovering that cell) and the result cells (`r<c>`).
 * Timeline `rd`: 0 = the input; 1 = collapse: every copy slides along the reduced axis into the cell at index 0 of
 * that axis and merges there into the result (for max, the copies that are not the maximum fade on the way); 2 = the
 * result moves below the input and takes its own shape (kept axes of size 1 with keepdims, or removed).
 */
import { Frame } from '../dotunit/choreo';
import type { RModel } from './reductions.math';
import { flatDirs, place, type Placed } from './tensor-ops';

export const CELL = 0.8;
/** The result cells are drawn larger, so at the landing cell they sit in front of the faint input cell. */
export const RESULT = 0.9;
/** Opacity the input fades to once it has been reduced. */
export const GHOST = 0.3;
/** Vertical gap (world units) between the input and the result below it. */
export const OUT_GAP = 2.5;

export interface Geo {
	input: Placed;
	output: Placed;
	/** True for the 8-bit image: its input cells are drawn as brightness instead of numbered tiles. */
	image: boolean;
	w: number;
	h: number;
}

export function geo(m: RModel): Geo {
	const input = place(m.x.shape, flatDirs(m.x.ndim), [0, 0]);
	const output = place(m.out, flatDirs(m.out.length), [0, -(input.h - 1) - OUT_GAP - 1]);
	return { input, output, image: m.src === 'gray', w: Math.max(input.w, output.w), h: input.h + OUT_GAP + output.h };
}

export const instances = (m: RModel) => 2 * m.x.size + m.r.size;
/** Frame capacity: the largest figure is the 8 x 8 image, its 64 moving copies and up to 64 results. */
export const CAP = 3 * 64;

export function writeIds(m: RModel, f: Frame): void {
	const n = m.x.size;
	for (let k = 0; k < n; k++) f.id[k] = f.id[n + k] = 'x' + k;
	for (let c = 0; c < m.r.size; c++) f.id[2 * n + c] = 'r' + c;
}

const ease = (u: number) => u * u * (3 - 2 * u);
const clamp01 = (u: number) => (u < 0 ? 0 : u > 1 ? 1 : u);

/** Progress of the two motions from the timeline step and the progress through it. */
export function phases(step: number, t: number): { collapse: number; move: number } {
	return { collapse: step >= 2 ? 1 : step === 1 ? t : 0, move: step >= 2 ? t : 0 };
}

/** Ids lit with the hovered one: a result cell lights its whole group; an input cell lights its result and group. */
export function linked(m: RModel, hot: ReadonlySet<string>): Set<string> {
	const out = new Set<string>();
	for (const id of hot) {
		const k = Number(id.slice(1));
		let c = -1;
		if (/^r\d+$/.test(id) && k < m.r.size) c = k;
		else if (/^x\d+$/.test(id) && k < m.x.size) c = m.group[k];
		if (c < 0) continue;
		out.add('r' + c);
		for (let j = 0; j < m.x.size; j++) if (m.group[j] === c) out.add('x' + j);
	}
	return out;
}

function cube(f: Frame, i: number, x: number, y: number, z: number, e: number): void {
	f.box(i, x - e / 2, x + e / 2, y - e / 2, y + e / 2, z - e / 2, z + e / 2);
}

/**
 * Writes one frame. Input bytes of the image are brightness (0 = background, 255 = full ink); numbers are flat tiles,
 * negatives hatched. Maxima flash once the collapse has started (every tying cell flashes: max has one value, however
 * many cells hold it).
 */
export function writeFrame(m: RModel, g: Geo, step: number, t: number, lit: ReadonlySet<string>, f: Frame): void {
	const { collapse, move } = phases(step, t);
	const n = m.x.size, P = g.input.pos, Q = g.output.pos;
	const u = ease(collapse), v = ease(move);
	for (let k = 0; k < n; k++) {
		const x = m.x.data[k];
		// The input record: fades as it is reduced.
		cube(f, k, P[3 * k], P[3 * k + 1], P[3 * k + 2], CELL);
		const flash = lit.has('x' + k) || (m.op === 2 && collapse > 0 && m.top[k] === 1);
		const a = 1 - (1 - GHOST) * u;
		if (g.image) f.look(k, 'ink', true, a * (0.06 + 0.94 * x / 255), 0, flash);
		else f.look(k, 'ink', false, a, x < 0 ? 1 : 0, flash);
		// The moving copy: slides to the landing cell, then merges (hidden before and after the collapse).
		const j = n + k, s = m.first[m.group[k]];
		const px = P[3 * k] + (P[3 * s] - P[3 * k]) * u, py = P[3 * k + 1] + (P[3 * s + 1] - P[3 * k + 1]) * u;
		cube(f, j, px, py, P[3 * k + 2], CELL);
		const keep = m.op !== 2 || m.top[k] === 1;
		const ma = collapse > 0 && collapse < 1 ? (keep ? 1 - clamp01((collapse - 0.8) / 0.2) : 1 - collapse) : 0;
		if (g.image) f.look(j, 'ink', true, ma * (0.06 + 0.94 * x / 255), 0, false);
		else f.look(j, 'ink', false, ma, x < 0 ? 1 : 0, false);
	}
	for (let c = 0; c < m.r.size; c++) {
		const i = 2 * n + c, s = m.first[c];
		const x = P[3 * s] + (Q[3 * c] - P[3 * s]) * v, y = P[3 * s + 1] + (Q[3 * c + 1] - P[3 * s + 1]) * v;
		cube(f, i, x, y, 0, RESULT);
		const a = clamp01((collapse - 0.75) / 0.25);
		f.look(i, 'vecC', false, a, m.r.data[c] < 0 ? 1 : 0, lit.has('r' + c));
	}
	f.n = 2 * n + m.r.size;
}
