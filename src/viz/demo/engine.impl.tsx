/**
 * Engine demo scenes (loaded lazily via engine.tsx): three ViewportCards on the shared `demo` store showing every
 * element of the visual language: header (2D/3D, Persp/Ortho, Overlays, Reset view), gizmo or XY corner, shader grid
 * with ticks, beams with halos and hot outlines, labels, the angle arc, the projection shadow band with its drop line
 * and right-angle mark, square tiles, the signed area bar and the signed parallelogram (hatched when negative), a
 * point, the N-panel (vector fields, a number field, read-only rows, separators, tooltips) and the status bar.
 */
import { angleBetween, cross, dot, isRight, norm, project, scale, signedArea2, sub, type V } from '../geom';
import { AngleArc } from '../scene/AngleArc';
import { AreaTile } from '../scene/AreaTile';
import { Arrow } from '../scene/Arrow';
import { Grid } from '../scene/Grid';
import { Show, type OverlayDef } from '../scene/handles';
import { Label } from '../scene/Label';
import type { Row } from '../scene/NPanel';
import { Parallelogram } from '../scene/Parallelogram';
import { Point } from '../scene/Point';
import { RightAngle } from '../scene/RightAngle';
import { Segment } from '../scene/Segment';
import type { StatusSpec } from '../scene/StatusBar';
import { ViewportCard } from '../scene/ViewportCard';
import { useVal } from '../store';

const O2: V = [0, 0];
const O3: V = [0, 0, 0];
const UP: V = [0, 1, 0];
const TIP: readonly [number, number] = [10, -12];
// Label text is DOM, not KaTeX, so plain Unicode is right here.
// The arc label carries only the number (the N-panel row says θ), so it fits inside the wedge between the beams.
const deg = (d: number) => `${d.toFixed(1)}°`;
// A true minus sign (U+2212), matching the N-panel fields.
const num = (x: number) => x.toFixed(1).replace('-', '−');
/** Angle in degrees, 0 for a zero vector (a reader can drag a tip onto the origin; angleBetween throws there). */
const angleOf = (a: V, b: V) => (norm(a) && norm(b) ? (angleBetween(a, b) * 180) / Math.PI : 0);
const vec = (vals: Record<string, unknown>, k: string) => vals[k] as V;

const use2 = (k: string) => useVal<readonly number[]>('demo', k, O2);

/**
 * Shadow of b on the line of a (the projection) as a soft band, the dashed drop from b's tip and the right-angle mark
 * at the foot. Derived points are computed here from the store and passed as literals; projecting onto a zero `a` is
 * undefined, so nothing is drawn then.
 */
function Projection(): React.JSX.Element | null {
	const a = use2('a');
	const b = use2('b');
	if (norm(a) === 0) return null;
	const foot = project(b, a);
	return (
		<>
			<Segment lesson="demo" id="proj" a={O2} b={foot} band role="vecC" />
			<Segment lesson="demo" id="drop" a={b} b={foot} dashed role="muted" />
			{/* The mark sits in the corner toward the origin: along -a when the shadow points along a, else along a. */}
			<RightAngle lesson="demo" id="drop" at={foot} u={dot(a, b) >= 0 ? scale(a, -1) : a} v={sub(b, foot)} />
		</>
	);
}

/** The triangle O, a, b may be dragged either way round; flip keeps the tile outside it (see AreaTile). */
function SquareA(): React.JSX.Element {
	const flip = signedArea2(use2('a'), use2('b')) < 0;
	return <AreaTile lesson="demo" id="sqa" a={O2} b="a" role="vecA" flip={flip} label={(s) => `|a|² ${num(s)}`} />;
}

function Arc2D(): React.JSX.Element {
	const r = useVal('demo', 'r', 0.6);
	return <AngleArc lesson="demo" id="th" at={O2} from="a" to="b" r={r} label={deg} />;
}

