/**
 * Lesson 1.6 (basis, span, independence, dimension): the one model every BS scene, formula, table, N-panel row and
 * challenge reads. Pure, three-free and DOM-free, so the tests import it directly; the scenes only draw what these
 * functions return.
 *
 * One store, `basis`, four groups of keys (one per scene):
 * - BS-1 lattice (2D): rulers `u1`, `u2` and a world vector `v`;
 * - BS-2 span (3D): three arrows `w1`, `w2`, `w3`;
 * - BS-3 rotated axes (2D): the angle `th` (degrees) of a perpendicular unit pair and a point `p`;
 * - BS-4 shadows on new rulers (3D): rulers `r1`, `r2`, `r3`, a world vector `v3` and the flag `ortho` (0/1).
 *
 * Basis convention (completion map, "Basis projection"): the rulers are the COLUMNS of B. The coordinates of v are
 * the solution c of B c = v (solve(B, v)); the dot-product readings are r = Bᵀv; for an orthonormal B they coincide,
 * c = Bᵀv. The Gram matrix of the rulers is BᵀB (entry ij = uᵢ·uⱼ); it is the identity exactly when the rulers are
 * orthonormal. The ghost Σ (uᵢ·v) uᵢ = B Bᵀ v is what "shadows as coordinates" would rebuild; it equals v only for an
 * orthonormal basis. Dependent rulers (zero volume) have no unique coordinates: `solve` returns null, never an
 * invented inverse.
 */
import type { LessonLinks, LinkGroup, Values } from '../links';
import type { LessonState, Val } from '../store';
import type { Move } from '../scene/handles';
import { add, cross, dot, norm, scale, sub, type V } from '../geom';
import { magnet, moved } from '../scene/drag';

export const LESSON = 'basis';

/** Component boxes of the inputs (N-panel ranges; every drag is clamped into them). */
export const RANGE2 = { min: -4.5, max: 4.5, step: 0.1 };
export const RANGE3 = { min: -3, max: 3, step: 0.1 };
export const RANGE_R = { min: -2.5, max: 2.5, step: 0.1 };
export const SPEC_TH = { min: -180, max: 180, step: 1 };

/** Frame of BS-4 at load: u₁ = (2, 2, 1)/3, u₂ = (−2, 1, 2)/3, u₃ = (1, −2, 2)/3 is orthonormal and right-handed. */
export const FRAME0: readonly V[] = [[2 / 3, 2 / 3, 1 / 3], [-2 / 3, 1 / 3, 2 / 3], [1 / 3, -2 / 3, 2 / 3]];

/**
 * Initial values. BS-1 starts on the prose example (u₁ = (1, 0), u₂ = (1, 1), v = (3, 4) = −1·u₁ + 4·u₂); BS-2 on the
 * prose's dependent triple (1, 0, 0), (0, 1, 0), (1, 1, 0); BS-3 on the 45° example with P = (3, 1); BS-4 on the
 * frame above with v = (3, 3, 0), whose coordinates are (4, −1, −1).
 */
export const INITIAL: Record<string, Val> = {
	u1: [1, 0],
	u2: [1, 1],
	v: [3, 4],
	w1: [1, 0, 0],
	w2: [0, 1, 0],
	w3: [1, 1, 0],
	th: 45,
	p: [3, 1],
	r1: FRAME0[0],
	r2: FRAME0[1],
	r3: FRAME0[2],
	v3: [3, 3, 0],
	ortho: 1,
};

type Vals = LessonState['vals'];

/** Relative tolerance of every "is it zero?" test: below it the value is rounding noise, not geometry. */
export const TOL = 1e-9;
/** Tolerance of the ✓ marks of BS-4 (Gram–Schmidt output carries float noise of ~1e-16). */
export const TOL_SHOW = 1e-6;

const vec = (v: Val | undefined, key: string, n: number): number[] => {
	if (typeof v === 'number' || !v || v.length !== n || !v.every(Number.isFinite)) throw new Error(`basis: "${key}" must be a finite ${n}-vector`);
	return [...v];
};
const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number' || !Number.isFinite(v)) throw new Error(`basis: "${key}" must be a finite number`);
	return v;
};

// ---------------------------------------------------------------------------------------------------------------
// Formatting. `+x.toFixed(n)` first so −0.001 prints 0.00, never −0.00; a missing value prints a dash (DOM) or
// `\text{--}` (TeX, which rejects raw non-ASCII).

