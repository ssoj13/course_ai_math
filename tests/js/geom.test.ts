import { expect, test } from 'bun:test';
import { add, angleArc, angleBetween, arcPoints, cross, dot, hatch, isRight, norm, normalize, project, rightOf, scale, signedArea2, sub, to3 } from '../../src/viz/geom';
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

test('cross is the 3D cross product and rejects other dimensions', () => {
	expect(cross([1, 0, 0], [0, 1, 0])).toEqual([0, 0, 1]);
	expect(cross([0, 1, 0], [1, 0, 0])).toEqual([0, 0, -1]);
	expect(() => cross([1, 0], [0, 1])).toThrow();
});
test('rightOf: 2D right side of a->b is outside a counter-clockwise polygon', () => {
	const s = rightOf([0, 0], [2, 0])!;
	expect(s[0]).toBeCloseTo(0);
	expect(s[1]).toBeCloseTo(-1);
	// CCW triangle O, A, B: the right side of every edge points away from the opposite vertex.
	const tri = [[0, 0], [3, 0], [0, 2]];
	for (let i = 0; i < 3; i++) {
		const a = tri[i];
		const b = tri[(i + 1) % 3];
		const c = tri[(i + 2) % 3];
		expect(dot(rightOf(a, b)!, sub(c, a))).toBeLessThan(0);
	}
	expect(rightOf([1, 1], [1, 1])).toBeNull();
	expect(() => rightOf([0, 0], [1, 0], [0, 0, 1])).toThrow(/2D/);
});
test('rightOf: 3D uses (b - a) x normal, matches 2D with normal +z, null when normal is parallel', () => {
	const s = rightOf([0, 0, 0], [2, 0, 0], [0, 0, 1])!;
	expect(s).toEqual([0, -1, 0]);
	expect(norm(rightOf([1, 2, 3], [4, 0, 1], [0, 1, 0])!)).toBeCloseTo(1);
	expect(rightOf([0, 0, 0], [0, 0, 2], [0, 0, 5])).toBeNull();
	expect(() => rightOf([0, 0, 0], [1, 0, 0])).toThrow(/normal/);
});
test('isRight: within the tolerance in degrees; zero vectors are never right angles', () => {
	expect(isRight([1, 0], [0, 3])).toBe(true);
	expect(isRight([1, 0, 0], [0, 0, -2])).toBe(true);
	expect(isRight([1, 0], [1, 100])).toBe(false); // 89.43 degrees
	expect(isRight([1, 0], [1, 100], 1)).toBe(true);
	expect(isRight([0, 0], [0, 1])).toBe(false);
});
test('angleArc: theta, arc midpoint on the bisector; zero rays give null instead of throwing', () => {
	const q = angleArc([0, 0], [1, 0], [0, 2], 1)!;
	expect(q.theta).toBeCloseTo(Math.PI / 2);
	expect(q.mid[0]).toBeCloseTo(Math.SQRT1_2);
	expect(q.mid[1]).toBeCloseTo(Math.SQRT1_2);
	expect(q.pts.length).toBe(33);
	expect(angleArc([0, 0], [0, 0], [1, 0], 1)).toBeNull();
	expect(angleArc([0, 0], [1, 0], [0, 0], 1)).toBeNull();
});
test('angleArc at 180 degrees: 2D CCW semicircle; 3D needs a normal, a parallel normal gives null', () => {
	const q = angleArc([1, 1], [2, 0], [-1, 0], 0.5)!;
	expect(q.theta).toBeCloseTo(Math.PI);
	expect(q.mid[0]).toBeCloseTo(1);
	expect(q.mid[1]).toBeCloseTo(1.5);
	const q3 = angleArc([0, 0, 0], [1, 0, 0], [-1, 0, 0], 1, 32, [0, 1, 0])!;
	expect(q3.mid[1]).toBeCloseTo(1);
	expect(angleArc([0, 0, 0], [1, 0, 0], [-1, 0, 0], 1, 32, [2, 0, 0])).toBeNull();
	expect(() => angleArc([0, 0, 0], [1, 0, 0], [-1, 0, 0], 1)).toThrow(/normal/);
	expect(() => angleArc([0, 0], [1, 0], [0, 1], 1, 7)).toThrow(/even/);
});
test('hatch: 45-degree segments inside the parallelogram, `step` apart', () => {
	const segs = hatch([0, 0], [1, 0], [0, 1], 0.25);
	expect(segs.length % 2).toBe(0);
	expect(segs.length / 2).toBe(6); // diagonal extent sqrt(2) / 0.25 = 5.66 -> 6 lines, half a step in from each corner
	const offs: number[] = [];
	for (let i = 0; i < segs.length; i += 2) {
		const [p, q] = [segs[i], segs[i + 1]];
		for (const x of [...p, ...q]) {
			expect(x).toBeGreaterThanOrEqual(-1e-9);
			expect(x).toBeLessThanOrEqual(1 + 1e-9);
		}
		const d = sub(q, p);
		expect(Math.abs(d[0] - d[1])).toBeLessThan(1e-9); // direction (1, 1)
		offs.push((p[0] - p[1]) / Math.SQRT2);
	}
	offs.sort((a, b) => a - b);
	for (let i = 1; i < offs.length; i++) expect(offs[i] - offs[i - 1]).toBeCloseTo(0.25);
});
test('hatch: 3D parallelogram stays in its plane; degenerate shapes give no lines; a bad step throws', () => {
	const o = [1, 1, 1];
	const u = [2, 0, 1];
	const v = [0, 1, 1];
	const n = cross(u, v);
	const segs = hatch(o, u, v, 0.3);
	expect(segs.length).toBeGreaterThan(4);
	for (const p of segs) expect(dot(sub(p, o), n)).toBeCloseTo(0);
	expect(hatch([0, 0], [1, 0], [2, 0], 0.1)).toEqual([]);
	expect(hatch([0, 0], [0, 0], [0, 1], 0.1)).toEqual([]);
	expect(() => hatch([0, 0], [1, 0], [0, 1], 0)).toThrow();
});
test('hatch caps the line count so a tiny step cannot flood the GPU', () => {
	expect(hatch([0, 0], [1, 0], [0, 1], 1e-6).length / 2).toBeLessThanOrEqual(400);
});
