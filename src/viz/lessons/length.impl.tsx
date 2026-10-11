import { BoxGeometry } from 'three';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useStep, useVals } from '../store';
import { scale, sub, type V } from '../geom';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { RightAngle } from '../scene/RightAngle';
import { AreaTile } from '../scene/AreaTile';
import { Circle } from '../scene/Circle';
import { Poly } from '../scene/Poly';
import { Label } from '../scene/Label';
import { Show, useOverlay } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './length.rows';
import { barX, ccw2, derive, fmt1, fmt2, fmt3, fmtVec, LESSON, mid, moveBar, N_BARS, outward, upPlaneNormal, walk3 } from './length.math';

/**
 * Lesson 1.3 scenes: LN-1 |v|² as a sum of component squares (2D, Σ timeline), LN-1-3D the same with three legs,
 * LN-2 normalization onto the unit circle, LN-2-3D onto the unit sphere, LN-3 an eight-component vector as bars.
 * Loaded lazily by the entry (length.tsx); shares its store, its one model (length.math.ts) and its hover ids.
 * Scene code reads `shown` values (eased writes glide), the DOM reads `vals`.
 */

type Vals = Parameters<typeof derive>[0];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);

/**
 * An invisible box over everything the reader can make the scene draw. The home view frames the content's bounds, so
 * framing the reach keeps the view still while tiles appear step by step (step 0 shows only the arrow).
 */
function Reach({ lo, hi }: { lo: V; hi: V }) {
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast}
		position={[(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, 0]} scale={[hi[0] - lo[0], hi[1] - lo[1], 0.01]} />;
}

const TIP: readonly [number, number] = [10, -12];
const O2: V = [0, 0];
const O3: V = [0, 0, 0];

// ---------------------------------------------------------------------------------------------------------------
// LN-1: Pythagoras tiles on the components, unfolded by the `sum2` timeline.

