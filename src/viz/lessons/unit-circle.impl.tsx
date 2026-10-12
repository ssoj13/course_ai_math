import { BoxGeometry } from 'three';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useHot, useVals } from '../store';
import type { V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Circle } from '../scene/Circle';
import { RightAngle } from '../scene/RightAngle';
import { Label } from '../scene/Label';
import { Stroke } from '../scene/view';
import { HOVER_RANK, Show, useHoverable, useOverlay, type Draggable } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './unit-circle.rows';
import {
	arc, cosSin, derive, fmt2, fmt3, fmtDeg, graph, GRAPH_X0, LESSON, MAGNET, moveAlong, moveOnCircle, rad, unrollAt, unrolled,
	type P3,
} from './unit-circle.math';

/**
 * Lesson 0.4 scenes: UC-1 cos and sin as the shadows of a point on the unit circle, UC-2 the arc unrolled onto a
 * radian ruler, UC-3 the cos and sin curves traced as the angle sweeps, UC-4 the special angles and the 45-degree
 * mirror. Blender-like flat viewport look (Task 6d). All four share one angle `th` (linked views). Loaded lazily by
 * the entry (unit-circle.tsx).
 */

type Vals = Parameters<typeof derive>[0];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);
const TAU = 2 * Math.PI;

/**
 * The reach of a scene: an invisible box over everything the reader can make it draw. The home view frames the
 * figure's bounds, so framing the reach keeps the view still while the point travels around the circle.
 */
function Reach({ lo, hi }: { lo: V; hi: V }) {
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast}
		position={[(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, 0]} scale={[hi[0] - lo[0], hi[1] - lo[1], 0.01]} />;
}

/**
 * An open polyline (an arc past 180 degrees, a traced graph, a ruler): the engine's one stroke recipe, hoverable by
 * id. `thin` = a secondary line (edge width, no halo), else a beam. Null points draw nothing (a zero angle).
 */
function Curve({ id, pts, role, thin = false, dashed = false, segments = false, rank = HOVER_RANK.guide }: {
	id?: string; pts: P3[] | null; role: SceneRole; thin?: boolean; dashed?: boolean; segments?: boolean; rank?: number;
}) {
	const pal = usePalette();
	const hot = useHot(LESSON, id ?? '');
	const hover = useHoverable(LESSON, id, rank);
	if (!pts || pts.length < 2) return null;
	return <group {...hover}>
		<Stroke points={pts} color={pal[role]} hot={Boolean(id) && hot} width={thin ? STYLE.edge : STYLE.core} halo={!thin}
			dashPx={dashed ? STYLE.dash : undefined} segments={segments} order={1} />
	</group>;
}

/** Point P on the unit circle at the current angle. */
const onCircle = (v: Vals): V => cosSin(derive(v).th);

/** The draggable point P: dragged by angle around the circle, magnet on the special angles. */
function HandleP({ role = 'vecA', move = moveOnCircle }: { role?: SceneRole; move?: Draggable['move'] }) {
	const locale = useLocale();
	return <Point lesson={LESSON} id="P" at={onCircle} keys={['th']} move={move} role={role} label={t(locale, 'uc.handleP')} />;
}

/** Short radial marks just outside the circle at the magnet angles: where a dragged point will stick. */
function MagnetTicks() {
	const pts: P3[] = [];
	for (const d of MAGNET) {
		if (d === 360) continue;
		const [c, s] = cosSin(d);
		pts.push([c * 1.03, s * 1.03, 0], [c * 1.1, s * 1.1, 0]);
	}
	return <Curve pts={pts} role="muted" thin segments />;
}

/** The angle theta as an arc from the positive x axis (any span up to a full turn) with its value at the bisector. */
function ThetaArc({ th, r, role = 'vecC', id = 'th', from = 0, text }: { th: number; r: number; role?: SceneRole; id?: string; from?: number; text: string }) {
	const labels = useOverlay('labels');
	const to = from + th;
	const [bx, by] = cosSin(from + th / 2);
	return <>
		<Curve id={id} pts={arc([0, 0], r, from, to)} role={role} thin rank={HOVER_RANK.arc} />
		{labels && th !== 0 && <Label lesson={LESSON} id={id} at={[bx * r, by * r]} away={[0, 0]} gap={6} role={role}>{text}</Label>}
	</>;
}

