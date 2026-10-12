/**
 * Pure, three-free math of `TileGrid` (docs/design/part0-1-storyboard.md §7.1): a box or a row of unit pieces clipped
 * exactly to a real-valued size, counted, mirrored for a negative side, and moved between two piece sets without a
 * pop. TileGrid.tsx renders what these functions write; tests check the same buffers.
 *
 * Shapes:
 * - box: pieces fill [0, sx] x [0, sy] x [0, sz] from `origin`. Every axis is either active (its exact size, even 0.1,
 *   counted) or explicitly thin (`thin[a]`, one display-only layer THIN thick, never counted). A negative active size
 *   needs `negative: 'mirror'`: the same pieces are laid out on the other side of `origin` (origin stays the anchor).
 * - row: `len` unit pieces along +x, `wrap` per band; bands advance toward -y in the xy plane and +z in the xz plane.
 *   `fill` (only for len <= 1) replaces the length by one body `fill` wide inside a unit outline (the outline is drawn,
 *   never counted).
 *
 * Slots: every piece has a slot number that is stable while the shape changes (box: i + 4j + 16k; row: n), so a
 * transition can match "the same piece" in the source and target states.
 */
import { fixed, int } from '../fmt';
import { omegaFor, SPRING_MS } from './motion';

export type Vec3 = readonly [number, number, number];
export type TileShape =
	| { kind: 'box'; size: Vec3; thin: readonly [boolean, boolean, boolean] }
	| { kind: 'row'; len: number; wrap: number };
export type TilePlane = 'xy' | 'xz';

export interface TileOpts {
	plane?: TilePlane;
	/** Low corner (for a mirrored axis: the anchor the pieces are mirrored through). 2D origins get z = 0. */
	origin?: readonly number[];
	/** Row only, len <= 1: width of the single body inside one unit outline. */
	fill?: number;
	negative?: 'mirror';
}

export interface TileCount {
	/** Unit pieces (every active side exactly 1). */
	whole: number;
	/** Measure (area / volume / length) of the clipped pieces: total - whole. */
	partial: number;
	/** Bodies drawn (whole + clipped). */
	pieces: number;
	/** Measure of the whole shape. */
	total: number;
}

/** World thickness of an explicitly thin (display-only) axis. */
export const THIN = 0.12;
/** Largest active box side: 4 x 4 x 4 = 64 bodies (powers lesson cube). */
export const BOX_MAX = 4;
export const WRAP_MAX = 16;
/** Instance capacity of one TileGrid (the 4^3 cube; rows are limited to the same capacity). */
export const MAX_TILES = 64;

const finiteAll = (xs: readonly number[]) => xs.every(Number.isFinite);

/** Throws on every input TileGrid cannot draw honestly; the message names the rule. */
export function checkTiles(shape: TileShape, opts: TileOpts = {}): void {
	const origin = opts.origin ?? [0, 0, 0];
	if (!finiteAll(origin) || origin.length < 2 || origin.length > 3) throw new Error('TileGrid: origin must be 2 or 3 finite numbers');
	if (shape.kind === 'box') {
		const { size, thin } = shape;
		if (size.length !== 3 || thin.length !== 3) throw new Error('TileGrid: a box needs three sizes and three thin flags');
		if (!finiteAll(size)) throw new Error('TileGrid: box size must be finite');
		if (thin.every(Boolean)) throw new Error('TileGrid: a box needs at least one active (counted) axis');
		if (opts.fill !== undefined) throw new Error('TileGrid: `fill` is for rows only');
		for (let a = 0; a < 3; a++) {
			if (thin[a]) continue;
			if (Math.abs(size[a]) > BOX_MAX) throw new Error(`TileGrid: active box side ${size[a]} exceeds ${BOX_MAX}`);
			if (size[a] < 0 && opts.negative !== 'mirror') throw new Error('TileGrid: a negative side needs negative: "mirror"');
		}
		return;
	}
	const { len, wrap } = shape;
	if (!Number.isFinite(len) || len < 0) throw new Error('TileGrid: row length must be finite and >= 0');
	if (!Number.isInteger(wrap) || wrap < 1 || wrap > WRAP_MAX) throw new Error(`TileGrid: wrap must be an integer in 1..${WRAP_MAX}`);
	if (Math.ceil(len) > MAX_TILES) throw new Error(`TileGrid: a row holds at most ${MAX_TILES} pieces`);
	if (opts.fill !== undefined) {
		if (!Number.isFinite(opts.fill) || opts.fill < 0 || opts.fill > 1) throw new Error('TileGrid: fill must be in [0, 1]');
		if (len > 1) throw new Error('TileGrid: fill needs a row of length <= 1');
	}
}

