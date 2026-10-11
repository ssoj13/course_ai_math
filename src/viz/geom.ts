/** Pure vector helpers for lesson scenes. Arrays in/out, any dimension unless noted; no DOM, no three. */
/** World-unit glass grid; zero means the fill alone is readable at this scale. */
export function gridStep(pxPerUnit: number): number { return pxPerUnit < 14 ? 0 : pxPerUnit >= 80 ? .5 : 1; }
/** Measured side tick law; endpoints remain exact, labels disappear before they crowd. */
export function rulerTicks(length: number, pxPerUnit: number): {value:number;major:boolean;label:boolean}[] {
 if(!Number.isFinite(length)||length<0||pxPerUnit<=0)return [];
 const step=pxPerUnit>=40?.5:1;
 return Array.from({length:Math.floor(length/step)+1},(_,i)=>{const value=i*step,major=Number.isInteger(value);return {value,major,label:major&&pxPerUnit>=24};});
}
export function dimensionLayout(a: V,b: V,wpp: number,normal: V=[0,0,1]): {a:number[];b:number[];mid:number[];out:number[]} {
 const d=sub(b,a),l=norm(d);
 const n=rightOf(a,b,a.length===3?normal:undefined);
 const out=l&&n?scale(n,14*wpp):d.map(()=>0);
 const aa=add(a,out),bb=add(b,out);
 return {a:aa,b:bb,mid:scale(add(aa,bb),.5),out};
}
/** Screen-space dimension endpoints; null means the segment cannot be measured from this view. */
export function screenDimension(a:V,b:V,out:V,px=14): {a:number[];b:number[];normal:number[]}|null {
 const dx=b[0]-a[0],dy=b[1]-a[1],length=Math.hypot(dx,dy);
 if(!Number.isFinite(length)||length<.01)return null;
 let nx=-dy/length,ny=dx/length;
 if(nx*(out[0]-a[0])+ny*(out[1]-a[1])<0){nx=-nx;ny=-ny;}
 return {a:[a[0]+px*nx,a[1]+px*ny],b:[b[0]+px*nx,b[1]+px*ny],normal:[nx,ny]};
}
/** World-aligned orthonormal plane basis. Anchor is world origin projected into the plane, never a face corner. */
export function worldPlaneBasis(u:V,v:V): [number[],number[]] {
 if(u.length===2)return [[1,0,0],[0,1,0]];
 let n=normalize(cross(u,v));
 const dominant=n.reduce((a,x,i)=>Math.abs(x)>Math.abs(n[a])?i:a,0);
 if(n[dominant]<0)n=scale(n,-1);
 const seed=Math.abs(n[0])<.9?[1,0,0]:[0,1,0];
 const x=normalize(sub(seed,scale(n,dot(seed,n))));
 return [x,normalize(cross(n,x))];
}
export type V = readonly number[];

/** Silent truncation by `zip` would hide a 2D/3D mix-up in a lesson, so mismatches are loud. */
function same(a: V, b: V): void {
	if (a.length !== b.length) throw new Error(`dimension mismatch: ${a.length} vs ${b.length}`);
}

export const add = (a: V, b: V): number[] => (same(a, b), a.map((x, i) => x + b[i]));
export const sub = (a: V, b: V): number[] => (same(a, b), a.map((x, i) => x - b[i]));
export const scale = (a: V, s: number): number[] => a.map((x) => x * s);
export const dot = (a: V, b: V): number => (same(a, b), a.reduce((s, x, i) => s + x * b[i], 0));
export const norm = (a: V): number => Math.hypot(...a);

/** Unit vector. Throws on zero length: a NaN direction would silently vanish a scene object. */
export function normalize(a: V): number[] {
	const n = norm(a);
	if (n === 0) throw new Error('cannot normalize a zero vector');
	return scale(a, 1 / n);
}

/** Angle in radians in [0, pi]. The cosine is clamped because rounding can push it just outside [-1, 1] (acos -> NaN). */
export function angleBetween(a: V, b: V): number {
	const d = norm(a) * norm(b);
	if (d === 0) throw new Error('angle with a zero vector is undefined');
	return Math.acos(Math.min(1, Math.max(-1, dot(a, b) / d)));
}