/** Signs of the coordinates in each quadrant; the quadrant the point is in is drawn bright. */
function QuadrantSigns({ quad }: { quad: number }) {
	const q: [number, V, string][] = [[1, [1.18, 1.18], '(+, +)'], [2, [-1.18, 1.18], '(−, +)'], [3, [-1.18, -1.18], '(−, −)'], [4, [1.18, -1.18], '(+, −)']];
	return <>{q.map(([n, at, text]) => <Label key={n} lesson={LESSON} id={'q' + n} at={at} role={quad === n ? 'ink' : 'muted'}>{text}</Label>)}</>;
}

/**
 * UC-1: P on the unit circle; its x shadow (cos, a solid bar on the x axis) and y shadow (sin, a bar on the y axis):
 * beams, not translucent shadow bands, which vanished on the axis lines of the same color. The
 * two dashed legs from P to the axes, which have the same lengths (the horizontal one is |cos|, the vertical |sin|).
 */
function Shadows() {
	const z = derive(useVals(LESSON));
	const labels = useOverlay('labels');
	const P: V = [z.cos, z.sin];
	const fx: V = [z.cos, 0];
	const fy: V = [0, z.sin];
	// Labels sit on the side away from P so they never cover the legs or the point.
	const side = z.cos >= 0 ? -1 : 1;
	return <>
		<Reach lo={[-1.35, -1.35]} hi={[1.35, 1.35]} />
		<Circle lesson={LESSON} id="circle" center={[0, 0]} r={1} role="muted" />
		<Show ov="ticks"><MagnetTicks /></Show>
		<ThetaArc th={z.th} r={0.28} text="θ" />
		<Segment lesson={LESSON} id="radius" a={[0, 0]} b={P} role="vecA" />
		{z.cos !== 0 && <Segment lesson={LESSON} id="cosb" a={[0, 0]} b={fx} role="axisX" />}
		{z.sin !== 0 && <Segment lesson={LESSON} id="sinb" a={[0, 0]} b={fy} role="axisY" />}
		{z.cos !== 0 && z.sin !== 0 && <>
			<Segment lesson={LESSON} id="cosl" a={P} b={fy} dashed role="axisX" />
			<Segment lesson={LESSON} id="sinl" a={P} b={fx} dashed role="axisY" />
			<RightAngle lesson={LESSON} at={fx} u={[0, z.sin]} v={[-z.cos, 0]} show />
		</>}
		<Point lesson={LESSON} id="cosf" at={fx} role="axisX" size={5} />
		<Point lesson={LESSON} id="sinf" at={fy} role="axisY" size={5} />
		{labels && <>
			{/* Pushed off the bar (away from P's side) by its own height + gap, not a fixed 16 px: label separation then keeps
			    moving it the same way past the tick numbers instead of trying "up", which put the red text on its red bar. */}
			<Label lesson={LESSON} id="cos" at={[z.cos / 2, 0]} away={[z.cos / 2, z.sin >= 0 ? 1 : -1]} gap={7} role="axisX">{'cos θ = ' + fmt2(z.cos)}</Label>
			<Label lesson={LESSON} id="sin" at={[0, z.sin / 2]} offset={[side * 48, 0]} role="axisY">{'sin θ = ' + fmt2(z.sin)}</Label>
			<Label lesson={LESSON} id="P" at={P} away={[0, 0]} gap={10}>{'(' + fmt2(z.cos) + ', ' + fmt2(z.sin) + ')'}</Label>
		</>}
		<Show ov="signs"><QuadrantSigns quad={z.quad} /></Show>
		<Point lesson={LESSON} id="o" at={[0, 0]} role="muted" size={5} />
		<HandleP />
	</>;
}

/** UC-2 handle: on the circle while the arc is rolled up, along the ruler once it has started to unroll. */
const moveUnroll: Draggable['move'] = (vals, raw, src) => ((vals.roll as number) === 0 ? moveOnCircle(vals, raw, src) : moveAlong(vals, raw, src));

