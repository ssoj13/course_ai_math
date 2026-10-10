import { expect, test } from 'bun:test';
import { hitHandle, hitRadius, keyDelta, moved, ndcToPx, pushAway, rayPlane, snap, worldPerPx } from '../../src/viz/scene/drag';
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
test('hit radius: 14 px for a mouse, 22 px for a finger or pen (fingers cover the handle)', () => {
	expect(hitRadius('mouse')).toBe(14);
	expect(hitRadius('touch')).toBe(22);
	expect(hitRadius('pen')).toBe(22);
	expect(hitHandle([100, 100], [{ id: 'a', at: [120, 100] }], hitRadius('touch'))).toBe('a');
	expect(hitHandle([100, 100], [{ id: 'a', at: [120, 100] }], hitRadius('mouse'))).toBeNull();
});
test('ndcToPx: NDC to stage pixels (y down); outside the clip volume is null', () => {
	expect(ndcToPx(0, 0, 0, 200, 100)).toEqual([100, 50]);
	expect(ndcToPx(1, 1, 0.5, 200, 100)).toEqual([200, 0]);
	expect(ndcToPx(-1, -1, -1, 200, 100)).toEqual([0, 100]);
	expect(ndcToPx(0, 0, 1.01, 200, 100)).toBeNull(); // behind the camera or past the far plane
	expect(ndcToPx(1.2, 0, 0, 200, 100)).toBeNull();
	expect(ndcToPx(0, -1.2, 0, 200, 100)).toBeNull();
});
test('pushAway moves a point `gap` px further from another; coincident points stay put', () => {
	expect(pushAway([10, 0], [0, 0], 5)).toEqual([15, 0]);
	const p = pushAway([3, 4], [0, 0], 5);
	expect(p[0]).toBeCloseTo(6);
	expect(p[1]).toBeCloseTo(8);
	expect(pushAway([1, 1], [1, 1], 5)).toEqual([1, 1]);
});
test('pushAway with a label box: the gap is measured from the box edge facing `from`, not its center', () => {
	// Straight right: half the width (20) is added.
	expect(pushAway([10, 0], [0, 0], 5, [40, 10])).toEqual([35, 0]);
	// Straight up (y down on screen): half the height (5) is added.
	expect(pushAway([0, -10], [0, 0], 5, [40, 10])).toEqual([0, -20]);
	// Diagonal: support of the box along (1, 1)/sqrt2 is (40 + 10) / 2 / sqrt2.
	const p = pushAway([3, 3], [0, 0], 0, [40, 10]);
	const s = 25 / Math.SQRT2;
	expect(p[0]).toBeCloseTo(3 + s / Math.SQRT2);
	expect(p[1]).toBeCloseTo(3 + s / Math.SQRT2);
});
