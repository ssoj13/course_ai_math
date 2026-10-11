import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
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
import { TileGrid } from '../scene/TileGrid';
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

const OV2 = (locale: Locale): readonly OverlayDef[] => [
	{ id: 'grid', label: t(locale, 'demo.grid'), on: true },
	{ id: 'labels', label: t(locale, 'demo.labels'), on: true },
	{ id: 'angle', label: t(locale, 'demo.angle'), on: true },
	{ id: 'shadow', label: t(locale, 'demo.shadow'), on: true },
	{ id: 'square', label: t(locale, 'demo.squareA'), on: true },
	{ id: 'par', label: t(locale, 'demo.parBA'), on: false },
];

const STATUS2 = (locale: Locale): StatusSpec => ({
	hints: {
		'a:tip': { mouse: t(locale, 'demo.aMouse'), touch: t(locale, 'demo.aTouch') },
		'b:tip': { mouse: t(locale, 'demo.bMouse'), touch: t(locale, 'demo.bTouch') },
		'p:pt': { mouse: t(locale, 'demo.pMouse'), touch: t(locale, 'demo.pTouch') },
		a: (v) => t(locale, 'demo.vector', { name: 'a', value: vec(v, 'a').map(num).join(', ') }),
		b: (v) => t(locale, 'demo.vector', { name: 'b', value: vec(v, 'b').map(num).join(', ') }),
		th: (v) => t(locale, 'demo.angleValue', { name: 'θ', value: angleOf(vec(v, 'a'), vec(v, 'b')).toFixed(1) }),
		proj: t(locale, 'demo.shadowHint'),
		drop: t(locale, 'demo.dropHint'),
		sqa: (v) => `|a|² = ${num(dot(vec(v, 'a'), vec(v, 'a')))}`,
		par: (v) => t(locale, 'demo.hatched', { value: `b×a = ${num(signedArea2(vec(v, 'b'), vec(v, 'a')))}` }),
	},
	// isRight: dragged grid values are perpendicular only up to rounding (a = (3, 1), b = (-0.3, 0.9): a·b = 1e-16).
	state: (v) => (isRight(vec(v, 'a'), vec(v, 'b')) ? 'a ⊥ b · a·b = 0' : null),
});

/** 2D: grid, draggable a and b, their angle, b's shadow on a, the square on a, the signed parallelogram, a point. */
export const Demo2D = () => {
 const locale = useLocale();
 return (
	<ViewportCard lesson="demo" id="d2" name={t(locale, 'demo.name2')} mode="2d" extent={4} overlays={OV2(locale)} rows={ROWS2(locale)} status={STATUS2(locale)} drawIn>
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
);};

const OV3 = (locale: Locale): readonly OverlayDef[] => [
	{ id: 'grid', label: t(locale, 'demo.floorGrid'), on: true },
	{ id: 'labels', label: t(locale, 'demo.labels'), on: true },
	{ id: 'angle', label: t(locale, 'demo.angle'), on: true },
	{ id: 'par', label: t(locale, 'demo.par'), on: true },
	{ id: 'disc', label: t(locale, 'demo.disc'), on: true },
];

const STATUS3 = (locale: Locale): StatusSpec => ({
	hints: {
		u: (v) => t(locale, 'demo.vector', { name: 'u', value: vec(v, 'u').map(num).join(', ') }),
		v: (v) => t(locale, 'demo.vector', { name: 'v', value: vec(v, 'v').map(num).join(', ') }),
		phi: (v) => t(locale, 'demo.angleValue', { name: 'φ', value: angleOf(vec(v, 'u'), vec(v, 'v')).toFixed(1) }),
		par3: (v) => `|u×v| = ${num(norm(cross(vec(v, 'u'), vec(v, 'v'))))}`,
	},
});