/**
 * The radian ruler: on the x axis from the arc's start (1, 0), one mark per radian (numbered below), and pi and 2 pi
 * marked above, in the color of the arc being measured.
 */
function RadianRuler() {
	const labels = useOverlay('labels');
	const marks: P3[] = [];
	for (let k = 0; k <= 6; k++) marks.push([1 + k, -0.09, 0], [1 + k, 0.09, 0]);
	const pis: P3[] = [[1 + Math.PI, -0.14, 0], [1 + Math.PI, 0.22, 0], [1 + TAU, -0.14, 0], [1 + TAU, 0.22, 0]];
	return <>
		<Curve id="ruler" pts={[[1, 0, 0], [1 + TAU + 0.35, 0, 0]]} role="ink" thin />
		<Curve pts={marks} role="ink" thin segments />
		<Curve pts={pis} role="vecB" thin segments />
		{labels && <>
			{[0, 1, 2, 3, 4, 5, 6].map((k) => <Label key={k} lesson={LESSON} at={[1 + k, 0]} offset={[0, 14]} role="muted">{String(k)}</Label>)}
			<Label lesson={LESSON} at={[1 + Math.PI, 0.22]} offset={[0, -10]} role="vecB">π</Label>
			<Label lesson={LESSON} at={[1 + TAU, 0.22]} offset={[0, -10]} role="vecB">2π</Label>
		</>}
	</>;
}

/**
 * UC-2: the arc of the angle on the unit circle, unrolled onto a ruler. Its length in radius units IS the angle in
 * radians: the radius (r = 1) sits next to it for comparison, and a full turn lands on 2 pi = 6.28.
 */
function Unroll() {
	const locale = useLocale();
	const z = derive(useVals(LESSON));
	const labels = useOverlay('labels');
	const end = unrollAt(z.rad, z.roll);
	return <>
		<Reach lo={[-1.4, -1.4]} hi={[1 + TAU + 0.6, 1.4]} />
		<Circle lesson={LESSON} id="circle" center={[0, 0]} r={1} role="muted" />
		<RadianRuler />
		{/* Where the arc came from, while it is (partly) unrolled. */}
		<Show ov="ghost">{z.roll > 0 && <Curve pts={arc([0, 0], 1, 0, z.th)} role="vecA" thin dashed />}</Show>
		<Segment lesson={LESSON} id="r" a={[0, 0]} b={[1, 0]} role="vecC" />
		{labels && <Label lesson={LESSON} id="r" at={[0.5, 0]} offset={[0, 14]} role="vecC">r = 1</Label>}
		<Curve id="arc" pts={unrolled(z.th, z.roll)} role="vecA" rank={HOVER_RANK.beam} />
		{labels && z.th > 0 && <Label lesson={LESSON} id="arc" at={end} away={z.roll < 0.5 ? [0, 0] : [end[0], -1]} gap={10} role="vecA">
			{t(locale, 'uc.labelArc', { s: fmt3(z.rad) })}
		</Label>}
		<Point lesson={LESSON} id="o" at={[0, 0]} role="muted" size={5} />
		<Point lesson={LESSON} id="P" at={(v) => unrollAt(rad(derive(v).th), derive(v).roll)} keys={['th']} move={moveUnroll} role="vecA" label={t(locale, 'uc.handleP')} />
	</>;
}

/** UC-3 graph frame: the graph's own vertical axis, the lines y = +-1 and a dashed mark at every quarter turn. */
function GraphFrame() {
	const labels = useOverlay('labels');
	const X1 = GRAPH_X0 + TAU;
	const quarters = [90, 180, 270, 360];
	const guides: P3[] = [[GRAPH_X0, 1, 0], [X1, 1, 0], [GRAPH_X0, -1, 0], [X1, -1, 0]];
	for (const d of quarters) guides.push([GRAPH_X0 + rad(d), -1, 0], [GRAPH_X0 + rad(d), 1, 0]);
	return <>
		<Curve pts={[[GRAPH_X0, -1.2, 0], [GRAPH_X0, 1.2, 0]]} role="ink" thin />
		<Curve pts={guides} role="muted" thin dashed segments />
		{labels && <>
			{quarters.map((d) => <Label key={d} lesson={LESSON} at={[GRAPH_X0 + rad(d), -1]} offset={[0, 14]} role="muted">{fmtDeg(d)}</Label>)}
			<Label lesson={LESSON} at={[GRAPH_X0, 1]} offset={[-12, 0]} role="muted">1</Label>
			<Label lesson={LESSON} at={[GRAPH_X0, -1]} offset={[-14, 0]} role="muted">−1</Label>
		</>}
	</>;
}

