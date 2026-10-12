import { useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useHot, useStep, useVals } from '../store';
import { add, norm, planeBasis, scale, sub, to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { Stroke } from '../scene/view';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Room } from '../scene/Stage';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Label } from '../scene/Label';
import { HOVER_RANK, Show, useHoverable, useOverlay } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rulerLine, rulerTicks } from './basis.math';
import { rowsFor, type Scene } from './matrix-columns.rows';
import { LESSON, STEP_COLUMNS, derive1, derive2, fmt1, fmt2, fmtV } from './matrix-columns.math';

/**
 * Lesson 2.2 scenes: MC-1 "columns are where the basis lands" (2D; Mx built from the columns head to tail, its
 * components = the row dot products) and MC-2 "new rulers" (3D; R's rows are three rulers, each row reading is a
 * shadow, the transfer turns the rulers onto the world axes carrying the readings, then the column view rebuilds Rv;
 * an orthonormal flag shows what breaks for bent rulers). Loaded lazily by the entry (matrix-columns.tsx).
 * Look: the Blender-style flat viewport (visual-language 6d/6e; user ruling 2026-10-11).
 */

type Vals = Parameters<typeof derive1>[0];

const O2: V = [0, 0];
const O3: V = [0, 0, 0];
const TIP: readonly [number, number] = [10, -12];
const RULER_ROLE: readonly SceneRole[] = ['vecA', 'vecB', 'vecC'];
const AXIS_ROLE: readonly SceneRole[] = ['axisX', 'axisY', 'axisZ'];
const SUB = ['₁', '₂', '₃'];
/** Half length of the drawn rulers in MC-2 (v = (5, 0, 2) and its shadows stay on them). */
const RULER_HALF = 5.5;
const ROOM2 = 4.5;

const isZero = (p: V) => norm(p) < 1e-9;
const mid = (p: V, q: V) => scale(add(p, q), 0.5);

/** Thin guide lines as one segment list: one hoverable object, 1.5 px, no halo (construction under the arrows). */
function Guides({ id, segs, role, opacity = 0.55 }: { id: string; segs: readonly (readonly V[])[]; role: SceneRole; opacity?: number }) {
	const pal = usePalette();
	const hot = useHot(LESSON, id);
	const hover = useHoverable(LESSON, id, HOVER_RANK.guide);
	const pts = useMemo(() => segs.flatMap(([a, b]) => [to3(a), to3(b)]), [segs]);
	if (pts.length < 2) return null;
	return <group {...hover}><Stroke points={pts} segments color={pal[role]} hot={hot} width={STYLE.edge} halo={false} opacity={opacity} order={0.8} /></group>;
}

function Floor({ extent, dims }: { extent: number; dims: 2 | 3 }) {
	const axes = useOverlay('axes');
	return <Grid lesson={LESSON} extent={extent} step={1} minor={dims === 2 ? 0.5 : undefined} axes={axes} ticks={dims === 2 && axes} lines={useOverlay('grid')} />;
}

// ---------------------------------------------------------------------------------------------------------------
// MC-1: columns are where the basis lands.

