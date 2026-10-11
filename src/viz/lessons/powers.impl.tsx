import { useEffect, useRef } from 'react';
import { BoxGeometry } from 'three';
import { useStore } from 'zustand';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { lessonStore, useVal, useVals } from '../store';
import type { V } from '../geom';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Label } from '../scene/Label';
import { Arrow } from '../scene/Arrow';
import { TileGrid } from '../scene/TileGrid';
import { THIN } from '../scene/tiles';
import { FnCurve } from '../scene/FnCurve';
import { snap } from '../scene/drag';
import { useWorldPx } from '../scene/view';
import { Show, useOverlay, type Draggable } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { powLabel, rowsFor, type Scene } from './powers.rows';
import {
	bandMid, cubeBox, derive, fmt2, fmt4, fmtG, fmtAmt, fmtInt, fmtSci, fmtVal, fmtX, LESSON, powerBox, RUNGS, rungBands, rungId, rungShape,
	rungTop, squareBox, sup,
} from './powers.math';

/**
 * Lesson 0.2 scenes (storyboard §7.7-§7.11): 0.2-1 the square and its root field, 0.2-2 the stick -> slab -> cube of
 * x^n, 0.2-3 the ladder of powers 3 .. -3, 0.2-4 area -> side and volume -> edge, 0.2-5 the curves y = t^g.
 * Blender-like flat viewport look (visual-language 6d/6e; user ruling 2026-10-11: no clay, no holo). Loaded lazily by
 * the entry (powers.tsx); shares its store, `derive()` and hot ids.
 */

type Vals = Parameters<typeof derive>[0];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);

/**
 * The reach of a scene: an invisible box over everything the reader can make it draw (the square or cube at its
 * largest, the label lanes of the ladder). The home view frames the figure's bounds, so framing the reach keeps the
 * view still while the shape grows and shrinks.
 */
function Reach({ lo, hi }: { lo: readonly number[]; hi: readonly number[] }) {
	const z0 = lo[2] ?? 0;
	const z1 = hi[2] ?? 0.01;
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast}
		position={[(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (z0 + z1) / 2]} scale={[hi[0] - lo[0], hi[1] - lo[1], Math.max(0.01, z1 - z0)]} />;
}

/** Screen distance of a 2D side measurement from its edge: past the axis tick numbers (11 px out, ~10 px tall). */
const SIDE_PX = 28;

/**
 * A measured side in 2D: an arrow from `a` along +x (`dir` 'x') or +y (`dir` 'y') of length `len`, SIDE_PX outside
 * the edge (below a bottom edge, above a top edge), with its value further out. The offset is in screen pixels, so
 * arrow and label clear the tick numbers at every zoom and card width; zero length draws only the value.
 */
function Measure({ at, len, text, id, role, side }: { at: number; len: number; text: string; id: string; role: 'vecA' | 'vecC'; side: 'below' | 'above' }) {
	const labels = useOverlay('labels');
	const off = useWorldPx(SIDE_PX, [0, 0]) ?? 0.5;
	const y = side === 'below' ? at - off : at + off;
	const dy = side === 'below' ? 13 : -13;
	return <>
		{len > 0 && <Arrow lesson={LESSON} id={id} from={[0, y]} to={[len, y]} role={role} drag="none" />}
		{labels && <Label lesson={LESSON} id={id} at={[len / 2, y]} offset={[0, dy]} role={role}>{text}</Label>}
	</>;
}

/** Side handle (0.2-1, 0.2-2): x only, snapped to 0.1; the linked resolver derives A, V and from in the same write. */
const moveSide: Draggable['move'] = (vals, raw) => ({ set: { x: snap((vals.x as number) + (raw[0] ?? 0)) } });