/** 3D: two arrows with legs, their angle (normal = up picks the plane at 180 degrees), their parallelogram, labels. */
export const Demo3D = () => {
 const locale = useLocale();
 return (
	<ViewportCard lesson="demo" id="d3" name={t(locale, 'demo.name3')} mode="3d" extent={3} overlays={OV3(locale)} rows={ROWS3(locale)} status={STATUS3(locale)}>
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
);};

const OVLC = (locale: Locale): readonly OverlayDef[] => [
	{ id: 'grid', label: t(locale, 'demo.grid'), on: true },
	{ id: 'squares', label: t(locale, 'demo.squares'), on: true },
	{ id: 'corr', label: t(locale, 'demo.corr'), on: true },
	{ id: 'labels', label: t(locale, 'demo.labels'), on: true },
];

const STATUSLC = (locale: Locale): StatusSpec => ({
	hints: {
		sqa: (v) => `a² = ${num(dot(vec(v, 'a'), vec(v, 'a')))}`,
		sqb: (v) => `b² = ${num(dot(vec(v, 'b'), vec(v, 'b')))}`,
		sqc: 'c² = a² + b² − 2a·b',
		corr: (v) => t(locale, 'demo.hatched', { value: `−2a·b = ${num(-2 * dot(vec(v, 'a'), vec(v, 'b')))}` }),
	},
});

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
export const DemoCosines = () => {
 const locale = useLocale();
 return (
	<ViewportCard lesson="demo" id="lc" name={t(locale, 'demo.nameLC')} mode="2d" extent={6} overlays={OVLC(locale)} rows={ROWSLC(locale)} status={STATUSLC(locale)}>
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
);};

// ---------------------------------------------------------------------------------------------------------------
// Task 6e lesson primitives.

const OVPOLAR = (locale: Locale): readonly OverlayDef[] => [
	{ id: 'grid', label: t(locale, 'demo.grid'), on: true },
	{ id: 'labels', label: t(locale, 'demo.labels'), on: true },
	{ id: 'locus', label: t(locale, 'demo.locus'), on: true },
];

const STATUSPOLAR = (locale: Locale): StatusSpec => ({
	hints: {
		'A:pt': { mouse: t(locale, 'demo.polarMouse'), touch: t(locale, 'demo.polarTouch') },
		'B:pt': { mouse: t(locale, 'demo.baseMouse'), touch: t(locale, 'demo.baseTouch') },
		locus: t(locale, 'demo.locusHint'),
	},
});

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
export const DemoPolar = () => {
 const locale = useLocale();
 return (
	<ViewportCard lesson="demo" id="pol" name={t(locale, 'demo.namePolar')} mode="2d" extent={5} center={[1, 1.5]} overlays={OVPOLAR(locale)} rows={ROWSPOLAR(locale)} status={STATUSPOLAR(locale)}>
		<Show ov="grid">
			<Grid lesson="demo" extent={5} step={1} />
		</Show>
		<Show ov="locus">
			<Circle lesson="demo" id="locus" center={O2} r={(x) => x.lb as number} role="muted" dashPx={STYLE.dash} />
		</Show>
		<PolarTriangle />
	</ViewportCard>
);};

const OVSIGN = (locale: Locale): readonly OverlayDef[] => [
	{ id: 'grid', label: t(locale, 'demo.grid'), on: true },
	{ id: 'half', label: t(locale, 'demo.half'), on: true },
	{ id: 'legs', label: t(locale, 'demo.legs'), on: true },
];

const STATUSSIGN = (locale: Locale): StatusSpec => ({
	hints: {
		'sb:tip': { mouse: t(locale, 'demo.sbMouse'), touch: t(locale, 'demo.sbTouch') },
		'sa:tip': { mouse: t(locale, 'demo.saMouse'), touch: t(locale, 'demo.saTouch') },
		hp_pos: t(locale, 'demo.positive'),
		hp_neg: t(locale, 'demo.negative'),
		'sa:x': (v) => `a₁ = ${num(vec(v, 'sa')[0])}`,
		'sa:y': (v) => `a₂ = ${num(vec(v, 'sa')[1])}`,
	},
	state: (v) => (Math.abs(dot(vec(v, 'sa'), vec(v, 'sb'))) < 1e-9 ? 'a ⊥ b · a·b = 0' : null),
});

