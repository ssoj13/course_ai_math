import { t, type Locale } from '../../i18n/core';
/**
 * Three-free model of the engine demo: N-panel rows (one array per card, passed to BOTH SceneIsland in MDX, which
 * reserves the phone chip row from them, and the ViewportCard), the pure derived values and the custom drag mappings
 * (`move`) of the lesson-primitive demos. Imported by the SSR-safe entry (engine.tsx), the scenes (engine.impl.tsx) and
 * tests, so the numbers on the page, the geometry in the scene and the tested math are one code path.
 */
import { magnet, snap } from '../scene/drag';
import type { Move } from '../scene/handles';
import type { Row } from '../scene/NPanel';
import { angleBetween, cross, dot, norm, normalize, scale, signedArea2, sub, type V } from '../geom';
import type { LessonState, Val } from '../store';

type Vals = LessonState['vals'];

export const vec = (v: Vals, k: string): V => v[k] as V;
export const num = (v: Vals, k: string): number => v[k] as number;
/** Angle in degrees, 0 for a zero vector (a reader can drag a tip onto the origin; angleBetween throws there). */
export const angleOf = (a: V, b: V): number => (norm(a) && norm(b) ? (angleBetween(a, b) * 180) / Math.PI : 0);
const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const rad = (deg: number) => (deg * Math.PI) / 180;

// ---------------------------------------------------------------------------------------------------------------
// Cards that existed before Task 6e (2D vectors, 3D vectors, law-of-cosines tiles).

const XY = { min: -4, max: 4, step: 0.1 };
export const ROWS2 = (locale: Locale): readonly Row[] => [
	{ id: 'a', label: 'a', kind: 'vec', key: 'a', dims: 2, spec: XY, tip: { text: t(locale, 'rows.aTip'), tex: String.raw`a = (a_x, a_y)` } },
	{ id: 'b', label: 'b', kind: 'vec', key: 'b', dims: 2, spec: XY, tip: { text: t(locale, 'rows.bTip'), tex: String.raw`b = (b_x, b_y)` } },
	{ id: 'p', label: 'P', kind: 'vec', key: 'p', dims: 2, spec: XY },
	{ id: 'th', label: 'θ', kind: 'ro', value: (v) => angleOf(vec(v, 'a'), vec(v, 'b')), fmt: (x) => `${x.toFixed(1)}°`, chars: 6, tip: { text: t(locale, 'rows.angleTip'), tex: String.raw`\theta = \arccos\frac{a\cdot b}{\lvert a\rvert\,\lvert b\rvert}` } },
	{ id: 'r', label: t(locale, 'rows.arc'), kind: 'num', key: 'r', spec: { min: 0.3, max: 2, step: 0.1 }, snap: 0.5, tip: { text: t(locale, 'rows.radiusTip') } },
	{ id: 'dot', label: 'a·b', kind: 'ro', value: (v) => dot(vec(v, 'a'), vec(v, 'b')), group: t(locale, 'rows.numbers'), tip: { title: t(locale, 'rows.dot'), text: t(locale, 'rows.dotTip'), tex: String.raw`a\cdot b = a_x b_x + a_y b_y` } },
	{ id: 'sqa', label: '|a|²', kind: 'ro', value: (v) => dot(vec(v, 'a'), vec(v, 'a')) },
	{ id: 'par', label: 'b×a', kind: 'ro', value: (v) => signedArea2(vec(v, 'b'), vec(v, 'a')), tip: { title: t(locale, 'rows.signed'), text: t(locale, 'rows.signedTip'), tex: String.raw`b_x a_y - b_y a_x` } },
];

