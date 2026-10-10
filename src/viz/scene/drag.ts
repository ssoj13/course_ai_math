/**
 * Pure drag math for scene handles: pixel hit-test, grid snapping, keyboard steps, pixel sizing, drag-plane rays.
 * Pointer hit-test in screen pixels: only a hit may start a drag
 * (and lock touch scrolling); a miss returns null so the browser keeps scrolling the page.
 */
import { add, dot, scale, sub, type V } from '../geom';

export interface Handle {
	id: string;
	at: [number, number];
}

/** Id of the nearest handle within `radiusPx` (inclusive) of the pointer; the first wins an exact tie. */
export function hitHandle(pointerPx: [number, number], handlesPx: readonly Handle[], radiusPx = 14): string | null {
	let best: string | null = null;
	let bestD = Infinity;
	for (const h of handlesPx) {
		const d = Math.hypot(h.at[0] - pointerPx[0], h.at[1] - pointerPx[1]);
		if (d <= radiusPx && d < bestD) {
			best = h.id;
			bestD = d;
		}
	}
	return best;
}

/** Grid step for dragged and keyboard-moved scene values. */
export const SNAP = 0.1;

/** `x` rounded to a multiple of `step`, re-rounded to the step's decimals (no 0.30000000000000004) and never -0. */
export function snap(x: number, step = SNAP): number {
	const dec = Math.max(0, -Math.floor(Math.log10(step)));
	return Number((Math.round(x / step) * step).toFixed(dec)) + 0;
}

/** Strips float noise from a sum of grid values without moving values that were deliberately off-grid. */
const clean = (x: number): number => Number(x.toFixed(10)) + 0;

type Vals = Readonly<Record<string, number | readonly number[]>>;

/**
 * New values for `keys` after moving them together by `raw` (world units, e.g. pointer travel on the drag plane).
 * The first key is the anchor: it lands on the grid and every other key gets the same delta, so a 'whole' drag
 * translates a vector without reshaping it. Components of `raw` beyond the vector's dimension are ignored (a 3D ray
 * hit moving 2D data), missing ones count as 0. A missing or scalar key is an authoring error and throws.
 */
export function moved(vals: Vals, keys: readonly string[], raw: readonly number[], step = SNAP): Record<string, number[]> {
	const vec = (k: string): readonly number[] => {
		const v = vals[k];
		if (v === undefined || typeof v === 'number') throw new Error(`store key "${k}" is not a vector`);
		return v;
	};
	const a0 = vec(keys[0]);
	const d = a0.map((x, i) => snap(x + (raw[i] ?? 0), step) - x);
	return Object.fromEntries(
		keys.map((k) => {
			const v = vec(k);
			if (v.length !== d.length) throw new Error(`dimension mismatch: "${keys[0]}" has ${d.length}, "${k}" has ${v.length}`);
			return [k, v.map((x, i) => clean(x + d[i]))];
		}),
	);
}

// [axis, sign]. PageUp/PageDown drive z because a 2D keyboard has no third arrow pair.
const KEY_AXIS: Record<string, readonly [number, number]> = {
	ArrowRight: [0, 1],
	ArrowLeft: [0, -1],
	ArrowUp: [1, 1],
	ArrowDown: [1, -1],
	PageUp: [2, 1],
	PageDown: [2, -1],
};

/** Raw move for a key press on a `dims`-dimensional handle (one step, Shift = ten), or null if the key does not move it. */
export function keyDelta(key: string, shift: boolean, dims: number, step = SNAP): number[] | null {
	const a = KEY_AXIS[key];
	if (!a || a[0] >= dims) return null;
	const d = new Array<number>(dims).fill(0);
	d[a[0]] = a[1] * (shift ? 10 * step : step);
	return d;
}

/** Camera facts needed to size things in pixels: drei's View sizes an orthographic frustum in pixels, so 1/zoom suffices. */
export type CamPx = { ortho: true; zoom: number } | { ortho: false; fovDeg: number; depth: number };

/** World units covered by one screen pixel at view depth `depth` (perspective) or anywhere (orthographic). */
export function worldPerPx(c: CamPx, hPx: number): number {
	if (c.ortho) return 1 / c.zoom;
	return (2 * c.depth * Math.tan((c.fovDeg * Math.PI) / 360)) / hPx;
}

/** Point where the ray `o + t*d` (t >= 0) meets the plane through `p` with normal `n`; null if parallel or behind. */
export function rayPlane(o: V, d: V, p: V, n: V): number[] | null {
	const dn = dot(d, n);
	if (Math.abs(dn) < 1e-12) return null;
	const t = dot(sub(p, o), n) / dn;
	return t < 0 ? null : add(o, scale(d, t));
}

/**
 * Hit radius in CSS px for a pointer type. A fingertip covers ~10 mm and hides the handle it aims at, so touch and
 * pen get a larger radius than a mouse; the mouse keeps 14 px so neighbouring handles stay separately pickable.
 */
export function hitRadius(pointerType: string): number {
	return pointerType === 'mouse' ? 14 : 22;
}

/**
 * Normalized device coordinates (three's `Vector3.project`) to stage-relative CSS px, y down. Null outside the clip
 * volume: |z| > 1 is behind a perspective camera or past the far plane (projecting it would mirror the point), and
 * |x|, |y| > 1 is off the stage, where a label would overflow the article and a handle cannot be seen to be picked.
 */
export function ndcToPx(x: number, y: number, z: number, w: number, h: number): [number, number] | null {
	if (Math.abs(x) > 1 || Math.abs(y) > 1 || Math.abs(z) > 1) return null;
	return [((x + 1) / 2) * w, ((1 - y) / 2) * h];
}

/**
 * New center for something at `p` moved further from `from` along the line through both, so that it stays `gap` px
 * clear. With `box` ([w, h] of a centered label) the gap is measured from the box edge facing `from`: the push adds
 * the box's half-extent along the direction (|ux|*w + |uy|*h) / 2, so wide text clears an arc as well as short text.
 * Coincident points have no direction and stay put.
 */
export function pushAway(
	p: readonly [number, number],
	from: readonly [number, number],
	gap: number,
	box: readonly [number, number] = [0, 0],
): [number, number] {
	const dx = p[0] - from[0];
	const dy = p[1] - from[1];
	const l = Math.hypot(dx, dy);
	if (l === 0) return [p[0], p[1]];
	const ux = dx / l;
	const uy = dy / l;
	const d = gap + (Math.abs(ux) * box[0] + Math.abs(uy) * box[1]) / 2;
	return [p[0] + ux * d, p[1] + uy * d];
}
