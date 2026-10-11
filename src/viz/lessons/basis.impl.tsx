import { useMemo } from 'react';
import { BoxGeometry } from 'three';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useHot, useVals } from '../store';
import { add, norm, planeBasis, scale, sub, to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { Stroke } from '../scene/view';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Parallelogram, Quad } from '../scene/Parallelogram';
import { AngleArc } from '../scene/AngleArc';
import { RightAngle } from '../scene/RightAngle';
import { Label } from '../scene/Label';
import { HOVER_RANK, Show, useHoverable, useOverlay } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './basis.rows';
import {
	EXT1, EXT3, LESSON, RULER_HALF, SPAN_HALF, TH_TARGETS, clipLine, derive1, derive2, derive3, derive4, fmt, fmtDeg, handleU1,
	lattice, moveTh, moveW, rulerLine, rulerTicks,
} from './basis.math';

/**
 * Lesson 1.6 scenes: BS-1 the movable lattice (2D), BS-2 span in 3D, BS-3 a rotated perpendicular unit pair (2D),
 * BS-4 shadows on new rulers with the orthonormal flag (3D). Loaded lazily by the entry (basis.tsx); shares its store,
 * its one model (basis.math.ts) and its hover ids. Scene code reads `shown` values, the DOM reads `vals`.
 * Look: the Blender-style flat viewport (visual-language 6d/6e; user ruling 2026-10-11).
 */

type Vals = Parameters<typeof derive1>[0];

const O2: V = [0, 0];
const O3: V = [0, 0, 0];
const TIP: readonly [number, number] = [10, -12];
const RULER_ROLE: readonly SceneRole[] = ['vecA', 'vecB', 'vecC'];
const SUB = ['₁', '₂', '₃'];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);

/**
 * The reach of a 2D scene: an invisible square over everything the reader can make it draw. The home view frames the
 * figure's bounds once (Stage), so framing the reach keeps the view still while rulers turn and lattices shear. The 3D
 * scenes frame their own figure (a reach cube's bounding sphere left the arrows a third of the view).
 */
function Reach({ half }: { half: number }) {
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast} scale={[2 * half, 2 * half, 0.01]} />;
}

/**
 * Thin guide lines drawn as one segment list (the lattice, the rulers' ticks): one hoverable object, a 1.5 px line
 * without halo at a reduced opacity, so it reads as construction under the arrows. Nothing to draw: nothing mounted
 * (drei's Line needs at least one segment).
 */
function Guides({ id, segs, role, opacity = 0.5 }: { id: string; segs: readonly (readonly V[])[]; role: SceneRole; opacity?: number }) {
	const pal = usePalette();
	const hot = useHot(LESSON, id);
	const hover = useHoverable(LESSON, id, HOVER_RANK.guide);
	const pts = useMemo(() => segs.flatMap(([a, b]) => [to3(a), to3(b)]), [segs]);
	if (pts.length < 2) return null;
	return <group {...hover}><Stroke points={pts} segments color={pal[role]} hot={hot} width={STYLE.edge} halo={false} opacity={opacity} order={0.8} /></group>;
}

/** Grid under the `grid` overlay with the Blender axes under `axes`; 2D scenes get half-unit minor lines and ticks. */
function Floor({ extent, dims }: { extent: number; dims: 2 | 3 }) {
	const axes = useOverlay('axes');
	return <Show ov="grid"><Grid lesson={LESSON} extent={extent} step={1} minor={dims === 2 ? 0.5 : undefined} axes={axes} ticks={dims === 2 && axes} /></Show>;
}

const mid = (p: V, q: V) => scale(add(p, q), 0.5);

// ---------------------------------------------------------------------------------------------------------------
// BS-1: the movable lattice.