/**
 * UC-3: the circle on the left and both curves on the right, theta along the graph in radians (1 world unit = 1
 * radian, the UC-2 ruler). The part already swept is drawn solid, the rest dashed. The sin point is level with P
 * (a dashed line carries the y shadow across); each graph point drops to the axis by its value, as long as the
 * shadow of the same color on the circle.
 */
function Waves() {
	const locale = useLocale();
	const z = derive(useVals(LESSON));
	const labels = useOverlay('labels');
	const gx = GRAPH_X0 + z.rad;
	const C: V = [gx, z.cos];
	const S: V = [gx, z.sin];
	const P: V = [z.cos, z.sin];
	return <>
		{/* Right of the graph's end: room for the cos/sin value labels beside the curve ends (48 px out + their width); with
		    0.6 they hit the stage edge at 360 degrees and were moved back onto the curves and the x axis letter. */}
		<Reach lo={[-1.4, -1.45]} hi={[GRAPH_X0 + TAU + 1.7, 1.45]} />
		<Circle lesson={LESSON} id="circle" center={[0, 0]} r={1} role="muted" />
		<GraphFrame />
		<Show ov="ghost">
			<Curve pts={graph('cos', z.th, 360)} role="axisX" thin dashed />
			<Curve pts={graph('sin', z.th, 360)} role="axisY" thin dashed />
		</Show>
		<Curve id="cosc" pts={graph('cos', 0, z.th)} role="axisX" rank={HOVER_RANK.beam} />
		<Curve id="sinc" pts={graph('sin', 0, z.th)} role="axisY" rank={HOVER_RANK.beam} />
		<Segment lesson={LESSON} id="radius" a={[0, 0]} b={P} role="vecA" />
		{z.cos !== 0 && <Segment lesson={LESSON} id="cosb" a={[0, 0]} b={[z.cos, 0]} role="axisX" />}
		{z.sin !== 0 && <Segment lesson={LESSON} id="sinb" a={[0, 0]} b={[0, z.sin]} role="axisY" />}
		<Segment lesson={LESSON} id="carry" a={P} b={S} dashed role="axisY" />
		{z.cos !== 0 && <Segment lesson={LESSON} id="cosd" a={C} b={[gx, 0]} dashed role="axisX" />}
		{z.sin !== 0 && <Segment lesson={LESSON} id="sind" a={S} b={[gx, 0]} dashed role="axisY" />}
		{labels && <>
			<Label lesson={LESSON} id="cos" at={C} offset={[48, 0]} role="axisX">{'cos = ' + fmt2(z.cos)}</Label>
			<Label lesson={LESSON} id="sin" at={S} offset={[48, 0]} role="axisY">{'sin = ' + fmt2(z.sin)}</Label>
		</>}
		<Point lesson={LESSON} id="o" at={[0, 0]} role="muted" size={5} />
		<Point lesson={LESSON} id="C" at={(v) => [GRAPH_X0 + rad(derive(v).th), derive(v).cos]} keys={['th']} move={moveAlong} role="axisX" label={t(locale, 'uc.handleCos')} />
		<Point lesson={LESSON} id="S" at={(v) => [GRAPH_X0 + rad(derive(v).th), derive(v).sin]} keys={['th']} move={moveAlong} role="axisY" label={t(locale, 'uc.handleSin')} />
		<HandleP />
	</>;
}