/** Vector projection of `a` onto `onto`. Throws if `onto` is zero. */
export function project(a: V, onto: V): number[] {
	const dd = dot(onto, onto);
	if (dd === 0) throw new Error('cannot project onto a zero vector');
	return scale(onto, dot(a, onto) / dd);
}

/**
 * Orthonormal basis (e1 along `from`, e2 toward `to` in their plane) and the angle theta in [0, pi] of the shorter
 * arc from `from` to `to`. Built by Gram-Schmidt, so it needs no cross product and works in 2D and 3D.
 * Antiparallel rays have no unique plane: 2D takes the CCW semicircle (e2 = `from` rotated +90 degrees), 3D takes
 * the plane through `from` and `normal`. Returns null only when that 3D `normal` is zero or parallel to the rays
 * (a data-driven degeneracy); a missing 3D `normal` or a zero ray throws.
 */
function arcBasis(from: V, to: V, normal?: V): { e1: number[]; e2: number[]; theta: number } | null {
	same(from, to);
	if (norm(from) === 0) throw new Error('arc "from" ray is a zero vector');
	if (norm(to) === 0) throw new Error('arc "to" ray is a zero vector');
	const e1 = normalize(from);
	const rest = sub(to, scale(e1, dot(to, e1))); // component of `to` perpendicular to `from`
	// Relative threshold: tiny perpendicular residue is rounding, not a real plane.
	if (norm(rest) > 1e-9 * norm(to)) {
		const e2 = normalize(rest);
		// e2 points toward `to`, so theta lies in (0, pi) and the sweep is automatically the short way.
		return { e1, e2, theta: Math.atan2(dot(to, e2), dot(to, e1)) };
	}
	if (dot(to, e1) > 0) return { e1, e2: e1, theta: 0 }; // e2 unused: theta = 0 keeps every point on e1
	if (from.length === 2) return { e1, e2: [-e1[1], e1[0]], theta: Math.PI };
	if (!normal) throw new Error('antiparallel 3D arc needs a `normal` to choose the plane');
	same(from, normal);
	const perp = sub(normal, scale(e1, dot(normal, e1)));
	if (norm(perp) <= 1e-9 * norm(normal)) return null;
	return { e1, e2: normalize(perp), theta: Math.PI };
}

/**
 * Points of the shorter arc from direction `from` to direction `to`, radius `r` around `center`
 * (n segments -> n+1 points), in the plane chosen by `arcBasis`.
 * Degenerate-but-teachable cases: parallel rays (0 degrees) give a zero-length arc (n+1 identical
 * points); antiparallel rays (180 degrees) sweep a semicircle (2D: CCW; 3D: needs `normal`, any vector
 * not parallel to `from`). Zero rays, an unusable normal, a bad radius or a bad segment count throw:
 * there is nothing sensible to draw. Scene code that must survive such data uses `angleArc`.
 */
export function arcPoints(center: V, from: V, to: V, r: number, n = 24, normal?: V): number[][] {
	same(center, from);
	if (!Number.isFinite(r) || r < 0) throw new Error(`arc radius must be finite and >= 0, got ${r}`);
	if (!Number.isInteger(n) || n < 1) throw new Error(`arc needs a positive integer segment count, got ${n}`);
	const b = arcBasis(from, to, normal);
	if (!b) throw new Error('arc `normal` is zero or parallel to the rays');
	return Array.from({ length: n + 1 }, (_, i) => {
		const t = (b.theta * i) / n;
		return add(center, add(scale(b.e1, r * Math.cos(t)), scale(b.e2, r * Math.sin(t))));
	});
}

/**
 * Arc data for an angle marker: points (see `arcPoints`), the angle `theta` in radians and the arc midpoint `mid`
 * (on the bisector, where the value label goes; for 180 degrees the top of the semicircle). `n` must be even so the
 * midpoint is a vertex. Returns null instead of throwing when the data has no angle (a zero ray, e.g. a vector
 * dragged onto the origin, or a 3D `normal` parallel to antiparallel rays): a reader's drag must never crash the
 * scene. Authoring errors (missing 3D normal for antiparallel rays, bad r or n) still throw.
 */
