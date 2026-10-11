import { expect, test } from 'bun:test';
import { COMPACT_W, hudLayout, hudTicks } from '../../src/viz/scene/hud';

test('instrument columns scale with the stage: a full-screen view gets proportionally sized instruments', () => {
	const card = hudLayout(430, 400);
	const full = hudLayout(1280, 850);
	expect(full.left).toBeGreaterThan(card.left * 1.5);
	expect(full.panels[0].size / 850).toBeGreaterThan(0.08);
	// Both columns together leave most of the width to the figure.
	expect((card.left + card.right) / 430).toBeLessThan(0.36);
	expect((full.left + full.right) / 1280).toBeLessThan(0.24);
});

test('elements stay inside their columns and never overlap each other', () => {
	for (const [w, h, gizmo] of [[430, 400, false], [1280, 850, true], [900, 560, false]] as const) {
		const L = hudLayout(w, h, gizmo, !gizmo);
		const [pl, pr] = L.panels;
		expect(pl.x + pl.size).toBeLessThanOrEqual(L.left);
		expect(pr.x).toBeGreaterThanOrEqual(w - L.right);
		// Right column top to bottom: dial, panel, planet, no overlap.
		expect(L.dial.y + L.dial.size * 1.2).toBeLessThan(pr.y);
		expect(pr.y + pr.size).toBeLessThan(L.planet.y - L.planet.size * 2.05);
		// Left column: panel above the target ring.
		expect(pl.y + pl.size).toBeLessThan(L.target.y - L.target.size * 1.9);
	}
});

test('3D gizmo pushes the dial down; the 2D XY corner shortens the left ruler', () => {
	expect(hudLayout(800, 500, true).dial.y).toBeGreaterThan(hudLayout(800, 500, false).dial.y + 80);
	expect(hudLayout(800, 500, false, true).ruler.bottom).toBe(500 - 56);
});

test('phones use the compact arrangement in two slim columns', () => {
	const L = hudLayout(COMPACT_W - 1, 300);
	expect(L.compact).toBe(true);
	expect(L.left + L.right).toBeLessThanOrEqual(80);
	expect(hudLayout(COMPACT_W, 300).compact).toBe(false);
});

test('ruler ticks: fine subdivisions by unit spacing, labels thinned before they crowd', () => {
	const units = (gap: number) => [2, 1, 0].map((v, i) => ({ v, y: 100 + i * gap }));
	expect(hudTicks(units(40), 10).length).toBe(3 + 2 * 4);
	expect(hudTicks(units(20), 10).length).toBe(3 + 2);
	expect(hudTicks(units(10), 10).length).toBe(3);
	expect(hudTicks(units(40), 10).filter((t) => t.label).map((t) => t.label)).toEqual(['2', '1', '0']);
	expect(hudTicks(units(10), 10).filter((t) => t.label).map((t) => t.label)).toEqual(['2', '0']);
	expect(hudTicks([], 10)).toEqual([]);
});
