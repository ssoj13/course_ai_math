import { useEffect, useMemo } from 'react';
import { useThree } from '@react-three/fiber';
import { BoxGeometry, BufferAttribute, BufferGeometry, PointsMaterial } from 'three';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { add, norm, scale, type V } from '../geom';
import { usePalette } from '../palette';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Parallelogram } from '../scene/Parallelogram';
import { Poly } from '../scene/Poly';
import { Circle } from '../scene/Circle';
import { Label } from '../scene/Label';
import { Show, useOverlay } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './vector-arithmetic.rows';
import {
	blend, combo, combo3, CORNER, EYES, fmt1, fmt2, fmtV, HEAD, lattice, LESSON, MOUTH, RANGE_C, RANGE_S, RANGE_ST, scaled, sum,
} from './vector-arithmetic.math';

/**
 * Lesson 1.2 scenes: VA-1 head-to-tail addition with the parallelogram, VA-2 scaling (negative and zero included),
 * VA-3 the linear combination s·a + t·b with a paint toggle that previews the span, VA-4 three arrows in 3D, VA-5
 * blend shapes as linear combinations of deltas. Blender-like flat look (task 6d). Loaded lazily by the entry
 * (vector-arithmetic.tsx); shares its store, its model and its hover ids.
 */

type Vals = Parameters<typeof sum>[0];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);
const O2: V = [0, 0];
const O3: V = [0, 0, 0];
const TIP: readonly [number, number] = [12, -12];
const mid = (p: V, q: V) => scale(add(p, q), 0.5);
const isZero = (p: V) => norm(p) < 1e-9;

/** Invisible box over everything the reader can make a scene draw: the home view frames it (as in lesson 0.G). */
function Reach({ lo, hi }: { lo: V; hi: V }) {
	const c = lo.map((x, i) => (x + hi[i]) / 2);
	const s = lo.map((x, i) => Math.max(hi[i] - x, 0.01));
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast} position={[c[0], c[1], c[2] ?? 0]} scale={[s[0], s[1], s[2] ?? 0.01]} />;
}

/**
 * Grid lines under "grid", axes and (2D) tick numbers under "axes". `extent`: in 2D the axis letters sit at +extent
 * (inside the framed reach), in 3D the axes are ±extent long.
 */
function Floor({ extent, three = false }: { extent: number; three?: boolean }) {
	const axes = useOverlay('axes');
	return <Grid lesson={LESSON} extent={extent} step={1} minor={three ? undefined : 0.5} axes={axes} ticks={!three && axes} lines={useOverlay('grid')} />;
}

/** An arrow name next to its tip, under the `labels` overlay; nothing for a zero arrow (there is no tip to name). */
function Name({ id, at, role, text }: { id: string; at: V; role: 'vecA' | 'vecB' | 'vecC' | 'ink'; text: string }) {
	return <Show ov="labels">{!isZero(at) && <Label lesson={LESSON} id={id} at={at} role={role} offset={TIP}>{text}</Label>}</Show>;
}

// ---------------------------------------------------------------------------------------------------------------
// VA-1: head to tail.