function Columns() {
	const v = useVals(LESSON);
	const z = derive1(v);
	const labels = useOverlay('labels');
	const rows = useOverlay('rows');
	return <>
		<Room lo={[-ROOM2, -ROOM2]} hi={[ROOM2, ROOM2]} />
		<Show ov="basis">
			<Arrow lesson={LESSON} id="e1" to={[1, 0]} role="muted" opacity={0.6} />
			<Arrow lesson={LESSON} id="e2" to={[0, 1]} role="muted" opacity={0.6} />
		</Show>
		<Arrow lesson={LESSON} id="x" to="x" role="muted" label="x" />
		{/* Column view: x₁ copies of col₁, then x₂ copies of col₂, head to tail. */}
		<Show ov="columns">
			{!isZero(z.leg) && <Segment lesson={LESSON} id="w1" a={O2} b={z.leg} dashed role="vecA" />}
			{!isZero(sub(z.y, z.leg)) && <Segment lesson={LESSON} id="w2" a={z.leg} b={z.y} dashed role="vecB" />}
		</Show>
		{/* Row view: the components of y are the two row dot products; the legs show them on the axes. */}
		{!isZero(z.y) && <Arrow lesson={LESSON} id="y" to={z.y} role="vecC" legs={rows} />}
		<Arrow lesson={LESSON} id="m1" to="m1" role="vecA" label="col₁" />
		<Arrow lesson={LESSON} id="m2" to="m2" role="vecB" label="col₂" />
		{labels && <>
			<Label lesson={LESSON} id="m1" at="m1" role="vecA" offset={TIP}>col₁</Label>
			<Label lesson={LESSON} id="m2" at="m2" role="vecB" offset={TIP}>col₂</Label>
			<Label lesson={LESSON} id="x" at="x" role="muted" offset={TIP}>x</Label>
			{!isZero(z.y) && <Label lesson={LESSON} id="y" at={z.y} role="vecC" away={O2} gap={8}>{'Mx = ' + fmtV(z.y)}</Label>}
			{!isZero(z.leg) && <Label lesson={LESSON} id="w1" at={mid(O2, z.leg)} role="vecA" away={mid(z.leg, z.y)} gap={4}>{fmt1(z.x[0]) + '·col₁'}</Label>}
		</>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// MC-2: new rulers in 3D.

/** Tick marks of a 3D ruler: a small cross (two bars across the ruler) at every k·u within RULER_HALF. */
function ticks3(u: V): number[][][] {
	const pb = norm(u) > 0 ? planeBasis(u) : null;
	if (!pb) return [];
	const [a, b] = pb.map((e) => scale(e, 0.07));
	return rulerTicks(u, RULER_HALF).flatMap((p) => [[sub(p, a), add(p, a)], [sub(p, b), add(p, b)]]);
}

/** Rows whose shadows a timeline step shows: none at the overview, rows 1..k at step k, all three after. */
const shownRows = (step: number) => (step <= 0 ? 0 : Math.min(3, step));

function Rulers() {
	const v = useVals(LESSON);
	const step = useStep(LESSON, 'rv');
	const z = derive2(v);
	const labels = useOverlay('labels');
	const moving = z.tau > 1e-3;
	const landed = z.tau > 0.999;
	const dirs = z.u.map((u) => z.at(u));
	const tk = dirs.map(ticks3);
	const vt = z.at(z.v);
	const nRows = shownRows(step);
	const cols = step >= STEP_COLUMNS && landed;
	const bent = !z.orthonormal && !moving;
	return <>
		{dirs.map((d, i) => {
			const line = rulerLine(d, RULER_HALF);
			return line && <Guides key={i} id={'q' + (i + 1)} segs={[line]} role={RULER_ROLE[i]} opacity={0.55} />;
		})}
		<Show ov="ticks">{tk.map((segs, i) => <Guides key={i} id={'q' + (i + 1)} segs={segs} role={RULER_ROLE[i]} opacity={0.9} />)}</Show>
		{/* One row = one reading: the perpendicular drop from v onto ruler i and the shadow it marks. */}
		<Show ov="shadows">{z.feet.slice(0, nRows).map((f, i) => {
			if (!f) return null;
			const id = 'rd' + (i + 1);
			const foot = z.at(f);
			return <group key={id}>
				{!isZero(foot) && <Segment lesson={LESSON} id={id} a={O3} b={foot} band role={RULER_ROLE[i]} />}
				{!isZero(sub(vt, foot)) && <Segment lesson={LESSON} id={id} a={vt} b={foot} dashed role={RULER_ROLE[i]} />}
				<Point lesson={LESSON} id={id} at={foot} role={RULER_ROLE[i]} size={6} />
				{labels && <Label lesson={LESSON} id={id} at={foot} role={RULER_ROLE[i]} away={vt} gap={6}>{'u' + SUB[i] + '·v = ' + fmt1(z.y[i])}</Label>}
			</group>;
		})}</Show>
		{/* Column view (after the transfer): Rv = v₁·col₁ + v₂·col₂ + v₃·col₃, head to tail, each in its world axis colour. */}
		{cols && z.colWalk.slice(1).map((q, j) => {
			const a = z.colWalk[j];
			return !isZero(sub(q, a)) && <group key={j}>
				<Segment lesson={LESSON} id={'cw' + (j + 1)} a={a} b={q} role={AXIS_ROLE[j]} />
				{labels && <Label lesson={LESSON} id={'cw' + (j + 1)} at={mid(a, q)} role={AXIS_ROLE[j]} offset={[0, 14]}>{fmt1(z.v[j]) + '·col' + SUB[j]}</Label>}
			</group>;
		})}
		{/* Bent rulers: the true coordinates walk to v along the rulers; the ghost treats the readings as coordinates. */}
		<Show ov="ghost">{bent && <>
			{z.walk && z.walk.slice(1).map((q, i) => {
				const a = z.walk![i];
				return !isZero(sub(q, a)) && <Segment key={i} lesson={LESSON} id={'c' + (i + 1)} a={a} b={q} dashed role={RULER_ROLE[i]} />;
			})}
			{z.gap > 1e-6 && <>
				<Arrow lesson={LESSON} id="ghost" to={z.ghost} role="muted" drag="none" />
				<Segment lesson={LESSON} id="gap" a={z.ghost} b={z.v} dashed role="hot" />
				{labels && <Label lesson={LESSON} id="ghost" at={z.ghost} role="muted" offset={[14, 14]}>B·Rv</Label>}
			</>}
		</>}</Show>
		{moving
			? <>
				{/* The world arrow stays where it is (a faint reference); the moving copy is v carried by R. */}
				<Arrow lesson={LESSON} id="p" to={z.v} role="muted" opacity={0.35} drag="none" />
				<Arrow lesson={LESSON} id="yv" to={vt} role="ink" legs={landed} />
				{dirs.map((d, i) => <Arrow key={i} lesson={LESSON} id={'q' + (i + 1)} to={d} role={RULER_ROLE[i]} drag="none" />)}
			</>
			: <>
				<Arrow lesson={LESSON} id="p" to="p" role="ink" label="v" />
				{(['q1', 'q2', 'q3'] as const).map((k, i) => <Arrow key={k} lesson={LESSON} id={k} to={k} role={RULER_ROLE[i]} label={'u' + SUB[i]} />)}
			</>}
		{labels && <>
			{!moving && <Label lesson={LESSON} id="p" at="p" role="ink" offset={TIP}>v</Label>}
			{moving && <Label lesson={LESSON} id="yv" at={vt} role="ink" offset={TIP}>{landed ? 'Rv = ' + fmtV(z.y) : 'R(τ)v'}</Label>}
			{dirs.map((d, i) => !isZero(d) && <Label key={i} lesson={LESSON} id={'q' + (i + 1)} at={d} role={RULER_ROLE[i]} offset={TIP}>{'u' + SUB[i]}</Label>)}
		</>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

function statusFor(scene: Scene, locale: Locale): StatusSpec {
	if (scene === 1) {
		const tip = (name: string) => ({ mouse: t(locale, 'mc.hintTipMouse', { name }), touch: t(locale, 'mc.hintTipTouch', { name }) });
		return {
			hints: {
				'm1:tip': tip('col₁'), 'm2:tip': tip('col₂'), 'x:tip': tip('x'),
				w1: (x: Vals) => t(locale, 'mc.hintW1', { k: fmt1(derive1(x).x[0]) }),
				w2: (x: Vals) => t(locale, 'mc.hintW2', { k: fmt1(derive1(x).x[1]) }),
				y: (x: Vals) => t(locale, 'mc.hintY', { y: fmtV(derive1(x).y) }),
				'y:x': (x: Vals) => t(locale, 'mc.hintLeg', { i: '1', y: fmt2(derive1(x).yRows[0]) }),
				'y:y': (x: Vals) => t(locale, 'mc.hintLeg', { i: '2', y: fmt2(derive1(x).yRows[1]) }),
			},
		};
	}
	const tip3 = (name: string) => ({ mouse: t(locale, 'mc.hintTip3', { name }), touch: t(locale, 'mc.hintTipTouch', { name }) });
	return {
		hints: {
			'p:tip': tip3('v'), 'q1:tip': tip3('u₁'), 'q2:tip': tip3('u₂'), 'q3:tip': tip3('u₃'),
			...Object.fromEntries([0, 1, 2].map((i) => ['rd' + (i + 1), (x: Vals) => t(locale, 'mc.hintReading', { name: 'u' + SUB[i] + '·v', value: fmt2(derive2(x).y[i]) })])),
			...Object.fromEntries([0, 1, 2].map((j) => ['cw' + (j + 1), (x: Vals) => t(locale, 'mc.hintColumn', { k: fmt1(derive2(x).v[j]), j: String(j + 1) })])),
			yv: (x: Vals) => t(locale, 'mc.hintYv', { y: fmtV(derive2(x).y) }),
			ghost: t(locale, 'mc.hintGhost'),
			gap: (x: Vals) => t(locale, 'mc.hintGap', { gap: fmt2(derive2(x).gap) }),
		},
		state: (x: Vals) => {
			const z = derive2(x);
			return z.c === null ? t(locale, 'mc.stateFlat') : z.orthonormal ? (z.vol > 0 ? t(locale, 'mc.stateRotation') : t(locale, 'mc.stateMirror')) : t(locale, 'mc.stateBent');
		},
	};
}

function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	const base = [ov('grid', 'mc.ovGrid'), ov('axes', 'mc.ovAxes'), ov('labels', 'mc.ovLabels')];
	if (scene === 1) return [...base, ov('basis', 'mc.ovBasis'), ov('columns', 'mc.ovColumns'), ov('rows', 'mc.ovRows')];
	return [...base, ov('ticks', 'mc.ovTicks'), ov('shadows', 'mc.ovShadows'), ov('ghost', 'mc.ovGhost')];
}

const NAMES = { 1: 'mc.name1', 2: 'mc.name2' } as const;
const EXTENT: Record<Scene, number> = { 1: ROOM2, 2: RULER_HALF };

export function MatrixScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	return <ViewportCard lesson={LESSON} id={'MC-' + scene} name={'MC-' + scene + ' · ' + t(locale, NAMES[scene])}
		mode={scene === 2 ? '3d' : '2d'} extent={EXTENT[scene]} rows={rowsFor(scene, locale)} overlays={overlaysFor(scene, locale)}
		status={statusFor(scene, locale)} drawIn={scene === 1}>
		<Floor extent={scene === 2 ? 4 : ROOM2} dims={scene === 2 ? 3 : 2} />
		{scene === 1 ? <Columns /> : <Rulers />}
	</ViewportCard>;
}
