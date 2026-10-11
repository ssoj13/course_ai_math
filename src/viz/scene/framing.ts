/**
 * Home-view framing of a stage (Blender's View All): pure, three-free math used by Stage's navigation and unit-tested.
 * The figure's world bounds come from `contentBox` (view.tsx); these functions turn them into a camera.
 *
 * 2D (orthographic): the bounds fill the free rectangle of the stage - minus an open N-panel (`inset`), the HUD
 * columns and formula plate (`pad`) and, with a holographic floor, the floor band under the figure.
 * 3D: the eight corners of the bounds are fitted in the camera's own frame for the home direction, so a flat or wide
 * figure fills the view (a bounding-sphere fit leaves it at ~30 % of the height).
 */

export type Vec3 = readonly [number, number, number];

/** Axis-aligned world bounds. */
export interface Bounds {
	min: Vec3;
	max: Vec3;
}

/** CSS px of the stage kept clear of the figure at the home view (HUD columns, formula plate). */
export interface FramePad {
	l: number;
	r: number;
	t: number;
	b: number;
}

/** Free px around the 2D figure, so labels just outside its bounds are not cut by the frame. */
export const MARGIN_2D = 16;
/**
 * 2D home view with a floor: the figure stands in the upper part of the stage and the floor (and the formula plate
 * over it) take this share of the height below it, as in the reference (figure ~60 %, floor band ~30 %).
 */
export const FLOOR_SHARE = 0.3;
/** 3D: the figure's projected extent fills this share of the free view (labels and arrow heads need the rest). */
export const FILL_3D = 0.86;

/**
 * 2D framing: orthographic zoom (px per world unit) and the camera centre that put `b` in the free rectangle of a
 * `width` x `height` stage. The camera looks at the centre of the area left of the N-panel (the view offset).
 */
export function fit2d(b: Bounds, width: number, height: number, inset: number, pad: FramePad, floor: boolean): { zoom: number; cx: number; cy: number } {
	const fw = Math.max(1, width - inset);
	const left = pad.l + MARGIN_2D;
	const right = fw - pad.r - MARGIN_2D;
	const top = pad.t + MARGIN_2D;
	const bottom = height - Math.max(pad.b, floor ? FLOOR_SHARE * height : 0) - MARGIN_2D;
	const bw = Math.max(b.max[0] - b.min[0], 1e-6);
	const bh = Math.max(b.max[1] - b.min[1], 1e-6);
	const zoom = Math.max(1e-6, Math.min((right - left) / bw, (bottom - top) / bh));
	// Screen x grows with world x, screen y against world y.
	const cx = (b.min[0] + b.max[0]) / 2 - ((left + right) / 2 - fw / 2) / zoom;
	const cy = (b.min[1] + b.max[1]) / 2 + ((top + bottom) / 2 - height / 2) / zoom;
	return { zoom, cx, cy };
}

const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const unit = (a: Vec3): Vec3 => {
	const n = Math.hypot(...a) || 1;
	return [a[0] / n, a[1] / n, a[2] / n];
};

/**
 * 3D framing of `b` seen from direction `dir` (target -> camera, y-up world), in a free view of `w` x `h` px with a
 * vertical half-fov tangent `tan`. Returns the target (bounds centre), the perspective distance and the orthographic
 * zoom that make every corner of the bounds visible within FILL_3D of the view.
 */
export function fit3d(b: Bounds, dir: Vec3, w: number, h: number, tan: number): { target: Vec3; dist: number; zoom: number } {
	const d = unit(dir);
	const right = unit(cross([0, 1, 0], d));
	const up = cross(d, right);
	const c: Vec3 = [(b.min[0] + b.max[0]) / 2, (b.min[1] + b.max[1]) / 2, (b.min[2] + b.max[2]) / 2];
	const tanV = tan * FILL_3D;
	const tanH = tan * (Math.max(1, w) / Math.max(1, h)) * FILL_3D;
	let dist = 1e-6;
	let hx = 1e-6;
	let hy = 1e-6;
	for (let i = 0; i < 8; i++) {
		const p: Vec3 = [i & 1 ? b.max[0] : b.min[0], i & 2 ? b.max[1] : b.min[1], i & 4 ? b.max[2] : b.min[2]];
		const rel: Vec3 = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
		const x = Math.abs(dot(rel, right));
		const y = Math.abs(dot(rel, up));
		const z = dot(rel, d);
		// A corner nearer the camera (z > 0) needs more distance for the same lateral offset.
		dist = Math.max(dist, z + x / tanH, z + y / tanV);
		hx = Math.max(hx, x);
		hy = Math.max(hy, y);
	}
	const zoom = Math.min((Math.max(1, w) * FILL_3D) / (2 * hx), (Math.max(1, h) * FILL_3D) / (2 * hy));
	return { target: c, dist, zoom };
}
