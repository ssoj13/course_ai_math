import { BoxGeometry } from 'three';
import { useStore } from 'zustand';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { sub, type V } from '../geom';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { RightAngle } from '../scene/RightAngle';
import { AreaTile } from '../scene/AreaTile';
import { Label } from '../scene/Label';
import { Arrow } from '../scene/Arrow';
import { TileGrid } from '../scene/TileGrid';
import { snap } from '../scene/drag';
import { Show, useOverlay, type Draggable } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './squares-and-plane.rows';
import { derive, fmt1, fmt2, fmt4, fmtInt, LESSON, squareBox } from './squares-and-plane.math';

/**
 * Lesson 0.G scenes (storyboard §7.3-§7.5): 0.G-1 the square of side x, 0.G-2 a point read on two rulers, 0.G-3 the
 * root as the side of a given area. Blender-like flat viewport look (visual-language 6d/6e; user ruling 2026-10-11). Loaded lazily by the entry (squares-and-plane.tsx);
 * shares its store, `derive()` and hot ids.
 */

type Vals = Parameters<typeof derive>[0];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);

/**
 * The reach of a scene: an invisible box over everything the reader can make it draw (the square at x = +-4, the
 * point anywhere in its range). The home view frames the figure's bounds (Stage), so framing the reach keeps the view
 * still while the square grows, shrinks and flips quadrant, instead of fitting whatever the first frame showed.
 */
function Reach({ lo, hi }: { lo: V; hi: V }) {
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast}
		position={[(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, 0]} scale={[hi[0] - lo[0], hi[1] - lo[1], 0.01]} />;
}

/**
 * The measured side: an arrow from the origin to the side's end, drawn just outside the square (below it for a positive
 * side, above it for a mirrored one), with its signed value. It grows from the origin in the draw-in; zero length draws
 * only the value at the origin.
 */
function Side({ to, text, id, role }: { to: number; text: string; id: string; role: 'vecA' | 'vecC' }) {
	const labels = useOverlay('labels');
	const y = to < 0 ? 0.3 : -0.3;
	return <>
		{to !== 0 && <Arrow lesson={LESSON} id={id} from={[0, y]} to={[to, y]} role={role} drag="none" />}
		{labels && <Label lesson={LESSON} id={id} at={[to / 2, y]} offset={[0, to < 0 ? -14 : 14]} role={role}>{text}</Label>}
	</>;
}

/** 0.G-1 side handle: x only, snapped to 0.1; the store's linked resolver derives A in the same write. */
const moveSide: Draggable['move'] = (vals, raw) => ({ set: { x: snap((vals.x as number) + (raw[0] ?? 0)) } });

/** 0.G-1: the square of side x, its signed side measurement with the handle at [x, 0], the origin. */
function Square() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const x = v.x as number;
	const labels = useOverlay('labels');
	const sideText = 'x = ' + fmt1(x);
	return <>
		<Reach lo={[-4.4, -4.4]} hi={[4.4, 4.4]} />
		<Show ov="tiles">
			<TileGrid lesson={LESSON} id="sq" shape={(s) => squareBox(s.x as number)} negative="mirror" role="vecA" count rise
				parts={{ whole: 'whole', partial: 'partial' }} label={labels ? (total) => 'x² = ' + total : false} />
		</Show>
		{/* Negative x points left: the sign the length alone cannot carry. */}
		<Side to={x} text={sideText} id="side" role="vecA" />
		<Point lesson={LESSON} id="o" at={[0, 0]} role="muted" size={6} />
		{labels && <Label lesson={LESSON} id="o" at={[0, 0]} offset={[-12, 12]}>0</Label>}
		<Point lesson={LESSON} id="side" at={(s) => [s.x as number, 0]} keys={['x']} move={moveSide} role="vecA" label={t(locale, 'sq.handleSide')} />
	</>;
}

