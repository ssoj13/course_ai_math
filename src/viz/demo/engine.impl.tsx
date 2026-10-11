/**
 * Engine demo scenes (loaded lazily via engine.tsx): ViewportCards on the shared `demo` store showing every element of
 * the visual language: header (2D/3D, Persp/Ortho, Overlays, Reset view), gizmo or XY corner, shader grid with ticks,
 * beams with halos and hot outlines, labels, the angle arc, the projection shadow band with its drop line and
 * right-angle mark, square tiles, the signed area bar and the signed parallelogram (hatched when negative), a point,
 * the N-panel (vector fields, number fields, read-only rows, a checkbox row, separators, tooltips) and the status bar;
 * and the lesson primitives of Task 6e: custom drag mappings (polar vertex, x-only vertex, magnet to the perpendicular,
 * light kept outside a sphere, a point kept on it), magnet ticks, half-planes (2D and the 3D disc), the lit sphere,
 * circles (2D locus, 3D terminator) and per-axis leg ids. Rows and drag mappings live in engine.rows.ts.
 */
import { add, cross, dot, isRight, norm, project, scale, signedArea2, sub, type V } from '../geom';
import { STYLE } from '../palette';
import { Circle } from '../scene/Circle';
import { HalfPlane } from '../scene/HalfPlane';
import { LitSphere } from '../scene/LitSphere';
import { angleOf, lamp, MAGNET, moveA, moveB, moveLight, moveP, moveSb, ROWS2, ROWS3, ROWSLAMP, ROWSLC, ROWSPOLAR, ROWSSIGN, SPHERE_R, tri, vec } from './engine.rows';
import { AngleArc } from '../scene/AngleArc';
import { AreaTile } from '../scene/AreaTile';
import { Arrow } from '../scene/Arrow';
import { Grid } from '../scene/Grid';
import { Show, type OverlayDef } from '../scene/handles';
import { Label } from '../scene/Label';
import { Parallelogram } from '../scene/Parallelogram';
import { Point } from '../scene/Point';
import { RightAngle } from '../scene/RightAngle';
import { Segment } from '../scene/Segment';
import type { StatusSpec } from '../scene/StatusBar';
import { ViewportCard } from '../scene/ViewportCard';
import { useVal, useVals } from '../store';

const O2: V = [0, 0];
const O3: V = [0, 0, 0];
const UP: V = [0, 1, 0];
const TIP: readonly [number, number] = [10, -12];
// Label text is DOM, not KaTeX, so plain Unicode is right here.
// The arc label carries only the number (the N-panel row says θ), so it fits inside the wedge between the beams.
const deg = (d: number) => `${d.toFixed(1)}°`;
// A true minus sign (U+2212), matching the N-panel fields.
const num = (x: number) => x.toFixed(1).replace('-', '−');

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
	{ id: 'disc', label: 'u·x = 0 disc', on: true },
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
		<Show ov="disc">
			{/* DP-3's perpendicular set: one disc, two single-sided faces (u's side tinted, the far side muted). */}
			<HalfPlane lesson="demo" id="disc_pos" normal="u" side="pos" role="vecA" opacity={0.12} />
			<HalfPlane lesson="demo" id="disc_neg" normal="u" side="neg" role="muted" opacity={0.12} />
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

// ---------------------------------------------------------------------------------------------------------------
// Task 6e lesson primitives.

const OVPOLAR: readonly OverlayDef[] = [
	{ id: 'grid', label: 'Grid', on: true },
	{ id: 'labels', label: 'Labels', on: true },
	{ id: 'locus', label: 'Circle |CA| = b', on: true },
];

const STATUSPOLAR: StatusSpec = {
	hints: {
		'A:pt': { mouse: 'LMB drag A · b and θ follow · θ sticks at 30°, 45°, 60°, 90° …', touch: 'Drag A · θ sticks at the ticks' },
		'B:pt': { mouse: 'LMB drag B along a · sets a', touch: 'Drag B along a' },
		locus: 'Every place A can be with this b: a circle around C',
	},
};

/** The triangle of the polar card: C fixed, B on the x axis (moveB), A in polar terms with the magnet (moveA). */
function PolarTriangle(): React.JSX.Element {
	const v = useVals('demo');
	const t = tri(v);
	return (
		<>
			<Segment lesson="demo" id="la" a={O2} b={t.B} role="vecA" />
			<Segment lesson="demo" id="lb" a={O2} b={t.A} role="vecB" />
			<Segment lesson="demo" id="lc" a={t.B} b={t.A} role="vecC" />
			<AngleArc lesson="demo" id="lth" at={O2} from={t.B} to={t.A} r={0.8} ticks={MAGNET} label={(d) => `${Math.round(d)}°`} />
			{t.cos === 0 && <RightAngle lesson="demo" id="lth" at={O2} u={t.B} v={t.A} />}
			<Point lesson="demo" id="C" at={O2} drag={false} />
			<Point lesson="demo" id="B" at={(x) => tri(x).B} keys={['la']} move={moveB} label="B" />
			<Point lesson="demo" id="A" at={(x) => tri(x).A} keys={['lb', 'lth']} move={moveA} label="A" />
			<Show ov="labels">
				<Label lesson="demo" at={O2} offset={[-12, 12]}>C</Label>
				<Label lesson="demo" id="B" at={t.B} offset={[10, 12]}>B</Label>
				<Label lesson="demo" id="A" at={t.A} offset={TIP}>A</Label>
			</Show>
		</>
	);
}