function Tiles2() {
	const v = useVals(LESSON), step = useStep(LESSON, 'sum2'), labels = useOverlay('labels');
	const s = derive(v).s;
	const [x, y] = s.comps;
	const foot: V = [x, 0];
	// Legs and their tiles face away from the triangle O, (x, 0), v; the hypotenuse tile away from the legs.
	const turn = ccw2(s.comps);
	const tileLabel = (name: string) => (labels ? (a: number) => name + ' = ' + fmt2(a) : undefined);
	return <>
		<Reach lo={[-4.5, -3.5]} hi={[7.5, 7.5]} />
		<Show ov="tiles">
			{step >= 1 && <AreaTile lesson={LESSON} id="sq1" a={O2} b={foot} role="axisX" flip={!turn} label={tileLabel('v₁²')} />}
			{step >= 2 && <AreaTile lesson={LESSON} id="sq2" a={foot} b="v" role="axisY" flip={!turn} label={tileLabel('v₂²')} />}
			{step >= 3 && <AreaTile lesson={LESSON} id="s2" a={O2} b="v" role="vecA" flip={turn} label={tileLabel('|v|²')} />}
		</Show>
		{x !== 0 && <Segment lesson={LESSON} id="c1" a={O2} b={foot} role="axisX" />}
		{y !== 0 && <Segment lesson={LESSON} id="c2" a={foot} b="v" role="axisY" />}
		{x !== 0 && y !== 0 && <RightAngle lesson={LESSON} id="rt" at={foot} u={[-x, 0]} v={[0, y]} show />}
		<Arrow lesson={LESSON} id="v" to="v" role="vecA" label="v" />
		<Show ov="labels">
			<Label lesson={LESSON} id="v" at="v" role="vecA" offset={TIP}>v</Label>
			{/* Component values sit inside the triangle (the side of each leg the tiles are not on). */}
			{x !== 0 && <Label lesson={LESSON} id="c1" at={mid(O2, foot)} offset={[0, y >= 0 ? -14 : 14]} role="axisX">{'v₁ = ' + fmt1(x)}</Label>}
			{y !== 0 && <Label lesson={LESSON} id="c2" at={mid(foot, s.comps)} offset={[x >= 0 ? -34 : 34, 0]} role="axisY">{'v₂ = ' + fmt1(y)}</Label>}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// LN-1-3D: the walk x, then z on the floor, then up y; Pythagoras twice.

function Tiles3() {
	const v = useVals(LESSON), step = useStep(LESSON, 'sum3'), labels = useOverlay('labels');
	const s = derive(v).s3;
	const [x, y, z] = s.comps;
	const w = walk3(s.comps);
	const tileLabel = (name: string) => (labels ? (a: number) => name + ' = ' + fmt2(a) : undefined);
	const floor: V = [0, 1, 0];
	const up = upPlaneNormal(s.comps);
	return <>
		<Show ov="tiles">
			{/* Floor tiles lie on the floor outside the floor triangle; the y tile stands on the floor diagonal's plane. */}
			{step >= 1 && <AreaTile lesson={LESSON} id="tq1" a={w.c0} b={w.c1} role="axisX" normal={outward(w.c0, w.c1, w.c2, floor)} label={tileLabel('v₁²')} />}
			{step >= 2 && <AreaTile lesson={LESSON} id="tq3" a={w.c1} b={w.c2} role="axisZ" normal={outward(w.c1, w.c2, w.c0, floor)} label={tileLabel('v₃²')} />}
			{step >= 3 && <AreaTile lesson={LESSON} id="tq2" a={w.c2} b={w.c3} role="axisY" normal={outward(w.c2, w.c3, w.c0, up)} label={tileLabel('v₂²')} />}
		</Show>
		{/* The first Pythagoras: the floor diagonal, |d|² = v₁² + v₃² (dashed: a construction line). */}
		{step >= 2 && w.d1 > 0 && <Segment lesson={LESSON} id="d1" a={w.c0} b={w.c2} dashed role="muted" />}
		{x !== 0 && <Segment lesson={LESSON} id="tc1" a={w.c0} b={w.c1} role="axisX" />}
		{z !== 0 && <Segment lesson={LESSON} id="tc3" a={w.c1} b={w.c2} role="axisZ" />}
		{y !== 0 && <Segment lesson={LESSON} id="tc2" a={w.c2} b={w.c3} role="axisY" />}
		<RightAngle lesson={LESSON} id="rt1" at={w.c1} u={scale(w.c1, -1)} v={[0, 0, z]} />
		<RightAngle lesson={LESSON} id="rt2" at={w.c2} u={scale(w.c2, -1)} v={[0, y, 0]} />
		<Arrow lesson={LESSON} id="v3" to="v3" role="vecA" label="v" />
		<Show ov="labels">
			<Label lesson={LESSON} id="v3" at="v3" role="vecA" offset={TIP}>v</Label>
			{step >= 2 && w.d1 > 0 && step < 4 && <Label lesson={LESSON} id="d1" at={mid(w.c0, w.c2)} role="muted" offset={[0, 14]}>{'√(v₁² + v₃²) = ' + fmt2(w.d1)}</Label>}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// LN-2 / LN-2-3D: normalization.

/** The unit sphere as three great circles (one per coordinate plane) and two latitudes. */
function WireSphere() {
	return <>
		<Circle lesson={LESSON} id="sphere" center={O3} r={1} normal={[0, 1, 0]} role="muted" />
		<Circle lesson={LESSON} id="sphere" center={O3} r={1} normal={[1, 0, 0]} role="muted" />
		<Circle lesson={LESSON} id="sphere" center={O3} r={1} normal={[0, 0, 1]} role="muted" />
		<Circle lesson={LESSON} id="sphere" center={[0, 0.5, 0]} r={Math.sqrt(0.75)} normal={[0, 1, 0]} role="muted" dashPx={[4, 6]} />
		<Circle lesson={LESSON} id="sphere" center={[0, -0.5, 0]} r={Math.sqrt(0.75)} normal={[0, 1, 0]} role="muted" dashPx={[4, 6]} />
	</>;
}

function Normalize({ three }: { three: boolean }) {
	const locale = useLocale(), v = useVals(LESSON);
	const key = three ? 'w3' : 'w';
	const z = three ? derive(v).n3 : derive(v).n;
	const O = three ? O3 : O2;
	// A direction perpendicular to v, |side| = |v|: 2D its clockwise turn, 3D v × y (or x when v is vertical). The |v|
	// and v̂ labels sit on that side of the arrow, so neither covers it.
	const [w0, w1, w2] = z.w;
	const side: V = !three ? [w1, -w0] : w0 === 0 && w2 === 0 ? [z.n, 0, 0] : scale([-w2, 0, w0], z.n / Math.hypot(w0, w2));
	return <>
		{!three && <Reach lo={[-4.5, -4.5]} hi={[4.5, 4.5]} />}
		<Show ov="unit">{three ? <WireSphere /> : <Circle lesson={LESSON} id="circle" center={O2} r={1} role="muted" />}</Show>
		{!z.zero && <>
			<Arrow lesson={LESSON} id={key} to={key} role="vecA" label="v" />
			{z.k > 0 && <Arrow lesson={LESSON} id="mid" to={z.mid} role="vecC" drag="none" />}
			<Point lesson={LESSON} id="hatPt" at={z.hat!} role="vecC" size={8} drag={false} />
		</>}
		{z.zero && <Point lesson={LESSON} id={key} at={key} role="hot" label="v" />}
		<Show ov="labels">
			{!z.zero && <>
				<Label lesson={LESSON} id={key} at={key} role="vecA" offset={TIP}>v</Label>
				{!three && <Label lesson={LESSON} id="nw" at={mid(O, z.w)} away={scale(side, -1)} gap={6} role="vecA">{'|v| = ' + fmt2(z.n)}</Label>}
				{/* Beside the unit point, off the arrow's line (pushed away from a point on the other side of it). */}
				<Label lesson={LESSON} id="hatPt" at={z.hat!} role="vecC" away={sub(z.hat!, side.map((c) => c / (z.n || 1)))} gap={8}>v̂</Label>
			</>}
			{z.zero && <Label lesson={LESSON} id="zero" at={O} role="hot" offset={[0, -18]}>{t(locale, 'ln.zeroLabel')}</Label>}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// LN-3: eight components as bars.

function Bars() {
	const v = useVals(LESSON), labels = useOverlay('labels');
	const u = derive(v).su.comps;
	return <>
		<Reach lo={[-0.4, -4.4]} hi={[8.4, 4.4]} />
		{u.map((c, i) => {
			const x = barX(i);
			return <group key={i}>
				<Poly lesson={LESSON} id={'u' + i} step={0} role="vecA" fill={0.3}
					pts={(vals) => { const h = (vals.u as number[])[i]; return [[x - 0.3, 0], [x + 0.3, 0], [x + 0.3, h], [x - 0.3, h]]; }}
					hatched={(vals) => (vals.u as number[])[i] < 0} />
				<Point lesson={LESSON} id={'u' + i} at={(vals) => [x, (vals.u as number[])[i]]} keys={['u']} move={moveBar(i)} role="vecA"
					label={'u' + SUB[i + 1]} />
				{labels && <Label lesson={LESSON} id={'u' + i} at={[x, c]} offset={[0, c >= 0 ? -16 : 16]} role="vecA">{fmt1(c)}</Label>}
				{labels && <Label lesson={LESSON} id={'ui' + i} at={[x, 0]} offset={[0, c >= 0 ? 14 : -14]} role="muted">{'u' + SUB[i + 1]}</Label>}
			</group>;
		})}
	</>;
}

const SUB = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈'];

// ---------------------------------------------------------------------------------------------------------------
// Cards.

function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const tip = (name: string) => ({ mouse: t(locale, 'ln.hintTipMouse', { name }), touch: t(locale, 'ln.hintTipTouch', { name }) });
	const tile = (i: number, three: boolean) => (v: Vals) => {
		const s = three ? derive(v).s3 : derive(v).s;
		return t(locale, 'ln.hintTile', { i: String(i), value: fmt2(s.sq[i - 1]) });
	};
	if (scene === 1) {
		return {
			hints: { 'v:tip': tip('v'), sq1: tile(1, false), sq2: tile(2, false), s2: (v) => t(locale, 'ln.hintHyp', { value: fmt2(derive(v).s.s2) }) },
			idle: t(locale, 'ln.idleSum'),
		};
	}
	if (scene === 2) {
		return {
			hints: {
				'v3:tip': { mouse: t(locale, 'ln.hintTip3', { name: 'v' }), touch: t(locale, 'ln.hintTipTouch', { name: 'v' }) },
				tq1: tile(1, true), tq2: tile(2, true), tq3: tile(3, true),
				d1: (v) => t(locale, 'ln.hintDiag', { value: fmt2(walk3(derive(v).s3.comps).d1) }),
			},
			idle: t(locale, 'ln.idleSum'),
		};
	}
	if (scene === 3 || scene === 4) {
		const three = scene === 4;
		const key = three ? 'w3' : 'w';
		const n = (v: Vals) => (three ? derive(v).n3 : derive(v).n);
		return {
			hints: {
				[key + ':tip']: three ? { mouse: t(locale, 'ln.hintTip3', { name: 'v' }), touch: t(locale, 'ln.hintTipTouch', { name: 'v' }) } : tip('v'),
				[key + ':pt']: three ? { mouse: t(locale, 'ln.hintTip3', { name: 'v' }), touch: t(locale, 'ln.hintTipTouch', { name: 'v' }) } : tip('v'),
				hatPt: (v) => t(locale, 'ln.hintHat', { hat: fmtVec(n(v).hatC, fmt3) }),
				mid: t(locale, 'ln.hintMid'),
			},
			idle: t(locale, three ? 'ln.idleNorm3' : 'ln.idleNorm'),
			state: (v) => (n(v).zero ? t(locale, 'ln.stateZero') : null),
		};
	}
	const bar = (i: number) => ({ mouse: t(locale, 'ln.hintBarMouse', { i: String(i + 1) }), touch: t(locale, 'ln.hintBarTouch', { i: String(i + 1) }) });
	return { hints: Object.fromEntries(Array.from({ length: N_BARS }, (_, i) => ['u' + i + ':pt', bar(i)])), idle: t(locale, 'ln.idleBars') };
}

function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	if (scene === 1 || scene === 2) return [ov('grid', 'ln.ovGrid'), ov('axes', 'ln.ovAxes'), ov('tiles', 'ln.ovTiles'), ov('labels', 'ln.ovLabels')];
	if (scene === 3 || scene === 4) return [ov('grid', 'ln.ovGrid'), ov('axes', 'ln.ovAxes'), ov('unit', scene === 4 ? 'ln.ovSphere' : 'ln.ovCircle'), ov('labels', 'ln.ovLabels')];
	return [ov('grid', 'ln.ovGrid'), ov('labels', 'ln.ovLabels')];
}

const CARD = {
	1: { id: 'LN-1', name: 'ln.name1', mode: '2d', extent: 6 },
	2: { id: 'LN-1-3D', name: 'ln.name2', mode: '3d', extent: 3.5 },
	3: { id: 'LN-2', name: 'ln.name3', mode: '2d', extent: 4.5 },
	4: { id: 'LN-2-3D', name: 'ln.name4', mode: '3d', extent: 3.5 },
	5: { id: 'LN-3', name: 'ln.name5', mode: '2d', extent: 4.6 },
} as const;

/** Grid lines under "grid", axes under "axes" (the bar chart has no axes flag: its baseline is always drawn). */
function Floor({ scene }: { scene: Scene }) {
	if (scene === 5) return <Show ov="grid"><Grid lesson={LESSON} extent={9} step={1} axes ticks={false} /></Show>;
	return <AxesFloor scene={scene} />;
}
function AxesFloor({ scene }: { scene: Scene }) {
	const axes = useOverlay('axes');
	const c = CARD[scene];
	return <Show ov="grid"><Grid lesson={LESSON} extent={c.mode === '3d' ? c.extent : 8} step={1} axes={axes} ticks={c.mode === '2d' && axes} /></Show>;
}

export function LengthScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const c = CARD[scene];
	return <ViewportCard lesson={LESSON} id={c.id} name={c.id + ' · ' + t(locale, c.name)} mode={c.mode} extent={c.extent}
		center={scene === 5 ? [4, 0] : scene === 1 ? [1.5, 2] : undefined}
		rows={rowsFor(scene, locale)} overlays={overlaysFor(scene, locale)} status={statusFor(scene, locale)} drawIn={scene === 1}>
		<Floor scene={scene} />
		{scene === 1 ? <Tiles2 /> : scene === 2 ? <Tiles3 /> : scene === 5 ? <Bars /> : <Normalize three={scene === 4} />}
	</ViewportCard>;
}
