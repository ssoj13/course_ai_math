/**
 * Home-view framing of a stage (Blender's View All): pure, three-free math used by Stage's navigation and unit-tested.
 * The figure's world bounds come from `contentBox` (view.tsx); these functions turn them into a camera.
 *
 * 2D (orthographic): the bounds fill the stage left of an open N-panel (`inset`), with a margin for labels.
 * 3D: the eight corners of the bounds are fitted in the camera's own frame for the home direction, so a flat or wide
 * figure fills the view (a bounding-sphere fit leaves it at ~30 % of the height).
 */

export type Vec3 = readonly [number, number, number];

/** Axis-aligned world bounds. */
export interface Bounds {
	min: Vec3;
	max: Vec3;
}

/** Free px around the 2D figure, so labels just outside its bounds (names over tips, tick numbers) are not cut. */
export const MARGIN_2D = 24;
/** The 2D XY corner widget (lab.css `.lab-xy`: 38 px at 8 px from the bottom-left) plus a 4 px gap. */
export const CORNER_2D = 50;
/** 3D: the figure's projected extent fills this share of the free view (labels and arrow heads need the rest). */
export const FILL_3D = 0.86;

/**
 * 2D framing: orthographic zoom (px per world unit) that fits `b` in a `width` x `height` stage left of an open
 * N-panel, with MARGIN_2D px around it, and the camera centre. The figure never runs under the XY corner widget: it
 * is kept either right of it or above it (CORNER_2D on that side), whichever leaves it larger. A figure framed
 * edge to edge put the triangle's vertex C, its angle ticks and DEMO-LC's ledger band under the widget.
 */
export function fit2d(b: Bounds, width: number, height: number, inset: number): { zoom: number; cx: number; cy: number } {
	const fw = Math.max(1, width - inset);
	const bw = Math.max(b.max[0] - b.min[0], 1e-6);
	const bh = Math.max(b.max[1] - b.min[1], 1e-6);
	const zoomFor = (left: number, bottom: number) => Math.max(1e-6, Math.min((fw - left - MARGIN_2D) / bw, (height - MARGIN_2D - bottom) / bh));
	const right = zoomFor(CORNER_2D, MARGIN_2D), up = zoomFor(MARGIN_2D, CORNER_2D);
	const [left, bottom, zoom] = right >= up ? [CORNER_2D, MARGIN_2D, right] : [MARGIN_2D, CORNER_2D, up];
	// The figure is centred in the free area between the margins: a wider left (bottom) margin moves it right (up),
	// i.e. the camera looks at a point left of (below) the box centre.
	return {
		zoom,
		cx: (b.min[0] + b.max[0]) / 2 - (left - MARGIN_2D) / (2 * zoom),
		cy: (b.min[1] + b.max[1]) / 2 - (bottom - MARGIN_2D) / (2 * zoom),
	};
}

/** Smallest box holding both. */
export function union(a: Bounds, b: Bounds): Bounds {
	return {
		min: [Math.min(a.min[0], b.min[0]), Math.min(a.min[1], b.min[1]), Math.min(a.min[2], b.min[2])],
		max: [Math.max(a.max[0], b.max[0]), Math.max(a.max[1], b.max[1]), Math.max(a.max[2], b.max[2])],
	};
}

/**
 * 2D: the drawn figure keeps at least this share of the zoom that would fit it alone, whatever room for dragging a
 * scene asks for. A room sized for the whole drag range (a board of +-8 around a 3-unit arrow) left the figure in
 * 40-60 px of a phone stage.
 */
export const MIN_FILL_2D = 0.5;

/**
 * What the 2D home view frames: the drawn figure `fig` plus as much of the scene's optional drag `room` (Stage `Room`)
 * as the stage allows without the figure dropping below MIN_FILL_2D of its own fit. The room grows the box from the
 * figure toward the room's bounds evenly on every side (a linear blend of the bounds, bisected), so a room that
 * extends mostly to one side keeps that bias. Without a room, or when the whole room fits, the result is exact.
 * The same rule at every stage size: a desktop stage usually shows the whole room, a phone shows less of it, and the
 * reader who drags beyond it gets the view grown after the drag (`outside`).
 */
export function roomFrame(fig: Bounds, room: Bounds | null, width: number, height: number, inset: number): Bounds {
	if (!room) return fig;
	const all = union(fig, room);
	const need = MIN_FILL_2D * fit2d(fig, width, height, inset).zoom;
	if (fit2d(all, width, height, inset).zoom >= need) return all;
	const at = (t: number): Bounds => ({
		min: fig.min.map((x, i) => x + t * (all.min[i] - x)) as unknown as Vec3,
		max: fig.max.map((x, i) => x + t * (all.max[i] - x)) as unknown as Vec3,
	});
	// fit2d's zoom falls monotonically as the box grows: bisect the largest blend that still keeps `need`.
	let lo = 0, hi = 1;
	for (let k = 0; k < 24; k++) {
		const mid = (lo + hi) / 2;
		if (fit2d(at(mid), width, height, inset).zoom >= need) lo = mid;
		else hi = mid;
	}
	return at(lo);
}

/**
 * Whether the figure `fig` now reaches outside the framed box `frame` by more than 1 % of the frame's size: the home
 * view must grow to keep showing it (a handle dragged past the edge of a figure-fitted view).
 */
export function outside(frame: Bounds, fig: Bounds): boolean {
	// One tolerance from the largest side: a flat 2D frame has no z extent, so a per-axis one would be zero there.
	const tol = 0.01 * Math.max(frame.max[0] - frame.min[0], frame.max[1] - frame.min[1], frame.max[2] - frame.min[2], 1e-9);
	for (let i = 0; i < 3; i++) {
		if (fig.min[i] < frame.min[i] - tol || fig.max[i] > frame.max[i] + tol) return true;
	}
	return false;
}

