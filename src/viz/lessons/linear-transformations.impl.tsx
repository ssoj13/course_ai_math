import { useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useHot, useVals } from '../store';
import { add, norm, scale, to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { Stroke } from '../scene/view';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Room } from '../scene/Stage';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Parallelogram } from '../scene/Parallelogram';
import { Poly } from '../scene/Poly';
import { Label } from '../scene/Label';
import { HOVER_RANK, Show, useHoverable, useOverlay } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { lattice } from './basis.math';
import { rowsFor, type Scene } from './linear-transformations.rows';
import { EXT, FIG, LESSON, T_DRAG, derive1, derive2, fmt1, fmt2, fmtV, handleCol, mappedGrid, moveCol } from './linear-transformations.math';

/**
 * Lesson 2.1 scenes: LT-1 "where the basis lands" (drag the images of e₁ and e₂; the grid, a letter F and the image of v
 * follow; a morph from the identity to the map) and LT-2 "linear or not?" (the same tests on a linear, a shifted and a
 * bent map). Loaded lazily by the entry (linear-transformations.tsx); shares its store, its model and its hover ids.
 * Look: the Blender-style flat viewport (visual-language 6d/6e; user ruling 2026-10-11).
 */

type Vals = Parameters<typeof derive1>[0];

const O2: V = [0, 0];
const TIP: readonly [number, number] = [10, -12];
/** Square the reader can move things in: the home view frames the figure plus this room. */
const ROOM = 4.5;

const isZero = (p: V) => norm(p) < 1e-9;
const mid = (p: V, q: V) => scale(add(p, q), 0.5);

/**
 * Thin guide lines drawn as one segment list (an image grid): one hoverable object, a 1.5 px line without halo at a
 * reduced opacity, so it reads as construction under the arrows. Nothing to draw: nothing mounted (drei's Line needs
 * at least one segment).
 */
function Guides({ id, segs, role, opacity = 0.55 }: { id: string; segs: readonly (readonly V[])[]; role: SceneRole; opacity?: number }) {
	const pal = usePalette();
	const hot = useHot(LESSON, id);
	const hover = useHoverable(LESSON, id, HOVER_RANK.guide);
	const pts = useMemo(() => segs.flatMap(([a, b]) => [to3(a), to3(b)]), [segs]);
	if (pts.length < 2) return null;
	return <group {...hover}><Stroke points={pts} segments color={pal[role]} hot={hot} width={STYLE.edge} halo={false} opacity={opacity} order={0.8} /></group>;
}

/** The world grid (the untouched input plane) under `grid`, with the axes and tick numbers under `axes`. */
function Floor() {
	const axes = useOverlay('axes');
	return <Grid lesson={LESSON} extent={ROOM} step={1} minor={0.5} axes={axes} ticks={axes} lines={useOverlay('grid')} />;
}

// ---------------------------------------------------------------------------------------------------------------
// LT-1: where the basis lands.

const handle1 = handleCol(1);
const handle2 = handleCol(2);
const move1 = moveCol(1);
const move2 = moveCol(2);

