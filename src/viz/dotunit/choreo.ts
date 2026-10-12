/**
 * Choreography of the DotUnit scenes as pure functions of (data, step, progress): where every box is, how far it has
 * faded in, whether it is hatched or flashing. Three-free and allocation-free per frame (a preallocated `Frame` is
 * rewritten), so the unit tests check exactly what the renderer draws, and the renderer (Boxes.tsx) only uploads it.
 *
 * Two timelines:
 * - `unit` (DotUnit, spec §2.4, steps 0-9): arrows, shadow, ruler; legs split out of the arrows; row and column; the
 *   column passes the row and tiles appear; the negative tile is hatched; tiles drop into the stack; the projection
 *   bar rises beside it to the same height; free.
 * - `pass` (any einsum preset, spec §3): operands; one shaft passes; all shafts pass; tiles drop into stacks; result.
 *
 * `t` is the progress (0..1) through the current step; a step's end state is t = 1, and every earlier step is complete.
 */
import { FACE, tileAt, type Axis } from './layout';
import type { DotData, SceneData } from './model';

/** Colour roles a box may use (index stored per instance; Boxes.tsx resolves them through the palette). */
export const ROLES = ['vecA', 'vecB', 'vecC', 'axisX', 'axisY', 'axisZ', 'ink', 'light', 'hot', 'muted'] as const;
export type BoxRole = (typeof ROLES)[number];
const R = Object.fromEntries(ROLES.map((r, i) => [r, i])) as Record<BoxRole, number>;
/** Leg colour of component i: the axis colours for x, y, z, then neutral ink. */
export const legRole = (i: number): BoxRole => (i === 0 ? 'axisX' : i === 1 ? 'axisY' : i === 2 ? 'axisZ' : 'ink');

/** Preallocated instance buffers: centre, size, role, fill share, hatch (0..1 wipe), alpha (fade in), flash (hot). */
export class Frame {
	readonly cap: number;
	n = 0;
	readonly c: Float32Array;
	readonly s: Float32Array;
	readonly role: Uint8Array;
	/** 1 = solid body (legs, bars), else a flat tile drawn at the engine's area fill with a full-colour edge. */
	readonly solid: Uint8Array;
	readonly hatch: Float32Array;
	readonly alpha: Float32Array;
	readonly flash: Uint8Array;
	/** Term id per instance (hover, hot); written when the data changes, not per frame. */
	readonly id: string[];
	constructor(cap: number) {
		this.cap = cap;
		this.c = new Float32Array(3 * cap);
		this.s = new Float32Array(3 * cap);
		this.role = new Uint8Array(cap);
		this.solid = new Uint8Array(cap);
		this.hatch = new Float32Array(cap);
		this.alpha = new Float32Array(cap);
		this.flash = new Uint8Array(cap);
		this.id = new Array<string>(cap).fill('');
	}
	/** Writes box `i` as the axis-aligned box [x0, x1] x [y0, y1] x [z0, z1] (any corner order). */
	box(i: number, x0: number, x1: number, y0: number, y1: number, z0: number, z1: number): void {
		this.c[3 * i] = (x0 + x1) / 2;
		this.c[3 * i + 1] = (y0 + y1) / 2;
		this.c[3 * i + 2] = (z0 + z1) / 2;
		this.s[3 * i] = Math.abs(x1 - x0);
		this.s[3 * i + 1] = Math.abs(y1 - y0);
		this.s[3 * i + 2] = Math.abs(z1 - z0);
	}
	/** Box `i` = lerp of two boxes given as centre/size arrays (morphs a tile into its stack segment). */
	lerp(i: number, ca: ArrayLike<number>, sa: ArrayLike<number>, cb: ArrayLike<number>, sb: ArrayLike<number>, u: number): void {
		for (let a = 0; a < 3; a++) {
			this.c[3 * i + a] = ca[a] + (cb[a] - ca[a]) * u;
			this.s[3 * i + a] = sa[a] + (sb[a] - sa[a]) * u;
		}
	}
	look(i: number, role: BoxRole, solid: boolean, alpha: number, hatch: number, flash: boolean): void {
		this.role[i] = R[role];
		this.solid[i] = solid ? 1 : 0;
		this.alpha[i] = alpha;
		this.hatch[i] = hatch;
		this.flash[i] = flash ? 1 : 0;
	}
}