const fixed = (x: number, n: number) => (+x.toFixed(n) + 0).toFixed(n);
/** Lengths, coordinates, dot products, areas: 2 decimals. */
export const fmt = (x: number) => (Number.isFinite(x) ? fixed(x, 2) : '–');
/** Ruler components and inputs: 1 decimal when on the 0.1 grid, else 2 (a unit ruler like 0.67). */
export const fmtIn = (x: number) => (Number.isFinite(x) ? (Math.abs(x * 10 - Math.round(x * 10)) < 1e-9 ? fixed(x, 1) : fixed(x, 2)) : '–');
/** Ruler components of BS-4 and BS-3: 3 decimals (0.707). */
export const fmt3 = (x: number) => (Number.isFinite(x) ? fixed(x, 3) : '–');
/** Whole degrees. */
export const fmtDeg = (x: number) => (Number.isFinite(x) ? fixed(x, 0) : '–');
export const texNum = (x: number) => (Number.isFinite(x) ? fixed(x, 2) : String.raw`\text{--}`);
export const texIn = (x: number) => (Number.isFinite(x) ? fmtIn(x) : String.raw`\text{--}`);
export const tex3 = (x: number) => (Number.isFinite(x) ? fixed(x, 3) : String.raw`\text{--}`);

// ---------------------------------------------------------------------------------------------------------------
// Linear algebra of the lesson (2D and 3D, tiny and explicit).

/** Signed area of the parallelogram on a, b (2D): a.x·b.y − a.y·b.x. Zero exactly when a and b are dependent. */
export const det2 = (a: V, b: V): number => a[0] * b[1] - a[1] * b[0];
/** Signed volume of the box on a, b, c (3D): a·(b × c). Zero exactly when the three are dependent (coplanar). */
export const det3 = (a: V, b: V, c: V): number => dot(a, cross(b, c));

/**
 * True when `d` (an area or a volume of vectors with lengths `lens`) is rounding noise: relative to the product of the
 * lengths, so tiny but honest bases are not called dependent and huge ones are not called independent by accident.
 */
const isZeroVolume = (d: number, lens: readonly number[]) => Math.abs(d) <= TOL * lens.reduce((p, l) => p * l, 1);

/**
 * Number of independent vectors among `vs` (the dimension of their span), by Gram–Schmidt with a relative tolerance:
 * a vector whose component outside the span of the earlier ones is rounding noise adds no direction.
 */
export function rank(vs: readonly V[]): number {
	const basis: number[][] = [];
	const scaleRef = Math.max(...vs.map(norm), 0);
	if (scaleRef === 0) return 0;
	for (const v of vs) {
		let r = [...v];
		for (const e of basis) r = sub(r, scale(e, dot(r, e)));
		const l = norm(r);
		if (l > TOL * scaleRef) basis.push(scale(r, 1 / l));
	}
	return basis.length;
}

/**
 * Coordinates of v in the basis whose columns are `cols` (2 or 3 vectors of that dimension), by Cramer's rule: the
 * c with Σ cᵢ colsᵢ = v. Null when the columns are dependent (no unique coordinates; the scene shows them as lost).
 */
export function solve(cols: readonly V[], v: V): number[] | null {
	const lens = cols.map(norm);
	if (cols.length === 2) {
		const d = det2(cols[0], cols[1]);
		if (d === 0 || isZeroVolume(d, lens)) return null;
		return [det2(v, cols[1]) / d, det2(cols[0], v) / d];
	}
	const [a, b, c] = cols;
	const d = det3(a, b, c);
	if (d === 0 || isZeroVolume(d, lens)) return null;
	return [det3(v, b, c) / d, det3(a, v, c) / d, det3(a, b, v) / d];
}

/** Rebuilds Σ cᵢ colsᵢ. */
export const combine = (cols: readonly V[], c: readonly number[]): number[] =>
	cols.reduce<number[]>((s, u, i) => add(s, scale(u, c[i])), cols[0].map(() => 0));

/**
 * Clips the line p + t·d (2D) to the square [−ext, ext]² (Liang–Barsky). Returns the two end points, or null when the
 * line misses the square or d is zero.
 */
