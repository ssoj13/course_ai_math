import { expect, test } from 'bun:test';
import { fit2d, fit3d, FILL_3D, FLOOR_SHARE, MARGIN_2D, type Bounds } from '../../src/viz/scene/framing';
import { hudLayout } from '../../src/viz/scene/hud';
import { hintSegments } from '../../src/viz/scene/StatusBar';
import { separate, type Placed } from '../../src/viz/scene/handles';

const NO_PAD = { l: 0, r: 0, t: 0, b: 0 };
const box = (x0: number, y0: number, x1: number, y1: number, z0 = 0, z1 = 0): Bounds => ({ min: [x0, y0, z0], max: [x1, y1, z1] });

test('2D: the figure fills the free rectangle above the floor band, between the pads', () => {
	const b = box(-1, -3, 6, 5);
	const f = fit2d(b, 600, 400, 0, { l: 60, r: 60, t: 0, b: 0 }, true);
	// Height-limited: 8 units into 400 - 0.3 * 400 - 2 margins.
	expect(f.zoom).toBeCloseTo((400 - FLOOR_SHARE * 400 - 2 * MARGIN_2D) / 8, 9);
	// Screen y of the box bottom = the bottom of the free rectangle.
	const y = (wy: number) => 200 - (wy - f.cy) * f.zoom;
	expect(y(-3)).toBeCloseTo(400 - FLOOR_SHARE * 400 - MARGIN_2D, 6);
	expect(y(5)).toBeCloseTo(MARGIN_2D, 6);
	// Horizontally centred between the pads.
	const x = (wx: number) => 300 + (wx - f.cx) * f.zoom;
	expect((x(-1) + x(6)) / 2).toBeCloseTo(300, 6);
});

test('2D: an open N-panel and a plate taller than the floor band shrink the rectangle', () => {
	const b = box(0, 0, 4, 4);
	const open = fit2d(b, 600, 400, 200, NO_PAD, false);
	expect(open.zoom).toBeCloseTo((400 - 2 * MARGIN_2D) / 4, 9);
	const plate = fit2d(b, 1200, 400, 0, { l: 0, r: 0, t: 0, b: 200 }, true);
	expect(plate.zoom).toBeCloseTo((400 - 200 - 2 * MARGIN_2D) / 4, 9);
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

test('HUD: the dial caption is part of the right column, above the galaxy panel, clear of the ruler ticks', () => {
	for (const [w, h, gizmo] of [[432, 395, true], [432, 395, false], [1080, 850, true], [328, 328, true]] as const) {
		const L = hudLayout(w, h, gizmo, !gizmo);
		const bottom = L.caption.top + L.caption.lines * 1.2 * L.caption.font;
		expect(bottom).toBeLessThanOrEqual(L.panels[1].y);
		expect(L.caption.right).toBeGreaterThanOrEqual(w - L.ruler.xr + L.ruler.major);
		// The planet's ring stays inside the right column (it never reaches the formula plate).
		expect(L.planet.x - 2.05 * L.planet.size).toBeGreaterThanOrEqual(w - L.right);
	}
});

test('status hint segments: the N-panel segment is marked (hidden on phones)', () => {
	const s = hintSegments('Ctrl+Wheel zoom · MMB pan · [N] panel');
	expect(s.map((x) => x.panel)).toEqual([false, false, true]);
	expect(s[2].tokens.find((t) => t.key)?.text).toBe('N');
});

test('label separation: an overlapping label moves to the nearest free slot; ticks never move', () => {
	const el = {} as HTMLElement;
	const mk = (x: number, y: number, small = false): Placed => ({ el, x, y, w: 40, h: 12, small });
	const list = [mk(100, 100), mk(105, 102), mk(100, 100, true)];
	separate(list);
	expect([list[0].x, list[0].y]).toEqual([100, 100]);
	// Moved down by one label height + gap, now clear of the first.
	expect(list[1].x).toBe(105);
	expect(list[1].y).toBe(102 + 12 + 2);
	expect([list[2].x, list[2].y]).toEqual([100, 100]);
});