const clamp01 = (x: number) => (x < 0 ? 0 : x > 1 ? 1 : x);
/** Smooth ease for authored motion (the springs of the engine are for value changes, not for scripted phases). */
const ease = (x: number) => {
	const u = clamp01(x);
	return u * u * (3 - 2 * u);
};

// ---------------------------------------------------------------------------------------------------------------
// DotUnit geometry (spec §2.3), static for given numbers.

/** Leg thickness, gap between slots, smallest slot (a zero component still gets a visible place). */
export const LEG = 0.14;
const GAP = 0.25;
const MIN_SLOT = 0.35;
/** Thickness of flat tiles and stack segments (they are rectangles in the xy plane). */
const FLAT = 0.02;

export interface DotGeom {
	n: number;
	/** Row slot start and width per component (legs of a along x at y = 0). */
	rowX: Float64Array;
	rowW: Float64Array;
	rowEnd: number;
	/** Column leg start (y) and slot height per component (legs of b standing at the column's x). */
	colY: Float64Array;
	colH: Float64Array;
	colX0: number;
	colX1: number;
	/** Column x at which pair i meets (the middle of a's slot i). */
	passX: Float64Array;
	stackX: number;
	barX: number;
	/** Running sums (stack segment ends) and the projection bar height |a|·shadow (0 when a = 0). */
	run: readonly number[];
	proj: number;
	d: DotData;
}

export function dotGeom(d: DotData): DotGeom {
	const n = d.a.length;
	const rowX = new Float64Array(n), rowW = new Float64Array(n), colY = new Float64Array(n), colH = new Float64Array(n), passX = new Float64Array(n);
	let x = 0, y = 0.5;
	for (let i = 0; i < n; i++) {
		rowX[i] = x;
		rowW[i] = Math.max(Math.abs(d.a[i]), MIN_SLOT);
		passX[i] = x + rowW[i] / 2;
		x += rowW[i] + GAP;
		colY[i] = y;
		colH[i] = Math.max(Math.abs(d.b[i]), MIN_SLOT);
		y += colH[i] + GAP;
	}
	const rowEnd = x - GAP;
	// Room left of the stack for its sum label (the column parks between the row and the stack).
	const stackX = rowEnd + 2.75;
	return { n, rowX, rowW, rowEnd, colY, colH, colX0: -1.5, colX1: rowEnd + 0.75, passX, stackX, barX: stackX + 1.6, run: d.run, proj: Number.isFinite(d.proj) ? d.proj : 0, d };
}

/** Instances of the DotUnit frame: n legs of a, n legs of b, n tiles, the projection bar, the "same height" tick. */
export const dotCap = (n: number) => 3 * n + 2;
export const DOT_BAR = (n: number) => 3 * n;
export const DOT_TICK = (n: number) => 3 * n + 1;

/** Duration (ms) of each step of the `unit` timeline (spec §2.4); step 0 is the engine's draw-in. */
export function unitMs(step: number, n: number): number {
	return [600, 250, 400, 400, 250 + 60 * n, 300 * n, 300, 300 * n, 250, 0][step] ?? 0;
}

/** Writes term ids of the DotUnit frame (legs a0…/b0…, tiles p0…, `proj`, `sum`). */
export function dotIds(n: number, f: Frame): void {
	for (let i = 0; i < n; i++) {
		f.id[i] = 'a' + i;
		f.id[n + i] = 'b' + i;
		f.id[2 * n + i] = 'p' + i;
	}
	f.id[DOT_BAR(n)] = 'proj';
	f.id[DOT_TICK(n)] = 'sum';
	f.n = dotCap(n);
}

/** Tile i of the DotUnit as a box: width |aᵢ| on a's slot, height |bᵢ| above the row (spec §2.3). */
function tileBox(g: DotGeom, i: number, c: number[], s: number[]): void {
	const w = Math.max(Math.abs(g.d.a[i]), 0.04), h = Math.max(Math.abs(g.d.b[i]), 0.04);
	c[0] = g.rowX[i] + w / 2;
	c[1] = LEG / 2 + h / 2;
	c[2] = 0;
	s[0] = w;
	s[1] = h;
	s[2] = FLAT;
}