export function clipLine(p: V, d: V, ext: number): [number[], number[]] | null {
	let lo = -Infinity;
	let hi = Infinity;
	for (let i = 0; i < 2; i++) {
		if (Math.abs(d[i]) < 1e-12) {
			if (Math.abs(p[i]) > ext) return null;
			continue;
		}
		const t1 = (-ext - p[i]) / d[i];
		const t2 = (ext - p[i]) / d[i];
		lo = Math.max(lo, Math.min(t1, t2));
		hi = Math.min(hi, Math.max(t1, t2));
	}
	if (!(hi > lo) || !Number.isFinite(lo) || !Number.isFinite(hi)) return null;
	return [add(p, scale(d, lo)), add(p, scale(d, hi))];
}

/** At most this many lines per family each side of the origin: a nearly dependent pair would ask for thousands. */
export const LATTICE_CAP = 24;

/**
 * One family of lattice lines of the basis (step, dir): the lines k·step + R·dir for integer k, clipped to the square
 * [−ext, ext]², as segment end-point pairs. A zero `dir` has no lines; a `step` along `dir` gives one line (all k land
 * on the same line). The count per side is what the square needs (the line offset grows by |step × dir|/|dir| per k),
 * capped at LATTICE_CAP.
 */
export function family(step: V, dir: V, ext: number): number[][][] {
	const ld = norm(dir);
	if (ld === 0) return [];
	const off = Math.abs(det2(step, dir)) / ld; // distance between neighbouring lines
	const reach = ext * Math.SQRT2;
	const kMax = off <= TOL * Math.max(1, norm(step)) ? 0 : Math.min(LATTICE_CAP, Math.floor(reach / off));
	const out: number[][][] = [];
	for (let k = -kMax; k <= kMax; k++) {
		const seg = clipLine(scale(step, k), dir, ext);
		if (seg) out.push(seg);
	}
	return out;
}

// ---------------------------------------------------------------------------------------------------------------
// BS-1: the movable lattice.

/** Square the lattice is clipped to (the 2D scene's home extent). */
export const EXT1 = 4.5;

export function derive1(vals: Vals) {
	const u1 = vec(vals.u1, 'u1', 2);
	const u2 = vec(vals.u2, 'u2', 2);
	const v = vec(vals.v, 'v', 2);
	const area = det2(u1, u2);
	const dim = rank([u1, u2]);
	const c = dim === 2 ? solve([u1, u2], v) : null;
	// Dimension 1: the span is the line of whichever ruler is not zero; v is reachable only if it lies on it.
	const along = norm(u1) > 0 ? u1 : u2;
	const onLine = dim === 1 && Math.abs(det2(along, v)) <= 1e-9 * norm(along) * Math.max(1, norm(v));
	return {
		u1, u2, v, area, dim, c,
		c1: c ? c[0] : NaN,
		c2: c ? c[1] : NaN,
		/** The first leg of the walk to v: c₁ steps of u₁ (null without coordinates). */
		leg: c ? scale(u1, c[0]) : null,
		/** Dimension 1: the span line clipped to the scene; null otherwise. */
		spanLine: dim === 1 ? clipLine([0, 0], along, EXT1) : null,
		onLine,
		zeroBoth: dim === 0,
	};
}

/** Lattice lines of BS-1: lines of constant s (parallel to u₂, counted by u₁) and of constant t (parallel to u₁). */
export function lattice(u1: V, u2: V, ext = EXT1): { s: number[][][]; t: number[][][] } {
	return { s: family(u1, u2, ext), t: family(u2, u1, ext) };
}

// ---------------------------------------------------------------------------------------------------------------
// BS-2: span of three arrows in 3D.

/** Half side of the translucent plane / length of the line drawn for a span of dimension 2 / 1. */
export const SPAN_HALF = 2.2;
/** Magnet zone of BS-2: a dragged arrow this close to the plane of the other two drops into it. */
export const PLANE_ZONE = 0.15;

export function derive2(vals: Vals) {
	const w = [vec(vals.w1, 'w1', 3), vec(vals.w2, 'w2', 3), vec(vals.w3, 'w3', 3)];
	const vol = det3(w[0], w[1], w[2]);
	const dim = rank(w);
	// The spanned plane (dimension 2): its normal from the first independent pair; the line (dimension 1): its direction.
	let normal: number[] | null = null;
	let dir: number[] | null = null;
	if (dim === 2) {
		for (const [i, j] of [[0, 1], [0, 2], [1, 2]] as const) {
			const n = cross(w[i], w[j]);
			if (norm(n) > TOL * norm(w[i]) * norm(w[j]) && norm(n) > 0) { normal = scale(n, 1 / norm(n)); break; }
		}
	}
	if (dim === 1) {
		const nz = w.find((x) => norm(x) > 0)!;
		dir = scale(nz, 1 / norm(nz));
	}
	return { w, vol, dim, normal, dir };
}