function Lattice() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const z = derive1(v);
	const labels = useOverlay('labels');
	const legs = useOverlay('legs');
	const lat = useMemo(() => lattice(v.u1 as V, v.u2 as V, EXT1), [v.u1, v.u2]);
	return <>
		<Reach half={EXT1} />
		<Show ov="lattice">
			{z.dim === 2 && <>
				<Guides id="lat_s" segs={lat.s} role="vecA" />
				<Guides id="lat_t" segs={lat.t} role="vecB" />
			</>}
			{z.spanLine && <>
				<Segment lesson={LESSON} id="span" a={z.spanLine[0]} b={z.spanLine[1]} role="vecC" />
				{labels && <Label lesson={LESSON} id="span" at={scale(z.spanLine[1], 0.8)} role="vecC" offset={[0, -16]}>{t(locale, 'bas.spanLine')}</Label>}
			</>}
		</Show>
		<Show ov="cell">
			{/* No text on the cell: its area is the panel's meter and the hover status; a label here crowded the rulers' names. */}
			<Parallelogram lesson={LESSON} id="area" a="u1" b="u2" role="vecC" />
		</Show>
		<Show ov="walk">{z.leg && <>
			<Segment lesson={LESSON} id="c1" a={O2} b={z.leg} dashed role="vecA" />
			<Segment lesson={LESSON} id="c2" a={z.leg} b={z.v} dashed role="vecB" />
			{labels && <>
				<Label lesson={LESSON} id="c1" at={mid(O2, z.leg)} role="vecA" away={mid(z.leg, z.v)} gap={4}>{fmt(z.c1) + '·u₁'}</Label>
				<Label lesson={LESSON} id="c2" at={mid(z.leg, z.v)} role="vecB" away={mid(O2, z.leg)} gap={4}>{fmt(z.c2) + '·u₂'}</Label>
			</>}
		</>}</Show>
		{z.zeroBoth && labels && <Label lesson={LESSON} id="span" at={O2} role="vecC" offset={[0, 18]}>{t(locale, 'bas.spanPoint')}</Label>}
		<Arrow lesson={LESSON} id="v" to="v" role="ink" legs={legs} />
		<Arrow lesson={LESSON} id="u1" to="u1" role="vecA" label="u₁" />
		<Arrow lesson={LESSON} id="u2" to="u2" role="vecB" label="u₂" />
		<Show ov="labels">
			<Label lesson={LESSON} id="u1" at="u1" role="vecA" offset={TIP}>u₁</Label>
			<Label lesson={LESSON} id="u2" at="u2" role="vecB" offset={TIP}>u₂</Label>
			<Label lesson={LESSON} id="v" at="v" role="ink" offset={TIP}>v</Label>
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// BS-2: span of three arrows in 3D.

const MOVE_W = [moveW(0), moveW(1), moveW(2)] as const;
const W_NAMES = ['a', 'b', 'c'] as const;

function Span3() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const z = derive2(v);
	const labels = useOverlay('labels');
	const plane = z.normal ? planeBasis(z.normal) : null;
	// The label of the span sits at a corner of the plane / the end of the line / the far corner of the box.
	const spanAt = plane ? add(scale(plane[0], 0.85 * SPAN_HALF), scale(plane[1], 0.85 * SPAN_HALF))
		: z.dir ? scale(z.dir, 0.9 * SPAN_HALF)
		: z.dim === 3 ? add(add(z.w[0], z.w[1]), z.w[2]) // the far corner of the box
		: [0, 0, 0];
	const spanText = t(locale, z.dim === 3 ? 'bas.spanSpace' : z.dim === 2 ? 'bas.spanPlane' : z.dim === 1 ? 'bas.spanLine' : 'bas.spanPoint');
	return <>
		<Show ov="span">
			{plane && <Quad lesson={LESSON} id="dim3" o={sub(O3, add(scale(plane[0], SPAN_HALF), scale(plane[1], SPAN_HALF)))}
				u={scale(plane[0], 2 * SPAN_HALF)} v={scale(plane[1], 2 * SPAN_HALF)} role="vecC" neg={false} />}
			{z.dir && <Segment lesson={LESSON} id="dim3" a={scale(z.dir, -SPAN_HALF)} b={scale(z.dir, SPAN_HALF)} role="vecC" />}
			{/* All of space: the box on the three arrows (three faces from the origin) shows the volume they enclose. */}
			{z.dim === 3 && <>
				<Parallelogram lesson={LESSON} id="vol" a="w1" b="w2" role="muted" />
				<Parallelogram lesson={LESSON} id="vol" a="w2" b="w3" role="muted" />
				<Parallelogram lesson={LESSON} id="vol" a="w3" b="w1" role="muted" />
			</>}
			{labels && <Label lesson={LESSON} id={z.dim === 3 ? 'vol' : 'dim3'} at={spanAt} role={z.dim === 3 ? 'ink' : 'vecC'} offset={[0, 16]}>{spanText}</Label>}
		</Show>
		{(['w1', 'w2', 'w3'] as const).map((k, i) => <Arrow key={k} lesson={LESSON} id={k} to={k} role={RULER_ROLE[i]} label={W_NAMES[i]} move={MOVE_W[i]} />)}
		<Show ov="labels">
			{(['w1', 'w2', 'w3'] as const).map((k, i) => <Label key={k} lesson={LESSON} id={k} at={k} role={RULER_ROLE[i]} offset={TIP}>{W_NAMES[i]}</Label>)}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// BS-3: a rotated perpendicular unit pair.