/** 0.2-1: the square of side x, its side measured below and above (x · x), the handle at [x, 0]. */
function Square() {
	const locale = useLocale();
	const x = useVal(LESSON, 'x', 3);
	const labels = useOverlay('labels');
	return <>
		<Reach lo={[-0.5, -1]} hi={[4.4, 4.6]} />
		<Show ov="tiles">
			<TileGrid lesson={LESSON} id="sq" shape={(s) => squareBox(s.x as number)} role="vecA" count rise
				parts={{ whole: 'whole', partial: 'partial' }} label={labels ? () => 'x² = ' + fmtAmt(x * x) : false} />
		</Show>
		<Measure at={0} len={x} text={'x = ' + fmtX(x)} id="side" role="vecA" side="below" />
		{/* The second factor along the top edge: the reader sees x · x, not x alone. */}
		<Measure at={x} len={x} text={'x = ' + fmtX(x)} id="top" role="vecA" side="above" />
		<Point lesson={LESSON} id="o" at={[0, 0]} role="muted" size={6} />
		<Point lesson={LESSON} id="side" at={(s) => [s.x as number, 0]} keys={['x']} move={moveSide} role="vecA" label={t(locale, 'pw.handleSide')} />
	</>;
}

/** Distance of the 3D edge measurements from the body, in world units. */
const EDGE_OFF = 0.5;

/**
 * 0.2-2: x^n on the floor. n = 1 a stick, 2 a slab, 3 a cube; one TileGrid morphs piece by piece between them. One
 * measured edge per active axis (x along the front, z along the left, y up the front right corner), the handle at
 * [x, 0, 0].
 */
function Power() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const x = v.x as number;
	const n = v.n as number;
	const labels = useOverlay('labels');
	const edges = useOverlay('edges');
	const text = 'x = ' + fmtX(x);
	// The home camera looks from +x +y +z: the faces z = max and x = max face it, so the measured edges run along
	// their bottom edges, just outside the body (a stick or a slab is THIN deep along z, a cube x deep).
	const zf = frontZ(n, x) + EDGE_OFF;
	const xf = x + EDGE_OFF;
	return <>
		<Reach lo={[-0.6, 0, -0.1]} hi={[4.6, 4, 4.6]} />
		<TileGrid lesson={LESSON} id="obj" shape={(s) => powerBox(s.n as number, s.x as number)} role="vecA" count morph
			parts={{ whole: 'wholeN', partial: 'partialN' }} label={labels ? () => powLabel(n) + ' = ' + fmtAmt(x ** n) : false} />
		{edges && x > 0 && <>
			<Arrow lesson={LESSON} id="side" from={[0, 0, zf]} to={[x, 0, zf]} role="vecA" drag="none" />
			{labels && <Label lesson={LESSON} id="side" at={[x / 2, 0, zf]} offset={[-10, 14]} role="vecA">{text}</Label>}
			{n >= 2 && <Arrow lesson={LESSON} id="edgez" from={[xf, 0, 0]} to={[xf, 0, x]} role="vecA" drag="none" />}
			{n >= 2 && labels && <Label lesson={LESSON} id="edgez" at={[xf, 0, x / 2]} offset={[14, 14]} role="vecA">{text}</Label>}
			{/* Up the left silhouette corner (0, y, x), just outside it. */}
			{n === 3 && <Arrow lesson={LESSON} id="edgey" from={[-EDGE_OFF, 0, x]} to={[-EDGE_OFF, x, x]} role="vecA" drag="none" />}
			{n === 3 && labels && <Label lesson={LESSON} id="edgey" at={[-EDGE_OFF, x / 2, x]} offset={[-32, 0]} role="vecA">{text}</Label>}
		</>}
		{/* The handle sits at the far end of the measured x edge (v3 measurement arrow with its handle). */}
		<Point lesson={LESSON} id="side" at={(s) => [s.x as number, 0, frontZ(s.n as number, s.x as number) + EDGE_OFF]} keys={['x']} move={moveSide}
			role="vecA" label={t(locale, 'pw.handleSide')} />
	</>;
}

/** Depth (z) of the front face of the x^n body: a cube or slab is x deep, a stick one thin layer. */
const frontZ = (n: number, x: number): number => (n >= 2 ? x : THIN);

/** x of the arrow lane right of the ladder's tiles (8 pieces per band), and the gap a label keeps from its anchor. */
const LANE_X = 8.7;
const GAP_PX = 6;