const XYZ = { min: -3, max: 3, step: 0.1 };
export const ROWS3 = (locale: Locale): readonly Row[] => [
	{ id: 'u', label: 'u', kind: 'vec', key: 'u', dims: 3, spec: XYZ },
	{ id: 'v', label: 'v', kind: 'vec', key: 'v', dims: 3, spec: XYZ },
	{ id: 'phi', label: 'φ', kind: 'ro', value: (v) => angleOf(vec(v, 'u'), vec(v, 'v')), fmt: (x) => `${x.toFixed(1)}°`, chars: 6 },
	{ id: 'dot3', label: 'u·v', kind: 'ro', value: (v) => dot(vec(v, 'u'), vec(v, 'v')), group: t(locale, 'rows.numbers') },
	{ id: 'par3', label: '|u×v|', kind: 'ro', value: (v) => norm(cross(vec(v, 'u'), vec(v, 'v'))), tip: { title: t(locale, 'rows.area'), text: t(locale, 'rows.areaTip'), tex: String.raw`\lvert u\times v\rvert` } },
];

export const ROWSLC = (locale: Locale): readonly Row[] => [
	{ id: 'a', label: 'a', kind: 'vec', key: 'a', dims: 2, spec: XY },
	{ id: 'b', label: 'b', kind: 'vec', key: 'b', dims: 2, spec: XY },
	{ id: 'sqa', label: 'a²', kind: 'ro', value: (v) => dot(vec(v, 'a'), vec(v, 'a')), group: t(locale, 'rows.areas') },
	{ id: 'sqb', label: 'b²', kind: 'ro', value: (v) => dot(vec(v, 'b'), vec(v, 'b')) },
	{ id: 'corr', label: '−2a·b', kind: 'ro', value: (v) => -2 * dot(vec(v, 'a'), vec(v, 'b')), tip: { title: t(locale, 'rows.correction'), text: t(locale, 'rows.correctionTip'), tex: String.raw`-2\,a\cdot b` } },
	{
		id: 'sqc',
		label: 'c²',
		kind: 'ro',
		value: (v) => {
			const c = sub(vec(v, 'a'), vec(v, 'b'));
			return dot(c, c);
		},
	},
];

// ---------------------------------------------------------------------------------------------------------------
// Polar card (LC-0/LC-1 vertex drags): triangle C = O, B = (la, 0), A = lb (cos lth, sin lth).

/** The magnet targets of the law-of-cosines lessons (degrees) and their zone. */
export const MAGNET: readonly number[] = [0, 30, 45, 60, 90, 120, 135, 150, 180];
export const ZONE = 2.5;
const LEN = { min: 0.5, max: 4 };

export interface Tri {
	B: V;
	A: V;
	cos: number;
	c: number;
	a2: number;
	b2: number;
	corr: number;
	c2: number;
}

/** The triangle of the polar card; |cos| < 1e-12 is snapped to 0 so 90 degrees is exactly Pythagoras. */
export function tri(v: Vals): Tri {
	const a = num(v, 'la');
	const b = num(v, 'lb');
	const th = rad(num(v, 'lth'));
	const c0 = Math.cos(th);
	const cos = Math.abs(c0) < 1e-12 ? 0 : c0;
	const A = [b * cos, b * Math.sin(th)];
	const B = [a, 0];
	const c2 = a * a + b * b - 2 * a * b * cos;
	return { B, A, cos, c: Math.sqrt(Math.max(0, c2)), a2: a * a, b2: b * b, corr: -2 * a * b * cos, c2 };
}

/** Vertex B moves along x only and writes the side length `la` (grid 0.1, clamped). */
export function moveB(v: Vals, raw: readonly number[]): Move {
	return { set: { la: clamp(snap(num(v, 'la') + raw[0]), LEN.min, LEN.max) } };
}

/**
 * Vertex A moves in polar terms: its distance from C writes `lb` (grid 0.1, clamped), its direction writes `lth` in
 * whole degrees with the magnet (one-way for key presses). Below the x axis the angle folds onto the nearer end of
 * [0, 180] (a triangle C -> B -> A stays counter-clockwise).
 */
