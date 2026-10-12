/**
 * Lesson 3.4 choreography as pure functions (three-free; the tests check exactly what the renderer draws): where the
 * cells of A, B and C = A op B sit, which of them are real memory and which are stride-0 repeats, and how the
 * `bc` timeline moves them. Drawn with the DotUnit's instanced `Boxes` renderer (one draw call for all cells).
 *
 * Timeline `bc`: 0 = the operands as stored (only their real elements), 1 = size-1 and missing axes stretch (the
 * repeats slide out of the element they repeat, drawn faint: no memory is written for them), 2 = the elementwise op
 * fills C cell by cell. Scene 3.4-2 (tint) has no timeline and is drawn at the end state.
 */
import { Frame, type BoxRole } from '../dotunit/choreo';
import type { BModel, Operand } from './broadcasting.math';
import { flatDirs, IMAGE_DIRS, place, TILE_DEPTH, unravel, type Placed } from './tensor-ops';

/** How the cells look: numbered tiles, or an RGB picture (bytes as brightness of the channel colour). */
export type Look = 'num' | 'image';

/** Frame capacity: three panels of at most 4^3 cells (the tint scene needs 3 x 48). */
export const CAP = 3 * 64;
/** Edge of a cell box (pitch 1). */
export const CELL = 0.8;
/** Empty space between the three panels (room for the operator sign). */
export const PANEL_GAP = 2;
/**
 * Opacity of the empty cells of C before the operation fills them: they show where the result will be, and keep the
 * figure's bounds (which the home view frames) the same at every step.
 */
export const SLOT = 0.08;
/** Opacity of a stride-0 repeat: it is seen, not stored. */
export const REPEAT = 0.35;
/** Largest tint, for the brightness of B's cells in the image look. */
export const TINT_MAX = 1.5;

const CHANNEL: readonly BoxRole[] = ['axisX', 'axisY', 'axisZ'];

export interface Geo {
	ok: boolean;
	/** Panels in draw order: A, B and (when the shapes broadcast) C. */
	panels: Placed[];
	/** Instances: A cells, then B cells, then C cells (each panel has as many as its drawn shape). */
	n: number;
	/** First instance of each panel. */
	base: number[];
	/** Total width and height of the figure (world units). */
	w: number;
	h: number;
}

/**
 * Panels: with a result, A and B are drawn INSIDE the result's shape (that is what broadcasting means: every operand
 * is read as if it had the result's shape), so the three panels are equal. Without one, each operand in its own shape.
 */
export function geo(m: BModel, look: Look): Geo {
	const shapes = m.ok ? [m.out!, m.out!, m.out!] : [m.sa, m.sb];
	const panels: Placed[] = [];
	let x = 0;
	for (const s of shapes) {
		const p = place(s, look === 'image' ? IMAGE_DIRS : flatDirs(s.length), [x, 0]);
		panels.push(p);
		x += p.w + PANEL_GAP;
	}
	const base: number[] = [];
	let n = 0;
	for (const p of panels) {
		base.push(n);
		n += p.pos.length / 3;
	}
	return { ok: m.ok, panels, n, base, w: x - PANEL_GAP, h: Math.max(...panels.map((p) => p.h)) };
}

/** Term ids of the instances: `a<k>`, `b<k>`, `c<k>` with k the cell's row-major position in its panel's shape. */
export function writeIds(g: Geo, f: Frame): void {
	const pre = ['a', 'b', 'c'];
	g.panels.forEach((_, p) => {
		const cnt = g.panels[p].pos.length / 3;
		for (let k = 0; k < cnt; k++) f.id[g.base[p] + k] = pre[p] + k;
	});
}

/** Progress of the two phases from the timeline step and the progress through it. */
export function phases(step: number, t: number): { stretch: number; fill: number } {
	return { stretch: step >= 2 ? 1 : step === 1 ? t : 0, fill: step >= 2 ? (step === 2 ? t : 1) : 0 };
}

/** Result cells to light with the hovered id: the cell, the two cells it combines, and the real elements behind them. */
export function linked(m: BModel, hot: ReadonlySet<string>): Set<string> {
	const out = new Set<string>();
	if (!m.ok) return out;
	for (const id of hot) {
		const k = Number(id.slice(1));
		if (!/^[abc]\d+$/.test(id) || !(k < m.C!.size)) continue;
		out.add('a' + k).add('b' + k).add('c' + k);
		out.add('a' + m.opA!.from[k]).add('b' + m.opB!.from[k]);
	}
	return out;
}

function box(f: Frame, i: number, pos: ArrayLike<number>, k: number, from: number, u: number): void {
	const h = CELL / 2;
	const x = pos[3 * from] + (pos[3 * k] - pos[3 * from]) * u;
	const y = pos[3 * from + 1] + (pos[3 * k + 1] - pos[3 * from + 1]) * u;
	const z = pos[3 * k + 2];
	f.box(i, x - h, x + h, y - h, y + h, z - TILE_DEPTH / 2, z + TILE_DEPTH / 2);
}

/**
 * Writes one frame. `lit` = ids flashed orange (the linked cells of the hovered one). Values set the look: in the
 * image look a byte is the brightness of its channel colour (0 = background, 255 = full colour), a tint is its share of
 * TINT_MAX; in the number look negative results are hatched.
 */
export function writeFrame(m: BModel, g: Geo, look: Look, step: number, t: number, lit: ReadonlySet<string>, f: Frame): void {
	const { stretch, fill } = phases(step, t);
	const ops: (Operand | null)[] = [m.opA, m.opB];
	const vals = [m.A, m.B, m.C];
	const roles: BoxRole[] = ['vecA', 'vecB', 'vecC'];
	g.panels.forEach((P, p) => {
		const cnt = P.pos.length / 3;
		const shape = m.ok ? m.out! : p === 0 ? m.sa : m.sb;
		for (let k = 0; k < cnt; k++) {
			const i = g.base[p] + k;
			const o = p < 2 ? ops[p] : null;
			// Value of this cell: the operand's element it reads (through the stride-0 view), or the result.
			const v = o ? o.t.data[o.t.offset + o.src[k]] : vals[p]!.data[k];
			const real = o ? o.real[k] === 1 : true;
			let alpha: number;
			if (p === 2) alpha = Math.max(SLOT, fill);
			else if (!m.ok || real) alpha = 1;
			else alpha = REPEAT * stretch;
			box(f, i, P.pos, k, o && !real ? o.from[k] : k, real ? 1 : stretch);
			const flash = lit.has(f.id[i]);
			if (look === 'image') {
				const c = unravel(shape, k)[shape.length - 1];
				const bright = p === 1 ? v / TINT_MAX : v / 255;
				const a = Math.max(0, Math.min(1, bright));
				f.look(i, CHANNEL[c], true, alpha * (0.06 + 0.94 * a), 0, flash);
			} else f.look(i, roles[p], false, alpha, v < 0 ? 1 : 0, flash);
		}
	});
	f.n = g.n;
}