/**
 * 0.2-3: the ladder. Seven fixed rung slots from 3 (top) to -3; rung j holds b^j unit pieces (8 per band), or for
 * j <= 0 one unit outline filled to b^j. Rungs above the current k are reached (muted), rung k is the guide's focus,
 * rungs below are empty slots whose values a later step reveals. Labels "b^j = value" stand left of the tiles,
 * right-aligned on the lane x = -0.3; arrows "÷ b" stand in a lane right of the tiles.
 */
function Ladder() {
	const v = useVals(LESSON);
	const base = v.base as number;
	const k = v.k as number;
	const labels = useOverlay('labels');
	const arrows = useOverlay('arrows');
	const b = fmtVal(base);
	return <>
		<Reach lo={[-5.5, -7.2]} hi={[10.5, 7.2]} />
		{RUNGS.map((j) => {
			const id = rungId(j);
			const y = bandMid(j, 0);
			return <group key={j}>
				{/* Shape and fill are functions of the drawn values (laid out in the frame that sees them); the colour of a
				    reached rung is a prop, so the rung is remounted when it becomes reached rather than repainted a frame late. */}
				<TileGrid key={j > k ? 'reached' : 'live'} lesson={LESSON} id={id} origin={[0, rungTop(j) - 1]} morph
					shape={(s) => rungShape(s.base as number, j, s.k as number).shape}
					fill={(s) => rungShape(s.base as number, j, s.k as number).fill}
					role={j > k ? 'muted' : 'vecA'} partialRole={j > k ? 'muted' : 'light'} />
				{labels && <Label lesson={LESSON} id={id} at={[-0.3, y]} away={[1, y]} gap={GAP_PX}>
					{b + sup(j) + ' = ' + (j >= k ? fmtVal(base ** j) : '?')}
				</Label>}
			</group>;
		})}
		{arrows && RUNGS.slice(0, -1).map((j) => {
			if (j - 1 < k) return null;
			const y0 = bandMid(j, rungBands(base, j) - 1);
			const y1 = bandMid(j - 1, 0);
			const mid = (y0 + y1) / 2;
			return <group key={'d' + j}>
				<Arrow lesson={LESSON} id="div" from={[LANE_X, y0]} to={[LANE_X, y1]} role="muted" drag="none" />
				{labels && <Label lesson={LESSON} id="div" at={[LANE_X + 0.2, mid]} away={[LANE_X - 1, mid]} gap={GAP_PX} role="muted">{'÷ ' + b}</Label>}
			</group>;
		})}
	</>;
}

/** 0.2-4, 2D: the square of area A; its side A^½ is an output (no handle). */
function RootSquare() {
	const A = useVal(LESSON, 'A', 9);
	const labels = useOverlay('labels');
	const s = Math.sqrt(A);
	return <>
		<Reach lo={[-0.5, -1]} hi={[4.4, 4.4]} />
		<Show ov="tiles">
			<TileGrid lesson={LESSON} id="root" shape={(w) => squareBox(Math.sqrt(w.A as number))} role="vecC" count
				label={labels ? () => 'A = ' + fmtAmt(A) : false} />
		</Show>
		<Measure at={0} len={s} text={'A^½ = ' + fmt4(s)} id="rootside" role="vecC" side="below" />
		<Point lesson={LESSON} id="o" at={[0, 0]} role="muted" size={6} />
	</>;
}

/** 0.2-4, 3D: the cube of volume V; its edge V^⅓ is an output (no handle). */
function RootCube() {
	const V = useVal(LESSON, 'V', 27);
	const labels = useOverlay('labels');
	const s = Math.cbrt(V);
	return <>
		<Reach lo={[-0.6, 0, -0.1]} hi={[4.6, 4, 4.6]} />
		<Show ov="tiles">
			<TileGrid lesson={LESSON} id="root" shape={(w) => cubeBox(Math.cbrt(w.V as number))} role="vecC" count
				label={labels ? () => 'V = ' + fmtAmt(V) : false} />
		</Show>
		{/* Along the front bottom edge (the camera looks from +x +y +z), as in 0.2-2. */}
		{s > 0 && <Arrow lesson={LESSON} id="rootside" from={[0, 0, s + EDGE_OFF]} to={[s, 0, s + EDGE_OFF]} role="vecC" drag="none" />}
		{labels && <Label lesson={LESSON} id="rootside" at={[s / 2, 0, s + EDGE_OFF]} offset={[-10, 16]} role="vecC">{'V^⅓ = ' + fmt4(s)}</Label>}
	</>;
}

