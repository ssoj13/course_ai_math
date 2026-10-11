import { BoxGeometry } from 'three';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useStep, useVals } from '../store';
import { type V } from '../geom';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { RightAngle } from '../scene/RightAngle';
import { AreaTile } from '../scene/AreaTile';
import { Poly } from '../scene/Poly';
import { Circle } from '../scene/Circle';
import { Label } from '../scene/Label';
import { Show, useOverlay } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './pythagoras.rows';
import { derive2, derive3, deriveDis, deriveR, fmt1, fmt2, leftovers, LESSON, triangle, triNormal } from './pythagoras.math';

/**
 * Lesson 0.3 scenes: 0.3-1 distance between two draggable points with the squares on the three sides, 0.3-2 the
 * four-triangle dissection proof (timeline `dis`), 0.3-3 the same in 3D as two Pythagoras steps (timeline `room`),
 * 0.3-5 inside a radius by squared distances. Blender-like flat viewport look (visual language 6d/6e). Loaded lazily
 * by the entry (pythagoras.tsx); shares its store, derive functions and hot ids.
 */

type Vals = Parameters<typeof derive2>[0];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);

/**
 * The reach of a 2D scene: an invisible box over the figure's default and usual range. The home view frames the
 * figure's bounds, so framing the reach keeps the view still while the reader drags, instead of fitting whatever the
 * first frame showed.
 */
function Reach({ lo, hi }: { lo: V; hi: V }) {
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast}
		position={[(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, 0]} scale={[hi[0] - lo[0], hi[1] - lo[1], 0.01]} />;
}

const mid = (a: readonly number[], b: readonly number[]): V => a.map((x, i) => (x + b[i]) / 2);
/** "3.0²", or "(−3.0)²" for a negative leg: the square label shows that the sign does not survive squaring. */
const pow2 = (v: number) => (v < 0 ? '(' + fmt1(v) + ')²' : fmt1(v) + '²');

// ---------------------------------------------------------------------------------------------------------------
// 0.3-1: two points, the legs (the grid walk), the straight line, the squares on the three sides.