/** Tick crossbars of a 2D ruler: a short bar across the line at every whole step k·u inside the square. */
function ticks2(u: V, ext: number): number[][][] {
	const n = [-u[1], u[0]];
	const bar = scale(n, 0.08);
	const out: number[][][] = [];
	for (let k = -Math.floor(ext); k <= Math.floor(ext); k++) {
		if (k === 0) continue;
		const p = scale(u, k);
		if (Math.abs(p[0]) > ext || Math.abs(p[1]) > ext) continue;
		out.push([sub(p, bar), add(p, bar)]);
	}
	return out;
}

/** Arc marks at the magnet targets of the angle drag (one list for the magnet and the marks). */
const ARC_TICKS = TH_TARGETS.filter((d) => d >= 0);

function Rotated() {
	const v = useVals(LESSON);
	const z = derive3(v);
	const labels = useOverlay('labels');
	const lines = [clipLine(O2, z.u1, EXT3), clipLine(O2, z.u2, EXT3)];
	const tk = useMemo(() => [ticks2(z.u1, EXT3), ticks2(z.u2, EXT3)], [z.u1, z.u2]);
	const feet = [z.f1, z.f2];
	const reads = [z.c1, z.c2];
	return <>
		<Reach half={EXT3} />
		{lines.map((seg, i) => seg && <Guides key={i} id={'ax' + (i + 1)} segs={[seg]} role={RULER_ROLE[i]} opacity={0.6} />)}
		<Show ov="ticks">{tk.map((segs, i) => <Guides key={i} id={'ax' + (i + 1)} segs={segs} role={RULER_ROLE[i]} opacity={0.8} />)}</Show>
		<Show ov="shadows">{feet.map((f, i) => {
			const id = 'sc' + (i + 1);
			const role = RULER_ROLE[i];
			return <group key={id}>
				{norm(f) > 1e-9 && <Segment lesson={LESSON} id={id} a={O2} b={f} band role={role} />}
				{norm(sub(z.p, f)) > 1e-9 && <Segment lesson={LESSON} id={id} a={z.p} b={f} dashed role={role} />}
				<RightAngle lesson={LESSON} id={id} at={f} u={sub(z.p, f)} v={i === 0 ? z.u1 : z.u2} show={norm(sub(z.p, f)) > 1e-9} />
				<Point lesson={LESSON} id={id} at={f} role={role} size={5} />
				{labels && <Label lesson={LESSON} id={id} at={f} role={role} away={z.p} gap={6}>{'c' + SUB[i] + ' = ' + fmt(reads[i])}</Label>}
			</group>;
		})}</Show>
		<Show ov="angle"><AngleArc lesson={LESSON} id="th" at={O2} from={[1, 0]} to={z.u1} r={0.75} ticks={ARC_TICKS}
			label={labels ? () => 'θ = ' + fmtDeg(z.th) + '°' : undefined} /></Show>
		<Arrow lesson={LESSON} id="ru1" to={z.u1} role="vecA" drag="none" />
		<Arrow lesson={LESSON} id="ru2" to={z.u2} role="vecB" drag="none" />
		<Point lesson={LESSON} id="ru1" at={handleU1} keys={['th']} move={moveTh} role="vecA" label="u₁" />
		<Point lesson={LESSON} id="P" at="p" role="ink" label="P" />
		<Show ov="labels">
			<Label lesson={LESSON} id="ru1" at={z.u1} role="vecA" offset={TIP}>u₁</Label>
			<Label lesson={LESSON} id="ru2" at={z.u2} role="vecB" offset={TIP}>u₂</Label>
			{/* Just the name: the numbers are in the panel and the table; a long label pushed the shadow values away. */}
			<Label lesson={LESSON} id="P" at="p" role="ink" offset={TIP}>P</Label>
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// BS-4: shadows on new rulers.

/** Tick marks of a 3D ruler: a small cross (two bars in the plane perpendicular to the ruler) at every k·u. */
function ticks3(u: V): number[][][] {
	const pb = norm(u) > 0 ? planeBasis(u) : null;
	if (!pb) return [];
	const [a, b] = pb.map((e) => scale(e, 0.07));
	return rulerTicks(u).flatMap((p) => [[sub(p, a), add(p, a)], [sub(p, b), add(p, b)]]);
}

function Rulers() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const z = derive4(v);
	const labels = useOverlay('labels');
	const tk = useMemo(() => [ticks3(v.r1 as V), ticks3(v.r2 as V), ticks3(v.r3 as V)], [v.r1, v.r2, v.r3]);
	const ghostOff = z.gap > 1e-6;
	return <>
		{z.u.map((u, i) => {
			const line = rulerLine(u);
			return line && <Guides key={i} id={'r' + (i + 1)} segs={[line]} role={RULER_ROLE[i]} opacity={0.55} />;
		})}
		<Show ov="ticks">{tk.map((segs, i) => <Guides key={i} id={'r' + (i + 1)} segs={segs} role={RULER_ROLE[i]} opacity={0.9} />)}</Show>
		<Show ov="shadows">{z.feet.map((f, i) => {
			if (!f) return null;
			const id = 'rd' + (i + 1);
			const role = RULER_ROLE[i];
			return <group key={id}>
				{norm(f) > 1e-9 && <Segment lesson={LESSON} id={id} a={O3} b={f} band role={role} />}
				{norm(sub(z.v, f)) > 1e-9 && <Segment lesson={LESSON} id={id} a={z.v} b={f} dashed role={role} />}
				<Point lesson={LESSON} id={id} at={f} role={role} size={5} />
			</group>;
		})}</Show>
		{/*
		  The true coordinates: the oblique walk c₁u₁, then c₂u₂, then c₃u₃, ending exactly on v, with its corners cᵢuᵢ on
		  the rulers. Drawn only for rulers that are not orthonormal: for an orthonormal frame the corners are the shadow
		  feet and the walk would only repeat the shadows.
		*/}
		<Show ov="walk">{z.walk && !z.orthonormal && <>
			{z.walk.slice(1).map((p, i) => {
				const a = z.walk![i];
				return norm(sub(p, a)) > 1e-9 && <Segment key={i} lesson={LESSON} id={'q' + (i + 1)} a={a} b={p} dashed role={RULER_ROLE[i]} />;
			})}
			{z.marks!.map((m, i) => <Point key={i} lesson={LESSON} id={'q' + (i + 1)} at={m} role={RULER_ROLE[i]} size={8} />)}
		</>}</Show>
		{/* What "shadows as coordinates" would rebuild: Σ (uᵢ·v) uᵢ, with the dashed gap to v when it misses. */}
		<Show ov="ghost">{ghostOff && <>
			<Arrow lesson={LESSON} id="ghost" to={z.ghost} role="muted" drag="none" />
			<Segment lesson={LESSON} id="gap" a={z.ghost} b={z.v} dashed role="hot" />
			{labels && <Label lesson={LESSON} id="ghost" at={z.ghost} role="muted" offset={[14, 14]}>{t(locale, 'bas.ghostLabel')}</Label>}
		</>}</Show>
		{!z.c && labels && <Label lesson={LESSON} id="hand" at={O3} role="hot" offset={[0, 22]}>{t(locale, 'bas.noCoords')}</Label>}
		<Arrow lesson={LESSON} id="v3" to="v3" role="ink" label="v" />
		{(['r1', 'r2', 'r3'] as const).map((k, i) => <Arrow key={k} lesson={LESSON} id={k} to={k} role={RULER_ROLE[i]} label={'u' + SUB[i]} />)}
		<Show ov="labels">
			<Label lesson={LESSON} id="v3" at="v3" role="ink" offset={TIP}>v</Label>
			{(['r1', 'r2', 'r3'] as const).map((k, i) => <Label key={k} lesson={LESSON} id={k} at={k} role={RULER_ROLE[i]} offset={TIP}>{'u' + SUB[i]}</Label>)}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

/** Status bar hints per scene; values are read live. */
function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const tip = (name: string) => ({ mouse: t(locale, 'bas.hintTipMouse', { name }), touch: t(locale, 'bas.hintTipTouch', { name }) });
	if (scene === 1) {
		return {
			hints: {
				'u1:tip': tip('u₁'), 'u2:tip': tip('u₂'), 'v:tip': tip('v'),
				lat_s: t(locale, 'bas.hintLatS'), lat_t: t(locale, 'bas.hintLatT'),
				area: (x: Vals) => t(locale, 'bas.hintArea', { area: fmt(derive1(x).area) }),
				span: t(locale, 'bas.hintSpan'),
				c1: (x: Vals) => t(locale, 'bas.hintC1', { c: fmt(derive1(x).c1) }),
				c2: (x: Vals) => t(locale, 'bas.hintC2', { c: fmt(derive1(x).c2) }),
			},
			state: (x: Vals) => {
				const z = derive1(x);
				return z.dim === 2 ? null : z.dim === 1 ? (z.onLine ? t(locale, 'bas.stateLineOn') : t(locale, 'bas.stateLineOff')) : t(locale, 'bas.statePoint');
			},
		};
	}
	if (scene === 2) {
		const tip3 = (name: string) => ({ mouse: t(locale, 'bas.hintTip3', { name }), touch: t(locale, 'bas.hintTipTouch', { name }) });
		return {
			hints: {
				'w1:tip': tip3('a'), 'w2:tip': tip3('b'), 'w3:tip': tip3('c'),
				dim3: t(locale, 'bas.hintPlane'),
				vol: (x: Vals) => t(locale, 'bas.hintVol', { vol: fmt(derive2(x).vol) }),
			},
		};
	}
	if (scene === 3) {
		return {
			hints: {
				'ru1:pt': { mouse: t(locale, 'bas.hintTurnMouse'), touch: t(locale, 'bas.hintTurnTouch') },
				'P:pt': tip('P'),
				sc1: (x: Vals) => t(locale, 'bas.hintShadow', { name: 'c₁', value: fmt(derive3(x).c1) }),
				sc2: (x: Vals) => t(locale, 'bas.hintShadow', { name: 'c₂', value: fmt(derive3(x).c2) }),
				th: t(locale, 'bas.hintTh'),
			},
		};
	}
	const tip3 = (name: string) => ({ mouse: t(locale, 'bas.hintTip3', { name }), touch: t(locale, 'bas.hintTipTouch', { name }) });
	return {
		hints: {
			'v3:tip': tip3('v'), 'r1:tip': tip3('u₁'), 'r2:tip': tip3('u₂'), 'r3:tip': tip3('u₃'),
			rd1: (x: Vals) => t(locale, 'bas.hintReading', { name: 'u₁·v', value: fmt(derive4(x).rd[0]) }),
			rd2: (x: Vals) => t(locale, 'bas.hintReading', { name: 'u₂·v', value: fmt(derive4(x).rd[1]) }),
			rd3: (x: Vals) => t(locale, 'bas.hintReading', { name: 'u₃·v', value: fmt(derive4(x).rd[2]) }),
			gap: (x: Vals) => t(locale, 'bas.hintGap', { gap: fmt(derive4(x).gap) }),
			ghost: t(locale, 'bas.hintGhost'),
		},
		state: (x: Vals) => (derive4(x).ortho ? t(locale, 'bas.stateOrtho') : t(locale, 'bas.stateFree')),
	};
}