/** UC-4 mirror handle: Q is P mirrored across y = x, at 90 - theta; dragging Q to angle a sets theta = 90 - a. */
const moveMirror: Draggable['move'] = (vals, raw, src) => {
	const mirrored = { ...vals, th: derive(vals).mir };
	const m = moveOnCircle(mirrored, raw, src).set.th as number;
	const th = (((90 - m) % 360) + 360) % 360;
	// Keep 360 reachable from Q as from P: the full turn and the start are the same point.
	return { set: { th: th === 0 && (vals.th as number) > 180 ? 360 : th } };
};

/**
 * UC-4: P at theta and its mirror image Q across the 45-degree diagonal. Mirroring across y = x swaps the two
 * coordinates, so Q = (sin theta, cos theta), the point at 90 - theta: 30 and 60 degrees trade their cos and sin.
 * The chord PQ crosses the diagonal at a right angle, at its midpoint.
 */
function Mirror() {
	const locale = useLocale();
	const z = derive(useVals(LESSON));
	const labels = useOverlay('labels');
	const P: V = [z.cos, z.sin];
	const Q: V = [z.sin, z.cos];
	const M: V = [(z.cos + z.sin) / 2, (z.cos + z.sin) / 2];
	const apart = Math.hypot(z.cos - z.sin, z.sin - z.cos) > 1e-6;
	return <>
		<Reach lo={[-1.35, -1.35]} hi={[1.35, 1.35]} />
		<Circle lesson={LESSON} id="circle" center={[0, 0]} r={1} role="muted" />
		<Show ov="ticks"><MagnetTicks /></Show>
		<Segment lesson={LESSON} id="diag" a={[-1.3, -1.3]} b={[1.3, 1.3]} dashed role="muted" />
		{labels && <Label lesson={LESSON} id="diag" at={[1.3, 1.3]} offset={[-22, -6]} role="muted">y = x</Label>}
		<ThetaArc th={z.th} r={0.3} role="vecA" id="th" text="θ" />
		<ThetaArc th={-z.th} r={0.42} role="vecB" id="thq" from={90} text="θ" />
		<Segment lesson={LESSON} id="radius" a={[0, 0]} b={P} role="vecA" />
		<Segment lesson={LESSON} id="radq" a={[0, 0]} b={Q} role="vecB" />
		<Show ov="legs">
			{z.cos !== 0 && z.sin !== 0 && <>
				<Segment lesson={LESSON} id="cosl" a={P} b={[0, z.sin]} dashed role="axisX" />
				<Segment lesson={LESSON} id="sinl" a={P} b={[z.cos, 0]} dashed role="axisY" />
				<Segment lesson={LESSON} id="qx" a={Q} b={[0, z.cos]} dashed role="axisY" />
				<Segment lesson={LESSON} id="qy" a={Q} b={[z.sin, 0]} dashed role="axisX" />
			</>}
		</Show>
		{apart && <>
			<Segment lesson={LESSON} id="chord" a={P} b={Q} dashed role="ink" />
			<RightAngle lesson={LESSON} at={M} u={[z.cos - M[0], z.sin - M[1]]} v={[1, 1]} show />
		</>}
		{labels && <>
			<Label lesson={LESSON} id="P" at={P} away={[0, 0]} gap={10} role="vecA">{'P (' + fmt2(z.cos) + ', ' + fmt2(z.sin) + ')'}</Label>
			{apart && <Label lesson={LESSON} id="Q" at={Q} away={[0, 0]} gap={10} role="vecB">{'Q (' + fmt2(z.sin) + ', ' + fmt2(z.cos) + ')'}</Label>}
		</>}
		<Point lesson={LESSON} id="o" at={[0, 0]} role="muted" size={5} />
		<Point lesson={LESSON} id="Q" at={(v) => { const d = derive(v); return [d.sin, d.cos]; }} keys={['th']} move={moveMirror} role="vecB" label={t(locale, 'uc.handleQ')} />
		<HandleP />
	</>;
}

/** Grid lines under the "grid" overlay; tick numbers only where they are the scene's own scale (UC-1, UC-4). */
function SceneGrid({ extent, step, ticks }: { extent: number; step: number; ticks: boolean }) {
	return <Grid lesson={LESSON} extent={extent} step={step} ticks={ticks} lines={useOverlay('grid')} />;
}

