/**
 * Lesson 1.9 (waves as vectors, the DFT as dot products, epicycles): the store contract and every number the scenes,
 * formulas, N-panel rows, readouts, challenges and tests show. Pure and three-free.
 *
 * Two stores:
 * - `waves-dft` (LESSON): the sampled signal `x` (N = 16 numbers) and which preset it came from (`src`, -1 = edited
 *   by hand), the selected ruler (`k`, `sine`), how many frequencies the rebuild adds back (`K`), and the epicycle
 *   drawing (`path`, M = 64 points flattened as x0, y0, x1, y1, ...; `shape`, -1 = drawn by the reader), the number of
 *   spinning sticks (`S`) and the draw mode (`draw`).
 * - `waves-dft-clock` (CLOCK): the epicycle time `t` (0..1 = one full loop) and `play`. It changes every frame while
 *   playing; a separate store keeps those writes away from every formula and readout of the main store.
 *
 * Conventions (the same as NumPy's `np.fft.rfft`): the cosine ruler of frequency k is cos(2 pi k n / N), the sine
 * ruler sin(2 pi k n / N), and rfft(x)[k] = (x . cos_k) - i (x . sin_k). No complex numbers are needed to compute it:
 * two dot products per frequency.
 */
import { fixed, int } from '../fmt';
import type { LessonLinks, LinkGroup, StepLink } from '../links';
import { finite } from '../links';
import type { LessonState, Val } from '../store';
import { dot } from '../geom';

export const LESSON = 'waves-dft';
export const CLOCK = 'waves-dft-clock';

/** Samples of the signal: the signal is a vector with N components. */
export const N = 16;
/** Highest frequency a real signal of N samples can hold (N / 2 turns per window, the Nyquist frequency). */
export const HALF = N / 2;
/** Points of an epicycle drawing. */
export const M = 64;
/** Largest number of spinning sticks: every frequency of M points except k = 0 (the pin, which does not spin). */
export const MAX_STICKS = M - 1;

type Vals = LessonState['vals'];

const TAU = 2 * Math.PI;

/** Reader-writable ranges (N-panel rows, drag clamps). */
export const SPEC = {
	k: { min: 0, max: HALF, step: 1 },
	K: { min: 0, max: HALF + 1, step: 1 },
	S: { min: 1, max: MAX_STICKS, step: 1 },
	/** One sample of the signal (drag range of a sample handle). */
	x: { min: -2, max: 2, step: 0.1 },
} as const;

/** Half-size of the epicycle drawing area: drawn points are clamped into it. */
export const DRAW_EXTENT = 3.4;

// ---------------------------------------------------------------------------------------------------------------
// Signals

/** Signal presets, in the order of the preset buttons. */
export const SIGNALS = ['mix', 'pure', 'between', 'fast', 'step'] as const;
export type SignalId = (typeof SIGNALS)[number];

/**
 * The continuous wave a preset samples, as a function of the (fractional) sample index u, or null for a signal that
 * is not one smooth wave (the step). Drawing it through the dots is what makes aliasing visible: the fast wave and the
 * k = 2 ruler pass through the same 16 dots.
 */
export function signalCurve(id: SignalId): ((u: number) => number) | null {
	switch (id) {
		case 'mix':
			return (u) => 1.2 * Math.cos((TAU * 2 * u) / N) + 0.6 * Math.sin((TAU * 5 * u) / N);
		case 'pure':
			return (u) => 1.5 * Math.cos((TAU * 3 * u) / N);
		case 'between':
			return (u) => 1.5 * Math.cos((TAU * 2.5 * u) / N);
		case 'fast':
			return (u) => 1.5 * Math.cos((TAU * 14 * u) / N);
		case 'step':
			return null;
	}
}

/** The N samples of a preset. */
export function signalOf(id: SignalId): number[] {
	const f = signalCurve(id);
	return Array.from({ length: N }, (_, n) => (f ? f(n) : n < HALF ? 1 : -1));
}

/** Preset index -> id; throws on an index that is not a preset. */
export function signalId(i: number): SignalId {
	const id = SIGNALS[i];
	if (id === undefined) throw new Error(`waves-dft: no signal preset ${i}`);
	return id;
}

// ---------------------------------------------------------------------------------------------------------------
// Rulers and the DFT

/** The cosine (or sine) ruler of frequency k: N samples of a wave that makes k full turns in the window. */
export function ruler(k: number, sine: boolean): number[] {
	return Array.from({ length: N }, (_, n) => (sine ? Math.sin : Math.cos)((TAU * k * n) / N));
}