/** Stack segment i: width 1 at stackX from S(i−1) to S(i); a negative product hangs from the top, narrower and in front. */
function segBox(g: DotGeom, i: number, c: number[], s: number[]): void {
	const lo = i ? g.run[i - 1] : 0, hi = g.run[i];
	const neg = g.d.prods[i] < 0;
	const w = neg ? 0.7 : 1;
	c[0] = g.stackX + 0.5;
	c[1] = (lo + hi) / 2;
	c[2] = neg ? 0.03 : 0;
	s[0] = w;
	s[1] = Math.max(Math.abs(hi - lo), 0.02);
	s[2] = FLAT;
}

const TC = [0, 0, 0], TS = [0, 0, 0], SC = [0, 0, 0], SS = [0, 0, 0], PA = [0, 0, 0], PB = [0, 0, 0];

/** Column x at this step/progress (it slides during step 5, spec §2.4). */
export function columnX(g: DotGeom, step: number, t: number): number {
	if (step < 5) return g.colX0;
	if (step > 5) return g.colX1;
	return g.colX0 + (g.colX1 - g.colX0) * ease(t);
}

/**
 * The DotUnit at (step, t). Face 1 (arrows, shadow, ruler) is drawn by engine primitives for steps 0-3; this frame
 * holds the face-2 boxes: legs (out of the arrows at step 3, into row/column at 4), tiles (5, hatched at 6), the drop
 * into the stack (7) and the projection bar with the tick (8).
 */
export function dotFrame(g: DotGeom, step: number, t: number, f: Frame, pairwise = false): void {
	const n = g.n, { a, b } = g.d;
	const colX = columnX(g, step, t);
	for (let v = 0; v < 2; v++) {
		const comp = v === 0 ? a : b;
		// Leg i of the 3D path (step 3): from P_i along axis i by comp[i]; only the first three components have an axis.
		PA[0] = PA[1] = PA[2] = 0;
		for (let i = 0; i < n; i++) {
			const k = v * n + i;
			const role = legRole(i);
			const neg = comp[i] < 0 ? 1 : 0;
			// End box: a's legs lie along x at y = 0; b's stand along y at the column's x.
			if (v === 0) {
				SC[0] = g.rowX[i] + g.rowW[i] / 2; SC[1] = 0; SC[2] = 0;
				SS[0] = Math.max(Math.abs(comp[i]), 0.04); SS[1] = LEG; SS[2] = LEG;
			} else {
				SC[0] = colX; SC[1] = g.colY[i] + g.colH[i] / 2; SC[2] = 0;
				SS[0] = LEG; SS[1] = Math.max(Math.abs(comp[i]), 0.04); SS[2] = LEG;
			}
			if (i < 3) {
				PB[0] = PA[0]; PB[1] = PA[1]; PB[2] = PA[2];
				PB[i] += comp[i];
				for (let ax = 0; ax < 3; ax++) {
					TC[ax] = (PA[ax] + PB[ax]) / 2;
					TS[ax] = ax === i ? Math.max(Math.abs(comp[i]), 0.04) : LEG;
				}
				PA[0] = PB[0]; PA[1] = PB[1]; PA[2] = PB[2];
			}
			if (step < 3) {
				f.box(k, 0, 0, 0, 0, 0, 0);
				f.look(k, role, true, 0, 0, false);
				continue;
			}
			if (step === 3) {
				if (i < 3) f.lerp(k, TC, TS, TC, TS, 0);
				else f.lerp(k, SC, SS, SC, SS, 0);
				f.look(k, role, true, i < 3 ? ease(t) : 0, neg, false);
				continue;
			}
			// Step 4: legs travel (staggered) from the arrow path to the row/column; legs beyond z fade in in place.
			const u = step === 4 ? ease(t * (1 + 0.25 * (n - 1)) - 0.25 * i) : 1;
			if (i < 3) f.lerp(k, TC, TS, SC, SS, u);
			else f.lerp(k, SC, SS, SC, SS, 0);
			const near = step === 5 && Math.abs(colX - g.passX[i]) < 0.3;
			f.look(k, role, true, i < 3 ? 1 : u, neg, near);
		}
	}
	for (let i = 0; i < n; i++) {
		const k = 2 * n + i;
		const neg = g.d.prods[i] < 0;
		tileBox(g, i, TC, TS);
		segBox(g, i, SC, SS);
		// Appear as the column passes (5), hatch wipe (6), drop one after another (7).
		const shown = step < 5 ? 0 : step > 5 ? 1 : clamp01((colX - g.passX[i]) / 0.4 + 0.5);
		// Drop order follows the printed summation order: one tile after another, or pairwise (0+1, 2+3, ...).
		const slots = pairwise ? Math.ceil(n / 2) : n, slot = pairwise ? i >> 1 : i;
		const drop = step < 7 ? 0 : step > 7 ? 1 : ease(t * slots - slot);
		f.lerp(k, TC, TS, SC, SS, drop);
		if (shown < 1) for (let ax = 0; ax < 2; ax++) f.s[3 * k + ax] *= shown;
		const hatch = !neg ? 0 : step < 6 ? 0 : step > 6 ? 1 : t;
		f.look(k, legRole(i), false, shown, hatch, step === 5 && Math.abs(colX - g.passX[i]) < 0.3);
	}
	// Projection bar: |a|·shadow in vecC beside the stack, rising at step 8; the tick joins both tops.
	const rise = step < 8 ? 0 : step > 8 ? 1 : ease(t);
	const top = g.proj * rise;
	f.box(DOT_BAR(n), g.barX, g.barX + 0.6, 0, top, 0, FLAT);
	f.look(DOT_BAR(n), 'vecC', false, step < 8 ? 0 : 1, g.proj < 0 ? 1 : 0, false);
	const sum = g.run[n - 1];
	// The tick joins the stack's end and the bar's top in the gap between them (both edges stay visible).
	f.box(DOT_TICK(n), g.stackX + 1.05, g.barX - 0.05, sum - 0.02, sum + 0.02, 0.04, 0.05);
	f.look(DOT_TICK(n), 'hot', true, step < 8 ? 0 : rise, 0, false);
	f.n = dotCap(n);
}