/** Status bar hints per scene; values are read live. */
function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const valuesHint = (v: Vals) => {
		const z = derive(v);
		return t(locale, 'uc.hintValues', { th: fmtDeg(z.th), cos: fmt2(z.cos), sin: fmt2(z.sin) });
	};
	const handle = { mouse: t(locale, 'uc.hintPMouse'), touch: t(locale, 'uc.hintPTouch') };
	if (scene === 1) {
		return {
			hints: {
				P: handle,
				cos: valuesHint, cosb: valuesHint, cosl: valuesHint, sin: valuesHint, sinb: valuesHint, sinl: valuesHint,
				th: (v: Vals) => t(locale, 'uc.hintArc', { th: fmtDeg(derive(v).th), rad: fmt3(derive(v).rad) }),
			},
			idle: { mouse: t(locale, 'uc.idleMouse'), touch: t(locale, 'uc.idleTouch') },
		};
	}
	if (scene === 2) {
		return {
			hints: {
				P: { mouse: t(locale, 'uc.hintRollMouse'), touch: t(locale, 'uc.hintRollTouch') },
				arc: (v: Vals) => t(locale, 'uc.hintArcLen', { rad: fmt3(derive(v).rad), th: fmtDeg(derive(v).th) }),
				r: t(locale, 'uc.hintRadius'),
				ruler: t(locale, 'uc.hintRuler'),
			},
			idle: t(locale, 'uc.idleRoll'),
		};
	}
	if (scene === 3) {
		const curve = { mouse: t(locale, 'uc.hintCurveMouse'), touch: t(locale, 'uc.hintCurveTouch') };
		return {
			hints: { P: handle, C: curve, S: curve, cosc: valuesHint, sinc: valuesHint, carry: t(locale, 'uc.hintCarry') },
			idle: { mouse: t(locale, 'uc.idleMouse'), touch: t(locale, 'uc.idleTouch') },
		};
	}
	return {
		hints: {
			P: handle,
			Q: { mouse: t(locale, 'uc.hintQMouse'), touch: t(locale, 'uc.hintQTouch') },
			chord: t(locale, 'uc.hintChord'),
			diag: t(locale, 'uc.hintDiag'),
		},
		idle: { mouse: t(locale, 'uc.idleMouse'), touch: t(locale, 'uc.idleTouch') },
	};
}

const CARD = {
	1: { id: 'UC-1', name: 'uc.nameShadows' }, 2: { id: 'UC-2', name: 'uc.nameRuler' },
	3: { id: 'UC-3', name: 'uc.nameWaves' }, 4: { id: 'UC-4', name: 'uc.nameMirror' },
} as const;

export function UnitScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	const overlays = scene === 1 ? [ov('grid', 'uc.ovGrid'), ov('labels', 'uc.ovLabels'), ov('signs', 'uc.ovSigns'), ov('ticks', 'uc.ovTicks')]
		: scene === 2 ? [ov('grid', 'uc.ovGrid'), ov('labels', 'uc.ovLabels'), ov('ghost', 'uc.ovGhostArc')]
		: scene === 3 ? [ov('grid', 'uc.ovGrid', false), ov('labels', 'uc.ovLabels'), ov('ghost', 'uc.ovGhostCurve')]
		: [ov('grid', 'uc.ovGrid'), ov('labels', 'uc.ovLabels'), ov('legs', 'uc.ovLegs'), ov('ticks', 'uc.ovTicks')];
	const wide = scene === 2 || scene === 3;
	const extent = wide ? 8 : 1.6;
	return <ViewportCard lesson={LESSON} id={CARD[scene].id} name={CARD[scene].id + ' · ' + t(locale, CARD[scene].name)} mode="2d"
		extent={extent} center={wide ? [3, 0] : [0, 0]}
		rows={rowsFor(scene, locale)} overlays={overlays} status={statusFor(scene, locale)}
		drawIn={scene === 1} panelOpen={!wide}>
		<SceneGrid extent={extent} step={wide ? 1 : 0.5} ticks={!wide} />
		{scene === 1 ? <Shadows /> : scene === 2 ? <Unroll /> : scene === 3 ? <Waves /> : <Mirror />}
	</ViewportCard>;
}