/** 0.2-5 point drag: t along x only, snapped to 0.01; the card clamps it to [0, 1]. */
const moveT: Draggable['move'] = (vals, raw) => ({ set: { t: snap((vals.t as number) + (raw[0] ?? 0), 0.01) } });

const powT = (t: number, v: Vals) => t ** (v.g as number);
const powInv = (t: number, v: Vals) => t ** (1 / (v.g as number));

/** Samples of a curve: t^32 bends within the last tenth of [0, 1] and t^0.2 within the first, so the grid is dense. */
const SAMPLES = 257;

/** 0.2-5: y = t^g, its inverse y = t^(1/g), the diagonal g = 1, the point (t, t^g) with dashed drops to both axes. */
function Curves() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const hot = useStore(lessonStore(LESSON), (s) => s.hot);
	const labels = useOverlay('labels');
	const tt = v.t as number;
	const g = v.g as number;
	const y = tt ** g;
	return <>
		<Reach lo={[-0.05, -0.05]} hi={[1.05, 1.05]} />
		<Show ov="diagonal">
			<Segment lesson={LESSON} id="diag" a={[0, 0]} b={[1, 1]} dashed role="muted" />
		</Show>
		<Show ov="inverse">
			<FnCurve lesson={LESSON} id="inv" f={powInv} from={0} to={1} samples={SAMPLES} role="vecB" width={1.5} halo={false} />
		</Show>
		<FnCurve lesson={LESSON} id="curve" f={powT} from={0} to={1} samples={SAMPLES} role="vecA" />
		<Show ov="drops">
			{y > 1e-6 && tt > 0 && <Segment lesson={LESSON} id="y" a={[tt, y]} b={[tt, 0]} dashed role="axisX" />}
			{y > 1e-6 && tt > 0 && <Segment lesson={LESSON} id="y" a={[tt, y]} b={[0, y]} dashed role="axisY" />}
		</Show>
		{/* The mirror of (t, t^g) across the diagonal lies on the inverse curve: shown while the read-back is hovered. */}
		{hot.has('yinv') && <Point lesson={LESSON} id="yinv" at={[y, tt]} role="vecB" size={7} />}
		<Point lesson={LESSON} id="pt" at={(s) => [s.t as number, (s.t as number) ** (s.g as number)]} keys={['t']} move={moveT} role="vecA"
			label={t(locale, 'pw.handlePoint')} />
		{/*
		  * On the free side of the point. g < 1: up and to the left (the curve is lower there; the horizontal drop runs
		  * left at exactly the point's height). g >= 1: down and to the right, between the rising curve and the t axis;
		  * when the point lies on the axis itself (t^32 at t = 0.5), straight up instead, above the flat curve.
		  */}
		{labels && <Label lesson={LESSON} id="y" at={[tt, y]} away={g < 1 ? [tt + 0.2, y - 0.3] : y < 0.1 ? [tt, y - 0.3] : [tt - 0.4, y + 0.15]}
			gap={12} role="vecA">
			{'tᵍ = ' + fmtSci(y)}
		</Label>}
	</>;
}

/** Grid lines under the "grid" overlay, axes under "axes" (cards without an axes flag have none). */
function SceneGrid({ extent, step, minor, axes }: { extent: number; step: number; minor?: number; axes: boolean }) {
	const lines = useOverlay('grid');
	return axes ? <AxesGrid extent={extent} step={step} minor={minor} lines={lines} />
		: <Grid lesson={LESSON} extent={extent} step={step} minor={minor} axes={false} ticks={false} lines={lines} />;
}
function AxesGrid({ extent, step, minor, lines }: { extent: number; step: number; minor?: number; lines: boolean }) {
	return <Grid lesson={LESSON} extent={extent} step={step} minor={minor} lines={lines} axes={useOverlay('axes')} />;
}