/**
 * The tile the animation is working on: the pair the column is passing (step 5) or the tile dropping into the stack
 * (step 7), else -1. The code view follows it (spec §2.1 CodeView 3 -> p{current}).
 */
export function dotCurrent(g: DotGeom, step: number, t: number, pairwise = false): number {
	if (step === 5) {
		const x = columnX(g, step, t);
		for (let i = 0; i < g.n; i++) if (Math.abs(x - g.passX[i]) < 0.3) return i;
		return -1;
	}
	if (step === 7 && t < 1) {
		const slots = pairwise ? Math.ceil(g.n / 2) : g.n, s = Math.min(slots - 1, Math.floor(t * slots));
		return pairwise ? 2 * s : s;
	}
	return -1;
}

/** Top of the stack (drawn) and of the projection bar: the "aha" pair of step 8 (spec §2.3, §7). */
export function dotTops(g: DotGeom): { stack: number; bar: number } {
	return { stack: g.run[g.n - 1], bar: g.proj };
}

// ---------------------------------------------------------------------------------------------------------------
// Generic einsum presets (`pass` timeline).

/** Steps of the `pass` timeline: operands, one shaft, all shafts, stacks, result. */
export const PASS_STEPS = 5;
export function passMs(step: number, sumLen: number): number {
	return [0, Math.min(1800, 300 * sumLen), 900, 900, 250][step] ?? 0;
}

/** Instance count of a scene: operand elements, then one box per tile (LOD 1) or per cell (bar / heat square). */
export function passCap(d: SceneData): number {
	const L = d.L;
	// LOD 2 adds one thin shaft line per cell, LOD 3 one sweeping plane (spec §1.4).
	return L.operands[0].n + L.operands[1].n + (L.lod === 1 ? L.cells.n * L.plan.sumLen : L.cells.n) + (L.lod === 2 ? L.cells.n : L.lod === 3 ? 1 : 0);
}