function SignRight(): React.JSX.Element | null {
	const a = use2('sa');
	const b = use2('sb');
	return norm(a) && norm(b) && Math.abs(dot(a, b)) < 1e-9 ? <RightAngle lesson="demo" id="sdot" at={O2} u={a} v={b} show /> : null;
}

/** DP-2: the plane pre-coloured by the sign of a·v, b's magnet to the perpendicular, per-axis leg ids of a. */
export const DemoSign = () => {
 const locale = useLocale();
 return (
	<ViewportCard lesson="demo" id="sign" name={t(locale, 'demo.nameSign')} mode="2d" extent={4} overlays={OVSIGN(locale)} rows={ROWSSIGN(locale)} status={STATUSSIGN(locale)}>
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
);};

const OVLAMP = (locale: Locale): readonly OverlayDef[] => [
	{ id: 'term', label: t(locale, 'demo.term'), on: true },
	{ id: 'vecs', label: t(locale, 'demo.vecs'), on: true },
	{ id: 'labels', label: t(locale, 'demo.labels'), on: true },
];

const STATUSLAMP = (locale: Locale): StatusSpec => ({
	hints: {
		'light:pt': { mouse: t(locale, 'demo.lightMouse'), touch: t(locale, 'demo.lightTouch') },
		'P:pt': { mouse: t(locale, 'demo.surfaceMouse'), touch: t(locale, 'demo.surfaceTouch') },
		term: t(locale, 'demo.termHint'),
		N: t(locale, 'demo.normalHint'),
		L: t(locale, 'demo.lightHint'),
	},
});

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
export const DemoLamp = () => {
 const locale = useLocale();
 return (
	<ViewportCard lesson="demo" id="lamp" name={t(locale, 'demo.nameLamp')} mode="3d" extent={3} overlays={OVLAMP(locale)} rows={ROWSLAMP(locale)} status={STATUSLAMP(locale)}>
		<LampSphere />
		<LampGuides />
		<Point lesson="demo" id="light" at="nl_light" move={moveLight} role="light" label={t(locale, 'rows.light')} />
		<Point lesson="demo" id="P" at="nl_pt" move={moveP} label="P" />
		<Show ov="labels">
			<Label lesson="demo" id="light" at="nl_light" role="light" offset={TIP}>{t(locale, 'rows.light')}</Label>
			<Label lesson="demo" id="P" at="nl_pt" offset={TIP}>P</Label>
		</Show>
	</ViewportCard>
);};

/**
 * DEMO-TILES: TileGrid in 3D, the primitive proof for the powers lesson. A cube of side 3 (27 unit bodies, seams
 * between them), a cube of side 2.5 (8 whole bodies, clipped slabs on its three far faces in the partial colour) and a
 * slab of 2.5 x 1.5 lying in the xz plane (thin y). Hover outlines a whole shape.
 */
export const DemoTiles = () => {
	const locale = useLocale();
	return (
		<ViewportCard lesson="demo" id="tiles" name={t(locale, 'demo.nameTiles')} mode="3d" extent={4}>
			<Grid lesson="demo" extent={4} step={1} />
			<TileGrid lesson="demo" id="cube3" origin={[-4, 0, -1.5]} shape={{ kind: 'box', size: [3, 3, 3], thin: [false, false, false] }} role="vecA" count label />
			<TileGrid lesson="demo" id="cube25" origin={[-0.5, 0, -1.25]} shape={{ kind: 'box', size: [2.5, 2.5, 2.5], thin: [false, false, false] }} role="vecB" count label />
			<TileGrid lesson="demo" id="slab" origin={[2.6, 0, -0.75]} shape={{ kind: 'box', size: [2.5, 0, 1.5], thin: [false, true, false] }} plane="xz" role="vecC" count label />
		</ViewportCard>
	);
};
