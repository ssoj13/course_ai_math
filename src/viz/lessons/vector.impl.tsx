import { BoxGeometry } from 'three';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useVals } from '../store';
import { add, norm, scale, type V } from '../geom';
import { ViewportCard } from '../scene/ViewportCard';
import { Room } from '../scene/Stage';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Label } from '../scene/Label';
import { Show, useDraggable, useOverlay } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './vector.rows';
import {
	arrow2, BOARD2, BOARD5, BOARD6, fmt1, fmt2, fmtV, LESSON, moveBoth, moveT, moveTip2, moveWhole2, RANGE_T, RANGE_V, RANGE_V3, STRIP, translate, trip,
} from './vector.math';

/**
 * Lesson 1.1 scenes: VE-1 the arrow and its list of numbers, VE-2 a vector has no address (free direction versus
 * position), VE-3 translating an object moves its points but not its normals, VE-4 the arrow in 3D, VE-5 / VE-6 the
 * vector between two points in 2D and 3D. Blender-like flat look (task 6d). Loaded lazily by the entry (vector.tsx);
 * shares its store, its model (vector.math.ts) and its hover ids.
 */

type Vals = Parameters<typeof arrow2>[0];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);
const O2: V = [0, 0];
const TIP: readonly [number, number] = [12, -12];

/**
 * 3D: an invisible box over the default figure and its usual range, framed with it (the axes alone are not measured).
 * 2D scenes declare their drag range as a `Room` instead: the home view frames the drawn arrow and only as much of the
 * room as keeps the arrow readable on a phone.
 */
function Reach({ lo, hi }: { lo: V; hi: V }) {
	const c = lo.map((x, i) => (x + hi[i]) / 2);
	const s = lo.map((x, i) => Math.max(hi[i] - x, 0.01));
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast} position={[c[0], c[1], c[2] ?? 0]} scale={[s[0], s[1], s[2] ?? 0.01]} />;
}

/** Grid lines under the "grid" overlay; axes and (2D) tick numbers under "axes". */
function Floor({ extent, three = false }: { extent: number; three?: boolean }) {
	const axes = useOverlay('axes');
	return <Grid lesson={LESSON} extent={extent} step={1} minor={three ? undefined : 0.5} axes={axes} ticks={!three && axes} lines={useOverlay('grid')} />;
}

/**
 * Dashed component legs of the arrow `from` → `to`, each parallel to its axis and as long as its component, starting
 * at the arrow's own start (a free arrow's legs travel with it). 2D: along x, then along y. 3D: along x and z on the
 * start's level, then up along y. Ids `${id}:x|y|z`, the same as the engine's Arrow legs, so table cells and formula
 * terms light exactly one leg. A zero component draws no leg.
 */
function Legs({ id, from, to, labels }: { id: string; from: V; to: V; labels: boolean }) {
	const d = to.map((x, i) => x - from[i]);
	const three = from.length === 3;
	const corners: V[] = three
		? [from, [to[0], from[1], from[2]], [to[0], from[1], to[2]], to]
		: [from, [to[0], from[1]], to];
	const axes = three ? [0, 2, 1] : [0, 1];
	const roles = ['axisX', 'axisY', 'axisZ'] as const;
	const names = ['x', 'y', 'z'];
	const mid = scale(add(from, to), 0.5);
	return <>{axes.map((axis, k) => {
		if (Math.abs(d[axis]) < 1e-9) return null;
		const a = corners[k], b = corners[k + 1];
		const legId = `${id}:${names[axis]}`;
		return <group key={legId}>
			<Segment lesson={LESSON} id={legId} a={a} b={b} dashed role={roles[axis]} />
			{labels && <Label lesson={LESSON} id={legId} at={scale(add(a, b), 0.5)} away={mid} gap={6} role={roles[axis]}>{names[axis] + ' = ' + fmt1(d[axis])}</Label>}
		</group>;
	})}</>;
}

// ---------------------------------------------------------------------------------------------------------------
// VE-1: the arrow and its list.