/** Overlay toggles per scene, each with its default. */
function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	if (scene === 1) return [ov('grid', 'bas.ovGrid'), ov('axes', 'bas.ovAxes'), ov('lattice', 'bas.ovLattice'), ov('cell', 'bas.ovCell'), ov('walk', 'bas.ovWalk'), ov('labels', 'bas.ovLabels'), ov('legs', 'bas.ovLegs', false)];
	if (scene === 2) return [ov('grid', 'bas.ovGrid'), ov('axes', 'bas.ovAxes'), ov('span', 'bas.ovSpan'), ov('labels', 'bas.ovLabels')];
	if (scene === 3) return [ov('grid', 'bas.ovGrid'), ov('axes', 'bas.ovAxes'), ov('ticks', 'bas.ovTicks'), ov('shadows', 'bas.ovShadows'), ov('angle', 'bas.ovAngle'), ov('labels', 'bas.ovLabels')];
	return [ov('grid', 'bas.ovGrid'), ov('axes', 'bas.ovAxes'), ov('ticks', 'bas.ovTicks'), ov('shadows', 'bas.ovShadows'), ov('walk', 'bas.ovWalk'), ov('ghost', 'bas.ovGhost'), ov('labels', 'bas.ovLabels')];
}

const NAME_KEYS = { 1: 'bas.name1', 2: 'bas.name2', 3: 'bas.name3', 4: 'bas.name4' } as const;
const EXTENT: Record<Scene, number> = { 1: EXT1, 2: SPAN_HALF, 3: EXT3, 4: RULER_HALF };

export function BasisScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const dims = scene === 2 || scene === 4 ? 3 : 2;
	return <ViewportCard lesson={LESSON} id={'BS-' + scene} name={'BS-' + scene + ' · ' + t(locale, NAME_KEYS[scene])}
		mode={dims === 3 ? '3d' : '2d'} extent={EXTENT[scene]}
		rows={rowsFor(scene, locale)} overlays={overlaysFor(scene, locale)} status={statusFor(scene, locale)}
		drawIn={scene === 1}>
		<Floor extent={EXTENT[scene]} dims={dims} />
		{scene === 1 ? <Lattice /> : scene === 2 ? <Span3 /> : scene === 3 ? <Rotated /> : <Rulers />}
	</ViewportCard>;
}
