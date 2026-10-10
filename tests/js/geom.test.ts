import { expect, test } from 'bun:test';
import { add, angleBetween, arcPoints, dot, norm, normalize, project, scale, signedArea2, sub, to3 } from '../../src/viz/geom';
test('dot/norm/angle', () => {
	expect(dot([1, 2], [3, 4])).toBe(11);
	expect(norm([3, 4])).toBe(5);
	expect(angleBetween([1, 0], [0, 1])).toBeCloseTo(Math.PI / 2);
	expect(angleBetween([1, 0], [1, 1e-17])).toBe(0); // acos clamp: no NaN from rounding above 1
});
test('angleBetween clamps the antiparallel rounding case and rejects zero', () => {
	const a = [0.1, 0.2, 0.3];
	expect(angleBetween(a, scale(a, -3))).toBeCloseTo(Math.PI);
	expect(() => angleBetween([0, 0], [1, 0])).toThrow();
});
test('add/sub/scale work in any dimension and reject mismatched lengths', () => {
	expect(add([1, 2, 3], [4, 5, 6])).toEqual([5, 7, 9]);
	expect(sub([1, 2], [4, 6])).toEqual([-3, -4]);
	expect(scale([1, 2, 3], 2)).toEqual([2, 4, 6]);
	expect(() => add([1, 2], [1, 2, 3])).toThrow();
	expect(() => dot([1], [1, 2])).toThrow();
});
test('projection and perpendicular remainder', () => {
	const p = project([3, 4], [1, 0]);
	// toEqual distinguishes -0 from 0, which scale() may produce; compare numerically.
	expect(p[0]).toBeCloseTo(3);
	expect(p[1]).toBeCloseTo(0);
	expect(dot(p, [0, 4])).toBeCloseTo(0);
});
test('projection in 3D onto a non-unit vector; zero target throws', () => {
	const p = project([1, 2, 3], [0, 0, 2]);
	expect(p[2]).toBeCloseTo(3);
	expect(p[0]).toBeCloseTo(0);
	expect(() => project([1, 2], [0, 0])).toThrow();
});
test('normalize zero throws', () => {
	expect(() => normalize([0, 0])).toThrow();
	expect(norm(normalize([3, 4, 12]))).toBeCloseTo(1);
});
test('signed area orientation', () => {
	expect(signedArea2([1, 0], [0, 1])).toBe(1);
	expect(signedArea2([0, 1], [1, 0])).toBe(-1);
	expect(() => signedArea2([1, 0, 0], [0, 1, 0])).toThrow();
});
test('arc endpoints lie on the radius', () => {
	const pts = arcPoints([0, 0], [1, 0], [0, 1], 2, 8);
	expect(pts[0][0]).toBeCloseTo(2);
	expect(pts.at(-1)![1]).toBeCloseTo(2);
	expect(pts.length).toBe(9);
});
test('3D arc: every point on the sphere of radius r, in the from/to plane, ends at both rays', () => {
	const c = [1, -2, 0.5];
	const from = [2, 0, 0];
	const to = [1, 3, 1];
	const r = 1.5;
	const pts = arcPoints(c, from, to, r, 12);
	expect(pts.length).toBe(13);
	// Plane normal = from x to.
	const nrm = [from[1] * to[2] - from[2] * to[1], from[2] * to[0] - from[0] * to[2], from[0] * to[1] - from[1] * to[0]];
	for (const p of pts) {
		const d = sub(p, c);
		expect(norm(d)).toBeCloseTo(r);
		expect(dot(d, nrm)).toBeCloseTo(0);
	}
	const first = normalize(sub(pts[0], c));
	const last = normalize(sub(pts[12], c));
	expect(angleBetween(first, from)).toBeCloseTo(0);
	expect(angleBetween(last, to)).toBeCloseTo(0);
	// Angular steps are uniform and sum to the angle between the rays.
	expect(angleBetween(sub(pts[0], c), sub(pts[12], c))).toBeCloseTo(angleBetween(from, to));
});
test('arc takes the shorter way regardless of orientation (2D clockwise)', () => {
	const pts = arcPoints([0, 0], [0, 1], [1, 0], 1, 4);
	const mid = pts[2];
	expect(mid[0]).toBeCloseTo(Math.SQRT1_2);
	expect(mid[1]).toBeCloseTo(Math.SQRT1_2);
});
test('arc: mismatched dims, bad n, zero rays throw with distinct messages', () => {
	expect(() => arcPoints([0, 0], [1, 0], [0, 1, 0], 1)).toThrow();
	expect(() => arcPoints([0, 0], [1, 0], [0, 1], 1, 0)).toThrow();
	expect(() => arcPoints([0, 0], [0, 0], [1, 0], 1)).toThrow(/"from"/);
	expect(() => arcPoints([0, 0], [1, 0], [0, 0], 1)).toThrow(/"to"/);
});
test('arc radius must be finite and non-negative', () => {
	expect(() => arcPoints([0, 0], [1, 0], [0, 1], -1)).toThrow();
	expect(() => arcPoints([0, 0], [1, 0], [0, 1], NaN)).toThrow();
	expect(() => arcPoints([0, 0], [1, 0], [0, 1], Infinity)).toThrow();
	expect(arcPoints([0, 0], [1, 0], [0, 1], 0, 2).length).toBe(3);
});
test('parallel rays give a zero-length arc of n+1 identical points', () => {
	const pts = arcPoints([1, 1, 1], [1, 0, 0], [2, 0, 0], 3, 5);
	expect(pts.length).toBe(6);
	for (const p of pts) {
		expect(p[0]).toBeCloseTo(4);
		expect(p[1]).toBeCloseTo(1);
		expect(p[2]).toBeCloseTo(1);
	}
});
test('2D antiparallel rays sweep the CCW semicircle', () => {
	const pts = arcPoints([0, 0], [1, 0], [-1, 0], 2, 4);
	expect(pts[2][0]).toBeCloseTo(0);
	expect(pts[2][1]).toBeCloseTo(2);
	expect(pts[4][0]).toBeCloseTo(-2);
});
test('3D antiparallel semicircle lies in the from/normal plane on the radius', () => {
	const c = [0, 1, 0];
	const pts = arcPoints(c, [1, 0, 0], [-2, 0, 0], 2, 6, [0, 0, 1]);
	expect(pts.length).toBe(7);
	for (const p of pts) {
		const d = sub(p, c);
		expect(norm(d)).toBeCloseTo(2);
		expect(d[1]).toBeCloseTo(0); // plane spanned by from (x) and normal (z)
	}
	expect(pts[3][2]).toBeCloseTo(2);
});
test('3D antiparallel without a usable normal throws', () => {
	expect(() => arcPoints([0, 0, 0], [1, 0, 0], [-1, 0, 0], 1)).toThrow(/normal/);
	expect(() => arcPoints([0, 0, 0], [1, 0, 0], [-1, 0, 0], 1, 8, [3, 0, 0])).toThrow(/normal/);
	expect(() => arcPoints([0, 0, 0], [1, 0, 0], [-1, 0, 0], 1, 8, [0, 0, 0])).toThrow(/normal/);
});

test('to3 lifts 2D points to z = 0 and rejects other dimensions', () => {
	expect(to3([1, 2])).toEqual([1, 2, 0]);
	expect(to3([1, 2, 3])).toEqual([1, 2, 3]);
	expect(() => to3([1])).toThrow();
});
