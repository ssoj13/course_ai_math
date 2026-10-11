import { expect, test } from 'bun:test';
import { axisPose, lerpPose, omegaFor, positionOf, poseOf, springProgress, springStep, type Pose } from '../../src/viz/scene/motion';

test('omegaFor: a spring released from rest is within tol of its target exactly after ms', () => {
	const w = omegaFor(180);
	const x = w * 0.18;
	expect((1 + x) * Math.exp(-x)).toBeCloseTo(0.01, 9);
	expect(omegaFor(250, 0.001)).toBeGreaterThan(omegaFor(250));
});

test('springStep is critically damped: from rest it approaches the target monotonically, never overshooting', () => {
	const w = omegaFor(180);
	let s = { x: 0, v: 0 };
	let prev = 0;
	for (const dt of [0.016, 0.004, 0.033, 0.016, 0.1, 0.008, 0.25, 0.016, 1]) {
		s = springStep(s, 1, w, dt);
		expect(s.x).toBeGreaterThanOrEqual(prev);
		expect(s.x).toBeLessThanOrEqual(1);
		prev = s.x;
	}
	expect(s.x).toBeCloseTo(1, 6);
});

test('springStep is frame-rate independent (the analytic solution composes exactly)', () => {
	const w = omegaFor(180);
	const one = springStep({ x: 2, v: -3 }, 5, w, 0.18);
	let many = { x: 2, v: -3 };
	for (let i = 0; i < 45; i++) many = springStep(many, 5, w, 0.004);
	expect(many.x).toBeCloseTo(one.x, 9);
	expect(many.v).toBeCloseTo(one.v, 9);
});

test('springProgress: 0 at start, monotone, >= 99% at the nominal duration, exactly 1 once settled', () => {
	expect(springProgress(0, 250)).toBe(0);
	let prev = 0;
	for (let t = 10; t <= 400; t += 10) {
		const p = springProgress(t, 250);
		expect(p).toBeGreaterThanOrEqual(prev);
		expect(p).toBeLessThanOrEqual(1);
		prev = p;
	}
	expect(springProgress(250, 250)).toBeGreaterThanOrEqual(0.99);
	expect(springProgress(1000, 250)).toBe(1);
	expect(springProgress(-5, 250)).toBe(0);
	expect(springProgress(10, 0)).toBe(1);
});

const P = (o: Partial<Pose>): Pose => ({ target: [0, 0, 0], polar: Math.PI / 2, azimuth: 0, dist: 10, zoom: 1, ...o });

test('poseOf / positionOf are inverse (three.js spherical convention, y up)', () => {
	const p = poseOf([3, 4, 5], [1, 1, 1], 2);
	const back = positionOf(p);
	[3, 4, 5].forEach((x, i) => expect(back[i]).toBeCloseTo(x, 9));
	expect(p.zoom).toBe(2);
	const z = positionOf(P({}));
	[0, 0, 10].forEach((x, i) => expect(z[i]).toBeCloseTo(x, 12));
});

test('lerpPose interpolates target, distance, polar; azimuth takes the short way round; zoom is geometric', () => {
	const a = P({ azimuth: (170 * Math.PI) / 180, zoom: 1, dist: 4, target: [0, 0, 0] });
	const b = P({ azimuth: (-170 * Math.PI) / 180, zoom: 4, dist: 8, target: [2, 0, -2] });
	const m = lerpPose(a, b, 0.5);
	expect(Math.abs(Math.abs(m.azimuth) - Math.PI)).toBeLessThan(1e-9);
	expect(m.zoom).toBeCloseTo(2, 9);
	expect(m.dist).toBeCloseTo(6, 9);
	expect(m.target).toEqual([1, 0, -1]);
	expect(lerpPose(a, b, 0)).toEqual(a);
	const end = lerpPose(a, b, 1);
	expect(Math.cos(end.azimuth)).toBeCloseTo(Math.cos(b.azimuth), 9);
	expect(Math.sin(end.azimuth)).toBeCloseTo(Math.sin(b.azimuth), 9);
});

test('axisPose looks along an axis from the given side, keeping target and distance', () => {
	const base = P({ target: [1, 2, 3], dist: 5, azimuth: 0.7, polar: 1 });
	const dirOf = (q: Pose) => positionOf(q).map((x, i) => (x - q.target[i]) / q.dist);
	const near = (v: number[], w: number[]) => v.forEach((x, i) => expect(x).toBeCloseTo(w[i], 3));
	near(dirOf(axisPose(0, 1, base)), [1, 0, 0]);
	near(dirOf(axisPose(0, -1, base)), [-1, 0, 0]);
	near(dirOf(axisPose(1, 1, base)), [0, 1, 0]);
	near(dirOf(axisPose(1, -1, base)), [0, -1, 0]);
	near(dirOf(axisPose(2, 1, base)), [0, 0, 1]);
	near(dirOf(axisPose(2, -1, base)), [0, 0, -1]);
	expect(axisPose(2, 1, base).target).toEqual([1, 2, 3]);
	expect(axisPose(2, 1, base).dist).toBe(5);
	// Top view: screen right = +x (azimuth 0, like Blender's top view), whatever the previous azimuth was.
	expect(axisPose(1, 1, base).azimuth).toBe(0);
});