function ArrowList() {
	const locale = useLocale(), v = useVals(LESSON), labels = useOverlay('labels'), legs = useOverlay('legs');
	const p = v.v as number[];
	const zero = norm(p) < 1e-9;
	return <>
		<Room lo={[-RANGE_V.max, -RANGE_V.max]} hi={[RANGE_V.max, RANGE_V.max]} />
		{legs && <Legs id="v" from={O2} to={p} labels={labels} />}
		<Arrow lesson={LESSON} id="v" to="v" role="vecA" label="v" />
		<Point lesson={LESSON} id="o" at={O2} role="muted" size={6} />
		{labels && <Label lesson={LESSON} id="v" at="v" role="vecA" offset={TIP}>{zero ? t(locale, 've.zeroLabel') : 'v = ' + fmtV(p)}</Label>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// VE-2: a vector has no address.

/** VE-2's two handles on one arrow: the tip (changes the components) and the middle of the shaft (slides it). */
function FreeHandles() {
	const locale = useLocale();
	useDraggable({ id: 'w:tip', owner: 'w', label: t(locale, 'viewport.moveTip', { name: 'v' }), lesson: LESSON, keys: ['w'], at: (v) => arrow2(v).to, move: moveTip2 });
	useDraggable({
		id: 'w:whole', owner: 'w', label: t(locale, 'viewport.move', { name: 'v' }), lesson: LESSON, keys: ['tail'],
		at: (v) => { const a = arrow2(v); return scale(add(a.from, a.to), 0.5); }, move: moveWhole2,
	});
	return null;
}

function NoAddress() {
	const locale = useLocale(), v = useVals(LESSON), labels = useOverlay('labels'), legs = useOverlay('legs'), ghost = useOverlay('ghost');
	const a = arrow2(v);
	const moved = norm(a.from) > 1e-9;
	return <>
		<Room lo={[BOARD2.min, BOARD2.min]} hi={[BOARD2.max, BOARD2.max]} />
		{/* The same vector drawn from the origin: two drawings, one vector. */}
		{ghost && !a.anchor && moved && <>
			<Arrow lesson={LESSON} id="ghost" to={a.w} role="muted" />
			{labels && <Label lesson={LESSON} id="ghost" at={a.w} role="muted" offset={TIP}>{t(locale, 've.ghostLabel')}</Label>}
		</>}
		{legs && <Legs id="w" from={a.from} to={a.to} labels={labels} />}
		<Arrow lesson={LESSON} id="w" from={a.from} to={a.to} role="vecA" drag="none" />
		<FreeHandles />
		<Point lesson={LESSON} id="tail" at={a.from} role="muted" size={6} />
		{a.anchor ? <>
			<Point lesson={LESSON} id="head" at={a.to} role="ink" />
			{labels && <Label lesson={LESSON} id="head" at={a.to} role="ink" offset={TIP}>{'P = ' + fmtV(a.w)}</Label>}
		</> : <>
			{labels && <Label lesson={LESSON} id="w" at={a.to} role="vecA" offset={TIP}>{'v = ' + fmtV(a.w)}</Label>}
			{labels && moved && <Label lesson={LESSON} id="tail" at={a.from} role="muted" offset={[-14, 14]}>{t(locale, 've.startLabel', { p: fmtV(a.from) })}</Label>}
		</>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// VE-3: translation moves points, not directions.

/** One copy of the floor strip: its edges, vertices and per-vertex normals (drawn from each vertex). */
function Strip({ pts, n, role, nRole, ghost }: { pts: readonly V[]; n: V; role: 'ink' | 'muted'; nRole: 'vecB' | 'muted'; ghost: boolean }) {
	const pre = ghost ? 'old' : 'new';
	return <>
		{pts.slice(1).map((p, i) => <Segment key={i} lesson={LESSON} id={pre + 'Floor'} a={pts[i]} b={p} dashed={ghost} role={role} />)}
		{pts.map((p, i) => <Arrow key={'n' + i} lesson={LESSON} id={ghost ? 'N' : 'N2'} from={p} to={add(p, n)} role={nRole} />)}
		{pts.map((p, i) => <Point key={'p' + i} lesson={LESSON} id={pre + 'P' + i} at={p} role={role} size={ghost ? 5 : 7} />)}
	</>;
}

function TranslateScene() {
	const locale = useLocale(), v = useVals(LESSON), before = useOverlay('before');
	const z = translate(v);
	// The translation arrow runs from the first vertex (the strip's free end), so its label has room beside it; the
	// normal labels sit beyond the last vertex's normals, away from the strip.
	const p0 = z.before[0], q0 = z.after[0], p2 = z.before[2], q2 = z.after[2];
	const moved = norm(z.T) > 1e-9;
	return <>
		<Room lo={[STRIP_LO[0] - 0.5, STRIP_LO[1] - 0.5]} hi={[STRIP_HI[0] + RANGE_T.max + 1, STRIP_HI[1] + RANGE_T.max + 1]} />
		{before && moved && <Strip pts={z.before} n={z.N} role="muted" nRole="muted" ghost />}
		<Strip pts={z.after} n={z.N2} role="ink" nRole="vecB" ghost={false} />
		{moved && <Arrow lesson={LESSON} id="T" from={p0} to={q0} role="vecA" drag="none" />}
		<Point lesson={LESSON} id="obj" at={(vals) => add(translate(vals).before[1], translate(vals).T)} keys={['T']} move={moveT} role="ink" label={t(locale, 've.handleObject')} />
		<Show ov="labels">
			{/* At the arrow's tail, pushed back along it: beside the shaft a wide label always crossed the diagonal. */}
			{moved && <Label lesson={LESSON} id="T" at={p0} role="vecA" away={q0} gap={8}>{'T = ' + fmtV(z.T)}</Label>}
			<Label lesson={LESSON} id="N2" at={add(q2, z.N2)} role="vecB" away={q2} gap={4}>{"N' = " + fmtV(z.N2, fmt2)}</Label>
			{before && moved && <Label lesson={LESSON} id="N" at={add(p2, z.N)} role="muted" away={p2} gap={4}>N</Label>}
		</Show>
	</>;
}
/** Bounds of the strip itself (the reach adds the largest translation and the unit normals to them). */
const STRIP_LO: V = [Math.min(...STRIP.map((p) => p[0])), Math.min(...STRIP.map((p) => p[1]))];
const STRIP_HI: V = [Math.max(...STRIP.map((p) => p[0])), Math.max(...STRIP.map((p) => p[1]))];

// ---------------------------------------------------------------------------------------------------------------
// VE-4: the arrow in 3D.

function Arrow3() {
	const locale = useLocale(), v = useVals(LESSON), labels = useOverlay('labels'), legs = useOverlay('legs');
	const p = v.v3 as number[];
	const zero = norm(p) < 1e-9;
	return <>
		{/* A small reach keeps the origin and the default arrow framed (the axes alone are not measured). */}
		<Reach lo={[-2, -1, -2]} hi={[3, 2.5, 2]} />
		{legs && <Legs id="v3" from={[0, 0, 0]} to={p} labels={labels} />}
		<Arrow lesson={LESSON} id="v3" to="v3" role="vecA" label="v" />
		{labels && <Label lesson={LESSON} id="v3" at="v3" role="vecA" offset={TIP}>{zero ? t(locale, 've.zeroLabel') : 'v = ' + fmtV(p)}</Label>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// VE-5 / VE-6: the vector between two points.

function BothHandle({ three }: { three: boolean }) {
	const locale = useLocale();
	const [a, b, board] = three ? ['A3', 'B3', BOARD6] as const : ['A', 'B', BOARD5] as const;
	useDraggable({
		id: 'd:whole', owner: three ? 'd3' : 'd', label: t(locale, 'viewport.move', { name: 'B − A' }), lesson: LESSON, keys: [a, b],
		at: (v) => { const z = trip(v, three); return scale(add(z.A, z.B), 0.5); }, move: moveBoth(a, b, board),
	});
	return null;
}

function Between({ three }: { three: boolean }) {
	const v = useVals(LESSON), labels = useOverlay('labels'), legs = useOverlay('legs'), ghost = useOverlay('ghost');
	const z = trip(v, three);
	const id = three ? 'd3' : 'd';
	const [aKey, bKey] = three ? ['A3', 'B3'] : ['A', 'B'];
	const board = three ? BOARD6 : BOARD5;
	const zero = z.len < 1e-9;
	// Point names are pushed directly away from the other point, so they never cover the arrow between them.
	const away = (p: V, q: V): V => (zero ? add(p, three ? [-1, -1, 0] : [-1, -1]) : q);
	return <>
		{/* 2D: the board is room to drag into. 3D: a box around the default points (the full board cube looked tiny). */}
		{three ? <Reach lo={[-2, -1, -2]} hi={[2.5, 2.5, 2]} /> : <Room lo={[board.min, board.min]} hi={[board.max, board.max]} />}
		{ghost && !zero && <Arrow lesson={LESSON} id="dghost" to={z.d} role="muted" />}
		{legs && !zero && <Legs id={id} from={z.start} to={z.end} labels={labels} />}
		{!zero && <Arrow lesson={LESSON} id={id} from={z.start} to={z.end} role="vecA" drag="none" />}
		<BothHandle three={three} />
		<Point lesson={LESSON} id="A" at={aKey} role="ink" label="A" />
		<Point lesson={LESSON} id="B" at={bKey} role="ink" label="B" />
		<Show ov="labels">
			{/* 3D names stay short: a three-number label pushed outward left the stage on a phone; the panel has them. */}
			<Label lesson={LESSON} id="A" at={aKey} role="ink" away={away(z.A, z.B)} gap={8}>{three ? 'A' : 'A ' + fmtV(z.A)}</Label>
			<Label lesson={LESSON} id="B" at={bKey} role="ink" away={away(z.B, z.A)} gap={8}>{three ? 'B' : 'B ' + fmtV(z.B)}</Label>
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const tip = (name: string) => ({ mouse: t(locale, 've.hintTipMouse', { name }), touch: t(locale, 've.hintTipTouch', { name }) });
	if (scene === 1) return { hints: { 'v:tip': tip('v'), 'v:x': t(locale, 've.hintLegX'), 'v:y': t(locale, 've.hintLegY') }, idle: t(locale, 've.idle1') };
	if (scene === 2) {
		return {
			hints: {
				'w:tip': tip('v'),
				'w:whole': (v: Vals) => t(locale, arrow2(v).anchor ? 've.hintWholeAnchored' : 've.hintWhole'),
				ghost: t(locale, 've.hintGhost'),
			},
			state: (v: Vals) => (arrow2(v).anchor ? t(locale, 've.stateAnchored') : null),
		};
	}
	if (scene === 3) {
		return {
			hints: { 'obj:pt': { mouse: t(locale, 've.hintObjMouse'), touch: t(locale, 've.hintObjTouch') }, N2: t(locale, 've.hintN2'), N: t(locale, 've.hintN') },
			state: (v: Vals) => (translate(v).asPoint ? t(locale, 've.stateAsPoint') : null),
		};
	}
	if (scene === 4) return { hints: { 'v3:tip': { mouse: t(locale, 've.hintTip3Mouse'), touch: t(locale, 've.hintTipTouch', { name: 'v' }) } } };
	const pt = { mouse: t(locale, 've.hintPointMouse'), touch: t(locale, 've.hintPointTouch') };
	return {
		hints: { 'A:pt': pt, 'B:pt': pt, 'd:whole': t(locale, 've.hintBoth'), dghost: t(locale, 've.hintGhost') },
		state: (v: Vals) => (trip(v, scene === 6).len < 1e-9 ? t(locale, 've.stateSame') : null),
	};
}

function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	const base = [ov('grid', 've.ovGrid'), ov('axes', 've.ovAxes'), ov('labels', 've.ovLabels')];
	if (scene === 1 || scene === 4) return [...base, ov('legs', 've.ovLegs')];
	if (scene === 2) return [...base, ov('legs', 've.ovLegs'), ov('ghost', 've.ovGhost')];
	if (scene === 3) return [...base, ov('before', 've.ovBefore')];
	return [...base, ov('legs', 've.ovLegs'), ov('ghost', 've.ovGhost', false)];
}

const NAMES = { 1: 've.name1', 2: 've.name2', 3: 've.name3', 4: 've.name4', 5: 've.name5', 6: 've.name6' } as const;
const EXTENT: Record<Scene, number> = { 1: 4.5, 2: 5.5, 3: 5, 4: 3.5, 5: 4.5, 6: 3 };
/**
 * Grid extent per scene: in 2D the axis letters sit at +extent and the ticks run to ±extent, so it is the upper edge of
 * the scene's board (letters inside the view); in 3D the axes are ±extent long and frame the home view.
 */
const GRID: Record<Scene, number> = { 1: 4, 2: 8, 3: 4, 4: RANGE_V3.max, 5: BOARD5.max, 6: BOARD6.max };
const CENTER: Partial<Record<Scene, [number, number]>> = { 2: [2.5, 2.5], 3: [0.5, 1], 5: [2.5, 2.5] };

export function VectorScene({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	const three = scene === 4 || scene === 6;
	return <ViewportCard lesson={LESSON} id={'VE-' + scene} name={'VE-' + scene + ' · ' + t(locale, NAMES[scene])}
		mode={three ? '3d' : '2d'} extent={EXTENT[scene]} center={CENTER[scene]}
		rows={rowsFor(scene, locale, v.rev as number)} overlays={overlaysFor(scene, locale)} status={statusFor(scene, locale)}
		drawIn={scene === 1}>
		<Floor extent={GRID[scene]} three={three} />
		{scene === 1 ? <ArrowList /> : scene === 2 ? <NoAddress /> : scene === 3 ? <TranslateScene />
			: scene === 4 ? <Arrow3 /> : <Between three={scene === 6} />}
	</ViewportCard>;
}