function Lands() {
	const v = useVals(LESSON);
	const z = derive1(v);
	const labels = useOverlay('labels');
	const lat = useMemo(() => lattice(z.m1, z.m2, ROOM), [z.m1, z.m2]);
	const drag = z.t >= T_DRAG;
	return <>
		<Room lo={[-ROOM, -ROOM]} hi={[ROOM, ROOM]} />
		{/* The image of the grid: lines x = k land on lines parallel to T(e₂), lines y = k on lines parallel to T(e₁). */}
		<Show ov="lattice">
			<Guides id="lat1" segs={lat.s} role="vecA" />
			<Guides id="lat2" segs={lat.t} role="vecB" />
		</Show>
		<Show ov="cell"><Parallelogram lesson={LESSON} id="area" a={z.m1} b={z.m2} role="vecC" /></Show>
		<Show ov="figure">
			<Poly lesson={LESSON} id="figIn" step={0} role="muted" fill={0} edge="dashed" pts={() => [...FIG]} />
			<Poly lesson={LESSON} id="fig" step={0} role="vecC" fill={0.22} pts={(vals) => derive1(vals).fig}
				hatched={(vals) => derive1(vals).detT < 0} />
		</Show>
		<Show ov="basis">
			<Arrow lesson={LESSON} id="e1" to={[1, 0]} role="muted" opacity={0.6} />
			<Arrow lesson={LESSON} id="e2" to={[0, 1]} role="muted" opacity={0.6} />
		</Show>
		{/* The input v and the walk to its image: v₁ copies of T(e₁), then v₂ copies of T(e₂). */}
		<Arrow lesson={LESSON} id="v" to="v" role="muted" label="v" />
		<Show ov="walk">
			{!isZero(z.leg) && <Segment lesson={LESSON} id="walk1" a={O2} b={z.leg} dashed role="vecA" />}
			{!isZero(add(z.avT, scale(z.leg, -1))) && <Segment lesson={LESSON} id="walk2" a={z.leg} b={z.avT} dashed role="vecB" />}
		</Show>
		{!isZero(z.avT) && <Arrow lesson={LESSON} id="av" to={z.avT} role="ink" />}
		<Arrow lesson={LESSON} id="a1" to={z.m1} role="vecA" drag="none" />
		<Arrow lesson={LESSON} id="a2" to={z.m2} role="vecB" drag="none" />
		<Point lesson={LESSON} id="a1" at={handle1} keys={['a1']} move={move1} drag={drag} role="vecA" label="T(e₁)" />
		<Point lesson={LESSON} id="a2" at={handle2} keys={['a2']} move={move2} drag={drag} role="vecB" label="T(e₂)" />
		{labels && <>
			<Label lesson={LESSON} id="a1" at={z.m1} role="vecA" offset={TIP}>T(e₁)</Label>
			<Label lesson={LESSON} id="a2" at={z.m2} role="vecB" offset={TIP}>T(e₂)</Label>
			<Label lesson={LESSON} id="v" at={z.v} role="muted" offset={TIP}>v</Label>
			{!isZero(z.avT) && <Label lesson={LESSON} id="av" at={z.avT} role="ink" away={O2} gap={8}>{'T(v) = ' + fmtV(z.avT)}</Label>}
			{!isZero(z.leg) && <Label lesson={LESSON} id="walk1" at={mid(O2, z.leg)} role="vecA" away={mid(z.leg, z.avT)} gap={4}>{fmt1(z.v[0]) + '·T(e₁)'}</Label>}
		</>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// LT-2: linear or not?

function Tests() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const z = derive2(v);
	const labels = useOverlay('labels');
	const grid = useMemo(() => mappedGrid(z.mode, z.a1, z.a2), [z.mode, z.a1, z.a2]);
	const missAdd = z.gapAdd > 1e-6;
	const missScale = z.gapScale > 1e-6;
	return <>
		<Room lo={[-ROOM, -ROOM]} hi={[ROOM, ROOM]} />
		{/* The image grid can reach far outside the view (aux: the home view frames the test arrows, not these lines). */}
		<group userData={{ aux: true }}>
			<Show ov="lattice">
				<Guides id="img1" segs={grid.vertical} role="vecA" />
				<Guides id="img2" segs={grid.horizontal} role="vecB" />
			</Show>
		</group>
		<Show ov="inputs">
			<Arrow lesson={LESSON} id="v" to="v" role="muted" opacity={0.6} label="v" />
			<Arrow lesson={LESSON} id="w" to="w" role="muted" opacity={0.6} label="w" />
		</Show>
		{/* Route 1: transform v and w, then add the images head to tail (the parallelogram's far corner). */}
		{!isZero(z.tv) && <Arrow lesson={LESSON} id="tv" to={z.tv} role="vecA" />}
		{!isZero(z.tw) && <Arrow lesson={LESSON} id="tw" to={z.tw} role="vecB" />}
		<Show ov="walk">
			{!isZero(z.tw) && <Segment lesson={LESSON} id="sum" a={z.tv} b={z.sum} dashed role="vecB" />}
			{!isZero(z.tv) && <Segment lesson={LESSON} id="sum" a={z.tw} b={z.sum} dashed role="vecA" />}
		</Show>
		<Point lesson={LESSON} id="sum" at={z.sum} role="ink" size={9} />
		{/* Route 2: add first, then transform the sum. */}
		{!isZero(z.tvw) && <Arrow lesson={LESSON} id="tvw" to={z.tvw} role="vecC" />}
		{missAdd && <Segment lesson={LESSON} id="gapAdd" a={z.sum} b={z.tvw} dashed role="hot" />}
		<Show ov="scaling">
			<Point lesson={LESSON} id="tsv" at={z.tsv} role="vecC" size={7} />
			<Point lesson={LESSON} id="stv" at={z.stv} role="ink" size={7} />
			{missScale && <Segment lesson={LESSON} id="gapScale" a={z.stv} b={z.tsv} dashed role="hot" />}
		</Show>
		<Point lesson={LESSON} id="origin" at={z.origin} role={z.originOk ? 'muted' : 'hot'} size={z.originOk ? 6 : 9} />
		{labels && <>
			{!isZero(z.tv) && <Label lesson={LESSON} id="tv" at={z.tv} role="vecA" offset={TIP}>T(v)</Label>}
			{!isZero(z.tw) && <Label lesson={LESSON} id="tw" at={z.tw} role="vecB" offset={TIP}>T(w)</Label>}
			<Label lesson={LESSON} id="sum" at={z.sum} role="ink" offset={[12, 14]}>T(v) + T(w)</Label>
			{missAdd && !isZero(z.tvw) && <Label lesson={LESSON} id="tvw" at={z.tvw} role="vecC" offset={TIP}>T(v + w)</Label>}
			{!z.originOk && <Label lesson={LESSON} id="origin" at={z.origin} role="hot" offset={[12, 14]}>{t(locale, 'lt.originMoved', { p: fmtV(z.origin) })}</Label>}
		</>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

function statusFor(scene: Scene, locale: Locale): StatusSpec {
	if (scene === 1) {
		const col = (name: string) => ({ mouse: t(locale, 'lt.hintColMouse', { name }), touch: t(locale, 'lt.hintColTouch', { name }) });
		return {
			hints: {
				'a1:pt': col('T(e₁)'), 'a2:pt': col('T(e₂)'),
				'v:tip': { mouse: t(locale, 'lt.hintVMouse'), touch: t(locale, 'lt.hintVTouch') },
				lat1: t(locale, 'lt.hintLat1'), lat2: t(locale, 'lt.hintLat2'),
				area: (x: Vals) => t(locale, 'lt.hintArea', { area: fmt2(derive1(x).detT) }),
				fig: t(locale, 'lt.hintFig'), figIn: t(locale, 'lt.hintFigIn'),
				av: (x: Vals) => t(locale, 'lt.hintAv', { p: fmtV(derive1(x).avT) }),
			},
			state: (x: Vals) => {
				const z = derive1(x);
				return z.t < T_DRAG ? t(locale, 'lt.stateMorph') : z.flat ? t(locale, 'lt.stateFlat') : z.detT < 0 ? t(locale, 'lt.stateMirror') : null;
			},
		};
	}
	return {
		hints: {
			img1: t(locale, 'lt.hintImg1'), img2: t(locale, 'lt.hintImg2'),
			tv: t(locale, 'lt.hintTv'), tw: t(locale, 'lt.hintTw'), tvw: t(locale, 'lt.hintTvw'), sum: t(locale, 'lt.hintSum'),
			gapAdd: (x: Vals) => t(locale, 'lt.hintGap', { gap: fmt2(derive2(x).gapAdd) }),
			gapScale: (x: Vals) => t(locale, 'lt.hintGap', { gap: fmt2(derive2(x).gapScale) }),
			origin: (x: Vals) => t(locale, 'lt.hintOrigin', { p: fmtV(derive2(x).origin) }),
		},
		state: (x: Vals) => {
			const z = derive2(x);
			return z.mode === 0 ? t(locale, 'lt.stateLinear') : z.mode === 1 ? t(locale, 'lt.stateShift') : t(locale, 'lt.stateBend');
		},
	};
}

function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	const base = [ov('grid', 'lt.ovGrid'), ov('axes', 'lt.ovAxes'), ov('lattice', 'lt.ovLattice'), ov('labels', 'lt.ovLabels')];
	if (scene === 1) return [...base, ov('cell', 'lt.ovCell'), ov('figure', 'lt.ovFigure'), ov('basis', 'lt.ovBasis'), ov('walk', 'lt.ovWalk')];
	return [...base, ov('walk', 'lt.ovRoutes'), ov('inputs', 'lt.ovInputs', false), ov('scaling', 'lt.ovScaling', false)];
}

const NAMES = { 1: 'lt.name1', 2: 'lt.name2' } as const;

export function LinearScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	return <ViewportCard lesson={LESSON} id={'LT-' + scene} name={'LT-' + scene + ' · ' + t(locale, NAMES[scene])}
		mode="2d" extent={EXT} rows={rowsFor(scene, locale)} overlays={overlaysFor(scene, locale)} status={statusFor(scene, locale)}
		drawIn={scene === 1}>
		<Floor />
		{scene === 1 ? <Lands /> : <Tests />}
	</ViewportCard>;
}