/** Polar drag (LC-0 vertices), magnet ticks on the arc, a 2D circle (the locus of A); the Ledger sits under the card. */
export const DemoPolar = () => (
	<ViewportCard lesson="demo" id="pol" name="DEMO-POLAR · Vertex drags" mode="2d" extent={5} center={[1, 1.5]} overlays={OVPOLAR} rows={ROWSPOLAR} status={STATUSPOLAR}>
		<Show ov="grid">
			<Grid lesson="demo" extent={5} step={1} />
		</Show>
		<Show ov="locus">
			<Circle lesson="demo" id="locus" center={O2} r={(x) => x.lb as number} role="muted" dashPx={STYLE.dash} />
		</Show>
		<PolarTriangle />
	</ViewportCard>
);

const OVSIGN: readonly OverlayDef[] = [
	{ id: 'grid', label: 'Grid', on: true },
	{ id: 'half', label: 'Half-planes', on: true },
	{ id: 'legs', label: 'Legs', on: true },
];

const STATUSSIGN: StatusSpec = {
	hints: {
		'sb:tip': { mouse: 'LMB drag tip of b · snaps to ⊥ near 90°', touch: 'Drag b · snaps to ⊥ near 90°' },
		'sa:tip': { mouse: 'LMB drag tip of a · the half-planes turn with it', touch: 'Drag a' },
		hp_pos: 'a·v > 0 here',
		hp_neg: 'a·v < 0 here',
		'sa:x': (v) => `a₁ = ${num(vec(v, 'sa')[0])}`,
		'sa:y': (v) => `a₂ = ${num(vec(v, 'sa')[1])}`,
	},
	state: (v) => (Math.abs(dot(vec(v, 'sa'), vec(v, 'sb'))) < 1e-9 ? 'a ⊥ b · a·b = 0' : null),
};

function SignRight(): React.JSX.Element | null {
	const a = use2('sa');
	const b = use2('sb');
	return norm(a) && norm(b) && Math.abs(dot(a, b)) < 1e-9 ? <RightAngle lesson="demo" id="sdot" at={O2} u={a} v={b} show /> : null;
}

/** DP-2: the plane pre-coloured by the sign of a·v, b's magnet to the perpendicular, per-axis leg ids of a. */
export const DemoSign = () => (
	<ViewportCard lesson="demo" id="sign" name="DEMO-SIGN · Half-planes" mode="2d" extent={4} overlays={OVSIGN} rows={ROWSSIGN} status={STATUSSIGN}>
		<Show ov="grid">
			<Grid lesson="demo" extent={4} step={1} />
		</Show>
		<Show ov="half">
			<HalfPlane lesson="demo" id="hp_pos" normal="sa" side="pos" role="vecA" />
			<HalfPlane lesson="demo" id="hp_neg" normal="sa" side="neg" role="muted" />
		</Show>
		<Arrow lesson="demo" id="sa" to="sa" role="vecA" legs label="a" />
		<Arrow lesson="demo" id="sb" to="sb" role="vecB" move={moveSb} label="b" />
		<SignRight />
		<Label lesson="demo" id="sa" at="sa" role="vecA" offset={TIP}>a</Label>
		<Label lesson="demo" id="sb" at="sb" role="vecB" offset={TIP}>b</Label>
	</ViewportCard>
);

const OVLAMP: readonly OverlayDef[] = [
	{ id: 'term', label: 'Terminator', on: true },
	{ id: 'vecs', label: 'N and L', on: true },
	{ id: 'labels', label: 'Labels', on: true },
];

const STATUSLAMP: StatusSpec = {
	hints: {
		'light:pt': { mouse: 'LMB drag light · stays outside the sphere', touch: 'Drag the light' },
		'P:pt': { mouse: 'LMB drag P on the surface', touch: 'Drag P on the surface' },
		term: 'N·L = 0 here · the ⊥ line of the sign scene, wrapped around a sphere',
		N: 'Surface normal at P',
		L: 'Direction from P to the light',
	},
};

/** N and L at the probe, and the terminator circle (all derived from the store). */
function LampGuides(): React.JSX.Element {
	const v = useVals('demo');
	const l = lamp(v);
	const P = vec(v, 'nl_pt');
	return (
		<>
			{l.term && (
				<Show ov="term">
					<Circle lesson="demo" id="term" center={l.term.c} r={l.term.r} normal={l.term.n} role="muted" dashPx={STYLE.dash} />
				</Show>
			)}
			<Show ov="vecs">
				<Arrow lesson="demo" id="N" from={P} to={add(P, scale(l.N, 1.2))} role="vecB" />
				<Arrow lesson="demo" id="L" from={P} to={add(P, scale(l.L, 1.2))} role="vecA" />
			</Show>
		</>
	);
}

function LampSphere(): React.JSX.Element {
	const raw = useVal('demo', 'nl_raw', 0);
	return <LitSphere lesson="demo" id="sphere" center={O3} r={SPHERE_R} light="nl_light" mode={raw ? 'raw' : 'lambert'} roles={{ base: 'vecC' }} />;
}

/** DP-4: Lambert's max(0, N·L) as a picture; the checkbox row shows the raw (negative) N·L. */
export const DemoLamp = () => (
	<ViewportCard lesson="demo" id="lamp" name="DEMO-LAMP · N·L" mode="3d" extent={3} overlays={OVLAMP} rows={ROWSLAMP} status={STATUSLAMP}>
		<LampSphere />
		<LampGuides />
		<Point lesson="demo" id="light" at="nl_light" move={moveLight} role="light" label="light" />
		<Point lesson="demo" id="P" at="nl_pt" move={moveP} label="P" />
		<Show ov="labels">
			<Label lesson="demo" id="light" at="nl_light" role="light" offset={TIP}>light</Label>
			<Label lesson="demo" id="P" at="nl_pt" offset={TIP}>P</Label>
		</Show>
	</ViewportCard>
);