/** Status bar hints per scene (storyboard status lines); values are read live. */
function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const side = { mouse: t(locale, 'pw.hintSideMouse'), touch: t(locale, 'pw.hintSideTouch') };
	if (scene === 1) {
		return {
			hints: {
				side,
				sq: (v: Vals) => {
					const z = derive(v);
					return z.partial > 0
						? t(locale, 'pw.hintSquarePartial', { x2: fmtAmt(z.x2), whole: fmtInt(z.whole), partial: fmtAmt(z.partial) })
						: t(locale, 'pw.hintSquare', { x2: fmtAmt(z.x2), whole: fmtInt(z.whole) });
				},
			},
			idle: { mouse: t(locale, 'pw.idleSquareMouse'), touch: t(locale, 'pw.idleSquareTouch') },
		};
	}
	if (scene === 2) {
		return {
			hints: {
				side: { mouse: t(locale, 'pw.hintAlongXMouse'), touch: t(locale, 'pw.hintAlongXTouch') },
				obj: (v: Vals) => {
					const z = derive(v);
					return z.n === 3
						? t(locale, 'pw.hintCube', { xn: fmtAmt(z.xn), layers: fmtInt(z.layers), slab: fmtInt(z.layers * z.layers) })
						: t(locale, 'pw.hintPower', { pow: powLabel(z.n), xn: fmtAmt(z.xn), whole: fmtInt(z.wholeN) });
				},
			},
			idle: { mouse: t(locale, 'pw.idleCubeMouse'), touch: t(locale, 'pw.idleCubeTouch') },
		};
	}
	if (scene === 3) {
		const rung = (j: number) => (v: Vals) => {
			const z = derive(v);
			if (j < z.k) return t(locale, 'pw.hintRungHidden', { b: fmtVal(z.base), j: sup(j) });
			const value = z.base ** j;
			return j > 0
				? t(locale, 'pw.hintRung', { b: fmtVal(z.base), j: sup(j), value: fmtVal(value), pieces: fmtInt(Math.ceil(value)) })
				: t(locale, 'pw.hintRungFill', { b: fmtVal(z.base), j: sup(j), value: fmtVal(value) });
		};
		return {
			hints: {
				...Object.fromEntries(RUNGS.map((j) => [rungId(j), rung(j)])),
				div: (v: Vals) => t(locale, 'pw.hintDiv', { b: fmtVal(derive(v).base) }),
			},
			idle: t(locale, 'pw.idleLadder'),
		};
	}
	if (scene === 4) {
		return {
			hints: {
				root: (v: Vals) => (v.d3 === 1 ? t(locale, 'pw.hintRootCube') : t(locale, 'pw.hintRootSquare')),
				rootside: t(locale, 'pw.hintRootSide'),
			},
			idle: t(locale, 'pw.idleRoot'),
		};
	}
	return {
		hints: {
			curve: (v: Vals) => {
				const g = v.g as number;
				const key = g > 1 ? 'pw.hintCurveBelow' : g < 1 ? 'pw.hintCurveAbove' : 'pw.hintCurveDiag';
				return t(locale, key, { g: fmtG(g) });
			},
			pt: { mouse: t(locale, 'pw.hintPtMouse'), touch: t(locale, 'pw.hintPtTouch') },
			y: (v: Vals) => t(locale, 'pw.hintY', { t: fmt2(v.t as number), g: fmtG(v.g as number), y: fmtSci(derive(v).y) }),
			inv: t(locale, 'pw.hintInv'),
		},
		idle: t(locale, 'pw.idleCurves'),
	};
}

const CARD = {
	1: { id: '0.2-1', name: 'pw.nameSquare' }, 2: { id: '0.2-2', name: 'pw.nameCube' }, 3: { id: '0.2-3', name: 'pw.nameLadder' },
	4: { id: '0.2-4', name: 'pw.nameRoot' }, 5: { id: '0.2-5', name: 'pw.namePow' },
} as const;