export function angleArc(center: V, from: V, to: V, r: number, n = 32, normal?: V): { pts: number[][]; theta: number; mid: number[] } | null {
	if (n % 2 !== 0) throw new Error(`angleArc needs an even segment count, got ${n}`);
	same(from, to);
	if (norm(from) === 0 || norm(to) === 0) return null;
	const b = arcBasis(from, to, normal);
	if (!b) return null;
	const pts = arcPoints(center, from, to, r, n, normal);
	return { pts, theta: b.theta, mid: pts[n / 2] };
}

/** z-component of the 2D cross product: signed parallelogram area, positive when b is counter-clockwise from a. */
export function signedArea2(a: V, b: V): number {
	if (a.length !== 2 || b.length !== 2) throw new Error('signedArea2 is 2D only');
	return a[0] * b[1] - a[1] * b[0];
}

/** A 2D or 3D point as a three.js-style [x, y, z]; 2D data lives in the z = 0 plane. */
export function to3(a: V): [number, number, number] {
	if (a.length === 2) return [a[0], a[1], 0];
	if (a.length === 3) return [a[0], a[1], a[2]];
	throw new Error(`expected a 2D or 3D point, got ${a.length} components`);
}

/**
 * 3D cross product a x b (right-handed): perpendicular to both, length |a||b| sin(theta) = the area of their
 * parallelogram (Parallelogram's 3D area, AreaTile's outward side via `rightOf`). Other dimensions throw.
 */
export function cross(a: V, b: V): number[] {
	if (a.length !== 3 || b.length !== 3) throw new Error('cross is 3D only');
	return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}

/**
 * Unit vector perpendicular to segment a->b on its right: 2D turns b - a clockwise; 3D is (b - a) x normal, which
 * equals the 2D rule for normal = +z. For a counter-clockwise polygon (as seen from `normal`) the right side of every
 * edge is the outside, which is how AreaTile defines "outward". Null when the data leaves the side undefined (a == b,
 * or a 3D normal parallel to the segment). A 3D call without `normal`, or a 2D call with one, is an authoring error.
 */
export function rightOf(a: V, b: V, normal?: V): number[] | null {
	const d = sub(b, a);
	const l = norm(d);
	if (d.length === 2) {
		if (normal) throw new Error('rightOf: a 2D segment takes no normal (its plane is z = 0)');
		return l === 0 ? null : [d[1] / l, -d[0] / l];
	}
	if (!normal) throw new Error('rightOf: a 3D segment needs a `normal` to define its right side');
	const s = cross(d, normal);
	const ls = norm(s);
	// Relative threshold: a normal almost along the segment leaves only rounding noise as the "side".
	return ls === 0 || ls <= 1e-9 * l * norm(normal) ? null : scale(s, 1 / ls);
}

/** True when u and v are within `tolDeg` degrees of perpendicular. Zero vectors have no angle, so never. */
export function isRight(u: V, v: V, tolDeg = 0.5): boolean {
	if (norm(u) === 0 || norm(v) === 0) return false;
	return Math.abs(angleBetween(u, v) - Math.PI / 2) <= (tolDeg * Math.PI) / 180;
}

/** Hard cap on hatch lines per shape: a tiny step (e.g. before the camera is fitted) must not build thousands. */
const HATCH_MAX = 400;

/**
 * Hatch for the parallelogram o + s*u + t*v (s, t in [0, 1]): endpoint pairs of parallel segments `step` apart
 * (perpendicular distance), clipped to the shape. Lines run at 45 degrees in the shape's plane frame: the world x/y
 * frame in 2D (every 2D hatch has the same slant), and (u, then v orthogonalized) in 3D. Spacing is anchored to the
 * shape's own extent (half a step in from the first corner), so the pattern moves with the shape instead of
 * swimming. A degenerate shape (zero or parallel u, v) has no area to hatch and returns []. More than HATCH_MAX lines
 * widen the step instead.
 */