export function moveA(v: Vals, raw: readonly number[], src: 'drag' | 'key'): Move {
	const { A } = tri(v);
	const p = [A[0] + raw[0], A[1] + raw[1]];
	const out: Record<string, Val> = { lb: clamp(snap(Math.hypot(p[0], p[1])), LEN.min, LEN.max) };
	if (Math.hypot(p[0], p[1]) === 0) return { set: out };
	let deg = (Math.atan2(p[1], p[0]) * 180) / Math.PI;
	if (deg < 0) deg = deg < -90 ? 180 : 0;
	const m = magnet(deg, MAGNET, ZONE, src === 'key' ? num(v, 'lth') : undefined);
	out.lth = Math.round(m);
	// A catch is an indirect jump: the angle glyph eases onto the target while b keeps following the pointer.
	return { set: out, ease: m !== deg ? ['lth'] : [] };
}

export const ROWSPOLAR = (locale: Locale): readonly Row[] => [
	{ id: 'la', label: 'a', kind: 'num', key: 'la', spec: { ...LEN, step: 0.1 }, snap: 0.5 },
	{ id: 'lb', label: 'b', kind: 'num', key: 'lb', spec: { ...LEN, step: 0.1 }, snap: 0.5 },
	{ id: 'lth', label: 'θ', kind: 'num', key: 'lth', spec: { min: 0, max: 180, step: 1 }, snap: MAGNET, fmt: (x) => `${x}°` },
	{ id: 'lc', label: 'c', kind: 'ro', value: (v) => tri(v).c },
	{ id: 'a2', label: 'a²', kind: 'ro', value: (v) => tri(v).a2, group: t(locale, 'rows.areas') },
	{ id: 'b2', label: 'b²', kind: 'ro', value: (v) => tri(v).b2 },
	{ id: 'corr', label: '−2ab cos θ', kind: 'ro', value: (v) => tri(v).corr },
	{ id: 'c2', label: 'c²', kind: 'ro', value: (v) => tri(v).c2 },
];

// ---------------------------------------------------------------------------------------------------------------
// Sign card (DP-2): a, b with half-planes; b sticks to the perpendicular of a.

/** sin 3 degrees: inside this |cos| band b snaps exactly perpendicular to a. */
export const PERP = Math.sin(rad(3));

/**
 * Tip of b, grid-snapped, then caught exactly perpendicular to a when within 3 degrees of it, on b's own side of a's
 * line (the lessons spec writes `cw(ahat) * sign(cross(a, b))`, which puts b on the OTHER side: for b counter-clockwise
 * of a the clockwise perpendicular is below a; the counter-clockwise one is meant). A key step only catches when it
 * moves toward the perpendicular, so the keyboard can leave it.
 */
export function moveSb(v: Vals, raw: readonly number[], src: 'drag' | 'key'): Move {
	const a = vec(v, 'sa');
	const b0 = vec(v, 'sb');
	const b = [snap(b0[0] + raw[0]), snap(b0[1] + raw[1])];
	const na = norm(a);
	const nb = norm(b);
	if (!na || !nb) return { set: { sb: b } };
	const cosT = dot(a, b) / (na * nb);
	const was = norm(b0) ? Math.abs(dot(a, b0) / (na * norm(b0))) : Infinity;
	if (Math.abs(cosT) >= PERP || (src === 'key' && Math.abs(cosT) >= was)) return { set: { sb: b } };
	const s = signedArea2(a, b) >= 0 ? 1 : -1;
	return { set: { sb: scale([-a[1] / na, a[0] / na], s * nb) }, ease: ['sb'] };
}

export const ROWSSIGN = (locale: Locale): readonly Row[] => [
	{ id: 'sa', label: 'a', kind: 'vec', key: 'sa', dims: 2, spec: XY },
	{ id: 'sb', label: 'b', kind: 'vec', key: 'sb', dims: 2, spec: XY },
	{ id: 'sa:x', label: 'a₁', kind: 'ro', value: (v) => vec(v, 'sa')[0], group: t(locale, 'rows.legs'), tip: { text: t(locale, 'rows.legTip') } },
	{ id: 'sa:y', label: 'a₂', kind: 'ro', value: (v) => vec(v, 'sa')[1] },
	{ id: 'sdot', label: 'a·b', kind: 'ro', value: (v) => dot(vec(v, 'sa'), vec(v, 'sb')), group: t(locale, 'rows.sign') },
];