/** Whole / partial / pieces / total of a valid shape (§7.1 "Counting"); thin axes contribute a factor of 1. */
export function tileCount(shape: TileShape, opts: TileOpts = {}): TileCount {
	checkTiles(shape, opts);
	if (shape.kind === 'box') {
		let whole = 1;
		let pieces = 1;
		let total = 1;
		for (let a = 0; a < 3; a++) {
			if (shape.thin[a]) continue;
			const d = Math.abs(shape.size[a]);
			whole *= Math.floor(d);
			pieces *= Math.ceil(d);
			total *= d;
		}
		return { whole, partial: total - whole, pieces, total };
	}
	if (opts.fill !== undefined) {
		const f = opts.fill;
		const whole = f === 1 ? 1 : 0;
		return { whole, partial: f - whole, pieces: f > 0 ? 1 : 0, total: f };
	}
	const whole = Math.floor(shape.len);
	return { whole, partial: shape.len - whole, pieces: Math.ceil(shape.len), total: shape.len };
}

/** Plate text of a total: integer when every active size is an integer, else 2 decimals (§7.1 `label`). */
export function tileTotal(shape: TileShape, total: number, fill?: number): string {
	const sizes = shape.kind === 'box' ? shape.size.filter((_, a) => !shape.thin[a]) : [fill ?? shape.len];
	return sizes.every(Number.isInteger) ? int(total) : fixed(total, 2);
}

/** Piece boxes of one state, indexed by piece (not slot): `lo`/`hi` hold 3 numbers per piece. */
export interface TileBuf {
	n: number;
	lo: Float64Array;
	hi: Float64Array;
	slot: Int32Array;
	/** 1 = a whole unit piece, 0 = clipped (drawn in the accent colour). */
	whole: Uint8Array;
	/** Per-axis piece counts of the state (thin axis 1; row: [ceil(len), 1, 1]); used to find growing axes. */
	counts: [number, number, number];
	/** Row band of each piece (box: the j index), for the row-by-row draw-in. */
	band: Int32Array;
	/** A filled row's unit outline (lo/hi of the frame) is valid while `hasOutline`. */
	hasOutline: boolean;
	outline: { lo: [number, number, number]; hi: [number, number, number] };
}

export function tileBuf(): TileBuf {
	return {
		n: 0,
		lo: new Float64Array(3 * MAX_TILES),
		hi: new Float64Array(3 * MAX_TILES),
		slot: new Int32Array(MAX_TILES),
		whole: new Uint8Array(MAX_TILES),
		counts: [0, 0, 0],
		band: new Int32Array(MAX_TILES),
		hasOutline: false,
		outline: { lo: [0, 0, 0], hi: [0, 0, 0] },
	};
}

// Layout temporaries (layoutTiles runs per drag frame; one call at a time, so module-level is safe).
const O3 = [0, 0, 0];
const IDX = [0, 0, 0];
const LO = [0, 0, 0];
const HI = [0, 0, 0];

/** Box slot of piece (i, j, k): stable while sizes change (each index < BOX_MAX). */
export const boxSlot = (i: number, j: number, k: number): number => i + BOX_MAX * j + BOX_MAX * BOX_MAX * k;

/**
 * Lays the pieces of `shape` out into `out` (reused; nothing is allocated, so it can run on every drag frame) and
 * returns it. Interval of piece n on an active axis of size s from origin o: [o + n, o + min(n + 1, |s|)], mirrored
 * to [o - min(n + 1, |s|), o - n] for s < 0; a thin axis is [o, o + THIN].
 */