export function hatch(o: V, u: V, v: V, step: number): number[][] {
	same(o, u);
	same(u, v);
	if (!(step > 0) || !Number.isFinite(step)) throw new Error(`hatch step must be finite and > 0, got ${step}`);
	const lu = norm(u);
	const lv = norm(v);
	if (lu === 0 || lv === 0) return [];
	let e1: number[];
	let e2: number[];
	if (o.length === 2) {
		if (Math.abs(signedArea2(u, v)) <= 1e-12 * lu * lv) return [];
		e1 = [1, 0];
		e2 = [0, 1];
	} else {
		e1 = scale(u, 1 / lu);
		const r = sub(v, scale(e1, dot(v, e1)));
		if (norm(r) <= 1e-9 * lv) return [];
		e2 = normalize(r);
	}
	// Corners in plane coordinates, in order around the (convex) shape.
	const loc = (p: V): number[] => [dot(p, e1), dot(p, e2)];
	const poly = [[0, 0], loc(u), loc(add(u, v)), loc(v)];
	// A hatch line is {p : (p.x - p.y) / sqrt2 = c}: direction (1, 1), i.e. 45 degrees in the plane frame.
	const f = (p: readonly number[]) => (p[0] - p[1]) / Math.SQRT2;
	const cs = poly.map(f);
	const lo = Math.min(...cs);
	const span = Math.max(...cs) - lo;
	const st = Math.max(step, span / HATCH_MAX);
	const k = Math.round(span / st);
	const out: number[][] = [];
	for (let i = 0; i < k; i++) {
		const c = lo + st * (i + 0.5);
		const hits: number[][] = [];
		for (let j = 0; j < 4; j++) {
			const p = poly[j];
			const q = poly[(j + 1) % 4];
			const fp = f(p) - c;
			const fq = f(q) - c;
			// Half-open test: a vertex exactly on the line is counted by one edge only.
			if ((fp <= 0 && fq > 0) || (fp > 0 && fq <= 0)) {
				const t = fp / (fp - fq);
				hits.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
			}
		}
		if (hits.length === 2) for (const h of hits) out.push(add(o, add(scale(e1, h[0]), scale(e2, h[1]))));
	}
	return out;
}

/** Axis tick positions (Grid): multiples of `step` within [-extent, extent] except 0; integer stepping avoids drift. */
export function tickValues(extent: number, step: number): number[] {
	const n = Math.floor(extent / step + 1e-9);
	const out: number[] = [];
	for (let i = -n; i <= n; i++) if (i !== 0) out.push(Number((i * step).toFixed(10)));
	return out;
}

/**
 * Orthonormal pair (u, v) spanning the plane perpendicular to the 3D vector `normal`, with (u, v, normal) right-handed.
 * u is built from the coordinate axis least parallel to `normal`, so it never degenerates. Null for a zero normal;
 * a non-finite normal throws.
 */
export function planeBasis(normal: V): [number[], number[]] | null {
	if (normal.length !== 3) throw new Error('planeBasis is 3D only');
	const l = norm(normal);
	// NaN would pass `l === 0` and spread NaN through every point: a computed normal that is not finite is a bug.
	if (!Number.isFinite(l)) throw new Error(`planeBasis: normal is not finite (${normal.join(', ')})`);
	if (l === 0) return null;
	const n = scale(normal, 1 / l);
	const ax = n.map(Math.abs);
	const i = ax[0] <= ax[1] && ax[0] <= ax[2] ? 0 : ax[1] <= ax[2] ? 1 : 2;
	const e = [0, 0, 0];
	e[i] = 1;
	const u = normalize(sub(e, scale(n, dot(e, n))));
	return [u, cross(n, u)];
}

/**
 * Closed loop of `n` segments (n + 1 points, the last equals the first) of radius `r` around `center`: in the plane
 * z = 0 for a 2D center, in the plane perpendicular to `normal` for a 3D one. `arcPoints` sweeps at most 180 degrees,
 * so a full circle (reach rings, the terminator of a lit sphere) needs its own parametrization. A 3D circle without
 * `normal`, a 2D one with it, a bad radius or segment count throw (authoring); a zero normal returns null (data).
 */