const OV2: readonly OverlayDef[] = [
	{ id: 'grid', label: 'Grid', on: true },
	{ id: 'labels', label: 'Labels', on: true },
	{ id: 'angle', label: 'Angle', on: true },
	{ id: 'shadow', label: 'Shadow', on: true },
	{ id: 'square', label: 'Square on a', on: true },
	{ id: 'par', label: 'Parallelogram b×a', on: false },
];

const XY = { min: -4, max: 4, step: 0.1 };
const ROWS2: readonly Row[] = [
	{ id: 'a', label: 'a', kind: 'vec', key: 'a', spec: XY, tip: { text: 'Vector a: drag its tip in the viewport or scrub here.', tex: String.raw`a = (a_x, a_y)` } },
	{ id: 'b', label: 'b', kind: 'vec', key: 'b', spec: XY, tip: { text: 'Vector b.', tex: String.raw`b = (b_x, b_y)` } },
	{ id: 'p', label: 'P', kind: 'vec', key: 'p', spec: XY },
	{ id: 'th', label: 'θ', kind: 'ro', value: (v) => angleOf(vec(v, 'a'), vec(v, 'b')), fmt: (x) => `${x.toFixed(1)}°`, tip: { text: 'Angle between a and b.', tex: String.raw`\theta = \arccos\frac{a\cdot b}{\lvert a\rvert\,\lvert b\rvert}` } },
	{ id: 'r', label: 'arc r', kind: 'num', key: 'r', spec: { min: 0.3, max: 2, step: 0.1 }, snap: 0.5, tip: { text: 'Radius of the angle arc (a plain number field).' } },
	{ id: 'dot', label: 'a·b', kind: 'ro', value: (v) => dot(vec(v, 'a'), vec(v, 'b')), group: 'Numbers', tip: { title: 'Dot product', text: 'Sum of products of matching components.', tex: String.raw`a\cdot b = a_x b_x + a_y b_y` } },
	{ id: 'sqa', label: '|a|²', kind: 'ro', value: (v) => dot(vec(v, 'a'), vec(v, 'a')) },
	{ id: 'par', label: 'b×a', kind: 'ro', value: (v) => signedArea2(vec(v, 'b'), vec(v, 'a')), tip: { title: 'Signed area', text: 'Negative (hatched) when the second vector is clockwise from the first.', tex: String.raw`b_x a_y - b_y a_x` } },
];

const STATUS2: StatusSpec = {
	hints: {
		'a:tip': { mouse: 'LMB drag the tip of a · arrows 0.1 · Shift 1.0', touch: 'Drag the tip of a' },
		'b:tip': { mouse: 'LMB drag the tip of b · arrows 0.1 · Shift 1.0', touch: 'Drag the tip of b' },
		'p:pt': { mouse: 'LMB drag P · arrows 0.1', touch: 'Drag P' },
		a: (v) => `Vector a = (${vec(v, 'a').map(num).join(', ')})`,
		b: (v) => `Vector b = (${vec(v, 'b').map(num).join(', ')})`,
		th: (v) => `Angle θ = ${angleOf(vec(v, 'a'), vec(v, 'b')).toFixed(1)}°`,
		proj: 'Shadow of b on a · its length × |a| = a·b',
		drop: 'Drop line from the tip of b · meets a at a right angle',
		sqa: (v) => `|a|² = ${num(dot(vec(v, 'a'), vec(v, 'a')))}`,
		par: (v) => `b×a = ${num(signedArea2(vec(v, 'b'), vec(v, 'a')))} · hatched = negative`,
	},
	// isRight: dragged grid values are perpendicular only up to rounding (a = (3, 1), b = (-0.3, 0.9): a·b = 1e-16).
	state: (v) => (isRight(vec(v, 'a'), vec(v, 'b')) ? 'a ⊥ b · a·b = 0' : null),
};

