// Lesson 1.9 math (src/viz/lessons/waves-dft.math.ts) against independent references: NumPy golden values, a
// complex radix-2 FFT written here (not the lesson's dot-product DFT), orthogonality, Parseval, aliasing, leakage, the
// rebuild and the epicycle chain.
import { describe, expect, test } from 'bun:test';
import {
	basis, chain, coefA, coefB, CHECKS, derive, deriveEpi, dft, energy, epiError, epicycles, gram, HALF, INITIAL, LINKS, M, N,
	pathSpread, rebuild, resampleClosed, ruler, shapeOf, SHAPES, signalOf, SIGNALS, spectrumEnergy, strongest, tip,
} from '../../src/viz/lessons/waves-dft.math';
import { lessonStore } from '../../src/viz/store';

type C = [number, number];

/** Independent reference: recursive radix-2 complex FFT, X_k = sum z_n e^(-2 pi i k n / L). */
function fft(z: readonly C[]): C[] {
	const L = z.length;
	if (L === 1) return [[z[0][0], z[0][1]]];
	const even = fft(z.filter((_, i) => i % 2 === 0));
	const odd = fft(z.filter((_, i) => i % 2 === 1));
	const out: C[] = new Array(L);
	for (let k = 0; k < L / 2; k++) {
		const a = (-2 * Math.PI * k) / L;
		const w: C = [Math.cos(a) * odd[k][0] - Math.sin(a) * odd[k][1], Math.cos(a) * odd[k][1] + Math.sin(a) * odd[k][0]];
		out[k] = [even[k][0] + w[0], even[k][1] + w[1]];
		out[k + L / 2] = [even[k][0] - w[0], even[k][1] - w[1]];
	}
	return out;
}
/** np.fft.rfft: the first N/2 + 1 outputs of the complex FFT of a real signal. */
const rfft = (x: readonly number[]): C[] => fft(x.map((v): C => [v, 0])).slice(0, x.length / 2 + 1);

const close = (a: number, b: number, tol = 1e-9) => Math.abs(a - b) <= tol;

describe('DFT = a list of dot products', () => {
	test('equals NumPy rfft (golden values printed by numpy 2.4.6)', () => {
		const golden: Record<string, C[]> = {
			step: [[0, 0], [2, -10.0546789843], [0, 0], [2, -2.9932115253], [0, 0], [2, -1.3363572758], [0, 0], [2, -0.3978247348], [0, 0]],
			between: [[1.5, 0], [1.5, 1.5585410125], [1.5, 6.9993686976], [1.5, -8.0157611427], [1.5, -2.6999286694], [1.5, -1.4770198614], [1.5, -0.8400090917], [1.5, -0.3879990804], [1.5, 0]],
		};
		for (const [id, want] of Object.entries(golden)) {
			const bins = dft(signalOf(id as (typeof SIGNALS)[number]));
			// rfft(x)[k] = (x . cos_k) - i (x . sin_k)
			bins.forEach((b, k) => {
				expect(close(b.a, want[k][0], 1e-8)).toBe(true);
				expect(close(-b.b, want[k][1], 1e-8)).toBe(true);
			});
		}
	});
	test('equals an independent FFT for every preset and for random signals', () => {
		let seed = 7;
		const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647) * 4 - 2;
		const signals = [...SIGNALS.map((id) => signalOf(id)), ...Array.from({ length: 5 }, () => Array.from({ length: N }, rnd)), new Array(N).fill(0)];
		for (const x of signals) {
			const ref = rfft(x);
			dft(x).forEach((b, k) => {
				expect(close(b.a, ref[k][0])).toBe(true);
				expect(close(-b.b, ref[k][1])).toBe(true);
			});
		}
	});
	test('a pure wave points along its own ruler only', () => {
		const bins = dft(signalOf('pure'));
		bins.forEach((b) => {
			expect(close(coefA(b), b.k === 3 ? 1.5 : 0)).toBe(true);
			expect(close(coefB(b), 0)).toBe(true);
		});
		// The mix: 1.2 of cos_2 and 0.6 of sin_5, nothing else.
		const mix = dft(signalOf('mix'));
		expect(close(coefA(mix[2]), 1.2) && close(coefB(mix[5]), 0.6)).toBe(true);
		expect(strongest(mix)).toEqual({ k: 2, sine: false });
	});
	test('ruler squared lengths: N at k = 0 and N/2 (sine ruler all zeros there), N/2 elsewhere', () => {
		for (const b of dft(signalOf('mix'))) {
			const edge = b.k === 0 || b.k === HALF;
			expect(b.cc).toBe(edge ? N : N / 2);
			expect(b.ss).toBe(edge ? 0 : N / 2);
		}
	});
});