export function circlePoints(center: V, r: number, n = 64, normal?: V): number[][] | null {
	if (!Number.isFinite(r) || r < 0) throw new Error(`circle radius must be finite and >= 0, got ${r}`);
	if (!Number.isInteger(n) || n < 3) throw new Error(`circle needs an integer segment count >= 3, got ${n}`);
	let u: V;
	let v: V;
	if (center.length === 2) {
		if (normal) throw new Error('circlePoints: a 2D circle takes no normal (its plane is z = 0)');
		u = [1, 0];
		v = [0, 1];
	} else {
		if (!normal) throw new Error('circlePoints: a 3D circle needs a `normal`');
		const b = planeBasis(normal);
		if (!b) return null;
		[u, v] = b;
	}
	return Array.from({ length: n + 1 }, (_, i) => {
		// The last point repeats the first exactly, so the loop closes without a rounding gap.
		const t = ((i % n) * 2 * Math.PI) / n;
		return add(center, add(scale(u, r * Math.cos(t)), scale(v, r * Math.sin(t))));
	});
}

/**
 * Radial tick marks of an angle arc (AngleArc `ticks`): for each target angle in degrees, measured from `from` toward
 * the arc's side (the side of `to`; for parallel rays 2D counter-clockwise, 3D toward `normal`), a segment from radius
 * `r` to `r + len` around `center`. Flat list of endpoint pairs. Zero rays have no arc and give []; targets outside
 * [0, 180] are skipped (the arc never goes there).
 */
export function arcTicks(center: V, from: V, to: V, r: number, targetsDeg: readonly number[], len: number, normal?: V): number[][] {
	same(center, from);
	if (norm(from) === 0 || norm(to) === 0) return [];
	const b = arcBasis(from, to, normal);
	if (!b) return [];
	let e2 = b.e2;
	if (b.theta === 0) {
		if (from.length === 2) e2 = [-b.e1[1], b.e1[0]];
		else {
			if (!normal) throw new Error('3D arc ticks need a `normal` for parallel rays');
			const p = sub(normal, scale(b.e1, dot(normal, b.e1)));
			if (norm(p) <= 1e-9 * norm(normal)) return [];
			e2 = normalize(p);
		}
	}
	const out: number[][] = [];
	for (const t of targetsDeg) {
		if (t < 0 || t > 180) continue;
		const a = (t * Math.PI) / 180;
		const d = add(scale(b.e1, Math.cos(a)), scale(e2, Math.sin(a)));
		out.push(add(center, scale(d, r)), add(center, scale(d, r + len)));
	}
	return out;
}

/**
 * The 2D half-plane {v : side * (normal . v) > 0} through the origin, clipped to the square [-size, size]^2: a convex
 * polygon of 3 to 5 vertices in counter-clockwise order (one Sutherland-Hodgman pass). [] for a zero normal (no line).
 */
export function halfPlanePoly(normal: V, side: 1 | -1, size: number): number[][] {
	if (normal.length !== 2) throw new Error('halfPlanePoly is 2D only (a 3D half-space is drawn as a disc)');
	if (norm(normal) === 0) return [];
	const sq = [
		[-size, -size],
		[size, -size],
		[size, size],
		[-size, size],
	];
	const f = (p: V) => side * dot(normal, p);
	const out: number[][] = [];
	for (let i = 0; i < 4; i++) {
		const p = sq[i];
		const q = sq[(i + 1) % 4];
		const fp = f(p);
		const fq = f(q);
		if (fp >= 0) out.push(p);
		if ((fp >= 0) !== (fq >= 0)) {
			const t = fp / (fp - fq);
			out.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
		}
	}
	return out;
}

/** Tick number text: integers plain, otherwise one decimal; a true minus sign. */
export const tickText = (v: number): string => (Number.isInteger(v) ? String(v) : v.toFixed(1)).replace('-', '−');