/** 2D: grid, draggable a and b, their angle, b's shadow on a, the square on a, the signed parallelogram, a point. */
export const Demo2D = () => (
	<ViewportCard lesson="demo" id="d2" name="DEMO-2D · Vectors" mode="2d" extent={4} overlays={OV2} rows={ROWS2} status={STATUS2} drawIn>
		<Show ov="grid">
			<Grid lesson="demo" extent={4} step={1} minor={0.5} />
		</Show>
		<Show ov="square">
			<SquareA />
		</Show>
		<Show ov="par">
			{/* b then a: clockwise at the default a, b, so the hatch of a negative signed area shows as soon as it is on. */}
			<Parallelogram lesson="demo" id="par" a="b" b="a" role="vecC" label={(s) => num(s)} />
		</Show>
		<Show ov="shadow">
			<Projection />
		</Show>
		<Show ov="angle">
			<Arc2D />
		</Show>
		<Arrow lesson="demo" id="a" to="a" role="vecA" legs />
		<Arrow lesson="demo" id="b" to="b" role="vecB" />
		<Show ov="labels">
			<Label lesson="demo" id="a" at="a" role="vecA" offset={TIP}>a</Label>
			<Label lesson="demo" id="b" at="b" role="vecB" offset={TIP}>b</Label>
		</Show>
		<Point lesson="demo" id="p" at="p" label="P" role="light" />
	</ViewportCard>
);

const OV3: readonly OverlayDef[] = [
	{ id: 'grid', label: 'Floor grid', on: true },
	{ id: 'labels', label: 'Labels', on: true },
	{ id: 'angle', label: 'Angle', on: true },
	{ id: 'par', label: 'Parallelogram', on: true },
];

const XYZ = { min: -3, max: 3, step: 0.1 };
const ROWS3: readonly Row[] = [
	{ id: 'u', label: 'u', kind: 'vec', key: 'u', spec: XYZ },
	{ id: 'v', label: 'v', kind: 'vec', key: 'v', spec: XYZ },
	{ id: 'phi', label: 'φ', kind: 'ro', value: (v) => angleOf(vec(v, 'u'), vec(v, 'v')), fmt: (x) => `${x.toFixed(1)}°` },
	{ id: 'dot3', label: 'u·v', kind: 'ro', value: (v) => dot(vec(v, 'u'), vec(v, 'v')), group: 'Numbers' },
	{ id: 'par3', label: '|u×v|', kind: 'ro', value: (v) => norm(cross(vec(v, 'u'), vec(v, 'v'))), tip: { title: 'Area', text: 'Area of the parallelogram of u and v.', tex: String.raw`\lvert u\times v\rvert` } },
];

const STATUS3: StatusSpec = {
	hints: {
		u: (v) => `Vector u = (${vec(v, 'u').map(num).join(', ')})`,
		v: (v) => `Vector v = (${vec(v, 'v').map(num).join(', ')})`,
		phi: (v) => `Angle φ = ${angleOf(vec(v, 'u'), vec(v, 'v')).toFixed(1)}°`,
		par3: (v) => `|u×v| = ${num(norm(cross(vec(v, 'u'), vec(v, 'v'))))}`,
	},
};

/** 3D: two arrows with legs, their angle (normal = up picks the plane at 180 degrees), their parallelogram, labels. */
export const Demo3D = () => (
	<ViewportCard lesson="demo" id="d3" name="DEMO-3D · Vectors" mode="3d" extent={3} overlays={OV3} rows={ROWS3} status={STATUS3}>
		<Show ov="grid">
			<Grid lesson="demo" extent={3} step={1} />
		</Show>
		<Show ov="par">
			<Parallelogram lesson="demo" id="par3" a="u" b="v" role="vecC" />
		</Show>
		<Show ov="angle">
			<AngleArc lesson="demo" id="phi" at={O3} from="u" to="v" r={0.7} normal={UP} label={deg} />
		</Show>
		<Arrow lesson="demo" id="u" to="u" role="vecA" legs />
		<Arrow lesson="demo" id="v" to="v" role="vecB" legs />
		<Show ov="labels">
			<Label lesson="demo" id="u" at="u" role="vecA" offset={TIP}>u</Label>
			<Label lesson="demo" id="v" at="v" role="vecB" offset={TIP}>v</Label>
		</Show>
	</ViewportCard>
);