/**
 * Drag/key mapping of BS-2 arrow `i`: the grid move, then (pointer drags only) a magnet into the plane of the other
 * two arrows when they span one and the new tip is within PLANE_ZONE of it, so coplanarity can be reached by hand. A
 * keyboard step stays on the 0.1 grid (exact steps can reach the plane; a magnet would undo steps that leave it).
 */
export function moveW(i: 0 | 1 | 2): (vals: Vals, raw: readonly number[], src: 'drag' | 'key') => Move {
	const key = `w${i + 1}`;
	return (vals, raw, src) => {
		const next = moved(vals, [key], raw)[key] as number[];
		if (src === 'key') return { set: { [key]: next } };
		const [a, b] = ([0, 1, 2] as const).filter((j) => j !== i).map((j) => vec(vals[`w${j + 1}`], `w${j + 1}`, 3));
		const n = cross(a, b);
		const ln = norm(n);
		if (ln <= TOL * norm(a) * norm(b) || ln === 0) return { set: { [key]: next } };
		const nh = scale(n, 1 / ln);
		const d = dot(next, nh);
		if (magnet(d, [0], PLANE_ZONE) !== 0 || d === 0) return { set: { [key]: next } };
		return { set: { [key]: sub(next, scale(nh, d)) }, ease: [key] };
	};
}

// ---------------------------------------------------------------------------------------------------------------
// BS-3: a rotated perpendicular unit pair.

/** Extent of BS-3 (the point P may sit anywhere in [−4.5, 4.5]², the N-panel range). */
export const EXT3 = 4.5;

/**
 * The rotated pair: u₁ at angle θ, u₂ = u₁ turned a quarter turn CLOCKWISE (both unit, perpendicular). Clockwise
 * matches the prose example: at 45° the pair is (1, 1)/√2, (1, −1)/√2, and at 90° it is (0, 1), (1, 0), the
 * "order matters" basis of section 4.
 */
export function frame2(thDeg: number): [number[], number[]] {
	const a = (thDeg * Math.PI) / 180;
	const c = Math.cos(a);
	const s = Math.sin(a);
	return [[c, s], [s, -c]];
}

export function derive3(vals: Vals) {
	const th = num(vals.th, 'th');
	const p = vec(vals.p, 'p', 2);
	const [u1, u2] = frame2(th);
	const c1 = dot(p, u1);
	const c2 = dot(p, u2);
	return {
		th, p, u1, u2, c1, c2,
		/** Feet of the perpendicular drops from P onto the two new axes: the shadows c₁u₁ and c₂u₂. */
		f1: scale(u1, c1),
		f2: scale(u2, c2),
		len2: dot(p, p),
		clen2: c1 * c1 + c2 * c2,
	};
}

/** Magnet targets of the angle drag: every 15°. */
export const TH_TARGETS = Array.from({ length: 25 }, (_, i) => -180 + 15 * i);
/** Handle of u₁ in BS-3 sits on the unit circle. */
export const handleU1 = (vals: Vals): number[] => frame2(num(vals.th, 'th'))[0];

/**
 * Drag/key mapping of u₁'s tip in BS-3: the angle of the moved tip, in whole degrees, magnet-caught to multiples of 15°
 * within 3° (a key step only catches toward a target, `magnet` with `from`). A move onto the origin keeps the angle.
 */
export function moveTh(vals: Vals, raw: readonly number[], src: 'drag' | 'key'): Move {
	const th0 = num(vals.th, 'th');
	const tip = add(frame2(th0)[0], [raw[0] ?? 0, raw[1] ?? 0]);
	if (norm(tip) < 1e-6) return { set: { th: th0 } };
	let th = Math.round((Math.atan2(tip[1], tip[0]) * 180) / Math.PI);
	// Keep the angle continuous across ±180 so a key step from 180 does not jump to −179 the wrong way around.
	if (th === -180) th = 180;
	const caught = magnet(th, TH_TARGETS, 3, src === 'key' ? th0 : undefined);
	return caught !== th ? { set: { th: caught }, ease: ['th'] } : { set: { th } };
}

// ---------------------------------------------------------------------------------------------------------------
// BS-4: shadows on new rulers, orthonormal or not.

export const RULERS = ['r1', 'r2', 'r3'] as const;
type RulerKey = (typeof RULERS)[number];

