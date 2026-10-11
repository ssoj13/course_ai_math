import { useLayoutEffect, useMemo } from 'react';
import { BoxGeometry, Color, DoubleSide, FrontSide } from 'three';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useHot, useVals } from '../store';
import { add, norm, scale, type V } from '../geom';
import { usePalette } from '../palette';
import { useDynGeo, writeGeo } from '../scene/view';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Parallelogram } from '../scene/Parallelogram';
import { AngleArc } from '../scene/AngleArc';
import { RightAngle } from '../scene/RightAngle';
import { Label } from '../scene/Label';
import { HOVER_RANK, Show, useHoverable, useOverlay } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './cross-product.rows';
import { LESSON, LIGHT, RANGE2, arcNormal, derive, derive2, fmt, fmtAngle, fmtVec, tri } from './cross-product.math';

/**
 * Lesson 1.8 scenes: CP-1 the cross product in 3D (normal, area, order), CP-2 the 2D signed area (orientation), CP-3 a
 * triangle's face normal, winding order and back-face culling. Loaded lazily by the entry (cross-product.tsx); shares
 * its store and its one model (cross-product.math.ts). Scene code reads `shown` values, the DOM reads `vals`.
 */

type Vals = Parameters<typeof derive>[0];

/** Home framing: CP-1 leaves room for a result as long as |a||b| (the 3D home distance is capped by the extent). */
const EXTENT: Record<Scene, number> = { 1: 4, 2: 4.5, 3: 3 };
const O3: V = [0, 0, 0];
const O2: V = [0, 0];
const TIP: readonly [number, number] = [10, -12];
/** Length of the CP-3 normal and light arrows (unit directions drawn longer, so their heads read at any zoom). */
const DIR_LEN = 1.2;

const mid = (p: V, q: V) => scale(add(p, q), 0.5);

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);

/**
 * CP-2's reach: an invisible square over the whole input range. The home view frames the figure's bounds, so framing
 * the reach keeps every tip the reader can type or drag in view (b dragged below the x axis left the first framing).
 */
function Reach({ r }: { r: number }) {
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast} scale={[2 * r, 2 * r, 0.01]} />;
}

/**
 * CP-1's reach: an invisible box around the inputs and BOTH orders of the result (±c). The draw-in grows arrows from
 * their tails, so when the home view is measured the result's head is still at the origin; and ticking b × a turns
 * the result over without a refit. Framing ±c keeps the result in view either way.
 */
function Reach3({ pts }: { pts: readonly V[] }) {
	const lo = [0, 1, 2].map((i) => Math.min(0, ...pts.map((p) => p[i])));
	const hi = [0, 1, 2].map((i) => Math.max(0, ...pts.map((p) => p[i])));
	const size = hi.map((h, i) => Math.max(h - lo[i], 0.01));
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast} position={lo.map((l, i) => l + size[i] / 2) as [number, number, number]} scale={size as [number, number, number]} />;
}

// ---------------------------------------------------------------------------------------------------------------
// CP-1: a × b in 3D.