// ---------------------------------------------------------------------------------------------------------------
// Lamp card (DP-4): lit sphere, point light kept outside, probe P kept on the surface.

export const SPHERE_R = 1.5;
const LIGHT_MIN = SPHERE_R + 0.3;

/** The light moves on the grid but never closer than r + 0.3 to the center (inside, there would be no terminator). */
export function moveLight(v: Vals, raw: readonly number[]): Move {
	const l0 = vec(v, 'nl_light');
	const l = l0.map((x, i) => snap(x + (raw[i] ?? 0)));
	const n = norm(l);
	if (n >= LIGHT_MIN) return { set: { nl_light: l } };
	// Pushed out along its own direction; at the center there is none, so the light stays where it was.
	return { set: { nl_light: n === 0 ? [...l0] : scale(l, LIGHT_MIN / n) } };
}

/** P moves freely and is projected back onto the sphere (no grid: the surface is not on it). */
export function moveP(v: Vals, raw: readonly number[]): Move {
	const p0 = vec(v, 'nl_pt');
	const q = p0.map((x, i) => x + (raw[i] ?? 0));
	return { set: { nl_pt: norm(q) === 0 ? [...p0] : scale(normalize(q), SPHERE_R) } };
}

export interface Lamp {
	N: V;
	L: V;
	ndotl: number;
	bright: number;
	/**
	 * Terminator circle: center, radius, normal (the horizon of the point light on the sphere). Null while the light is
	 * not outside the sphere (a typed value can put it there; the drag mapping cannot): a light inside has no horizon.
	 */
	term: { c: V; r: number; n: V } | null;
}

export function lamp(v: Vals): Lamp {
	const P = vec(v, 'nl_pt');
	const Lp = vec(v, 'nl_light');
	const N = normalize(P);
	const L = normalize(sub(Lp, P));
	const ndotl = dot(N, L);
	const d = norm(Lp);
	const r = SPHERE_R;
	const ln = d > 0 ? scale(Lp, 1 / d) : null;
	// Offset outward by 0.01 so the dashed ring is not half-buried in the sphere's own pixels.
	const term = ln && d > r ? { c: scale(ln, (r * r) / d + 0.01), r: r * Math.sqrt(1 - (r / d) ** 2), n: ln } : null;
	return { N, L, ndotl, bright: Math.max(0, ndotl), term };
}

const LXYZ = { min: -4, max: 4, step: 0.1 };
const fmt1 = (x: number) => (Math.round(x * 10) / 10 + 0).toFixed(1).replace('-', '−');
export const ROWSLAMP = (locale: Locale): readonly Row[] => [
	{ id: 'light', label: t(locale, 'rows.light'), kind: 'vec', key: 'nl_light', dims: 3, spec: LXYZ },
	{ id: 'nl_raw', label: t(locale, 'rows.raw'), kind: 'bool', key: 'nl_raw', tip: { text: t(locale, 'rows.rawTip') } },
	// Unit vectors at one decimal: "(−0.7, −0.3, −0.7)" is the widest text, 18 characters, which fits the 200 px sidebar.
	{ id: 'N', label: 'N', kind: 'ro', value: (v) => lamp(v).N, fmt: fmt1, chars: 18, group: t(locale, 'rows.probe') },
	{ id: 'L', label: 'L', kind: 'ro', value: (v) => lamp(v).L, fmt: fmt1, chars: 18 },
	{ id: 'NdotL', label: 'N·L', kind: 'ro', value: (v) => lamp(v).ndotl, fmt: (x) => (x + 0).toFixed(3).replace('-', '−') },
	{ id: 'bright', label: t(locale, 'rows.bright'), kind: 'ro', value: (v) => (num(v, 'nl_raw') ? lamp(v).ndotl : lamp(v).bright), fmt: (x) => (x + 0).toFixed(3).replace('-', '−') },
];