/** One frequency of the DFT: the two dot products and the two squared ruler lengths. */
export interface Bin {
	k: number;
	/** x . cos_k: how much the signal points along the cosine ruler (= rfft(x)[k].real). */
	a: number;
	/** x . sin_k (= -rfft(x)[k].imag). */
	b: number;
	/** cos_k . cos_k: N for k = 0 and k = N/2, N/2 otherwise. */
	cc: number;
	/** sin_k . sin_k: 0 for k = 0 and k = N/2 (the sine ruler is all zeros there), N/2 otherwise. */
	ss: number;
}

/** The real DFT of an N-vector: frequencies 0..N/2, each as two dot products. */
export function dft(x: readonly number[]): Bin[] {
	if (x.length !== N) throw new Error(`waves-dft: the signal needs ${N} samples, got ${x.length}`);
	return Array.from({ length: HALF + 1 }, (_, k) => {
		const c = ruler(k, false);
		const s = ruler(k, true);
		return { k, a: dot(x, c), b: dot(x, s), cc: exact(dot(c, c)), ss: exact(dot(s, s)) };
	});
}

/** Ruler lengths are whole numbers (N, N/2 or 0); the sum of 16 squared cosines lands within rounding of one. */
const exact = (v: number): number => Math.round(v * 1e9) / 1e9 + 0;

/**
 * How many copies of each ruler the signal holds: the projection coefficient (x . r) / (r . r) of lesson 1.5. A sine
 * ruler that is all zeros (k = 0 and k = N/2) holds nothing.
 */
export const coefA = (b: Bin): number => b.a / b.cc;
export const coefB = (b: Bin): number => (b.ss > 0 ? b.b / b.ss : 0);

/**
 * The signal rebuilt from the first K frequencies (k < K): add each ruler back, scaled by its coefficient. K = N/2 + 1
 * uses every ruler and gives the signal back exactly (the rulers are an orthogonal basis of the 16-dimensional space).
 */
export function rebuildAt(bins: readonly Bin[], K: number, u: number): number {
	let v = 0;
	for (const b of bins) {
		if (b.k >= K) break;
		v += coefA(b) * Math.cos((TAU * b.k * u) / N) + coefB(b) * Math.sin((TAU * b.k * u) / N);
	}
	return v;
}

export function rebuild(bins: readonly Bin[], K: number): number[] {
	return Array.from({ length: N }, (_, n) => rebuildAt(bins, K, n));
}

/** Sum of squares of a list: the squared length of the signal vector (its "energy"). */
export const energy = (x: readonly number[]): number => x.reduce((s, v) => s + v * v, 0);

/**
 * The same energy read off the spectrum (Parseval's theorem): Pythagoras in the ruler basis, each ruler contributing
 * (x . r)^2 / (r . r), the squared length of the signal's shadow on it.
 */
export const spectrumEnergy = (bins: readonly Bin[]): number =>
	bins.reduce((s, b) => s + (b.a * b.a) / b.cc + (b.ss > 0 ? (b.b * b.b) / b.ss : 0), 0);

/** The 16 non-zero rulers, in grid order: cos_0..cos_8, then sin_1..sin_7. Together they are a basis of 16-D space. */
export interface Basis {
	id: string;
	k: number;
	sine: boolean;
	v: number[];
}
export function basis(): Basis[] {
	const cos = Array.from({ length: HALF + 1 }, (_, k) => ({ id: 'c' + k, k, sine: false, v: ruler(k, false) }));
	const sin = Array.from({ length: HALF - 1 }, (_, i) => ({ id: 's' + (i + 1), k: i + 1, sine: true, v: ruler(i + 1, true) }));
	return [...cos, ...sin];
}

/** Every dot product between two rulers of the basis (the orthogonality grid): zero off the diagonal. */
export function gram(): number[][] {
	const b = basis();
	return b.map((p) => b.map((q) => exact(dot(p.v, q.v))));
}

// ---------------------------------------------------------------------------------------------------------------
// Epicycles

/**
 * One stick of the epicycle chain: frequency k (full turns per loop; negative = clockwise), length r and starting
 * angle phase (radians), and its starting offset (x, y) = r (cos phase, sin phase).
 */
export interface Stick {
	k: number;
	r: number;
	phase: number;
	x: number;
	y: number;
}

