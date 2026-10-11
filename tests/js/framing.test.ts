import { expect, test } from 'bun:test';
import { CORNER_2D, fit2d, fit3d, FILL_3D, MARGIN_2D, stepRefitDelay, type Bounds } from '../../src/viz/scene/framing';
import { hintSegments } from '../../src/viz/scene/StatusBar';
import { separate, type Placed } from '../../src/viz/scene/handles';

const box = (x0: number, y0: number, x1: number, y1: number, z0 = 0, z1 = 0): Bounds => ({ min: [x0, y0, z0], max: [x1, y1, z1] });

test('2D: the figure fills the stage left of the N-panel, with a margin, centred on its box', () => {
	const b = box(-1, -3, 6, 5);
	const f = fit2d(b, 600, 400, 0);
	// Height-limited: 8 units into 400 - 2 margins.
	expect(f.zoom).toBeCloseTo((400 - 2 * MARGIN_2D) / 8, 9);
	// Width has room to spare, so the figure keeps clear of the XY corner widget by moving right (camera left).
	expect(f.cx).toBeCloseTo(2.5 - (CORNER_2D - MARGIN_2D) / (2 * f.zoom), 9);
	expect(f.cy).toBe(1);
	// An open N-panel narrows the free width: width-limited now, so the figure moves up over the widget instead.
	const open = fit2d(b, 600, 400, 300);
	expect(open.zoom).toBeCloseTo((300 - 2 * MARGIN_2D) / 7, 9);
	expect(open.cx).toBe(2.5);
	expect(open.cy).toBeCloseTo(1 - (CORNER_2D - MARGIN_2D) / (2 * open.zoom), 9);
	// Neither way: the left edge of the figure is right of the widget, or its bottom edge above it.
	for (const g of [f, open]) {
		const fw = g === f ? 600 : 300;
		const left = fw / 2 + (b.min[0] - g.cx) * g.zoom, bottom = 400 / 2 - (b.min[1] - g.cy) * g.zoom;
		expect(left >= CORNER_2D - 1e-9 || bottom <= 400 - CORNER_2D + 1e-9).toBe(true);
	}
});

test('3D: every corner of the bounds projects inside the view; a flat figure is framed by its box, not a sphere', () => {
	const tan = Math.tan((40 * Math.PI) / 360);
	const dir = [1.6, 1.2, 1.8] as const;
	const n = Math.hypot(...dir);
	const d = [dir[0] / n, dir[1] / n, dir[2] / n] as const;
	const flat = box(-3, 0, 3, 0.3, -1, 1);
	const f = fit3d(flat, d, 800, 500, tan);
	const right = [d[2], 0, -d[0]].map((v) => v / Math.hypot(d[2], d[0]));
	const up = [d[1] * right[2] - d[2] * right[1], d[2] * right[0] - d[0] * right[2], d[0] * right[1] - d[1] * right[0]];
	let tight = 0;
	for (let i = 0; i < 8; i++) {
		const p = [i & 1 ? 3 : -3, i & 2 ? 0.3 : 0, i & 4 ? 1 : -1];
		const rel = p.map((v, k) => v - f.target[k]);
		const depth = f.dist - (rel[0] * d[0] + rel[1] * d[1] + rel[2] * d[2]);
		const sx = Math.abs(rel[0] * right[0] + rel[1] * right[1] + rel[2] * right[2]) / depth / (tan * (800 / 500));
		const sy = Math.abs(rel[0] * up[0] + rel[1] * up[1] + rel[2] * up[2]) / depth / tan;
		expect(Math.max(sx, sy)).toBeLessThanOrEqual(FILL_3D + 1e-9);
		tight = Math.max(tight, sx, sy);
	}
	// Some corner touches the fill limit: the figure is as large as allowed.
	expect(tight).toBeCloseTo(FILL_3D, 6);
	// A bounding-sphere fit of the same box would stand further away.
	const r = Math.hypot(3, 0.15, 1);
	expect(f.dist).toBeLessThan(r / Math.sin(Math.atan(tan)));
});