/** 0.G-2: P, its two dashed legs to the axes (each carrying its reading), the feet, the right angles, hover tiles. */
function Plane() {
	const v = useVals(LESSON);
	const hot = useStore(lessonStore(LESSON), (s) => s.hot);
	const labels = useOverlay('labels');
	const z = derive(v);
	const P: V = [z.px, z.py];
	const fx: V = [z.px, 0];
	const fy: V = [0, z.py];
	// Labels sit on the side away from P so they never cover the legs (P above the x axis -> x label below it).
	const below = z.py >= 0 ? 14 : -14;
	const beside = z.px < 0 ? 30 : -30;
	return <>
		<Reach lo={[-4.8, -4.8]} hi={[4.8, 4.8]} />
		<Show ov="legs">
			{z.py !== 0 && <Segment lesson={LESSON} id="legx" a={P} b={fx} dashed role="axisX" />}
			{z.px !== 0 && <Segment lesson={LESSON} id="legy" a={P} b={fy} dashed role="axisY" />}
			<Point lesson={LESSON} id="footx" at={fx} role="muted" size={4} />
			<Point lesson={LESSON} id="footy" at={fy} role="muted" size={4} />
			<RightAngle lesson={LESSON} id="rtx" at={fx} u={sub(P, fx)} v={[-z.px, 0]} show />
			<RightAngle lesson={LESSON} id="rty" at={fy} u={sub(P, fy)} v={[0, -z.py]} show />
			{labels && <Label lesson={LESSON} id="legx" at={fx} offset={[0, 2 * below]} role="axisX">{'x = ' + fmt1(z.px)}</Label>}
			{labels && <Label lesson={LESSON} id="legy" at={fy} offset={[beside, 0]} role="axisY">{'y = ' + fmt1(z.py)}</Label>}
		</Show>
		{/* Transient squares of the two readings, on the rulers, away from P: x² on [0, x] of the x axis, y² on [0, y]. */}
		{hot.has('px2') && <AreaTile lesson={LESSON} id="px2" a={[0, 0]} b={fx} role="axisX" flip={z.px * z.py < 0} label={(a) => 'x² = ' + fmt2(a)} />}
		{hot.has('py2') && <AreaTile lesson={LESSON} id="py2" a={[0, 0]} b={fy} role="axisY" flip={z.px * z.py > 0} label={(a) => 'y² = ' + fmt2(a)} />}
		<Point lesson={LESSON} id="P" at="P" role="ink" />
		{labels && <Label lesson={LESSON} id="P" at={P} offset={[0, z.py >= 0 ? -16 : 16]}>{'(' + fmt1(z.px) + ', ' + fmt1(z.py) + ')'}</Label>}
	</>;
}

/** 0.G-3: the square driven by A, its side √A measured as an OUTPUT (no handle), the origin. */
function Root() {
	const v = useVals(LESSON);
	const labels = useOverlay('labels');
	const s = Math.sqrt(v.A as number);
	const sideText = '√A = ' + fmt4(s);
	return <>
		<Reach lo={[-0.4, -0.8]} hi={[4.4, 4.4]} />
		<Show ov="tiles">
			<TileGrid lesson={LESSON} id="sq" shape={(w) => squareBox(Math.sqrt(w.A as number))} role="vecC" count
				label={labels ? (total) => 'A = ' + total : false} />
		</Show>
		<Side to={s} text={sideText} id="side" role="vecC" />
		<Point lesson={LESSON} id="o" at={[0, 0]} role="muted" size={6} />
		{labels && <Label lesson={LESSON} id="o" at={[0, 0]} offset={[-12, 12]}>0</Label>}
	</>;
}

/** The grid (Overlays "grid"; its axes under "axes" where the card has them). 0.G-3 has no axes (§7.5). */
function AxesGrid({ extent }: { extent: number }) {
	return <Grid lesson={LESSON} extent={extent} step={1} axes={useOverlay('axes')} />;
}
function SceneGrid({ extent, axes }: { extent: number; axes: boolean }) {
	if (!useOverlay('grid')) return null;
	return axes ? <AxesGrid extent={extent} /> : <Grid lesson={LESSON} extent={extent} step={1} axes={false} />;
}