export interface Epicycles {
	/** Where the chain is pinned: the average point of the drawing (frequency 0, the stick that does not spin). */
	pin: [number, number];
	/** Spinning sticks, longest first. */
	list: Stick[];
}

/** A stick shorter than this is rounding noise of a frequency the drawing does not have: length 0, angle 0. */
const TINY = 1e-9;

/**
 * The sticks of a closed drawing of M points: for every frequency k of -(M/2 - 1)..M/2, four dot products of the
 * point lists X and Y with the rulers c = cos(2 pi k m / M) and s = sin(2 pi k m / M) give the stick's starting
 * offset: x = (X . c + Y . s) / M, y = (Y . c - X . s) / M. (In the complex numbers of lesson A4 this is the DFT of
 * the points z = x + iy, divided by M.)
 */
export function epicycles(path: readonly number[]): Epicycles {
	if (path.length !== 2 * M) throw new Error(`waves-dft: a drawing needs ${M} points, got ${path.length / 2}`);
	const X = Array.from({ length: M }, (_, m) => path[2 * m]);
	const Y = Array.from({ length: M }, (_, m) => path[2 * m + 1]);
	let pin: [number, number] = [0, 0];
	const list: Stick[] = [];
	for (let k = -(M / 2 - 1); k <= M / 2; k++) {
		const c = Array.from({ length: M }, (_, m) => Math.cos((TAU * k * m) / M));
		const s = Array.from({ length: M }, (_, m) => Math.sin((TAU * k * m) / M));
		const x = (dot(X, c) + dot(Y, s)) / M;
		const y = (dot(Y, c) - dot(X, s)) / M;
		if (k === 0) pin = [x, y];
		else if (Math.hypot(x, y) < TINY) list.push({ k, r: 0, phase: 0, x: 0, y: 0 });
		else list.push({ k, r: Math.hypot(x, y), phase: Math.atan2(y, x), x, y });
	}
	// Longest first. Lengths within rounding of each other (symmetric drawings: k = 2 and k = -2 of the heart) count as
	// equal and keep a fixed order by |k|, then k: otherwise the last bits of Math.cos, which differ between JavaScript
	// engines, could reorder the chain between the server render and the browser.
	list.sort((p, q) => (Math.abs(q.r - p.r) > TINY ? q.r - p.r : Math.abs(p.k) - Math.abs(q.k) || p.k - q.k));
	return { pin, list };
}

/** Joints of the chain at time t (0..1 = one loop) using the first S sticks: the pin, then the end of every stick. */
export function chain(e: Epicycles, S: number, t: number): [number, number][] {
	const out: [number, number][] = [e.pin];
	let [x, y] = e.pin;
	for (const st of e.list.slice(0, S)) {
		const a = st.phase + TAU * st.k * t;
		x += st.r * Math.cos(a);
		y += st.r * Math.sin(a);
		out.push([x, y]);
	}
	return out;
}

/** The pen: the end of the last stick. */
export function tip(e: Epicycles, S: number, t: number): [number, number] {
	const j = chain(e, S, t);
	return j[j.length - 1];
}

/**
 * Measured rebuild error: the average squared distance between the pen and the drawing's points at the moments the
 * drawing was sampled (t = m / M). Zero with every stick; never grows when a stick is added.
 */
export function epiError(path: readonly number[], e: Epicycles, S: number): number {
	let sum = 0;
	for (let m = 0; m < M; m++) {
		const [x, y] = tip(e, S, m / M);
		sum += (x - path[2 * m]) ** 2 + (y - path[2 * m + 1]) ** 2;
	}
	return sum / M;
}

/** Average squared distance of the drawing's points from the pin: the error with no spinning stick at all. */
export function pathSpread(path: readonly number[], e: Epicycles): number {
	let sum = 0;
	for (let m = 0; m < M; m++) sum += (path[2 * m] - e.pin[0]) ** 2 + (path[2 * m + 1] - e.pin[1]) ** 2;
	return sum / M;
}

/**
 * M points spaced evenly along a closed polyline (the last point joins the first). Returns null for a polyline that
 * is too short to draw anything (fewer than 3 points or a total length under 0.5).
 */
export function resampleClosed(pts: readonly (readonly [number, number])[]): number[] | null {
	if (pts.length < 3) return null;
	const loop = [...pts, pts[0]];
	const seg = loop.slice(1).map((p, i) => Math.hypot(p[0] - loop[i][0], p[1] - loop[i][1]));
	const total = seg.reduce((s, v) => s + v, 0);
	if (!(total >= 0.5)) return null;
	const out: number[] = [];
	let i = 0;
	let start = 0;
	for (let m = 0; m < M; m++) {
		const d = (total * m) / M;
		while (i < seg.length - 1 && start + seg[i] < d) start += seg[i++];
		const f = seg[i] > 0 ? (d - start) / seg[i] : 0;
		const a = loop[i];
		const b = loop[i + 1];
		out.push(a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f);
	}
	return out;
}