export function layoutTiles(shape: TileShape, opts: TileOpts, out: TileBuf): TileBuf {
	checkTiles(shape, opts);
	const o0 = opts.origin ?? [0, 0, 0];
	const ox = o0[0], oy = o0[1], oz = o0[2] ?? 0;
	out.n = 0;
	out.hasOutline = false;
	const put = (lo0: number, lo1: number, lo2: number, hi0: number, hi1: number, hi2: number, slot: number, whole: boolean, band: number) => {
		const n = out.n++;
		out.lo[3 * n] = lo0; out.lo[3 * n + 1] = lo1; out.lo[3 * n + 2] = lo2;
		out.hi[3 * n] = hi0; out.hi[3 * n + 1] = hi1; out.hi[3 * n + 2] = hi2;
		out.slot[n] = slot;
		out.whole[n] = whole ? 1 : 0;
		out.band[n] = band;
	};
	if (shape.kind === 'box') {
		const o = O3;
		o[0] = ox; o[1] = oy; o[2] = oz;
		const c = out.counts;
		for (let a = 0; a < 3; a++) c[a] = shape.thin[a] ? 1 : Math.ceil(Math.abs(shape.size[a]));
		const idx = IDX, lo = LO, hi = HI;
		// An active zero side is an empty box (ceil(0) = 0 pieces on that axis).
		for (let k = 0; k < c[2]; k++)
			for (let j = 0; j < c[1]; j++)
				for (let i = 0; i < c[0]; i++) {
					idx[0] = i; idx[1] = j; idx[2] = k;
					let whole = true;
					for (let a = 0; a < 3; a++) {
						if (shape.thin[a]) {
							lo[a] = o[a];
							hi[a] = o[a] + THIN;
							continue;
						}
						const s = shape.size[a];
						const d = Math.abs(s);
						const near = idx[a];
						const far = Math.min(idx[a] + 1, d);
						if (far - near !== 1) whole = false;
						// Mirrored: same distances from the anchor, on the negative side (low corner = o - far).
						lo[a] = s < 0 ? o[a] - far : o[a] + near;
						hi[a] = s < 0 ? o[a] - near : o[a] + far;
					}
					put(lo[0], lo[1], lo[2], hi[0], hi[1], hi[2], boxSlot(i, j, k), whole, j);
				}
		return out;
	}
	const xz = (opts.plane ?? 'xy') === 'xz';
	const width = opts.fill ?? shape.len;
	const pieces = Math.ceil(width);
	out.counts[0] = pieces;
	out.counts[1] = 1;
	out.counts[2] = 1;
	for (let n = 0; n < pieces; n++) {
		const col = n % shape.wrap;
		const band = Math.floor(n / shape.wrap);
		const size = Math.min(1, width - n);
		const lx = ox + col;
		if (xz) put(lx, oy, oz + band, lx + size, oy + THIN, oz + band + 1, n, size === 1, band);
		else put(lx, oy - band, oz, lx + size, oy - band + 1, oz + THIN, n, size === 1, band);
	}
	if (opts.fill !== undefined) {
		out.hasOutline = true;
		const { lo, hi } = out.outline;
		lo[0] = ox; lo[1] = oy; lo[2] = oz;
		hi[0] = ox + 1; hi[1] = oy + (xz ? THIN : 1); hi[2] = oz + (xz ? 1 : THIN);
	}
	return out;
}

/** Union bounds of the laid-out pieces into `lo`/`hi`; false when there are none. */
export function tileBounds(buf: TileBuf, lo: number[], hi: number[]): boolean {
	if (!buf.n) return false;
	for (let a = 0; a < 3; a++) {
		lo[a] = Infinity;
		hi[a] = -Infinity;
	}
	for (let n = 0; n < buf.n; n++)
		for (let a = 0; a < 3; a++) {
			lo[a] = Math.min(lo[a], buf.lo[3 * n + a]);
			hi[a] = Math.max(hi[a], buf.hi[3 * n + a]);
		}
	return true;
}

/**
 * Column-major instance matrix of a unit body centred at the origin, scaled to the box [lo, hi] stored as six numbers
 * at `box[src..src+6]` (the three.js layout of InstancedMesh.instanceMatrix), shrunk by `k` about its centre (the
 * draw-in): diagonal = k (hi - lo), translation = (lo + hi) / 2.
 */