/** Status bar hints per scene (storyboard status lines); values are read live. */
function statusFor(scene: Scene, locale: Locale): StatusSpec {
	if (scene === 1) {
		return {
			hints: {
				side: { mouse: t(locale, 'sq.hintSideMouse'), touch: t(locale, 'sq.hintSideTouch') },
				sq: (v: Vals) => {
					const z = derive(v);
					return z.partial > 0
						? t(locale, 'sq.hintSquarePartial', { x2: fmt2(z.x2), whole: fmtInt(z.whole), partial: fmt2(z.partial), pieces: fmtInt(z.partialPieces) })
						: t(locale, 'sq.hintSquare', { x2: fmt2(z.x2), whole: fmtInt(z.whole) });
				},
			},
		};
	}
	if (scene === 2) {
		return {
			hints: {
				P: { mouse: t(locale, 'sq.hintPMouse'), touch: t(locale, 'sq.hintPTouch') },
				legx: (v: Vals) => t(locale, 'sq.hintLegX', { px: fmt1(derive(v).px) }),
				legy: (v: Vals) => t(locale, 'sq.hintLegY', { py: fmt1(derive(v).py) }),
			},
			idle: { mouse: t(locale, 'sq.idlePointMouse'), touch: t(locale, 'sq.idlePointTouch') },
		};
	}
	return {
		hints: {
			sq: { mouse: t(locale, 'sq.hintRootSquareMouse'), touch: t(locale, 'sq.hintRootSquareTouch') },
			side: (v: Vals) => t(locale, 'sq.hintRootSide', { sqrtA: fmt4(derive(v).sqrtA) }),
		},
		// A negative side set in 0.G-1 becomes the principal (positive) root the moment A is written here: say so first.
		idle: {
			mouse: (v: Vals) => ((v.x as number) < 0 ? t(locale, 'sq.idleRootNegative', { x: fmt1(v.x as number) }) : t(locale, 'sq.idleRootMouse')),
			touch: (v: Vals) => ((v.x as number) < 0 ? t(locale, 'sq.idleRootNegative', { x: fmt1(v.x as number) }) : t(locale, 'sq.idleRootTouch')),
		},
	};
}

const CARD = { 1: { id: '0.G-1', name: 'sq.nameSquare' }, 2: { id: '0.G-2', name: 'sq.namePoint' }, 3: { id: '0.G-3', name: 'sq.nameRoot' } } as const;

export function SquaresScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	const overlays = scene === 1 ? [ov('grid', 'sq.ovGrid'), ov('tiles', 'sq.ovTiles'), ov('labels', 'sq.ovLabels'), ov('axes', 'sq.ovAxes')]
		: scene === 2 ? [ov('grid', 'sq.ovGrid'), ov('axes', 'sq.ovAxes'), ov('legs', 'sq.ovLegs'), ov('labels', 'sq.ovLabels')]
		: [ov('grid', 'sq.ovGrid'), ov('tiles', 'sq.ovTiles'), ov('labels', 'sq.ovLabels')];
	const extent = scene === 2 ? 5 : 4.5;
	return <ViewportCard lesson={LESSON} id={CARD[scene].id} name={CARD[scene].id + ' · ' + t(locale, CARD[scene].name)} mode="2d"
		extent={extent} center={scene === 3 ? [2, 2] : [0, 0]}
		rows={rowsFor(scene, locale)} overlays={overlays} status={statusFor(scene, locale)}
		drawIn={scene === 1}>
		<SceneGrid extent={extent} axes={scene !== 3} />
		{scene === 1 ? <Square /> : scene === 2 ? <Plane /> : <Root />}
	</ViewportCard>;
}