/** Term ids of a pass frame (static per data). */
export function passIds(d: SceneData, idOf: { elem(o: 0 | 1, e: number): string; cell(c: number): string; tile(c: number, k: number): string }, f: Frame): void {
	const L = d.L;
	let i = 0;
	for (const o of [0, 1] as const) for (let e = 0; e < L.operands[o].n; e++) f.id[i++] = idOf.elem(o, e);
	if (L.lod === 1) for (let c = 0; c < L.cells.n; c++) for (let k = 0; k < L.plan.sumLen; k++) f.id[i++] = idOf.tile(c, k);
	else for (let c = 0; c < L.cells.n; c++) f.id[i++] = idOf.cell(c);
	if (L.lod === 2) for (let c = 0; c < L.cells.n; c++) f.id[i++] = idOf.cell(c);
	if (L.lod === 3) f.id[i++] = 'sweep';
	f.n = i;
}

const SIDE = 0.9;
const THIN = 0.06;

/** Flat square of side `w` centred at p, facing axis `normal`. */
function square(f: Frame, i: number, p: ArrayLike<number>, w: number, normal: Axis): void {
	const h = w / 2, t = THIN / 2;
	f.box(i, p[0] - (normal === 0 ? t : h), p[0] + (normal === 0 ? t : h), p[1] - (normal === 1 ? t : h), p[1] + (normal === 1 ? t : h), p[2] - (normal === 2 ? t : h), p[2] + (normal === 2 ? t : h));
}

const LO = [0, 0, 0], HI = [0, 0, 0];
/** [lo, hi] of a box side: the segment's own extent along the shaft axis, a thin w across it. */
const span = (a: number, b: number, along: boolean, w: number): [number, number] => (along ? [Math.min(a, b) - 0.5, Math.max(a, b) + 0.5] : [(a + b) / 2 - w / 2, (a + b) / 2 + w / 2]);

/**
 * Stack segment of cell c from value lo to hi along the stack direction, cross-section `w`. A negative product's
 * segment overlaps the one below it (it pulls the running sum down), so it is drawn beside the stack (`side`), not
 * hidden inside it.
 */
function segment(f: Frame, i: number, d: SceneData, c: number, lo: number, hi: number, w: number, side: boolean): void {
	const L = d.L, s = L.axes.stackDir, p = L.cells.pos, q = s === 0 ? 1 : 0;
	const a = p[3 * c + s] + lo * d.hs, b = p[3 * c + s] + hi * d.hs;
	for (let ax = 0; ax < 3; ax++) {
		const mid = p[3 * c + ax] + (side && ax === q ? 0.45 : 0);
		const half = side && ax === q ? 0.18 : w / 2;
		LO[ax] = ax === s ? Math.min(a, b) : mid - half;
		HI[ax] = ax === s ? Math.max(Math.max(a, b), Math.min(a, b) + 0.02) : mid + half;
	}
	f.box(i, LO[0], HI[0], LO[1], HI[1], LO[2], HI[2]);
}
const P = [0, 0, 0];

/**
 * A generic preset at (step, t). `focus` = the cell whose shaft passes first (step 1) and stays lit. LOD 1 draws every
 * tile and its drop into a stack of segments; LOD 2 one bar per cell; LOD 3 one heat square per cell (spec §1.4).
 */