/** Drawing presets, in the order of the preset buttons. */
export const SHAPES = ['heart', 'star', 'square', 'eight'] as const;
export type ShapeId = (typeof SHAPES)[number];

/** The M points of a preset drawing (flattened), all inside DRAW_EXTENT. */
export function shapeOf(id: ShapeId): number[] {
	const ring = (f: (u: number) => [number, number]) => Array.from({ length: M }, (_, m) => f((TAU * m) / M)).flat();
	switch (id) {
		case 'heart':
			// The classic heart curve, scaled to about 5.4 x 4.9 units and centred.
			return ring((u) => [0.17 * 16 * Math.sin(u) ** 3, 0.17 * (13 * Math.cos(u) - 5 * Math.cos(2 * u) - 2 * Math.cos(3 * u) - Math.cos(4 * u)) + 0.4]);
		case 'eight':
			return ring((u) => [3 * Math.sin(u), 1.6 * Math.sin(2 * u)]);
		case 'star': {
			// Five points: outer radius 3, inner 1.2, first point straight up.
			const corners = Array.from({ length: 10 }, (_, i): [number, number] => {
				const r = i % 2 ? 1.2 : 3;
				const a = Math.PI / 2 + (i * Math.PI) / 5;
				return [r * Math.cos(a), r * Math.sin(a)];
			});
			return resampleClosed(corners) as number[];
		}
		case 'square':
			return resampleClosed([[-2.2, -2.2], [2.2, -2.2], [2.2, 2.2], [-2.2, 2.2]]) as number[];
	}
}

export function shapeId(i: number): ShapeId {
	const id = SHAPES[i];
	if (id === undefined) throw new Error(`waves-dft: no drawing preset ${i}`);
	return id;
}

// ---------------------------------------------------------------------------------------------------------------
// Store contract

const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`waves-dft: "${key}" must be a number`);
	finite(v);
	return v;
};
const vec = (v: Val | undefined, key: string, n: number): readonly number[] => {
	if (typeof v === 'number' || v === undefined || v.length !== n) throw new Error(`waves-dft: "${key}" must be ${n} numbers`);
	finite(v);
	return v;
};
const sameList = (a: readonly number[], b: readonly number[]) => a.length === b.length && a.every((v, i) => Math.abs(v - b[i]) < 1e-12);

/**
 * A preset index and the list it stands for are one linked group (src -> x, shape -> path): choosing a preset writes
 * its list in the same store update (eased, so the dots glide into the new signal), and writing the list directly (a
 * drag, a freehand drawing) marks the preset as -1. Both at once must agree (initial snapshot, checkpoints).
 */
function presetLink(index: string, list: string, n: number, values: (i: number) => number[]): LinkGroup {
	return {
		keys: [index, list],
		canonical: list,
		resolve(input) {
			const hasI = Object.hasOwn(input, index);
			const hasL = Object.hasOwn(input, list);
			if (hasI) {
				const i = num(input[index], index);
				if (i >= 0) {
					const v = values(i);
					if (hasL && !sameList(vec(input[list], list, n), v)) throw new Error(`Conflicting linked inputs ${index} and ${list}`);
					return { [index]: i, [list]: v };
				}
				if (!hasL) throw new Error(`waves-dft: ${index} = -1 needs its ${list}`);
			}
			return { [index]: -1, [list]: [...vec(input[list], list, n)] };
		},
		shown: () => ({}),
	};
}

/**
 * k, K and S are whole-number choices (which ruler, how many frequencies, how many sticks), not quantities: an eased
 * write would draw ruler 1.87 or a chain with 2.4 sticks. The store's integer step adapters keep them whole and make
 * every write a cut, whatever wrote it (typed value, arrow key, scrub).
 */
const whole = (key: string, min: number, max: number): StepLink => ({ key, min, max, value: (i) => i + min, index: (v) => v - min });