function Sum() {
	const v = useVals(LESSON), z = sum(v);
	return <>
		<Reach lo={[-3, -3]} hi={[6, 5]} />
		{/* A plain fill, not Parallelogram: that primitive hatches a clockwise pair as a negative area, which means
		    nothing here (the sum does not care about orientation). */}
		<Show ov="parallelogram"><Poly lesson={LESSON} id="para" step={0} role="vecC" fill={0.14} edge="none"
			pts={(vals) => { const q = sum(vals); return [O2, q.a, q.ab, q.b]; }} /></Show>
		{/* "Walk a, then b" and "walk b, then a": the two copies that close the parallelogram. */}
		<Show ov="walks">
			{!isZero(z.b) && <Arrow lesson={LESSON} id="b" from={z.a} to={z.ab} role="vecB" />}
			{!isZero(z.a) && <Arrow lesson={LESSON} id="a" from={z.b} to={z.ab} role="vecA" />}
		</Show>
		<Arrow lesson={LESSON} id="a" to="a" role="vecA" />
		<Arrow lesson={LESSON} id="b" to="b" role="vecB" />
		{!isZero(z.ab) && <Arrow lesson={LESSON} id="sum" to={z.ab} role="vecC" />}
		<Point lesson={LESSON} id="o" at={O2} role="muted" size={6} />
		<Name id="a" at={z.a} role="vecA" text="a" />
		<Name id="b" at={z.b} role="vecB" text="b" />
		<Name id="sum" at={z.ab} role="vecC" text={'a + b = ' + fmtV(z.ab)} />
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// VA-2: scaling.

function Scale() {
	const locale = useLocale(), v = useVals(LESSON), z = scaled(v);
	const far = RANGE_C.max * Math.max(-RANGE_S.min, RANGE_S.max) + 0.5;
	return <>
		<Reach lo={[-far, -far]} hi={[far, far]} />
		{/* The line through a: every s·a lies on it, so scaling can stretch or flip, never tilt. */}
		<Show ov="line">{!isZero(z.c) && <Segment lesson={LESSON} id="line" a={scale(z.c, -far / norm(z.c) * 1.5)} b={scale(z.c, far / norm(z.c) * 1.5)} dashed role="muted" />}</Show>
		{!isZero(z.sc) && <Arrow lesson={LESSON} id="sc" to={z.sc} role="vecC" />}
		<Arrow lesson={LESSON} id="c" to="c" role="vecA" label="a" />
		<Point lesson={LESSON} id="o" at={O2} role={z.sense === 0 ? 'vecC' : 'muted'} size={z.sense === 0 ? 10 : 6} />
		<Name id="c" at={z.c} role="vecA" text="a" />
		<Show ov="labels">
			{z.sense === 0
				? <Label lesson={LESSON} id="sc" at={O2} role="vecC" offset={[14, 16]}>{t(locale, 'va.zeroLabel')}</Label>
				: <Label lesson={LESSON} id="sc" at={z.sc} role="vecC" offset={[12, 14]}>{fmt1(z.s).replace('-', '−') + '·a = ' + fmtV(z.sc)}</Label>}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// VA-3: linear combination and the paint stamps.

/** Most stamps kept: a long scrub session stays bounded (oldest stamps are dropped first). */
const STAMPS = 4096;

/**
 * The paint toggle's stamps: while `paint` is on, every new tip of s·a + t·b the reader reaches is kept as a dot, so
 * scrubbing s and t fills the region the combinations reach (the span, before it has a name). Turning paint on starts
 * an empty canvas. Lesson-local: one THREE.Points with a preallocated buffer (no allocation per stamp), redrawn on
 * demand; marked `aux` so the home view never frames it.
 */
function Stamps() {
	const pal = usePalette();
	const invalidate = useThree((s) => s.invalidate);
	const geo = useMemo(() => {
		const g = new BufferGeometry();
		g.setAttribute('position', new BufferAttribute(new Float32Array(STAMPS * 3), 3));
		g.setDrawRange(0, 0);
		return g;
	}, []);
	const mat = useMemo(() => new PointsMaterial({ size: 4, sizeAttenuation: false, depthWrite: false, transparent: true, opacity: 0.85 }), []);
	useEffect(() => () => { geo.dispose(); mat.dispose(); }, [geo, mat]);
	useEffect(() => { mat.color.set(pal.vecC); invalidate(); }, [mat, pal.vecC, invalidate]);
	useEffect(() => {
		const store = lessonStore(LESSON);
		const pos = geo.getAttribute('position') as BufferAttribute;
		let n = 0, last = '', painting = false;
		const sync = () => {
			const vals = store.getState().vals;
			const on = vals.paint === 1;
			if (on && !painting) { n = 0; last = ''; }
			painting = on;
			if (on) {
				const r = combo(vals).r;
				const key = r.join();
				if (key !== last) {
					last = key;
					const i = n % STAMPS;
					pos.setXYZ(i, r[0], r[1], 0);
					pos.needsUpdate = true;
					n++;
				}
			}
			geo.setDrawRange(0, on ? Math.min(n, STAMPS) : 0);
			invalidate();
		};
		sync();
		return store.subscribe(sync);
	}, [geo, invalidate]);
	return <points geometry={geo} material={mat} userData={{ aux: true }} renderOrder={-1} raycast={noRaycast} />;
}

function Combo() {
	const v = useVals(LESSON), z = combo(v);
	return <>
		<Reach lo={[-8, -6]} hi={[8, 6]} />
		<Show ov="lattice">{!z.zero && !z.parallel && lattice(z.a, z.b).map(([p, q], i) =>
			<Segment key={i} lesson={LESSON} id="lattice" a={p} b={q} role="muted" />)}</Show>
		<Stamps />
		<Arrow lesson={LESSON} id="la" to="la" role="vecA" label="a" />
		<Arrow lesson={LESSON} id="lb" to="lb" role="vecB" label="b" />
		{/* The walk: s·a from the origin, then t·b from its tip; the sum r from the origin. */}
		{!isZero(z.sa) && <Arrow lesson={LESSON} id="sa" to={z.sa} role="vecA" />}
		{!isZero(z.tb) && <Arrow lesson={LESSON} id="tb" from={z.sa} to={z.r} role="vecB" />}
		{!isZero(z.r) && <Arrow lesson={LESSON} id="r" to={z.r} role="vecC" />}
		<Point lesson={LESSON} id="rt" at={z.r} role="vecC" size={8} />
		<Name id="la" at={z.a} role="vecA" text="a" />
		<Name id="lb" at={z.b} role="vecB" text="b" />
		<Show ov="labels"><Label lesson={LESSON} id="r" at={z.r} role="vecC" offset={[14, 14]}>{'r = ' + fmtV(z.r)}</Label></Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// VA-4: three arrows in 3D.

function Combo3() {
	const v = useVals(LESSON), z = combo3(v);
	return <>
		{/* The plane of a and b: every s·a + t·b (u = 0) lies on it. Drawn over the scrub range of s and t. */}
		<Show ov="plane">{norm(z.a) > 1e-9 && norm(z.b) > 1e-9 && <Parallelogram lesson={LESSON} id="plane" at={add(scale(z.a, RANGE_ST.min), scale(z.b, RANGE_ST.min))}
			a={scale(z.a, RANGE_ST.max - RANGE_ST.min)} b={scale(z.b, RANGE_ST.max - RANGE_ST.min)} role="muted" />}</Show>
		{!isZero(z.sa) && <Arrow lesson={LESSON} id="sa3" to={z.sa} role="vecA" />}
		{!isZero(z.tb) && <Arrow lesson={LESSON} id="tb3" from={z.sa} to={z.ab} role="vecB" />}
		{!isZero(z.uc) && <Arrow lesson={LESSON} id="uc3" from={z.ab} to={z.r} role="vecC" />}
		{!isZero(z.r) && <Arrow lesson={LESSON} id="r3" to={z.r} role="ink" />}
		<Show ov="labels">
			{!isZero(z.sa) && <Label lesson={LESSON} id="sa3" at={mid(O3, z.sa)} role="vecA" offset={[0, 14]}>s·a</Label>}
			{!isZero(z.tb) && <Label lesson={LESSON} id="tb3" at={mid(z.sa, z.ab)} role="vecB" offset={[0, 14]}>t·b</Label>}
			{!isZero(z.uc) && <Label lesson={LESSON} id="uc3" at={mid(z.ab, z.r)} role="vecC" offset={[14, 0]}>u·c</Label>}
			<Label lesson={LESSON} id="r3" at={z.r} role="ink" offset={TIP}>{'r = ' + fmtV(z.r)}</Label>
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// VA-5: blend shapes.

function Face() {
	const v = useVals(LESSON), z = blend(v), labels = useOverlay('labels'), deltas = useOverlay('deltas');
	return <>
		<Reach lo={[-2.4, -1.2]} hi={[2.4, 3.4]} />
		<Circle lesson={LESSON} id="head" center={HEAD.center} r={HEAD.r} role="muted" />
		{EYES.map((p, i) => <Point key={i} lesson={LESSON} id="eye" at={p} role="muted" size={10} />)}
		<Show ov="base"><Poly lesson={LESSON} id="base" step={0} pts={() => [...MOUTH]} role="muted" fill={0} edge="dashed" /></Show>
		<Poly lesson={LESSON} id="mouth" step={0} pts={(vals) => blend(vals).out} role="vecA" fill={0.18} />
		{deltas && MOUTH.map((p, i) => <group key={i}>
			{!isZero(z.d1[i]) && <Segment lesson={LESSON} id={i === CORNER ? 'd1' : 'd1v'} a={p} b={z.mid[i]} role="vecB" />}
			{!isZero(z.d2[i]) && <Segment lesson={LESSON} id={i === CORNER ? 'd2' : 'd2v'} a={z.mid[i]} b={z.out[i]} role="vecC" />}
		</group>)}
		{z.out.map((p, i) => <Point key={i} lesson={LESSON} id={i === CORNER ? 'corner' : 'vtx'} at={p} role="ink" size={i === CORNER ? 8 : 5} />)}
		{labels && <Label lesson={LESSON} id="corner" at={z.corner} role="ink" offset={[16, 0]}>{"P' = " + fmtV(z.corner, fmt2)}</Label>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const tip = (name: string) => ({ mouse: t(locale, 'va.hintTipMouse', { name }), touch: t(locale, 'va.hintTipTouch', { name }) });
	if (scene === 1) {
		return {
			hints: { 'a:tip': tip('a'), 'b:tip': tip('b'), sum: t(locale, 'va.hintSum'), para: t(locale, 'va.hintPara') },
			idle: t(locale, 'va.idle1'),
		};
	}
	if (scene === 2) {
		return {
			hints: { 'c:tip': tip('a'), sc: (v: Vals) => t(locale, 'va.hintSc', { s: fmt1(scaled(v).s) }), line: t(locale, 'va.hintLine') },
			idle: t(locale, 'va.idle2'),
		};
	}
	if (scene === 3) {
		return {
			hints: { 'la:tip': tip('a'), 'lb:tip': tip('b'), r: t(locale, 'va.hintR'), lattice: t(locale, 'va.hintLattice') },
			idle: t(locale, 'va.idle3'),
			state: (v: Vals) => (v.paint === 1 ? t(locale, 'va.statePaint') : null),
		};
	}
	if (scene === 4) return { hints: { plane: t(locale, 'va.hintPlane'), r3: t(locale, 'va.hintR3') }, idle: t(locale, 'va.idle4') };
	return { hints: { corner: t(locale, 'va.hintCorner'), d1: t(locale, 'va.hintD1'), d2: t(locale, 'va.hintD2'), base: t(locale, 'va.hintBase') }, idle: t(locale, 'va.idle5') };
}

function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	const base = [ov('grid', 'va.ovGrid'), ov('axes', 'va.ovAxes'), ov('labels', 'va.ovLabels')];
	if (scene === 1) return [...base, ov('parallelogram', 'va.ovParallelogram'), ov('walks', 'va.ovWalks')];
	if (scene === 2) return [...base, ov('line', 'va.ovLine')];
	if (scene === 3) return [...base, ov('lattice', 'va.ovLattice', false)];
	if (scene === 4) return [...base, ov('plane', 'va.ovPlane')];
	return [ov('labels', 'va.ovLabels'), ov('base', 'va.ovBase'), ov('deltas', 'va.ovDeltas')];
}

const NAMES = { 1: 'va.name1', 2: 'va.name2', 3: 'va.name3', 4: 'va.name4', 5: 'va.name5' } as const;
const EXTENT: Record<Scene, number> = { 1: 5, 2: 6.5, 3: 8, 4: 3, 5: 2.4 };
const GRID: Record<Scene, number> = { 1: 5, 2: 6, 3: 6, 4: 3, 5: 0 };

export function ArithmeticScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	return <ViewportCard lesson={LESSON} id={'VA-' + scene} name={'VA-' + scene + ' · ' + t(locale, NAMES[scene])}
		mode={scene === 4 ? '3d' : '2d'} extent={EXTENT[scene]} center={scene === 5 ? [0, 1.1] : undefined}
		rows={rowsFor(scene, locale)} overlays={overlaysFor(scene, locale)} status={statusFor(scene, locale)}
		drawIn={scene === 1}>
		{scene !== 5 && <Floor extent={GRID[scene]} three={scene === 4} />}
		{scene === 1 ? <Sum /> : scene === 2 ? <Scale /> : scene === 3 ? <Combo /> : scene === 4 ? <Combo3 /> : <Face />}
	</ViewportCard>;
}