test('status hint segments: the N-panel segment is marked (hidden on phones)', () => {
	const s = hintSegments('Ctrl+Wheel zoom · MMB pan · [N] panel');
	expect(s.map((x) => x.panel)).toEqual([false, false, true]);
	expect(s[2].tokens.find((t) => t.key)?.text).toBe('N');
});

test('label separation: an overlapping label moves to the nearest free slot; ticks never move', () => {
	const el = {} as HTMLElement;
	const mk = (x: number, y: number, small = false): Placed => ({ el, x, y, w: 40, h: 12, small, hide: false });
	const list = [mk(100, 100), mk(105, 102), mk(300, 100, true)];
	separate(list);
	expect([list[0].x, list[0].y]).toEqual([100, 100]);
	// Moved down by one label height + gap, now clear of the first.
	expect(list[1].x).toBe(105);
	expect(list[1].y).toBe(102 + 12 + 2);
	expect([list[2].x, list[2].y]).toEqual([300, 100]);
});

test('label separation: tick numbers are fixed obstacles, also when registered after the label', () => {
	const el = {} as HTMLElement;
	const mk = (x: number, y: number, small = false): Placed => ({ el, x, y, w: 40, h: 12, small, hide: false });
	// An object label on the x-axis numbers (DP-2 "|b| cos θ") moves off them; the ticks stay where the axis put them.
	const list = [mk(100, 100), mk(90, 100, true), mk(115, 100, true)];
	separate(list);
	expect([list[0].x, list[0].y]).toEqual([100, 100 + 12 + 2]);
	expect([list[1].x, list[1].y, list[2].x, list[2].y]).toEqual([90, 100, 115, 100]);
});

test('a new timeline step re-measures the home view after its step motion; bookkeeping keys and reduced motion', () => {
	expect(stepRefitDelay({ derive: 0 }, { derive: 0, 'derive:max': 3 }, 180, false)).toBeNull();
	expect(stepRefitDelay({ derive: 0 }, { derive: 1, 'derive:max': 1, 'derive:phases': 1 }, 180, false)).toBe(240);
	expect(stepRefitDelay({ cut: 8 }, { cut: 9, 'cut:phases': 3 }, 180, false)).toBe(600);
	expect(stepRefitDelay({ derive: 7 }, { derive: 8 }, 180, true)).toBe(0);
	expect(stepRefitDelay({ a: 1 }, { a: 1 }, 180, false)).toBeNull();
});
test('3D: a round object is framed by its bounding sphere, not its box corners; a flat one keeps its box', () => {
	const tan = Math.tan((40 * Math.PI) / 360);
	const dir = [1.6, 1.2, 1.8] as const;
	const r = 1.5;
	const ball = box(-r, -r, r, r, -r, r);
	const byBox = fit3d(ball, dir, 600, 400, tan);
	const bySphere = fit3d(ball, dir, 600, 400, tan, [{ box: ball, sphere: { c: [0, 0, 0], r } }]);
	// Both enclose the ball; the sphere needs less room, so the ball is drawn larger (DP-4's lamp).
	expect(bySphere.dist).toBeLessThan(byBox.dist * 0.8);
	expect(bySphere.zoom).toBeGreaterThan(byBox.zoom * 1.25);
	// Ortho: the sphere's silhouette (radius r) fills exactly FILL_3D of the limiting side.
	expect(bySphere.zoom).toBeCloseTo((400 * FILL_3D) / (2 * r), 9);
	// A flat quad's sphere is looser than its box: the fit keeps the box.
	const flat = box(-3, 0, 3, 0, -1, 1);
	const quad = fit3d(flat, dir, 600, 400, tan, [{ box: flat, sphere: { c: [0, 0, 0], r: Math.hypot(3, 1) } }]);
	const plain = fit3d(flat, dir, 600, 400, tan);
	expect(quad.dist).toBeCloseTo(plain.dist, 9);
	expect(quad.zoom).toBeCloseTo(plain.zoom, 9);
});