export const LINKS: LessonLinks = {
	groups: [presetLink('src', 'x', N, (i) => signalOf(signalId(i))), presetLink('shape', 'path', 2 * M, (i) => shapeOf(shapeId(i)))],
	timelines: { 'wd-k': whole('k', SPEC.k.min, SPEC.k.max), 'wd-K': whole('K', SPEC.K.min, SPEC.K.max), 'wd-S': whole('S', SPEC.S.min, SPEC.S.max) },
};

export const INITIAL: Record<string, Val> = {
	src: 0,
	x: signalOf('mix'),
	k: 1,
	sine: 0,
	K: 1,
	shape: 0,
	path: shapeOf('heart'),
	S: 3,
	draw: 0,
};
export const INITIAL_CLOCK: Record<string, Val> = { t: 0, play: 0 };

/** Everything the formulas, rows, readouts and challenges show for the signal scenes. */
export function derive(v: Vals) {
	const x = vec(v.x, 'x', N);
	const k = num(v.k, 'k');
	const sine = num(v.sine, 'sine') !== 0;
	const K = num(v.K, 'K');
	const bins = dft(x);
	const bin = bins[k];
	if (!bin) throw new Error(`waves-dft: k = ${k} is outside 0..${HALF}`);
	const r = ruler(k, sine);
	const rr = sine ? bin.ss : bin.cc;
	const d = sine ? bin.b : bin.a;
	const back = rebuild(bins, K);
	return {
		x,
		k,
		sine,
		K,
		src: num(v.src, 'src'),
		bins,
		r,
		products: x.map((xn, n) => xn * r[n]),
		/** The dot product with the selected ruler. */
		dot: d,
		/** Its squared length (0 for the all-zero sine rulers k = 0 and k = N/2). */
		rr,
		/** The projection coefficient: how many copies of this ruler the signal holds. */
		coef: rr > 0 ? d / rr : 0,
		/** Amplitude of frequency k: the length of the signal's shadow on the plane of its two rulers, per sample. */
		amp: Math.hypot(coefA(bin), coefB(bin)),
		energy: energy(x),
		specEnergy: spectrumEnergy(bins),
		back,
		/** Squared length of what the rebuild still misses. */
		err: energy(x.map((xn, n) => xn - back[n])),
	};
}
export type Derived = ReturnType<typeof derive>;

/** Epicycle numbers derived from the store (sticks, pen error, spread). */
export function deriveEpi(v: Vals) {
	const path = vec(v.path, 'path', 2 * M);
	const S = num(v.S, 'S');
	const e = epicycles(path);
	const err = epiError(path, e, S);
	const spread = pathSpread(path, e);
	return { path, S, shape: num(v.shape, 'shape'), e, err, spread, rel: spread > 0 ? err / spread : 0 };
}

/** The ruler (k, sine) that the signal points along most strongly: largest |coefficient| (ties: lowest k, cosine first). */
export function strongest(bins: readonly Bin[]): { k: number; sine: boolean } {
	let best = { k: 0, sine: false, c: -1 };
	for (const b of bins) {
		for (const sine of [false, true]) {
			const c = Math.abs(sine ? coefB(b) : coefA(b));
			if (c > best.c + 1e-9) best = { k: b.k, sine, c };
		}
	}
	return { k: best.k, sine: best.sine };
}

/** Challenge predicates; store reads only, so the keyboard and typed values solve them too. */
export const CHECKS = {
	strongest: (v: Vals) => {
		const d = derive(v);
		const s = strongest(d.bins);
		return d.energy > 0.5 && d.k === s.k && d.sine === s.sine;
	},
	alias: (v: Vals) => {
		const d = derive(v);
		return d.src === SIGNALS.indexOf('fast') && d.k === 2 && !d.sine;
	},
	exactEarly: (v: Vals) => {
		const d = derive(v);
		return d.energy > 0.5 && d.K < HALF + 1 && d.err < 1e-9;
	},
	heart: (v: Vals) => {
		const d = deriveEpi(v);
		return d.shape === SHAPES.indexOf('heart') && d.rel < 0.01;
	},
	drawn: (v: Vals) => deriveEpi(v).shape === -1,
};

/** Sample n's horizontal position in the lane scenes (the 16 samples centred on 0, half a unit apart). */
export const xpos = (u: number): number => (u - (N - 1) / 2) * 0.5;

/**
 * Two decimals ("1.20", "-0.60"); a negative zero prints as 0. The shared ixed settles last-bit engine noise, so a
 * value such as 0.425 (heart stick) prints the same on the server and in the browser.
 */
export const fmt2 = (v: number): string => fixed(v, 2);
/** Integers ("8"). */
export const fmtInt = (v: number): string => int(v);