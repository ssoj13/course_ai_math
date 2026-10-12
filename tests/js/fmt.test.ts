import { describe, expect, test } from 'bun:test';
import { fixed, int, settle, trim } from '../../src/viz/fmt';
import { fmtField } from '../../src/viz/scrub';
import { showVal } from '../../src/viz/tex';

/** The double `k` units in the last place away from `x`: what another engine's Math.cos may return. */
function ulps(x: number, k: number): number {
	const b = new DataView(new ArrayBuffer(8));
	b.setFloat64(0, x);
	b.setBigInt64(0, b.getBigInt64(0) + BigInt(x >= 0 ? k : -k));
	return b.getFloat64(0);
}

/** Values that land on a display boundary when computed exactly (0.425 = the heart stick of the 1.9 lesson). */
const EDGES: [number, number][] = [
	[0.425, 2], [0.125, 2], [-0.375, 2], [2.5, 0], [0.05, 1], [1.0005, 3], [67.5, 0], [12.345, 2], [-0.005, 2], [1e3 + 0.5, 0],
];

describe('shared number formatter', () => {
	test('last-bit noise prints the same text (server vs browser Math.cos)', () => {
		// Before the shared formatter, toFixed straddled the boundary: one engine printed 0.43, the other 0.42.
		expect(ulps(0.425, 4).toFixed(2)).not.toBe(ulps(0.425, -4).toFixed(2));
		for (const [x, d] of EDGES) {
			const want = fixed(x, d);
			for (const k of [-8, -3, -1, 1, 3, 8]) expect(fixed(ulps(x, k), d)).toBe(want);
			expect(trim(ulps(x, 2), d)).toBe(trim(ulps(x, -2), d));
		}
		expect(int(ulps(67.5, 3))).toBe(int(ulps(67.5, -3)));
	});

	test('cancellation noise around zero never prints a sign', () => {
		for (const d of [0, 1, 2, 3, 4]) {
			expect(fixed(-6.123233995736766e-17, d)).toBe((0).toFixed(d));
			expect(fixed(-0, d)).toBe((0).toFixed(d));
			expect(fixed(-0.0004, d)).toBe(d >= 4 ? '-0.0004' : (0).toFixed(d));
		}
		expect(int(-0.2)).toBe('0');
		expect(trim(-1e-12, 2)).toBe('0');
		expect(settle(-0, 2)).toBe(0);
		expect(Object.is(settle(-1e-15, 2), 0)).toBe(true);
	});

	test('ordinary values print exactly as toFixed', () => {
		for (const x of [0, 1, -1, 3.14159, -2.71828, 0.1 + 0.2, 1234.5678, -0.866, 1 / 3, 99.994, 99.996])
			for (const d of [0, 1, 2, 3, 4]) {
				const plain = x.toFixed(d);
				expect(fixed(x, d)).toBe(Number(plain) === 0 ? (0).toFixed(d) : plain);
			}
		expect(trim(2.5, 2)).toBe('2.5');
		expect(trim(4, 2)).toBe('4');
		expect(fixed(Infinity, 2)).toBe('Infinity');
		expect(Number.isNaN(settle(NaN, 2))).toBe(true);
	});

	test('engine formatters (formula values, N-panel fields) go through it', () => {
		expect(showVal('k', ulps(0.425, 3))).toBe(showVal('k', ulps(0.425, -3)));
		expect(showVal('k', -0.001)).toBe('0');
		expect(fmtField(ulps(0.425, 3), 0.01)).toBe(fmtField(ulps(0.425, -3), 0.01));
		expect(fmtField(-0.0001, 0.01)).toBe('0.000');
		expect(fmtField(-1e-17, 0.01)).toBe('0.00');
		expect(fmtField(-1.5, 0.1)).toBe('−1.50');
	});
});