const OVLC: readonly OverlayDef[] = [
	{ id: 'grid', label: 'Grid', on: true },
	{ id: 'squares', label: 'Squares on the sides', on: true },
	{ id: 'corr', label: 'Correction −2a·b', on: true },
	{ id: 'labels', label: 'Labels', on: true },
];

const ROWSLC: readonly Row[] = [
	{ id: 'a', label: 'a', kind: 'vec', key: 'a', spec: XY },
	{ id: 'b', label: 'b', kind: 'vec', key: 'b', spec: XY },
	{ id: 'sqa', label: 'a²', kind: 'ro', value: (v) => dot(vec(v, 'a'), vec(v, 'a')), group: 'Areas' },
	{ id: 'sqb', label: 'b²', kind: 'ro', value: (v) => dot(vec(v, 'b'), vec(v, 'b')) },
	{ id: 'corr', label: '−2a·b', kind: 'ro', value: (v) => -2 * dot(vec(v, 'a'), vec(v, 'b')), tip: { title: 'Correction', text: 'Hatched when negative (angle below 90°).', tex: String.raw`-2\,a\cdot b` } },
	{ id: 'sqc', label: 'c²', kind: 'ro', value: (v) => { const c = sub(vec(v, 'a'), vec(v, 'b')); return dot(c, c); } },
];

const STATUSLC: StatusSpec = {
	hints: {
		sqa: (v) => `a² = ${num(dot(vec(v, 'a'), vec(v, 'a')))}`,
		sqb: (v) => `b² = ${num(dot(vec(v, 'b'), vec(v, 'b')))}`,
		sqc: 'c² = a² + b² − 2a·b',
		corr: (v) => `−2a·b = ${num(-2 * dot(vec(v, 'a'), vec(v, 'b')))} · hatched = negative`,
	},
};

const corr = (v: Record<string, unknown>) => -2 * dot(v.a as V, v.b as V);

/** Squares on the three sides of the triangle O, a, b; `flip` keeps them outside for either orientation. */
function CosineTiles(): React.JSX.Element {
	const flip = signedArea2(use2('a'), use2('b')) < 0;
	return (
		<>
			<AreaTile lesson="demo" id="sqa" a={O2} b="a" role="vecA" flip={flip} label="a²" />
			<AreaTile lesson="demo" id="sqb" a="b" b={O2} role="vecB" flip={flip} label="b²" />
			<AreaTile lesson="demo" id="sqc" a="a" b="b" role="vecC" flip={flip} label="c²" />
		</>
	);
}

/**
 * Law of cosines layout: squares on the three sides and the correction −2 a·b as a signed bar on a fixed 10-unit base
 * (height = area / 10). At the default a, b the angle is acute, so the bar is negative: it grows inward and is hatched.
 */
export const DemoCosines = () => (
	<ViewportCard lesson="demo" id="lc" name="DEMO-LC · Law of cosines" mode="2d" extent={6} overlays={OVLC} rows={ROWSLC} status={STATUSLC}>
		<Show ov="grid">
			<Grid lesson="demo" extent={6} step={1} />
		</Show>
		<Show ov="squares">
			<CosineTiles />
		</Show>
		<Show ov="corr">
			<AreaTile lesson="demo" id="corr" a={[-5, -5]} b={[5, -5]} role="muted" signed area={corr} label={(s) => `−2a·b ${num(s)}`} />
		</Show>
		<Arrow lesson="demo" id="a" to="a" role="vecA" />
		<Arrow lesson="demo" id="b" to="b" role="vecB" />
		<Show ov="labels">
			<Label lesson="demo" id="a" at="a" role="vecA" offset={TIP}>a</Label>
			<Label lesson="demo" id="b" at="b" role="vecB" offset={TIP}>b</Label>
		</Show>
	</ViewportCard>
);