export function passFrame(d: SceneData, step: number, t: number, focus: number, f: Frame): void {
	const L = d.L, n = L.plan.sumLen;
	// A picture with nothing on z (dot, matrix x vector) is read from the front: its flat squares face the viewer.
	const planar = L.axes.onAxis[2] === null && L.axes.shaft !== 2 && L.axes.stackDir !== 2;
	const normalOut: Axis = planar ? 2 : L.axes.shaft;
	let i = 0;
	for (const o of [0, 1] as const) {
		const op = L.operands[o];
		for (let e = 0; e < op.n; e++, i++) {
			square(f, i, op.pos.subarray(3 * e, 3 * e + 3), SIDE, planar ? 2 : op.normal);
			f.look(i, o === 0 ? 'vecA' : 'vecB', false, 1, d.vals[o][e] < 0 ? 1 : 0, false);
		}
	}
	if (L.lod === 1) {
		const prods = d.prods!, run = d.run!;
		for (let c = 0; c < L.cells.n; c++) {
			for (let k = 0; k < n; k++, i++) {
				const p = prods[c * n + k];
				const side = Math.max(0.12, SIDE * Math.sqrt(d.maxAbsP > 0 ? Math.abs(p) / d.maxAbsP : 0));
				tileAt(L, c, k, P);
				square(f, i, P, side, normalOut);
				TC[0] = f.c[3 * i]; TC[1] = f.c[3 * i + 1]; TC[2] = f.c[3 * i + 2];
				TS[0] = f.s[3 * i]; TS[1] = f.s[3 * i + 1]; TS[2] = f.s[3 * i + 2];
				segment(f, i, d, c, k ? run[c * n + k - 1] : 0, run[c * n + k], 0.8, p < 0);
				SC[0] = f.c[3 * i]; SC[1] = f.c[3 * i + 1]; SC[2] = f.c[3 * i + 2];
				SS[0] = f.s[3 * i]; SS[1] = f.s[3 * i + 1]; SS[2] = f.s[3 * i + 2];
				// Appear along the shaft (focus cell at step 1, every cell at step 2), then drop (3).
				const order = clamp01(t * n - k);
				const shown = step < 1 ? 0 : step === 1 ? (c === focus ? order : 0) : step === 2 && c !== focus ? order : 1;
				const drop = step < 3 ? 0 : step > 3 ? 1 : ease(t * (n + 1) / n - k / n);
				f.lerp(i, TC, TS, SC, SS, drop);
				f.look(i, 'vecC', drop >= 1, shown, p < 0 ? 1 : 0, (step === 1 || step === 2) && c === focus && shown > 0 && shown < 1);
			}
		}
	} else {
		for (let c = 0; c < L.cells.n; c++, i++) {
			const v = d.out[c];
			if (L.lod === 2) segment(f, i, d, c, 0, v, 0.8, false);
			else {
				P[0] = L.cells.pos[3 * c]; P[1] = L.cells.pos[3 * c + 1]; P[2] = L.cells.pos[3 * c + 2];
				square(f, i, P, 1.1, normalOut);
			}
			const heat = L.lod === 3 ? (d.maxAbsOut > 0 ? 0.15 + 0.85 * Math.abs(v) / d.maxAbsOut : 0.15) : 1;
			f.look(i, v < 0 ? 'vecB' : 'vecC', L.lod === 2, step < 3 ? 0 : (step === 3 ? ease(t) : 1) * heat, v < 0 ? 1 : 0, false);
		}
		const s = L.axes.shaft, far = n - 1;
		if (L.lod === 2) {
			// One thin line per shaft while the shafts pass (the focus cell's alone at step 1, every cell at step 2).
			for (let c = 0; c < L.cells.n; c++, i++) {
				tileAt(L, c, 0, LO);
				tileAt(L, c, far, HI);
				const w = 0.06;
				f.box(i, ...span(LO[0], HI[0], s === 0, w), ...span(LO[1], HI[1], s === 1, w), ...span(LO[2], HI[2], s === 2, w));
				const on = step === 1 ? c === focus : step === 2;
				f.look(i, 'light', true, on ? 1 : 0, 0, step === 1 && c === focus);
			}
		} else {
			// A plane sweeping along the shaft from its far end to the output face (steps 1 and 2).
			tileAt(L, 0, far, P);
			const from = P[s], to = L.cells.pos[s];
			const at = from + (to - from) * (step === 1 || step === 2 ? ease(t) : 1);
			for (let a = 0; a < 3; a++) {
				LO[a] = a === s ? at - 0.02 : L.lo[a];
				HI[a] = a === s ? at + 0.02 : L.hi[a];
			}
			f.box(i, LO[0], HI[0], LO[1], HI[1], LO[2], HI[2]);
			f.look(i, 'light', false, step === 1 || step === 2 ? 1 : 0, 0, false);
			i++;
		}
	}
	f.n = i;
}

/** World anchor of cell c's label: just beyond its stack top (LOD 1/2) or on its heat square (LOD 3). */
export function cellTop(d: SceneData, c: number, out: number[]): number[] {
	const L = d.L, s = L.axes.stackDir;
	out[0] = L.cells.pos[3 * c];
	out[1] = L.cells.pos[3 * c + 1];
	out[2] = L.cells.pos[3 * c + 2];
	if (L.lod < 3) out[s] += d.out[c] * d.hs;
	return out;
}

/** Output face offset used by the layout (re-exported for scenes that place extra marks, e.g. the bias). */
export const OUT_FACE = FACE;