const dot = (a: Vec3, b: Vec3) =>a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vec3, b: Vec3): Vec3 => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const unit = (a: Vec3): Vec3 => {
	const n = Math.hypot(...a) || 1;
	return [a[0] / n, a[1] / n, a[2] / n];
};

/**
 * One drawn object's world bounds for the 3D fit: its box, and its bounding sphere when it has geometry. Each one
 * encloses the object, so the fit may use whichever needs less room per object: a sphere's box corners project up to
 * ~1.6 r from its centre in the home direction, which left DP-4's lit sphere at a third of the view height, while a
 * flat quad or a thin beam is enclosed much more tightly by its box.
 */
export interface Part {
	box: Bounds;
	sphere?: { c: Vec3; r: number };
}

/**
 * 3D framing of `b` seen from direction `dir` (target -> camera, y-up world), in a free view of `w` x `h` px with a
 * vertical half-fov tangent `tan`. Returns the target, the perspective distance and the orthographic zoom that make
 * every object visible within FILL_3D of the view. `parts` (the objects inside `b`) refine the fit; without them the
 * box itself is the one object. `corner` (px) is the navigation gizmo's square in the top-right corner: the figure
 * is fitted either left of it or below it, whichever leaves it larger, and the target moves so the figure is centred
 * in that area (the gizmo used to sit on DP-3's arrow tip and on the lamp's label).
 */
export function fit3d(b: Bounds, dir: Vec3, w: number, h: number, tan: number, parts?: readonly Part[], corner = 0): { target: Vec3; dist: number; zoom: number } {
	const d = unit(dir);
	const right = unit(cross([0, 1, 0], d));
	const up = cross(d, right);
	const c: Vec3 = [(b.min[0] + b.max[0]) / 2, (b.min[1] + b.max[1]) / 2, (b.min[2] + b.max[2]) / 2];
	const proj = (p: Vec3) => { const rel: Vec3 = [p[0] - c[0], p[1] - c[1], p[2] - c[2]]; return [Math.abs(dot(rel, right)), Math.abs(dot(rel, up)), dot(rel, d)] as const; };
	/** Distance and zoom that fit every object into a `fw` x `fh` px area centred on the target. */
	const solve = (fw: number, fh: number) => {
		const tanV = tan * FILL_3D;
		const tanH = tan * (Math.max(1, fw) / Math.max(1, fh)) * FILL_3D;
		/** Distance and half extents (camera frame) one enclosure needs; a point nearer the camera (z > 0) needs more distance. */
		const ofBox = (q: Bounds) => {
			let dist = 1e-6, hx = 1e-6, hy = 1e-6;
			for (let i = 0; i < 8; i++) {
				const [x, y, z] = proj([i & 1 ? q.max[0] : q.min[0], i & 2 ? q.max[1] : q.min[1], i & 4 ? q.max[2] : q.min[2]]);
				dist = Math.max(dist, z + x / tanH, z + y / tanV);
				hx = Math.max(hx, x);
				hy = Math.max(hy, y);
			}
			return { dist, hx, hy };
		};
		// Over a sphere, the linear bound z + x / tan peaks at the centre's value plus r * sqrt(1 + 1 / tan²).
		const ofSphere = (q: { c: Vec3; r: number }) => {
			const [x, y, z] = proj(q.c);
			return { dist: Math.max(z + x / tanH + q.r * Math.hypot(1, 1 / tanH), z + y / tanV + q.r * Math.hypot(1, 1 / tanV)), hx: x + q.r, hy: y + q.r };
		};
		let dist = 1e-6, hx = 1e-6, hy = 1e-6;
		for (const p of parts?.length ? parts : [{ box: b }]) {
			const a = ofBox(p.box), q = p.sphere ? ofSphere(p.sphere) : a;
			dist = Math.max(dist, Math.min(a.dist, q.dist));
			hx = Math.max(hx, Math.min(a.hx, q.hx));
			hy = Math.max(hy, Math.min(a.hy, q.hy));
		}
		return { dist, zoom: Math.min((Math.max(1, fw) * FILL_3D) / (2 * hx), (Math.max(1, fh) * FILL_3D) / (2 * hy)) };
	};
	if (!(corner > 0)) return { target: c, ...solve(w, h) };
	const left = solve(w - corner, h), below = solve(w, h - corner);
	const pick = left.zoom >= below.zoom ? left : below, axis = pick === left ? right : up;
	// Half the corner in world units at the target (ortho exactly, perspective at the target depth to first order).
	const k = corner / 2 / pick.zoom;
	return { target: [c[0] + axis[0] * k, c[1] + axis[1] * k, c[2] + axis[2] * k], ...pick };
}

/**
 * Step-revealed figures (squares appearing at a timeline step, a rectangle growing in) change the figure after the
 * first-view settling window, so the home view must be measured again when a timeline step changes. This returns how
 * long to wait before measuring (the step's polygon tweens take `springMs` per phase), or null when `next` is not a
 * new step of any timeline (history `:max` and `:phases` bookkeeping alone does not count). Reduced motion: at once.
 */
export function stepRefitDelay(prev: Readonly<Record<string, number>>, next: Readonly<Record<string, number>>, springMs: number, reduced: boolean): number | null {
	let phases = 0;
	for (const k of Object.keys(next)) {
		if (k.includes(':') || next[k] === prev[k]) continue;
		phases = Math.max(phases, next[k + ':phases'] ?? 1);
	}
	if (!phases) return null;
	return reduced ? 0 : springMs * phases + 60;
}
