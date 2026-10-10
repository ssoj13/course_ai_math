import { expect, test } from 'bun:test';
import { hitHandle, keyDelta, moved, rayPlane, snap, worldPerPx } from '../../src/viz/scene/drag';
test('a miss returns null so the page can scroll', () => {
	expect(hitHandle([100, 100], [{ id: 'a', at: [200, 200] }])).toBeNull();
});
test('nearest handle within radius wins', () => {
	expect(hitHandle([100, 100], [{ id: 'a', at: [110, 100] }, { id: 'b', at: [104, 100] }])).toBe('b');
});
test('empty handle list is a miss', () => {
	expect(hitHandle([0, 0], [])).toBeNull();
});
test('radius is inclusive and configurable; exact ties keep the first handle', () => {
	expect(hitHandle([0, 0], [{ id: 'a', at: [14, 0] }])).toBe('a');
	expect(hitHandle([0, 0], [{ id: 'a', at: [15, 0] }])).toBeNull();
	expect(hitHandle([0, 0], [{ id: 'a', at: [15, 0] }], 20)).toBe('a');
	expect(hitHandle([0, 0], [{ id: 'a', at: [5, 0] }, { id: 'b', at: [-5, 0] }])).toBe('a');
});


test('snap lands on the 0.1 grid without float noise or -0', () => {
	expect(snap(0.1 + 0.2)).toBe(0.3);
	expect(snap(1.04)).toBe(1);
	expect(snap(1.06)).toBe(1.1);
	expect(Object.is(snap(-0.04), 0)).toBe(true);
	expect(snap(2.6, 1)).toBe(3);
});

test('tip drag: the dragged point snaps to the grid', () => {
	expect(moved({ a: [3, 1] }, ['a'], [0.26, -0.44])).toEqual({ a: [3.3, 0.6] });
});

test('whole drag translates every key by the anchor delta, so the vector keeps its shape', () => {
	const out = moved({ o: [0, 0], a: [3, 1] }, ['o', 'a'], [1.04, 0.96]);
	expect(out).toEqual({ o: [1, 1], a: [4, 2] });
});

test('extra raw components (3D ray hit on 2D data) are ignored; missing ones count as 0', () => {
	expect(moved({ a: [1, 1] }, ['a'], [0.5, 0, 9])).toEqual({ a: [1.5, 1] });
	expect(moved({ a: [1, 1, 1] }, ['a'], [0.5, 0.5])).toEqual({ a: [1.5, 1.5, 1] });
});

test('moving a missing or scalar key, or keys of different dimension, is a loud error', () => {
	expect(() => moved({}, ['a'], [1, 0])).toThrow(/a/);
	expect(() => moved({ a: 2 }, ['a'], [1, 0])).toThrow(/a/);
	expect(() => moved({ o: [0, 0], a: [1, 1, 1] }, ['o', 'a'], [1, 0])).toThrow(/dimension/);
});

test('keyboard steps: arrows move x/y by 0.1, Shift by 1, PageUp/PageDown move z in 3D only', () => {
	expect(keyDelta('ArrowRight', false, 2)).toEqual([0.1, 0]);
	expect(keyDelta('ArrowLeft', false, 2)).toEqual([-0.1, 0]);
	expect(keyDelta('ArrowUp', true, 2)).toEqual([0, 1]);
	expect(keyDelta('ArrowDown', false, 3)).toEqual([0, -0.1, 0]);
	expect(keyDelta('PageUp', false, 3)).toEqual([0, 0, 0.1]);
	expect(keyDelta('PageDown', true, 3)).toEqual([0, 0, -1]);
	expect(keyDelta('PageUp', false, 2)).toBeNull();
	expect(keyDelta('Enter', false, 2)).toBeNull();
});

test('world units per pixel: orthographic = 1/zoom, perspective grows with depth', () => {
	expect(worldPerPx({ ortho: true, zoom: 50 }, 400)).toBeCloseTo(0.02);
	// fov 90 deg: the visible height at depth 5 is 2*5*tan(45 deg) = 10 world units over 500 px.
	expect(worldPerPx({ ortho: false, fovDeg: 90, depth: 5 }, 500)).toBeCloseTo(0.02);
	expect(worldPerPx({ ortho: false, fovDeg: 90, depth: 10 }, 500)).toBeCloseTo(0.04);
});

test('ray/plane intersection; parallel or behind-the-origin rays miss', () => {
	const hit = rayPlane([1, 2, 10], [0, 0, -1], [0, 0, 0], [0, 0, 1])!;
	expect(hit.map((x) => +x.toFixed(9))).toEqual([1, 2, 0]);
	const slant = rayPlane([0, 0, 0], [1, 1, 1], [0, 0, 3], [0, 0, 1])!;
	expect(slant.map((x) => +x.toFixed(9))).toEqual([3, 3, 3]);
	expect(rayPlane([0, 0, 1], [1, 0, 0], [0, 0, 0], [0, 0, 1])).toBeNull();
	expect(rayPlane([0, 0, 1], [0, 0, 1], [0, 0, 0], [0, 0, 1])).toBeNull();
});