export function boxMatrix(box: ArrayLike<number>, src: number, k: number, out: Float32Array, at: number): void {
	out.fill(0, at, at + 16);
	for (let a = 0; a < 3; a++) {
		out[at + 5 * a] = k * (box[src + 3 + a] - box[src + a]);
		out[at + 12 + a] = (box[src + a] + box[src + 3 + a]) / 2;
	}
	out[at + 15] = 1;
}

/**
 * Motion state of one TileGrid, indexed by slot: the drawn box of every live piece (6 numbers lo/hi), its spring
 * velocity, its target and whether it is alive (in the target), dying (only in the source: it shrinks to a stub, then
 * hides) or absent. `cut` moves every piece to its target at once (direct manipulation, reduced motion, no morph).
 */
export interface TileMotion {
	cur: Float64Array;
	vel: Float64Array;
	to: Float64Array;
	/** 0 absent, 1 alive (in the target), 2 dying (shrinking out). */
	state: Uint8Array;
	whole: Uint8Array;
	band: Int32Array;
	/** Per-axis piece counts of the previous target (to find the axes along which a piece is born or dies). */
	counts: [number, number, number];
}

export function tileMotion(): TileMotion {
	return {
		cur: new Float64Array(6 * MAX_TILES),
		vel: new Float64Array(6 * MAX_TILES),
		to: new Float64Array(6 * MAX_TILES),
		state: new Uint8Array(MAX_TILES),
		whole: new Uint8Array(MAX_TILES),
		band: new Int32Array(MAX_TILES),
		counts: [0, 0, 0],
	};
}

/** Index of slot `s` along each axis (box slots; a row slot is its x index). */
function slotIndex(s: number, row: boolean, out: number[]): void {
	if (row) {
		out[0] = s;
		out[1] = 0;
		out[2] = 0;
		return;
	}
	out[0] = s % BOX_MAX;
	out[1] = Math.floor(s / BOX_MAX) % BOX_MAX;
	out[2] = Math.floor(s / (BOX_MAX * BOX_MAX));
}

const idxTmp = [0, 0, 0];

/**
 * Collapses the box at `box[at..at+6]` (lo, hi) into `out` on every axis where its slot index lies beyond `counts`
 * (the axes it is born or dies along): the side keeps min(THIN, its size) at the end nearer the anchor, so a born piece
 * grows out of its neighbour and a dying one shrinks into it. Never larger than the exact piece (a 0.1 piece is not
 * enlarged to THIN).
 */
export function stubBox(box: ArrayLike<number>, at: number, slot: number, row: boolean, counts: readonly number[], origin: readonly number[], out: Float64Array, to: number): void {
	slotIndex(slot, row, idxTmp);
	let any = false;
	for (let a = 0; a < 3; a++) if (idxTmp[a] >= counts[a]) any = true;
	for (let a = 0; a < 3; a++) {
		const lo = box[at + a];
		const hi = box[at + 3 + a];
		// A slot missing from the other state only because another axis shrank: collapse along every grown axis;
		// with no axis beyond `counts` (a different shape kind) collapse along x.
		const grow = any ? idxTmp[a] >= counts[a] : a === 0;
		if (!grow) {
			out[to + a] = lo;
			out[to + 3 + a] = hi;
			continue;
		}
		const size = Math.min(THIN, hi - lo);
		// Mirrored pieces lie below the anchor: their near end is `hi`.
		const mirrored = hi <= (origin[a] ?? 0) + 1e-12 && lo < (origin[a] ?? 0);
		out[to + a] = mirrored ? hi - size : lo;
		out[to + 3 + a] = mirrored ? hi : lo + size;
	}
}

const OMEGA = omegaFor(SPRING_MS);
const EPS = 1e-4;

/**
 * Sets the motion's targets to `buf`. With `cut` every piece jumps to its target (the source is forgotten); without,
 * a piece new in the target starts as a stub of itself, a piece missing from the target shrinks to a stub and dies.
 * Counts (TileCount) always describe `buf`, the target truth, whatever is still on screen.
 */
const SEEN = new Uint8Array(MAX_TILES);

