import { describe, expect, test } from 'bun:test';
import type { InstancedBufferGeometry, InterleavedBufferAttribute } from 'three';
import { Line2 } from 'three/examples/jsm/lines/Line2.js';
import { LineGeometry } from 'three/examples/jsm/lines/LineGeometry.js';
import { LineSegments2 } from 'three/examples/jsm/lines/LineSegments2.js';
import { LineSegmentsGeometry } from 'three/examples/jsm/lines/LineSegmentsGeometry.js';
import { capFor, lineCap, segCount, writeLine } from '../../src/viz/scene/fatLine';

type Cached = LineSegmentsGeometry & { _maxInstanceCount?: number };

/**
 * The WebGL renderer's view of an instanced geometry, copied from three r186: WebGLBindingStates caches
 * `_maxInstanceCount = meshPerAttribute * data.count` the first time the geometry is bound (only when undefined),
 * WebGLGeometries deletes it on the geometry's `dispose` event, and WebGLRenderer draws
 * `min(instanceCount, _maxInstanceCount)` instances. Returns the instance count a frame would draw.
 */
function draw(geo: Cached): number {
	if (!geo.userData.watched) {
		geo.userData.watched = true;
		geo.addEventListener('dispose', () => delete geo._maxInstanceCount);
	}
	const a = geo.getAttribute('instanceStart') as InterleavedBufferAttribute;
	const data = a.data as InterleavedBufferAttribute['data'] & { meshPerAttribute: number };
	if (geo._maxInstanceCount === undefined) geo._maxInstanceCount = data.meshPerAttribute * data.count;
	return Math.min(geo.instanceCount, geo._maxInstanceCount);
}

/** A polyline as drei's `Line` builds it: geometry from the first points, then `computeLineDistances`. */
function drei(pts: number[]): Cached {
	const geo = new LineGeometry();
	geo.setPositions(pts);
	new Line2(geo).computeLineDistances();
	return geo;
}

const poly = (n: number): number[] => Array.from({ length: n }, (_, i) => [i, i % 2, 0]).flat();

describe('fat line writes', () => {
	test('upstream setPositions keeps the first drawn count (the bug writeLine fixes)', () => {
		const geo = drei(poly(2));
		expect(draw(geo)).toBe(1);
		geo.setPositions(poly(11));
		expect(draw(geo)).toBe(1); // ten segments were asked for, the renderer still draws one
	});

	test('a line grows and shrinks at any time and draws exactly its segments', () => {
		const geo = drei(poly(2));
		expect(draw(geo)).toBe(1);
		for (const n of [3, 10, 64, 2, 40, 1, 0, 63, 64, 5]) {
			writeLine(geo, poly(n));
			expect(draw(geo)).toBe(segCount(n, false));
			expect(lineCap(geo)).toBeGreaterThanOrEqual(segCount(n, false));
		}
	});

	test('the epicycle trace grows one segment at a time from 1 to 512', () => {
		const geo = drei([0, 0, 0]);
		for (let n = 1; n <= 513; n++) {
			writeLine(geo, poly(n));
			expect(draw(geo)).toBe(n - 1);
		}
	});

	test('no reallocation while the count fits; growth doubles the capacity', () => {
		const geo = drei(poly(2));
		writeLine(geo, poly(10));
		const buf = (geo.getAttribute('instanceStart') as InterleavedBufferAttribute).data;
		expect(lineCap(geo)).toBe(capFor(9));
		for (const n of [10, 4, 10, 2, 9]) {
			writeLine(geo, poly(n));
			expect((geo.getAttribute('instanceStart') as InterleavedBufferAttribute).data).toBe(buf);
		}
		writeLine(geo, poly(18));
		expect(lineCap(geo)).toBe(32);
		expect((geo.getAttribute('instanceStart') as InterleavedBufferAttribute).data).not.toBe(buf);
	});

	test('the unused tail repeats the last segment, so bounds see only the drawn points', () => {
		const geo = drei(poly(2));
		writeLine(geo, [0, 0, 0, 10, 0, 0, 10, 10, 0, 20, 20, 5]);
		writeLine(geo, [1, 1, 0, 2, 3, 0]);
		const box = geo.boundingBox!;
		expect([box.min.x, box.min.y, box.max.x, box.max.y, box.max.z]).toEqual([1, 1, 2, 3, 0]);
		const start = geo.getAttribute('instanceStart') as InterleavedBufferAttribute;
		for (let i = 0; i < start.count; i++) expect([start.getX(i), start.getY(i)]).toEqual([1, 1]);
	});

	test('dash distances follow the written points like computeLineDistances', () => {
		const geo = drei(poly(2));
		const pts = [0, 0, 0, 3, 4, 0, 3, 4, 12];
		writeLine(geo, pts);
		const ref = drei(pts);
		const d = (g: InstancedBufferGeometry, name: string, i: number) => (g.getAttribute(name) as InterleavedBufferAttribute).getX(i);
		for (let i = 0; i < 2; i++) {
			expect(d(geo, 'instanceDistanceStart', i)).toBeCloseTo(d(ref, 'instanceDistanceStart', i), 6);
			expect(d(geo, 'instanceDistanceEnd', i)).toBeCloseTo(d(ref, 'instanceDistanceEnd', i), 6);
		}
		expect(d(geo, 'instanceDistanceEnd', 1)).toBeCloseTo(17, 6);
	});

	test('segment pairs (drei `segments`) take the points two by two', () => {
		const geo: Cached = new LineSegmentsGeometry();
		geo.setPositions([0, 0, 0, 1, 0, 0]);
		new LineSegments2(geo).computeLineDistances();
		expect(draw(geo)).toBe(1);
		writeLine(geo, [0, 0, 0, 1, 0, 0, 5, 5, 0, 6, 5, 0, 9, 9, 0, 9, 8, 0], true);
		expect(draw(geo)).toBe(3);
		const s = geo.getAttribute('instanceStart') as InterleavedBufferAttribute;
		const e = geo.getAttribute('instanceEnd') as InterleavedBufferAttribute;
		expect([s.getX(1), s.getY(1), e.getX(1), e.getY(1)]).toEqual([5, 5, 6, 5]);
		expect(() => writeLine(geo, [0, 0, 0, 1, 1, 1, 2, 2, 2], true)).toThrow();
	});

	test('an empty or one-point list draws nothing and keeps finite bounds', () => {
		const geo = drei(poly(5));
		writeLine(geo, []);
		expect(draw(geo)).toBe(0);
		writeLine(geo, [4, 5, 6]);
		expect(draw(geo)).toBe(0);
		expect(geo.boundingSphere!.center.toArray()).toEqual([4, 5, 6]);
		expect(() => writeLine(geo, [1, 2])).toThrow();
	});
});