/**
 * Orthonormal frame from three vectors taken in `order` (Gram–Schmidt): the first is normalized, each next one loses
 * its components along the earlier ones and is normalized. A vector with nothing left (zero or dependent) is replaced
 * by the cross product of the first two (the third slot) or by the coordinate axis least aligned with the first (the
 * second slot), so the result is always a frame. The handedness of `vs` is kept when `vs` already spans space;
 * a completed frame is right-handed. Returns the frame in the ORIGINAL slot order, or null when the leader is zero.
 */
export function gramSchmidt(vs: readonly V[], order: readonly number[] = [0, 1, 2]): number[][] | null {
	const out: number[][] = [];
	for (const [pos, i] of order.entries()) {
		let r = [...vs[i]];
		for (const e of out) r = sub(r, scale(e, dot(r, e)));
		let l = norm(r);
		if (l <= TOL * Math.max(1, norm(vs[i]))) {
			if (pos === 0) return null;
			if (pos === 2) r = cross(out[0], out[1]);
			else {
				const e0 = out[0].map(Math.abs);
				const k = e0[0] <= e0[1] && e0[0] <= e0[2] ? 0 : e0[1] <= e0[2] ? 1 : 2;
				const ax = [0, 0, 0];
				ax[k] = 1;
				r = sub(ax, scale(out[0], dot(ax, out[0])));
			}
			l = norm(r);
		}
		out.push(scale(r, 1 / l));
	}
	const frame: number[][] = [[], [], []];
	order.forEach((i, pos) => (frame[i] = out[pos]));
	return frame;
}

const isFrame = (rs: readonly V[]): boolean =>
	rs.every((r, i) => Math.abs(norm(r) - 1) < 1e-12 && rs.every((q, j) => j <= i || Math.abs(dot(r, q)) < 1e-12));

/**
 * The orthonormal flag as a link group: while `ortho` is 1, every write of a ruler (drag, key, typed field) and the
 * switch itself re-orthonormalize the three rulers by Gram–Schmidt, the written ruler first (it keeps its direction)
 * and the others in order u₁ → u₂ → u₃. While `ortho` is 0 the rulers are free. One rule for every write path.
 * `shown` re-orthonormalizes an eased frame each frame (only u₁, the canonical key, eases), so the drawn rulers stay
 * a frame mid-motion; an already orthonormal frame is returned untouched (no float churn).
 */
export function orthoLink(): LinkGroup {
	return {
		keys: [...RULERS, 'ortho'],
		canonical: 'r1',
		resolve(input, current) {
			const val = (k: string) => (Object.hasOwn(input, k) ? input[k] : current[k]);
			const flag = num(val('ortho'), 'ortho');
			if (flag !== 0 && flag !== 1) throw new Error('basis: "ortho" is 0 or 1');
			const rs = RULERS.map((k) => vec(val(k), k, 3));
			const out: Record<string, Val> = { ortho: flag, ...Object.fromEntries(RULERS.map((k, i) => [k, rs[i]])) };
			if (flag === 0) return out;
			const lead = RULERS.findIndex((k) => Object.hasOwn(input, k));
			const first = lead < 0 ? 0 : lead;
			const order = [first, ...[0, 1, 2].filter((i) => i !== first)];
			// A zero leader has no direction to keep: the previous rulers stay (the write is refused, not invented).
			const frame = gramSchmidt(rs, order) ?? gramSchmidt(RULERS.map((k) => vec(current[k], k, 3)), [0, 1, 2]);
			if (!frame) throw new Error('basis: no ruler to build an orthonormal frame from');
			RULERS.forEach((k: RulerKey, i) => (out[k] = frame[i]));
			return out;
		},
		shown(values: Values) {
			if (values.ortho !== 1) return {};
			const rs = RULERS.map((k) => values[k] as V);
			if (isFrame(rs)) return {};
			const frame = gramSchmidt(rs);
			return frame ? Object.fromEntries(RULERS.map((k, i) => [k, frame[i]])) : {};
		},
	};
}

export const LINKS: LessonLinks = { groups: [orthoLink()] };

/** Length of the drawn rulers each side of the origin (in world units). */
export const RULER_HALF = 3.2;