function Distance2D() {
	const locale = useLocale();
	const z = derive2(useVals(LESSON));
	const labels = useOverlay('labels');
	const legs = z.dx !== 0 && z.dy !== 0;
	// Squares go outward: on the right of each edge of a counter-clockwise triangle P -> R -> Q, else on the left.
	const flip = !z.ccw;
	const squares = useOverlay('squares');
	// The squares fill the outside of the triangle and its inside is too small for text on a phone. With them shown,
	// each square carries the numbers ("3.0² = 9.00", coloured like its leg; "d² = 25.00") and the sides carry none
	// (d itself is in the formula and the panel); without them the three side labels sit outside the triangle, pushed
	// away from its centroid. A zero leg has no inside: any side will do.
	const g: V = legs ? [(z.P[0] + z.R[0] + z.Q[0]) / 3, (z.P[1] + z.R[1] + z.Q[1]) / 3] : [z.P[0] + 1, z.P[1] - 1];
	return <>
		<Reach lo={[-3.6, -1.6]} hi={[8.6, 9.6]} />
		<Show ov="squares">
			<AreaTile lesson={LESSON} id="dx2" a={z.P} b={z.R} role="vecA" flip={flip} label={labels ? (a) => pow2(z.dx) + ' = ' + fmt2(a) : undefined} />
			<AreaTile lesson={LESSON} id="dy2" a={z.R} b={z.Q} role="vecB" flip={flip} label={labels ? (a) => pow2(z.dy) + ' = ' + fmt2(a) : undefined} />
			<AreaTile lesson={LESSON} id="d2" a={z.Q} b={z.P} role="vecC" flip={flip} label={labels ? (a) => 'd² = ' + fmt2(a) : undefined} />
		</Show>
		{z.dx !== 0 && <Segment lesson={LESSON} id="dx" a={z.P} b={z.R} role="vecA" />}
		{z.dy !== 0 && <Segment lesson={LESSON} id="dy" a={z.R} b={z.Q} role="vecB" />}
		{z.d > 0 && <Segment lesson={LESSON} id="d" a={z.P} b={z.Q} role="vecC" />}
		{legs && <RightAngle lesson={LESSON} id="corner" at={z.R} u={[z.P[0] - z.R[0], 0]} v={[0, z.Q[1] - z.R[1]]} show />}
		{labels && <>
			{!squares && z.dx !== 0 && <Label lesson={LESSON} id="dx" at={mid(z.P, z.R)} away={g} gap={4} role="vecA">{'dx = ' + fmt1(z.dx)}</Label>}
			{!squares && z.dy !== 0 && <Label lesson={LESSON} id="dy" at={mid(z.R, z.Q)} away={g} gap={4} role="vecB">{'dy = ' + fmt1(z.dy)}</Label>}
			{!squares && z.d > 0 && <Label lesson={LESSON} id="d" at={mid(z.P, z.Q)} away={g} gap={4} role="vecC">{'d = ' + fmt2(z.d)}</Label>}
			<Label lesson={LESSON} id="P" at={z.P} offset={[-16, 14]}>P</Label>
			<Label lesson={LESSON} id="Q" at={z.Q} offset={[16, -14]}>Q</Label>
		</>}
		<Point lesson={LESSON} id="P" at="P" role="ink" label={t(locale, 'pt.handleP')} />
		<Point lesson={LESSON} id="Q" at="Q" role="ink" label={t(locale, 'pt.handleQ')} />
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// 0.3-2: the dissection. Four copies of the triangle in a square of side a + b; the timeline slides them.

const TRIS = [0, 1, 2, 3] as const;

function Dissection() {
	const v = useVals(LESSON);
	const step = useStep(LESSON, 'dis');
	const labels = useOverlay('labels');
	const z = deriveDis(v);
	const s = z.s;
	const lab = (txt: string) => (labels ? () => txt : undefined);
	return <>
		<Reach lo={[-0.6, -0.6]} hi={[8.6, 8.6]} />
		{/* The big square: its outline stays while the pieces move inside it. */}
		<Poly lesson={LESSON} id="big" timeline="dis" pts={(w) => { const k = deriveDis(w).s; return [[0, 0], [k, 0], [k, k], [0, k]]; }}
			role="muted" fill={0.04} edge="solid" />
		{/* What the triangles leave uncovered: the tilted square before the slide, two squares after it. */}
		{step <= 1 && <Poly lesson={LESSON} id="c2" timeline="dis" pts={(w) => leftovers(deriveDis(w).a, deriveDis(w).b).c2} role="vecC" fill={0.3} edge="solid"
			label={lab('c² = ' + fmt2(z.c2))} />}
		{step >= 4 && <>
			<Poly lesson={LESSON} id="a2" timeline="dis" pts={(w) => leftovers(deriveDis(w).a, deriveDis(w).b).a2} role="vecA" fill={0.3} edge="solid" label={lab('a² = ' + fmt2(z.a2))} />
			<Poly lesson={LESSON} id="b2" timeline="dis" pts={(w) => leftovers(deriveDis(w).a, deriveDis(w).b).b2} role="vecB" fill={0.3} edge="solid" label={lab('b² = ' + fmt2(z.b2))} />
		</>}
		{TRIS.map((k) => <Poly key={k} lesson={LESSON} id={'tri' + (k + 1)} timeline="dis"
			pts={(w, st) => { const d = deriveDis(w); return triangle(d.a, d.b, k, st); }}
			role="light" fill={0.22} edge="solid" label={step === 1 ? lab('½ab = ' + fmt2(z.tri)) : undefined} />)}
		{labels && <>
			{/* Side lengths on the big square's edges: they read the same in both arrangements. */}
			<Label lesson={LESSON} id="a" at={[z.a / 2, 0]} offset={[0, 14]} role="vecA">a</Label>
			<Label lesson={LESSON} id="b" at={[z.a + z.b / 2, 0]} offset={[0, 14]} role="vecB">b</Label>
			<Label lesson={LESSON} id="big" at={[s / 2, s]} offset={[0, -14]} role="muted">{'a + b = ' + fmt1(s)}</Label>
			{step <= 1 && <Label lesson={LESSON} id="c" at={mid([z.a, 0], [0, z.b])} away={[s / 2, s / 2]} gap={4} role="vecC">c</Label>}
		</>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// 0.3-3: 3D. Walk along x, then along z (the floor, y-up), then up along y.

const BOX_EDGES: readonly (readonly [number, number])[] = [
	[0, 1], [1, 3], [3, 2], [2, 0], [4, 5], [5, 7], [7, 6], [6, 4], [0, 4], [1, 5], [2, 6], [3, 7],
];

/** The 12 edges of the box with opposite corners P and Q (the room the diagonal crosses), dashed. */
function Box({ P, Q }: { P: V; Q: V }) {
	const corner = (i: number): V => [i & 1 ? Q[0] : P[0], i & 4 ? Q[1] : P[1], i & 2 ? Q[2] : P[2]];
	return <>{BOX_EDGES.map(([i, j]) => {
		const a = corner(i), b = corner(j);
		if (a.every((x, k) => x === b[k])) return null;
		return <Segment key={i + '-' + j} lesson={LESSON} id="box" a={a} b={b} role="muted" dashed />;
	})}</>;
}

function Distance3D() {
	const locale = useLocale();
	const z = derive3(useVals(LESSON));
	const step = useStep(LESSON, 'room');
	const labels = useOverlay('labels');
	const floorN = triNormal(z.P, z.X, z.F);
	const upN = triNormal(z.P, z.F, z.Q);
	const squares = useOverlay('squares');
	const sq = (txt: string, leg?: number) => (labels ? (a: number) => (leg === undefined ? txt : pow2(leg)) + ' = ' + fmt2(a) : undefined);
	// A leg is labelled on its line unless its square (which prints the leg) is shown in this step.
	const legLabel = (inSquare: boolean) => !(squares && inSquare);
	return <>
		<Show ov="box"><Box P={z.P} Q={z.Q} /></Show>
		{/* Step 0: the floor triangle P, X, F and its squares. Step 1: the upright triangle P, F, Q and its squares. */}
		<Show ov="squares">
			{step === 0 && floorN && <>
				<AreaTile lesson={LESSON} id="lx2" a={z.P} b={z.X} normal={floorN} role="axisX" label={sq('dx²', z.dx)} />
				<AreaTile lesson={LESSON} id="lz2" a={z.X} b={z.F} normal={floorN} role="axisZ" label={sq('dz²', z.dz)} />
				<AreaTile lesson={LESSON} id="f2" a={z.F} b={z.P} normal={floorN} role="vecB" label={sq('f²')} />
			</>}
			{step === 1 && upN && <>
				<AreaTile lesson={LESSON} id="f2" a={z.P} b={z.F} normal={upN} role="vecB" label={sq('f²')} />
				<AreaTile lesson={LESSON} id="ly2" a={z.F} b={z.Q} normal={upN} role="axisY" label={sq('dy²', z.dy)} />
				<AreaTile lesson={LESSON} id="s2" a={z.Q} b={z.P} normal={upN} role="vecC" label={sq('d²')} />
			</>}
		</Show>
		{z.dx !== 0 && <Segment lesson={LESSON} id="lx" a={z.P} b={z.X} role="axisX" />}
		{z.dz !== 0 && <Segment lesson={LESSON} id="lz" a={z.X} b={z.F} role="axisZ" />}
		{z.dy !== 0 && <Segment lesson={LESSON} id="ly" a={z.F} b={z.Q} role="axisY" />}
		{z.f > 0 && <Segment lesson={LESSON} id="f" a={z.P} b={z.F} role="vecB" dashed={step === 2} />}
		{z.d > 0 && <Segment lesson={LESSON} id="s" a={z.P} b={z.Q} role="vecC" />}
		{z.dx !== 0 && z.dz !== 0 && <RightAngle lesson={LESSON} id="cx" at={z.X} u={[z.P[0] - z.X[0], 0, 0]} v={[0, 0, z.F[2] - z.X[2]]} show />}
		{z.f > 0 && z.dy !== 0 && <RightAngle lesson={LESSON} id="cf" at={z.F} u={[z.P[0] - z.F[0], 0, z.P[2] - z.F[2]]} v={[0, z.Q[1] - z.F[1], 0]} show />}
		{labels && <>
			{/* Only the triangle of the current step is labelled; step 2 names the three legs and the diagonal. */}
			{step !== 1 && legLabel(step === 0) && z.dx !== 0 && <Label lesson={LESSON} id="lx" at={mid(z.P, z.X)} offset={[0, 14]} role="axisX">{'dx = ' + fmt1(z.dx)}</Label>}
			{step !== 1 && legLabel(step === 0) && z.dz !== 0 && <Label lesson={LESSON} id="lz" at={mid(z.X, z.F)} offset={[18, 8]} role="axisZ">{'dz = ' + fmt1(z.dz)}</Label>}
			{step !== 0 && legLabel(step === 1) && z.dy !== 0 && <Label lesson={LESSON} id="ly" at={mid(z.F, z.Q)} offset={[34, 0]} role="axisY">{'dy = ' + fmt1(z.dy)}</Label>}
			{step !== 2 && z.f > 0 && <Label lesson={LESSON} id="f" at={mid(z.P, z.F)} offset={[-10, 16]} role="vecB">{'f = ' + fmt2(z.f)}</Label>}
			{step !== 0 && z.d > 0 && <Label lesson={LESSON} id="s" at={mid(z.P, z.Q)} offset={[-34, -10]} role="vecC">{'d = ' + fmt2(z.d)}</Label>}
			<Label lesson={LESSON} id="P3" at={z.P} offset={[-14, 12]}>P</Label>
			<Label lesson={LESSON} id="Q3" at={z.Q} offset={[14, -12]}>Q</Label>
		</>}
		<Point lesson={LESSON} id="P3" at="P3" role="ink" label={t(locale, 'pt.handleP')} />
		<Point lesson={LESSON} id="Q3" at="Q3" role="ink" label={t(locale, 'pt.handleQ')} />
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// 0.3-5: inside the radius. The test compares d² with r²; the root is never needed.

function Radius() {
	const locale = useLocale();
	const z = deriveR(useVals(LESSON));
	const labels = useOverlay('labels');
	const X: V = [z.x, z.y];
	const word = t(locale, z.side === 'in' ? 'pt.inside' : z.side === 'on' ? 'pt.onCircle' : 'pt.outside');
	return <>
		<Reach lo={[-4.6, -4.6]} hi={[4.6, 4.6]} />
		<Circle lesson={LESSON} id="r2" center={[0, 0]} r={(w) => w.r as number} role="vecC" dashPx={[7, 5]} />
		<Show ov="legs">
			{z.x !== 0 && z.y !== 0 && <>
				<Segment lesson={LESSON} id="lgx" a={[0, 0]} b={[z.x, 0]} role="axisX" dashed />
				<Segment lesson={LESSON} id="lgy" a={[z.x, 0]} b={X} role="axisY" dashed />
			</>}
		</Show>
		{z.d > 0 && <Segment lesson={LESSON} id="xd" a={[0, 0]} b={X} role="vecA" />}
		<Point lesson={LESSON} id="o" at={[0, 0]} role="muted" size={6} />
		{labels && <>
			<Label lesson={LESSON} id="r2" at={[z.r * Math.SQRT1_2, -z.r * Math.SQRT1_2]} offset={[16, 12]} role="vecC">{'r = ' + fmt1(z.r)}</Label>
			<Label lesson={LESSON} id="X" at={X} offset={[0, z.y >= 0 ? -18 : 18]} role={z.side === 'out' ? 'muted' : 'hot'}>{word}</Label>
			{z.d > 0 && <Label lesson={LESSON} id="xd" at={mid([0, 0], X)} away={[-z.y, z.x]} gap={6} role="vecA">{'d² = ' + fmt2(z.d2)}</Label>}
		</>}
		<Point lesson={LESSON} id="X" at="X" role="ink" label={t(locale, 'pt.handleX')} />
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

function statusFor(scene: Scene, locale: Locale): StatusSpec {
	if (scene === 1) {
		return {
			hints: {
				P: { mouse: t(locale, 'pt.hintPointMouse'), touch: t(locale, 'pt.hintPointTouch') },
				Q: { mouse: t(locale, 'pt.hintPointMouse'), touch: t(locale, 'pt.hintPointTouch') },
				dx: (v: Vals) => t(locale, 'pt.hintDx', { dx: fmt1(derive2(v).dx) }),
				dy: (v: Vals) => t(locale, 'pt.hintDy', { dy: fmt1(derive2(v).dy) }),
				d: (v: Vals) => t(locale, 'pt.hintD', { d: fmt2(derive2(v).d), walk: fmt1(derive2(v).walk) }),
				dx2: (v: Vals) => t(locale, 'pt.hintSquare', { name: 'dx²', value: fmt2(derive2(v).dx2) }),
				dy2: (v: Vals) => t(locale, 'pt.hintSquare', { name: 'dy²', value: fmt2(derive2(v).dy2) }),
				d2: (v: Vals) => t(locale, 'pt.hintD2', { d2: fmt2(derive2(v).d2) }),
			},
			idle: { mouse: t(locale, 'pt.idle1Mouse'), touch: t(locale, 'pt.idle1Touch') },
		};
	}
	if (scene === 2) {
		return {
			hints: {
				tri1: t(locale, 'pt.hintTri'), tri2: t(locale, 'pt.hintTri'), tri3: t(locale, 'pt.hintTri'), tri4: t(locale, 'pt.hintTri'),
				c2: (v: Vals) => t(locale, 'pt.hintSquare', { name: 'c²', value: fmt2(deriveDis(v).c2) }),
				a2: (v: Vals) => t(locale, 'pt.hintSquare', { name: 'a²', value: fmt2(deriveDis(v).a2) }),
				b2: (v: Vals) => t(locale, 'pt.hintSquare', { name: 'b²', value: fmt2(deriveDis(v).b2) }),
				big: (v: Vals) => t(locale, 'pt.hintBig', { big: fmt2(deriveDis(v).big) }),
			},
			idle: { mouse: t(locale, 'pt.idle2Mouse'), touch: t(locale, 'pt.idle2Touch') },
		};
	}
	if (scene === 3) {
		return {
			hints: {
				P3: { mouse: t(locale, 'pt.hint3Mouse'), touch: t(locale, 'pt.hintPointTouch') },
				Q3: { mouse: t(locale, 'pt.hint3Mouse'), touch: t(locale, 'pt.hintPointTouch') },
				f: (v: Vals) => t(locale, 'pt.hintF', { f: fmt2(derive3(v).f) }),
				s: (v: Vals) => t(locale, 'pt.hintS', { d: fmt2(derive3(v).d) }),
				box: t(locale, 'pt.hintBox'),
			},
			idle: { mouse: t(locale, 'pt.idle3Mouse'), touch: t(locale, 'pt.idle3Touch') },
		};
	}
	return {
		hints: {
			X: { mouse: t(locale, 'pt.hintPointMouse'), touch: t(locale, 'pt.hintPointTouch') },
			r2: (v: Vals) => t(locale, 'pt.hintCircle', { r: fmt1(deriveR(v).r), r2: fmt2(deriveR(v).r2) }),
			xd: (v: Vals) => t(locale, 'pt.hintXd', { d2: fmt2(deriveR(v).d2), d: fmt2(deriveR(v).d) }),
		},
		idle: { mouse: t(locale, 'pt.idle5Mouse'), touch: t(locale, 'pt.idle5Touch') },
	};
}

const CARD = {
	1: { id: '0.3-1', name: 'pt.name1' }, 2: { id: '0.3-2', name: 'pt.name2' }, 3: { id: '0.3-3', name: 'pt.name3' }, 5: { id: '0.3-5', name: 'pt.name5' },
} as const;

function SceneGrid({ extent }: { extent: number }) {
	return <Grid lesson={LESSON} extent={extent} step={1} lines={useOverlay('grid')} axes={useOverlay('axes')} />;
}

export function PythagorasScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	const base = [ov('grid', 'pt.ovGrid'), ov('axes', 'pt.ovAxes'), ov('labels', 'pt.ovLabels')];
	const overlays = scene === 1 ? [...base, ov('squares', 'pt.ovSquares')]
		: scene === 2 ? [ov('labels', 'pt.ovLabels')]
		// 3D: the legs run parallel to the axes in the axis colours; axis lines through the origin would sit on them.
		: scene === 3 ? [ov('grid', 'pt.ovGrid'), ov('axes', 'pt.ovAxes', false), ov('labels', 'pt.ovLabels'), ov('squares', 'pt.ovSquares'), ov('box', 'pt.ovBox')]
		: [...base, ov('legs', 'pt.ovLegs')];
	const extent = scene === 1 ? 9 : scene === 2 ? 5 : scene === 3 ? 3 : 4.5;
	const center: V = scene === 1 ? [2.5, 4] : scene === 2 ? [4, 4] : [0, 0];
	return <ViewportCard lesson={LESSON} id={CARD[scene].id} name={CARD[scene].id + ' · ' + t(locale, CARD[scene].name)}
		mode={scene === 3 ? '3d' : '2d'} extent={extent} center={center}
		rows={rowsFor(scene, locale)} overlays={overlays} status={statusFor(scene, locale)} drawIn={scene === 1}>
		{scene !== 2 && <SceneGrid extent={extent} />}
		{scene === 1 ? <Distance2D /> : scene === 2 ? <Dissection /> : scene === 3 ? <Distance3D /> : <Radius />}
	</ViewportCard>;
}