function Cross3() {
	const locale = useLocale(), v = useVals(LESSON), z = derive(v), labels = useOverlay('labels'), legs = useOverlay('legs');
	const name = z.swap ? 'b × a' : 'a × b';
	// The area label sits at the parallelogram's centre, clear of the origin where the arc, the right-angle marks and the
	// three arrow tails crowd together.
	const centre = scale(add(z.a, z.b), 0.5);
	return <>
		<Reach3 pts={[z.a, z.b, z.c, scale(z.c, -1)]} />
		<Show ov="area">
			<Parallelogram lesson={LESSON} id="par" a="a" b="b" role="vecC" />
			{labels && !z.parallel && <Label lesson={LESSON} id="par" at={centre} role="vecC">{t(locale, 'cp.areaLabel', { area: fmt(z.len) })}</Label>}
		</Show>
		<Arrow lesson={LESSON} id="a" to="a" role="vecA" label="a" />
		<Arrow lesson={LESSON} id="b" to="b" role="vecB" label="b" />
		{/* The result is an output: no handle, it follows a and b (and the order). */}
		<Arrow lesson={LESSON} id="c" to={z.c} role="vecC" drag="none" legs={legs} />
		<Show ov="right">{!z.parallel && <>
			<RightAngle lesson={LESSON} id="perpA" at={O3} u={z.c} v="a" show />
			<RightAngle lesson={LESSON} id="perpB" at={O3} u={z.c} v="b" show />
		</>}</Show>
		<Show ov="angle"><AngleArc lesson={LESSON} id="th" at={O3} from="a" to="b" r={0.5} normal={arcNormal(z.a, z.c)}
			label={labels ? (d) => 'θ = ' + fmtAngle(d) + '°' : undefined} /></Show>
		<Show ov="labels">
			<Label lesson={LESSON} id="a" at="a" role="vecA" offset={TIP}>a</Label>
			<Label lesson={LESSON} id="b" at="b" role="vecB" offset={TIP}>b</Label>
			{z.parallel
				? <Label lesson={LESSON} id="c" at={O3} role="hot" offset={[14, 18]}>{name + ' = (0, 0, 0)'}</Label>
				: <Label lesson={LESSON} id="c" at={z.c} role="vecC" offset={TIP}>{name}</Label>}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// CP-2: the 2D signed area.

/**
 * The turn from a to b: a short arrow tangent to the angle arc at b's side, pointing the way the shorter turn goes
 * (counter-clockwise when the signed area is positive), and the word for it next to the arc.
 */
function Turn({ a, b, sa, word }: { a: V; b: V; sa: number; word: string }) {
	const r = 0.9;
	const nb = norm(b);
	const bh = scale(b, 1 / nb);
	const end = scale(bh, r);
	// Tangent of the circle at `end` in the direction of travel: b̂ turned a quarter counter-clockwise (or clockwise).
	const tan = sa > 0 ? [-bh[1], bh[0]] : [bh[1], -bh[0]];
	const start = add(end, scale(tan, -0.45));
	const bis = add(scale(a, 1 / norm(a)), bh);
	const at = norm(bis) > 1e-9 ? scale(bis, (r + 0.15) / norm(bis)) : scale([-bh[1], bh[0]], r);
	return <>
		<Arrow lesson={LESSON} id="turn" from={start} to={end} role="ink" drag="none" />
		<Show ov="labels"><Label lesson={LESSON} id="turn" at={at} away={O2} gap={6} role="ink">{word}</Label></Show>
	</>;
}

function Signed2() {
	const locale = useLocale(), v = useVals(LESSON), z = derive2(v);
	const signed = (x: number) => (x > 0 ? '+' : '') + fmt(x);
	return <>
		<Reach r={RANGE2.max} />
		<Show ov="area">
			<Parallelogram lesson={LESSON} id="par2" a="a2" b="b2" role="vecC" label={useOverlay('labels') ? (area) => signed(area) : undefined} />
		</Show>
		<Arrow lesson={LESSON} id="a2" to="a2" role="vecA" label="a" />
		<Arrow lesson={LESSON} id="b2" to="b2" role="vecB" label="b" />
		<Show ov="turn">
			{z.orient !== 'line' && <>
				<AngleArc lesson={LESSON} id="th2" at={O2} from="a2" to="b2" r={0.9} />
				<Turn a={z.a} b={z.b} sa={z.sa} word={t(locale, z.orient === 'ccw' ? 'cp.ccw' : 'cp.cw')} />
			</>}
		</Show>
		<Show ov="labels">
			<Label lesson={LESSON} id="a2" at="a2" role="vecA" offset={TIP}>a</Label>
			<Label lesson={LESSON} id="b2" at="b2" role="vecB" offset={TIP}>b</Label>
			{z.orient === 'line' && z.na > 0 && z.nb > 0 && <Label lesson={LESSON} id="sa" at={mid(z.a, z.b)} role="hot" offset={[0, 18]}>{t(locale, 'cp.onLine')}</Label>}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// CP-3: triangle normal, winding and culling.

/**
 * The triangle's face as the GPU sees it: three vertices in the LISTED order, flat-lit by the fixed light with
 * max(0, N·L) (an unlit face is black). With culling on, the material is FrontSide, so three.js (like any rasterizer)
 * drops the face whenever its listed order runs clockwise on screen, which is exactly when its normal points away
 * from the camera. With culling off (DoubleSide) the face is always drawn, lit by its own normal, never flipped.
 */
function Face() {
	const pal = usePalette(), v = useVals(LESSON), z = tri(v), hot = useHot(LESSON, 'face');
	const hover = useHoverable(LESSON, 'face', HOVER_RANK.fill);
	const geo = useDynGeo(3, [0, 1, 2]);
	useLayoutEffect(() => writeGeo(geo, z.P), [geo, z.P]);
	const color = useMemo(() => new Color(pal.ink).multiplyScalar(z.N ? Math.max(0, z.ndotl) : 0), [pal.ink, z.N, z.ndotl]);
	if (!z.N) return null;
	return <group {...hover}>
		<mesh geometry={geo} frustumCulled={false} renderOrder={0}>
			<meshBasicMaterial color={hot ? color.clone().lerp(new Color(pal.hot), 0.25) : color} side={z.cull ? FrontSide : DoubleSide}
				polygonOffset polygonOffsetFactor={1} polygonOffsetUnits={1} />
		</mesh>
	</group>;
}

/** Subscripts of the listed order: P₀, P₁, P₂. */
const SUB = ['₀', '₁', '₂'];

function Triangle() {
	const locale = useLocale(), v = useVals(LESSON), z = tri(v);
	// Position of each named vertex in the listed order: A is always first; B and C trade places when reversed.
	const order = { tA: 0, tB: z.rev ? 2 : 1, tC: z.rev ? 1 : 2 } as const;
	const names = { tA: 'A', tB: 'B', tC: 'C' } as const;
	const tipN = z.N ? add(z.centroid, scale(z.N, DIR_LEN)) : null;
	const tipL = add(z.centroid, scale(LIGHT, DIR_LEN));
	return <>
		<Face />
		<Segment lesson={LESSON} id="face" a="tA" b="tB" role="muted" />
		<Segment lesson={LESSON} id="face" a="tB" b="tC" role="muted" />
		<Segment lesson={LESSON} id="face" a="tC" b="tA" role="muted" />
		<Show ov="edges">
			<Arrow lesson={LESSON} id="e1" from={z.P[0]} to={z.P[1]} role="vecA" drag="none" />
			<Arrow lesson={LESSON} id="e2" from={z.P[0]} to={z.P[2]} role="vecB" drag="none" />
			<Show ov="labels">
				<Label lesson={LESSON} id="e1" at={mid(z.P[0], z.P[1])} away={z.centroid} gap={4} role="vecA">e₁</Label>
				<Label lesson={LESSON} id="e2" at={mid(z.P[0], z.P[2])} away={z.centroid} gap={4} role="vecB">e₂</Label>
			</Show>
		</Show>
		<Show ov="normal">{tipN && <>
			<Arrow lesson={LESSON} id="N" from={z.centroid} to={tipN} role="vecC" drag="none" />
			<Show ov="labels"><Label lesson={LESSON} id="N" at={tipN} role="vecC" offset={TIP}>N</Label></Show>
		</>}</Show>
		<Show ov="light">
			<Arrow lesson={LESSON} id="L" from={z.centroid} to={tipL} role="light" drag="none" />
			<Show ov="labels"><Label lesson={LESSON} id="L" at={tipL} role="light" offset={TIP}>L</Label></Show>
		</Show>
		{(['tA', 'tB', 'tC'] as const).map((k) => <Point key={k} lesson={LESSON} id={k} at={k} role="ink" label={names[k]} />)}
		<Show ov="labels">
			{(['tA', 'tB', 'tC'] as const).map((k) => <Label key={k} lesson={LESSON} id={k} at={k} away={z.centroid} gap={6} role="ink">{names[k] + ' = P' + SUB[order[k]]}</Label>)}
			{!z.N && <Label lesson={LESSON} id="face" at={z.centroid} role="hot" offset={[0, 18]}>{t(locale, 'cp.noNormal')}</Label>}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const tip = (name: string) => ({ mouse: t(locale, 'cp.hintTipMouse', { name }), touch: t(locale, 'cp.hintTipTouch', { name }) });
	if (scene === 1) {
		return {
			hints: {
				'a:tip': tip('a'), 'b:tip': tip('b'),
				c: (v: Vals) => { const z = derive(v); return t(locale, 'cp.hintC', { name: z.swap ? 'b × a' : 'a × b', c: fmtVec(z.c), len: fmt(z.len) }); },
				par: (v: Vals) => t(locale, 'cp.hintArea', { area: fmt(derive(v).area) }),
				perpA: t(locale, 'cp.hintPerp'), perpB: t(locale, 'cp.hintPerp'),
			},
			idle: { mouse: t(locale, 'cp.idle1Mouse'), touch: t(locale, 'cp.idle1Touch') },
		};
	}
	if (scene === 2) {
		return {
			hints: {
				'a2:tip': tip('a'), 'b2:tip': tip('b'),
				par2: (v: Vals) => t(locale, 'cp.hintSigned', { sa: fmt(derive2(v).sa) }),
				turn: t(locale, 'cp.hintTurn'),
			},
		};
	}
	return {
		hints: {
			'tA:pt': tip('A'), 'tB:pt': tip('B'), 'tC:pt': tip('C'),
			face: (v: Vals) => { const z = tri(v); return z.N ? t(locale, 'cp.hintFace', { ndotl: fmt(z.ndotl), bright: fmt(z.bright) }) : t(locale, 'cp.noNormal'); },
			N: t(locale, 'cp.hintN'), L: t(locale, 'cp.hintL'),
		},
		idle: { mouse: t(locale, 'cp.idle3Mouse'), touch: t(locale, 'cp.idle3Touch') },
	};
}

function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	if (scene === 1) return [ov('grid', 'cp.ovGrid'), ov('axes', 'cp.ovAxes'), ov('labels', 'cp.ovLabels'), ov('area', 'cp.ovArea'), ov('right', 'cp.ovRight'), ov('angle', 'cp.ovAngle', false), ov('legs', 'cp.ovLegs', false)];
	if (scene === 2) return [ov('grid', 'cp.ovGrid'), ov('axes', 'cp.ovAxes'), ov('labels', 'cp.ovLabels'), ov('area', 'cp.ovArea'), ov('turn', 'cp.ovTurn')];
	return [ov('grid', 'cp.ovGrid'), ov('axes', 'cp.ovAxes'), ov('labels', 'cp.ovLabels'), ov('edges', 'cp.ovEdges'), ov('normal', 'cp.ovNormal'), ov('light', 'cp.ovLight')];
}

const NAME_KEYS = { 1: 'cp.name1', 2: 'cp.name2', 3: 'cp.name3' } as const;

function Floor({ scene }: { scene: Scene }) {
	const axes = useOverlay('axes');
	return <Show ov="grid"><Grid lesson={LESSON} extent={EXTENT[scene]} step={1} minor={scene === 2 ? 0.5 : undefined} axes={axes} ticks={scene === 2 && axes} /></Show>;
}

export function CrossScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	return <ViewportCard lesson={LESSON} id={'CP-' + scene} name={'CP-' + scene + ' · ' + t(locale, NAME_KEYS[scene])}
		mode={scene === 2 ? '2d' : '3d'} extent={EXTENT[scene]}
		rows={rowsFor(scene, locale)} overlays={overlaysFor(scene, locale)} status={statusFor(scene, locale)}
		drawIn={scene === 1}>
		<Floor scene={scene} />
		{scene === 1 ? <Cross3 /> : scene === 2 ? <Signed2 /> : <Triangle />}
	</ViewportCard>;
}