type OvKey = 'pw.ovGrid' | 'pw.ovTiles' | 'pw.ovLabels' | 'pw.ovAxes' | 'pw.ovEdges' | 'pw.ovArrows' | 'pw.ovDiagonal' | 'pw.ovInverse' | 'pw.ovDrops';
const OVERLAYS: Record<Scene, readonly [string, OvKey][]> = {
	1: [['grid', 'pw.ovGrid'], ['tiles', 'pw.ovTiles'], ['labels', 'pw.ovLabels'], ['axes', 'pw.ovAxes']],
	2: [['grid', 'pw.ovGrid'], ['axes', 'pw.ovAxes'], ['edges', 'pw.ovEdges'], ['labels', 'pw.ovLabels']],
	3: [['grid', 'pw.ovGrid'], ['labels', 'pw.ovLabels'], ['arrows', 'pw.ovArrows']],
	4: [['grid', 'pw.ovGrid'], ['tiles', 'pw.ovTiles'], ['labels', 'pw.ovLabels']],
	5: [['grid', 'pw.ovGrid'], ['diagonal', 'pw.ovDiagonal'], ['inverse', 'pw.ovInverse'], ['drops', 'pw.ovDrops'], ['labels', 'pw.ovLabels'], ['axes', 'pw.ovAxes']],
};

/** Shared card frame of one scene: header name, rows, overlays, status from the scene number. */
function Card({ scene, mode, extent, center, drawIn, children }: {
	scene: Scene; mode: '2d' | '3d'; extent: number; center?: V; drawIn?: boolean; children: React.ReactNode;
}) {
	const locale = useLocale();
	const card = CARD[scene];
	return <ViewportCard lesson={LESSON} id={card.id} name={card.id + ' · ' + t(locale, card.name)} mode={mode}
		extent={extent} center={center} rows={rowsFor(scene, locale)} status={statusFor(scene, locale)} drawIn={drawIn}
		overlays={OVERLAYS[scene].map(([id, key]) => ({ id, label: t(locale, key), on: true }))}>
		{children}
	</ViewportCard>;
}

/**
 * 0.2-4 shows the square in 2D or the cube in 3D on one card: the `3D` row writes `d3`, and the card is remounted in
 * the other mode (a card's mode is fixed for its lifetime). The values live in the store, so the reader's side
 * survives the switch (A = 6.25 -> x = 2.5 -> V = 15.625). Clicking the checkbox focused it in the old card; focus
 * moves to the same checkbox of the new card, so keyboard users keep their place.
 */
function RootCard() {
	const d3 = useVal<number>(LESSON, 'd3', 0);
	const box = useRef<HTMLDivElement>(null);
	const first = useRef(true);
	useEffect(() => {
		if (first.current) {
			first.current = false;
			return;
		}
		const lost = !document.activeElement || document.activeElement === document.body;
		if (lost) box.current?.querySelector<HTMLInputElement>('input[type="checkbox"]')?.focus();
	}, [d3]);
	return <div ref={box} style={{ display: 'contents' }}>
		{d3 === 1
			? <Card key="3d" scene={4} mode="3d" extent={4.5}><SceneGrid extent={4.5} step={1} axes={false} /><RootCube /></Card>
			: <Card key="2d" scene={4} mode="2d" extent={4.5} center={[2, 2]}><SceneGrid extent={4.5} step={1} axes={false} /><RootSquare /></Card>}
	</div>;
}

export function PowersScene({ scene }: { scene: Scene }) {
	if (scene === 1) return <Card scene={1} mode="2d" extent={4.5} center={[2, 2]} drawIn><SceneGrid extent={4.5} step={1} axes /><Square /></Card>;
	if (scene === 2) return <Card scene={2} mode="3d" extent={4.5}><SceneGrid extent={4.5} step={1} axes /><Power /></Card>;
	if (scene === 3) return <Card scene={3} mode="2d" extent={7.7} center={[2.5, 0]}><SceneGrid extent={11} step={1} axes={false} /><Ladder /></Card>;
	if (scene === 4) return <RootCard />;
	// The grid extent is its tick range around the origin: 1.2 so that the t = 1 tick is numbered.
	return <Card scene={5} mode="2d" extent={0.6} center={[0.5, 0.5]}>
		<SceneGrid extent={1.2} step={0.5} minor={0.1} axes />
		<Curves />
	</Card>;
}