export function derive4(vals: Vals) {
	const u = RULERS.map((k) => vec(vals[k], k, 3));
	const v = vec(vals.v3, 'v3', 3);
	const ortho = vals.ortho === 1;
	/** Dot-product readings uᵢ·v (rows of Bᵀ times v). */
	const rd = u.map((ui) => dot(ui, v));
	/** Gram matrix BᵀB: gᵢⱼ = uᵢ·uⱼ. */
	const g = u.map((ui) => u.map((uj) => dot(ui, uj)));
	const lens = u.map(norm);
	const vol = det3(u[0], u[1], u[2]);
	const c = solve(u, v);
	const ghost = combine(u, rd);
	const gap = norm(sub(ghost, v));
	const unit = lens.map((l) => Math.abs(l - 1) < TOL_SHOW);
	const perp = [[0, 1], [0, 2], [1, 2]].map(([i, j]) => Math.abs(g[i][j]) < TOL_SHOW);
	return {
		u, v, ortho, rd, g, lens, vol, c, ghost, gap, unit, perp,
		orthonormal: unit.every(Boolean) && perp.every(Boolean),
		/** +1 right-handed, −1 left-handed, 0 flat (dependent rulers). */
		hand: c === null ? 0 : Math.sign(vol),
		/** Perpendicular feet of v on each ruler line (the true shadows), null for a zero ruler. */
		feet: u.map((ui) => (norm(ui) === 0 ? null : scale(ui, dot(v, ui) / dot(ui, ui)))),
		/** Coordinate marks cᵢuᵢ on the rulers and the oblique walk O → c₁u₁ → c₁u₁ + c₂u₂ → v (null without c). */
		marks: c ? u.map((ui, i) => scale(ui, c[i])) : null,
		walk: c ? [[0, 0, 0], scale(u[0], c[0]), add(scale(u[0], c[0]), scale(u[1], c[1])), [...v]] : null,
	};
}

/** Ticks of a ruler: the points k·u for integer k ≠ 0 within RULER_HALF of the origin (at most 12 per side). */
export function rulerTicks(u: V, half = RULER_HALF): number[][] {
	const l = norm(u);
	if (l === 0) return [];
	const n = Math.min(12, Math.floor(half / l + 1e-9));
	const out: number[][] = [];
	for (let k = -n; k <= n; k++) if (k !== 0) out.push(scale(u, k));
	return out;
}

/** End points of a ruler's line through the origin, RULER_HALF each side (null for a zero ruler). */
export function rulerLine(u: V, half = RULER_HALF): [number[], number[]] | null {
	const l = norm(u);
	return l === 0 ? null : [scale(u, -half / l), scale(u, half / l)];
}

// ---------------------------------------------------------------------------------------------------------------
// Challenges: store reads only, so typing in the N-panel alone solves each.

export const near = (a: number, b: number, tol = 1e-9): boolean => Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) <= tol;

export const CHECKS = {
	/** BS-1: both rulers non-zero and dependent: the lattice is one line. */
	collapse: (v: Vals) => { const z = derive1(v); return z.dim === 1 && norm(z.u1) > 0 && norm(z.u2) > 0; },
	/** BS-1: v stays (3, 4) and reads (2, 1) in a basis other than the standard one. */
	readsTwoOne: (v: Vals) => { const z = derive1(v); return near(z.v[0], 3) && near(z.v[1], 4) && near(z.c1, 2, 1e-6) && near(z.c2, 1, 1e-6); },
	/** BS-2: the three arrows span all of space. */
	space: (v: Vals) => derive2(v).dim === 3,
	/** BS-2: a plane again, but not the starting plane z = 0. */
	otherPlane: (v: Vals) => { const z = derive2(v); return z.dim === 2 && z.normal !== null && Math.abs(Math.abs(z.normal[2]) - 1) > 1e-6; },
	/** BS-3: P lies on the first new axis (its second reading is 0). */
	onAxis: (v: Vals) => Math.abs(derive3(v).c2) < 0.05,
	/** BS-3: P = (3, 1) reads (1, 3) in the rotated pair: the same two numbers in the other order. */
	swapped: (v: Vals) => { const z = derive3(v); return near(z.c1, 1, 0.05) && near(z.c2, 3, 0.05); },
	/** BS-4: flag off, and the ghost misses v by more than 1. */
	ghostMiss: (v: Vals) => { const z = derive4(v); return !z.ortho && z.gap > 1; },
	/** BS-4: flag back on after the rulers were bent: the ghost lands on v again. */
	backOn: (v: Vals) => { const z = derive4(v); return z.ortho && z.gap < 1e-6 && !(RULERS.every((k, i) => (v[k] as V).every((x, j) => Math.abs(x - FRAME0[i][j]) < 1e-9))); },
};