describe('the rulers are an orthogonal basis', () => {
	test('16 rulers, every pair perpendicular, diagonal = squared lengths', () => {
		const g = gram();
		const b = basis();
		expect(b.length).toBe(N);
		g.forEach((row, i) =>
			row.forEach((v, j) => {
				const want = i !== j ? 0 : b[i].k === 0 || b[i].k === HALF ? N : N / 2;
				expect(v).toBe(want);
			}),
		);
	});
	test('Parseval: energy of the signal = energy of the spectrum (Pythagoras in the ruler basis)', () => {
		for (const id of SIGNALS) {
			const x = signalOf(id);
			expect(close(energy(x), spectrumEnergy(dft(x)), 1e-9)).toBe(true);
			// NumPy form: sum x^2 = (|X0|^2 + 2 sum |Xk|^2 + |X_N/2|^2) / N
			const X = rfft(x).map(([re, im]) => re * re + im * im);
			const np = (X[0] + 2 * X.slice(1, HALF).reduce((s, v) => s + v, 0) + X[HALF]) / N;
			expect(close(energy(x), np, 1e-9)).toBe(true);
		}
	});
	test('inverse: adding every scaled ruler back gives the signal exactly; fewer rulers never do better', () => {
		for (const id of SIGNALS) {
			const x = signalOf(id);
			const bins = dft(x);
			const full = rebuild(bins, HALF + 1);
			full.forEach((v, n) => expect(close(v, x[n], 1e-12)).toBe(true));
			let prev = Infinity;
			for (let K = 0; K <= HALF + 1; K++) {
				const back = rebuild(bins, K);
				const err = energy(x.map((v, n) => v - back[n]));
				expect(err).toBeLessThanOrEqual(prev + 1e-12);
				prev = err;
			}
			expect(prev).toBeLessThan(1e-20);
		}
	});
});

describe('honest cases', () => {
	test('aliasing: k = 14 gives the same 16 numbers as the k = 2 cosine ruler (and sine flips)', () => {
		const fast = signalOf('fast');
		const two = ruler(2, false).map((v) => 1.5 * v);
		fast.forEach((v, n) => expect(close(v, two[n], 1e-12)).toBe(true));
		const s14 = Array.from({ length: N }, (_, n) => Math.sin((2 * Math.PI * 14 * n) / N));
		ruler(2, true).forEach((v, n) => expect(close(s14[n], -v, 1e-12)).toBe(true));
		expect(strongest(dft(fast))).toEqual({ k: 2, sine: false });
	});
	test('leakage: a wave between rulers (k = 2.5) spills into every frequency', () => {
		const bins = dft(signalOf('between'));
		for (const b of bins) expect(Math.hypot(b.a, b.b)).toBeGreaterThan(1);
	});
});

describe('store contract', () => {
	test('a preset index writes its signal, a hand edit marks it -1; drawings the same', () => {
		const s = lessonStore('waves-dft-test', INITIAL, LINKS);
		expect(s.getState().vals.x).toEqual(signalOf('mix'));
		s.getState().set('src', SIGNALS.indexOf('step'));
		expect(s.getState().vals.x).toEqual(signalOf('step'));
		const edited = [...signalOf('step')];
		edited[3] = 0.5;
		s.getState().set('x', edited);
		expect(s.getState().vals.src).toBe(-1);
		s.getState().set('shape', SHAPES.indexOf('star'));
		expect(s.getState().vals.path).toEqual(shapeOf('star'));
		s.getState().set('path', shapeOf('eight'));
		expect(s.getState().vals.shape).toBe(-1);
		expect(() => s.getState().set('src', -1)).toThrow();
	});
	test('k, K and S stay whole and are never eased (no ruler 1.87 drawn mid-glide)', () => {
		const s = lessonStore('waves-dft-test-whole', INITIAL, LINKS);
		s.getState().set('k', 3, 'ease');
		expect([s.getState().vals.k, s.getState().shown.k]).toEqual([3, 3]);
		s.getState().set('S', 2.6, 'ease');
		expect([s.getState().vals.S, s.getState().shown.S]).toEqual([2, 2]);
		s.getState().set('K', 40);
		expect(s.getState().vals.K).toBe(HALF + 1);
	});
	test('derive and the challenges read the initial values', () => {
		const d = derive(INITIAL);
		expect(d.k).toBe(1);
		expect(close(d.dot, 0)).toBe(true);
		expect(CHECKS.strongest(INITIAL)).toBe(false);
		expect(CHECKS.strongest({ ...INITIAL, k: 2 })).toBe(true);
		expect(CHECKS.alias({ ...INITIAL, src: SIGNALS.indexOf('fast'), x: signalOf('fast'), k: 2 })).toBe(true);
		expect(CHECKS.exactEarly({ ...INITIAL, K: 6 })).toBe(true);
		expect(CHECKS.exactEarly({ ...INITIAL, K: 5 })).toBe(false);
	});
});