test('label separation: of two tick numbers printed over each other the later one is hidden, and is no obstacle', () => {
	const el = {} as HTMLElement;
	const mk = (x: number, y: number, small = false): Placed => ({ el, x, y, w: 14, h: 10, small, hide: false });
	// x-axis "−1" below the axis and y-axis "−1" left of it, zoomed out: they touch near the origin.
	const list = [mk(-20, 11, true), mk(-11, 18, true), mk(-11, 26)];
	separate(list);
	expect(list.map((p) => p.hide)).toEqual([false, true, false]);
	expect([list[1].x, list[1].y]).toEqual([-11, 18]);
	// An object label over the hidden number only (not the shown one) stays where it is.
	expect([list[2].x, list[2].y]).toEqual([-11, 26]);
});

test('label separation: a label pushed off its anchor keeps moving the same way before trying other sides', () => {
	const el = {} as HTMLElement;
	const tick = (x: number, y: number): Placed => ({ el, x, y, w: 12, h: 10, small: true, hide: false });
	// Pushed down-right off a band (DP-2 shadow) onto the x-axis numbers: it goes further down-right, not up over the arrows.
	const label: Placed = { el, x: 100, y: 100, w: 80, h: 14, small: false, hide: false, ux: 0.6, uy: 0.8 };
	const list = [tick(90, 100), tick(130, 100), label];
	separate(list);
	expect(label.x).toBeGreaterThan(100);
	expect(label.y).toBeGreaterThan(100);
	const along = (0.6 * 80 + 0.8 * 14) / 2 + 2;
	expect(label.x).toBeCloseTo(100 + 0.6 * along, 9);
	expect(label.y).toBeCloseTo(100 + 0.8 * along, 9);
});

test('3D: the figure keeps clear of the gizmo corner, left of it or below it, centred in the area left', () => {
	const tan = Math.tan((40 * Math.PI) / 360);
	const dir = [1.6, 1.2, 1.8] as const;
	const b = box(-2, -1, 2, 1, -1, 1);
	const free = fit3d(b, dir, 600, 400, tan);
	const g = fit3d(b, dir, 600, 400, tan, undefined, 94);
	// Less room, so the figure is drawn a little smaller, never larger.
	expect(g.zoom).toBeLessThanOrEqual(free.zoom);
	expect(g.zoom).toBeCloseTo(Math.max(fit3d(b, dir, 506, 400, tan).zoom, fit3d(b, dir, 600, 306, tan).zoom), 9);
	// The target moved by half the corner in screen px (world = px / zoom) along the camera's right or up axis.
	const shift = Math.hypot(g.target[0], g.target[1], g.target[2]);
	expect(shift).toBeCloseTo(94 / 2 / g.zoom, 9);
});

test('label separation: the navigation widget is an obstacle labels move off', () => {
	const el = {} as HTMLElement;
	const label: Placed = { el, x: 300, y: 40, w: 40, h: 12, small: false, hide: false };
	separate([label], [{ x: 310, y: 45, w: 84, h: 84 }]);
	// Moved clear of the 84 px gizmo box centred at (310, 45).
	expect(Math.abs(label.x - 310) * 2 >= 40 + 84 + 4 || Math.abs(label.y - 45) * 2 >= 12 + 84 + 4).toBe(true);
});

test('label separation: a slot cutting the label at the stage edge is not free', () => {
	const el = {} as HTMLElement;
	// Pushed down-right off its anchor, past the bottom of a 300 x 200 stage: it moves back inside.
	const label: Placed = { el, x: 150, y: 196, w: 60, h: 12, small: false, hide: false, ux: 0, uy: 1 };
	separate([label], [], [300, 200]);
	expect(label.y + label.h / 2).toBeLessThanOrEqual(200);
	expect(label.y - label.h / 2).toBeGreaterThanOrEqual(0);
});