export function retarget(m: TileMotion, buf: TileBuf, row: boolean, origin: readonly number[], cut: boolean): void {
	const seen = SEEN;
	seen.fill(0);
	for (let n = 0; n < buf.n; n++) {
		const s = buf.slot[n];
		seen[s] = 1;
		for (let a = 0; a < 3; a++) {
			m.to[6 * s + a] = buf.lo[3 * n + a];
			m.to[6 * s + 3 + a] = buf.hi[3 * n + a];
		}
		m.whole[s] = buf.whole[n];
		m.band[s] = buf.band[n];
		if (cut || m.state[s] === 0) {
			if (cut) for (let c = 6 * s; c < 6 * s + 6; c++) m.cur[c] = m.to[c];
			else stubBox(m.to, 6 * s, s, row, m.counts, origin, m.cur, 6 * s);
			m.vel.fill(0, 6 * s, 6 * s + 6);
		}
		m.state[s] = 1;
	}
	for (let s = 0; s < MAX_TILES; s++) {
		if (seen[s] || m.state[s] === 0) continue;
		if (cut) {
			m.state[s] = 0;
			continue;
		}
		if (m.state[s] === 1) stubBox(m.cur, 6 * s, s, row, buf.counts, origin, m.to, 6 * s);
		m.state[s] = 2;
	}
	m.counts[0] = buf.counts[0];
	m.counts[1] = buf.counts[1];
	m.counts[2] = buf.counts[2];
}

/**
 * Advances every live or dying piece by `dtMs` with the shared critically damped spring (motion.ts `springStep`,
 * inlined so a 64-piece morph allocates nothing per frame); true while anything moves.
 */
export function stepMotion(m: TileMotion, dtMs: number): boolean {
	const dt = Math.max(0, dtMs) / 1000;
	const e = Math.exp(-OMEGA * dt);
	let moving = false;
	for (let s = 0; s < MAX_TILES; s++) {
		if (m.state[s] === 0) continue;
		let done = true;
		for (let c = 6 * s; c < 6 * s + 6; c++) {
			const c1 = m.cur[c] - m.to[c];
			const c2 = m.vel[c] + OMEGA * c1;
			m.cur[c] = m.to[c] + (c1 + c2 * dt) * e;
			m.vel[c] = (c2 - OMEGA * (c1 + c2 * dt)) * e;
			if (Math.abs(m.cur[c] - m.to[c]) > EPS || Math.abs(m.vel[c]) > EPS) done = false;
		}
		if (done) {
			// Arrival lands exactly; a dying piece that reached its stub is hidden.
			for (let c = 6 * s; c < 6 * s + 6; c++) m.cur[c] = m.to[c];
			m.vel.fill(0, 6 * s, 6 * s + 6);
			if (m.state[s] === 2) m.state[s] = 0;
		} else moving = true;
	}
	return moving;
}

/** Share of the role colour in a body's fill (the rest is the viewport background): a muted, opaque 6d fill. */
export const FILL = 0.38;
/** Factor a dimmed (non-matching) piece keeps of its fill and edge. */
export const DIM = 0.35;

/**
 * Hover dim rule of §7.1: with the `whole` term lit (lit = 1) the clipped pieces dim, with `partial` lit (2) the whole
 * pieces dim, with both or neither nothing dims.
 */
export const dimmed = (whole: boolean, lit: number): boolean => (lit === 1 && !whole) || (lit === 2 && whole);

/** Share of the role colour in a piece's body (`fill`) or edge frame over the background. */
export const share = (part: 'fill' | 'edge', dim: boolean): number => (part === 'fill' ? FILL : 1) * (dim ? DIM : 1);

/**
 * sRGB mix a + (b - a) t of two '#rrggbb' colours, written raw into `out` at `at` (the tile shader outputs instance
 * colours unconverted, as the flat canvas renders palette hex exactly).
 */
export function mixHex(a: string, b: string, t: number, out: Float32Array, at: number): void {
	const pa = parseInt(a.slice(1), 16);
	const pb = parseInt(b.slice(1), 16);
	for (let c = 0; c < 3; c++) {
		const sh = 16 - 8 * c;
		out[at + c] = (((pa >> sh) & 255) * (1 - t) + ((pb >> sh) & 255) * t) / 255;
	}
}

/** Edge-frame vertices of one drawn piece: 4 edges in 2D (its face toward the camera), 12 in 3D; 2 vertices each. */
export const frameVerts = (flat: boolean): number => (flat ? 8 : 24);