describe('epicycles', () => {
	const paths = [...SHAPES.map((id) => shapeOf(id)), resampleClosed([[0, 0], [3, 0.4], [1, 2.5], [-2, 1], [-1, -2.2]]) as number[]];
	test('stick lengths and starting angles are |FFT| / M and arg of the drawing points', () => {
		for (const path of paths) {
			const ref = fft(Array.from({ length: M }, (_, m): C => [path[2 * m], path[2 * m + 1]]));
			const e = epicycles(path);
			expect(close(e.pin[0], ref[0][0] / M) && close(e.pin[1], ref[0][1] / M)).toBe(true);
			expect(e.list.length).toBe(M - 1);
			for (const st of e.list) {
				const [re, im] = ref[(st.k + M) % M];
				expect(close(st.r, Math.hypot(re, im) / M)).toBe(true);
				if (st.r > 1e-9) expect(close(Math.cos(st.phase - Math.atan2(im, re)), 1, 1e-9)).toBe(true);
			}
			// Longest stick first.
			for (let i = 1; i < e.list.length; i++) expect(e.list[i].r).toBeLessThanOrEqual(e.list[i - 1].r + 1e-9);
		}
	});
	test('with every stick the pen retraces the drawing through all its points', () => {
		for (const path of paths) {
			const e = epicycles(path);
			for (let m = 0; m < M; m++) {
				const [x, y] = tip(e, M - 1, m / M);
				expect(close(x, path[2 * m], 1e-9) && close(y, path[2 * m + 1], 1e-9)).toBe(true);
			}
			expect(epiError(path, e, M - 1)).toBeLessThan(1e-18);
		}
	});
	test('more sticks = smaller error (measured), and the measured error matches the left-out stick lengths', () => {
		for (const path of paths) {
			const e = epicycles(path);
			let prev = pathSpread(path, e);
			for (let S = 1; S < M; S++) {
				const err = epiError(path, e, S);
				expect(err).toBeLessThanOrEqual(prev + 1e-12);
				const leftOut = e.list.slice(S).reduce((s, st) => s + st.r * st.r, 0);
				expect(close(err, leftOut, 1e-9)).toBe(true);
				prev = err;
			}
		}
	});
	test('one stick draws a circle around the pin; the chain has one joint per stick', () => {
		const e = epicycles(shapeOf('heart'));
		const r = e.list[0].r;
		for (let i = 0; i < 16; i++) {
			const [x, y] = tip(e, 1, i / 16);
			expect(close(Math.hypot(x - e.pin[0], y - e.pin[1]), r, 1e-12)).toBe(true);
		}
		expect(chain(e, 5, 0.3).length).toBe(6);
	});
	test('resampling: M evenly spaced points; degenerate strokes are rejected', () => {
		const sq = resampleClosed([[0, 0], [4, 0], [4, 4], [0, 4]]) as number[];
		expect(sq.length).toBe(2 * M);
		for (let m = 0; m < M; m++) {
			const n = (m + 1) % M;
			expect(close(Math.hypot(sq[2 * n] - sq[2 * m], sq[2 * n + 1] - sq[2 * m + 1]), 16 / M, 1e-9)).toBe(true);
		}
		expect(resampleClosed([[0, 0], [0.1, 0]])).toBeNull();
		expect(resampleClosed([[0, 0], [0.1, 0], [0.1, 0.05]])).toBeNull();
	});
	test('the heart challenge needs a real number of sticks', () => {
		const at = (S: number) => deriveEpi({ ...INITIAL, S }).rel;
		expect(at(1)).toBeGreaterThan(0.01);
		expect(at(M - 1)).toBeLessThan(1e-12);
	});
});
