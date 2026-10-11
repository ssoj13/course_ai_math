import { BoxGeometry } from 'three';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useVals } from '../store';
import { add, norm, scale, sub, type V } from '../geom';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { RightAngle } from '../scene/RightAngle';
import { AreaTile } from '../scene/AreaTile';
import { AngleArc } from '../scene/AngleArc';
import { HalfPlane } from '../scene/HalfPlane';
import { Label } from '../scene/Label';
import { snap } from '../scene/drag';
import { Show, useOverlay, type Draggable } from '../scene/handles';
import { useWorldPx } from '../scene/view';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './projection.rows';
import { axes, ccw, derive, fmt2, fmtDeg, fmtVec, LESSON, SPEC } from './projection.math';

/**
 * Lesson 1.5 scenes: PJ-1 the signed shadow of a on the line of b, PJ-2 the vector projection, the perpendicular
 * remainder and the Pythagorean tiles, PJ-3 coordinates as shadows on an axis pair (orthonormal or not), PJ-4 the
 * shadow on a plane in 3D. Loaded lazily by the entry (projection.tsx); shares its store, its one model
 * (projection.math.ts) and its hover ids.
 */

type Vals = Parameters<typeof derive>[0];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);

/** Invisible box over what the reader can make the scene draw, so the home view frames it (see length.impl.tsx). */
function Reach({ lo, hi }: { lo: V; hi: V }) {
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast}
		position={[(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, 0]} scale={[hi[0] - lo[0], hi[1] - lo[1], 0.01]} />;
}

const TIP: readonly [number, number] = [10, -12];
const O2: V = [0, 0];
const O3: V = [0, 0, 0];
/** Half length of a drawn "infinite" line through O. */
const LINE = 7;
const mid = (p: V, q: V) => scale(add(p, q), 0.5);
const tiny = (v: V) => norm(v) < 1e-9;

/** The line of a unit direction through O, dashed, in the axis' colour. */
function AxisLine({ id, dir, role }: { id: string; dir: V; role: 'muted' | 'vecB' | 'axisX' | 'axisY' }) {
	return <Segment lesson={LESSON} id={id} a={scale(dir, -LINE)} b={scale(dir, LINE)} dashed role={role} />;
}

/** Screen distance of a shadow's ruler from its line: clear of the axis line and of the arrow lying on it. */
const RULER_PX = 9;

/**
 * A shadow on a line: a solid ruler O -> foot drawn RULER_PX beside the line, on the side away from the tip (on the
 * line itself it would hide under the axis arrow, which lies along the same line), the dashed drop from the tip to the
 * foot ("light from straight above the line"), the right angle there and the foot point. The value label sits beyond
 * the ruler, away from the tip, so it never covers the drop or the arrow.
 */
function ShadowOn({ id, foot, tip, dir, role, text }: { id: string; foot: V; tip: V; dir: V; role: 'vecC' | 'axisX' | 'axisY'; text: string }) {
	const labels = useOverlay('labels');
	const px = useWorldPx(RULER_PX, O2) ?? 0;
	const drop = sub(tip, foot);
	// Unit normal of the line, turned to the side opposite the tip (either side when the tip is on the line).
	const nrm: V = [-dir[1], dir[0]];
	const side = nrm[0] * drop[0] + nrm[1] * drop[1] > 0 ? scale(nrm, -1) : nrm;
	const off = scale(side, px);
	const m = mid(O2, foot);
	return <>
		{!tiny(foot) && <Segment lesson={LESSON} id={id} a={off} b={add(foot, off)} role={role} />}
		{!tiny(drop) && <Segment lesson={LESSON} id={id + ':ray'} a={tip} b={foot} dashed role="light" />}
		{!tiny(drop) && !tiny(foot) && <RightAngle lesson={LESSON} id={id + ':rt'} at={foot} u={drop} v={scale(foot, -1)} show />}
		<Point lesson={LESSON} id={id} at={foot} role={role} size={7} drag={false} />
		{labels && <Label lesson={LESSON} id={id} at={add(m, off)} away={sub(m, side)} gap={6} role={role}>{text}</Label>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// PJ-1: the signed shadow.

function Shadow() {
	const locale = useLocale(), v = useVals(LESSON), labels = useOverlay('labels');
	const z = derive(v).p;
	return <>
		<Reach lo={[-4.5, -4.5]} hi={[4.5, 4.5]} />
		{z.bh && <>
			<Show ov="line"><AxisLine id="axis" dir={z.bh} role="muted" /></Show>
			<ShadowOn id="shadow" foot={z.proj!} tip={z.a} dir={z.bh} role="vecC" text={'a·b/|b| = ' + fmt2(z.s)} />
			<Arrow lesson={LESSON} id="b" to="b" role="vecB" label="b" />
			<Show ov="angle"><AngleArc lesson={LESSON} id="th" at={O2} from="b" to="a" r={0.6} label={labels ? (d) => 'θ = ' + fmtDeg(d) : undefined} /></Show>
		</>}
		{!z.bh && <Point lesson={LESSON} id="b" at="b" role="hot" label="b" />}
		<Arrow lesson={LESSON} id="a" to="a" role="vecA" label="a" />
		<Show ov="labels">
			<Label lesson={LESSON} id="a" at="a" role="vecA" offset={TIP}>a</Label>
			{z.bh && <Label lesson={LESSON} id="b" at="b" role="vecB" offset={TIP}>b</Label>}
			{!z.bh && <Label lesson={LESSON} id="noAxis" at={O2} role="hot" offset={[0, 18]}>{t(locale, 'pj.noAxis')}</Label>}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// PJ-2: vector projection, remainder, Pythagoras.

function Split() {
	const locale = useLocale(), v = useVals(LESSON), labels = useOverlay('labels'), tiles = useOverlay('tiles');
	const z = derive(v).p;
	const tileLabel = (name: string) => (labels ? (a: number) => name + ' = ' + fmt2(a) : undefined);
	// Tiles face away from the right triangle O, foot, a: outward is the right of each edge for a counter-clockwise walk.
	const flip = z.proj ? !ccw(z.proj, z.a) : false;
	return <>
		<Reach lo={[-5, -5]} hi={[5, 5]} />
		{z.bh && z.proj && z.perp && <>
			<Show ov="line"><AxisLine id="axis" dir={z.bh} role="muted" /></Show>
			<Show ov="tiles">
				<AreaTile lesson={LESSON} id="proj2" a={O2} b={z.proj} role="vecC" flip={flip} label={tileLabel('|proj|²')} />
				<AreaTile lesson={LESSON} id="perp2" a={z.proj} b="a" role="light" flip={flip} label={tileLabel('|perp|²')} />
				<AreaTile lesson={LESSON} id="na2" a="a" b={O2} role="vecA" flip={flip} label={tileLabel('|a|²')} />
			</Show>
			<Arrow lesson={LESSON} id="b" to="b" role="vecB" label="b" />
			{!tiny(z.proj) && <Arrow lesson={LESSON} id="proj" to={z.proj} role="vecC" />}
			{!tiny(z.perp) && <Arrow lesson={LESSON} id="perp" from={z.proj} to={z.a} role="light" />}
			<RightAngle lesson={LESSON} id="rt" at={z.proj} u={z.perp} v={scale(z.proj, -1)} show={!tiny(z.perp) && !tiny(z.proj)} />
		</>}
		{!z.bh && <Point lesson={LESSON} id="b" at="b" role="hot" label="b" />}
		<Arrow lesson={LESSON} id="a" to="a" role="vecA" label="a" />
		<Show ov="labels">
			<Label lesson={LESSON} id="a" at="a" role="vecA" offset={TIP}>a</Label>
			{z.bh && <Label lesson={LESSON} id="b" at="b" role="vecB" offset={TIP}>b</Label>}
			{/* With the squares on, their labels name proj and perp already (an arrow name would sit on its square's label). */}
			{!tiles && z.proj && !tiny(z.proj) && <Label lesson={LESSON} id="proj" at={z.proj} role="vecC" away={z.a} gap={8}>proj</Label>}
			{!tiles && z.proj && z.perp && !tiny(z.perp) && <Label lesson={LESSON} id="perp" at={mid(z.proj, z.a)} role="light" away={O2} gap={8}>perp</Label>}
			{!z.bh && <Label lesson={LESSON} id="noAxis" at={O2} role="hot" offset={[0, 18]}>{t(locale, 'pj.noAxis')}</Label>}
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// PJ-3: coordinates as shadows.

/** Angle (degrees) of a 2D point, used by the axis handles. */
const angleOf = (p: V) => Math.atan2(p[1], p[0]) * 180 / Math.PI;
const clamp = (x: number, r: { min: number; max: number }) => Math.min(r.max, Math.max(r.min, x));

/** u₁'s tip turns the pair: the new φ is the angle of the dragged point, in whole degrees. */
const moveU1: Draggable['move'] = (vals, raw) => {
	const [u1] = axes(vals.phi as number, vals.gap as number);
	return { set: { phi: clamp(snap(angleOf(add(u1, raw)), 1), SPEC.phi) } };
};
/** u₂'s tip opens or closes the pair: the new gap is the angle from u₁ to the dragged point. */
const moveU2: Draggable['move'] = (vals, raw) => {
	const [, u2] = axes(vals.phi as number, vals.gap as number);
	let gap = angleOf(add(u2, raw)) - (vals.phi as number);
	gap = ((gap % 360) + 540) % 360 - 180;
	return { set: { gap: clamp(snap(gap, 1), SPEC.gap) } };
};

function Coords() {
	const v = useVals(LESSON), labels = useOverlay('labels'), coords = useOverlay('coords');
	const q = derive(v).q;
	const { u1, u2 } = q;
	const part = scale(u1, q.co[0]);
	return <>
		<Reach lo={[-5, -5]} hi={[5, 5]} />
		<Show ov="line">
			<AxisLine id="line1" dir={u1} role="axisX" />
			<AxisLine id="line2" dir={u2} role="axisY" />
		</Show>
		<ShadowOn id="sh1" foot={q.feet[0]} tip={q.c} dir={u1} role="axisX" text={'a·u₁ = ' + fmt2(q.sh[0])} />
		<ShadowOn id="sh2" foot={q.feet[1]} tip={q.c} dir={u2} role="axisY" text={'a·u₂ = ' + fmt2(q.sh[1])} />
		{/* Oblique axes: the real coordinates walk along u₁, then parallel to u₂; the rebuild from shadows misses. */}
		{!q.ortho && coords && <>
			<Segment lesson={LESSON} id="co1" a={O2} b={part} role="axisX" />
			<Segment lesson={LESSON} id="co2" a={part} b={q.c} role="axisY" />
			<Point lesson={LESSON} id="rebuild" at={q.rebuild} role="hot" size={8} drag={false} />
			<Segment lesson={LESSON} id="miss" a={q.rebuild} b={q.c} dashed role="hot" />
			{labels && <Label lesson={LESSON} id="rebuild" at={q.rebuild} role="hot" away={q.c} gap={8}>{'(a·u₁)u₁ + (a·u₂)u₂'}</Label>}
		</>}
		<Arrow lesson={LESSON} id="u1" to={u1} role="axisX" drag="none" />
		<Arrow lesson={LESSON} id="u2" to={u2} role="axisY" drag="none" />
		<Point lesson={LESSON} id="u1" at={(s) => axes(s.phi as number, s.gap as number)[0]} keys={['phi']} move={moveU1} role="axisX" label="u₁" />
		<Point lesson={LESSON} id="u2" at={(s) => axes(s.phi as number, s.gap as number)[1]} keys={['gap']} move={moveU2} role="axisY" label="u₂" />
		<Arrow lesson={LESSON} id="c" to="c" role="vecA" label="a" />
		<Show ov="labels">
			<Label lesson={LESSON} id="c" at="c" role="vecA" offset={TIP}>a</Label>
			<Label lesson={LESSON} id="u1" at={u1} role="axisX" away={O2} gap={10}>u₁</Label>
			<Label lesson={LESSON} id="u2" at={u2} role="axisY" away={O2} gap={10}>u₂</Label>
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// PJ-4: the shadow on a plane.

function Plane() {
	const v = useVals(LESSON), labels = useOverlay('labels'), basis = useOverlay('basis'), refl = useOverlay('refl');
	const g = derive(v).g;
	return <>
		<Show ov="plane">
			<HalfPlane lesson={LESSON} id="plane" normal="n3" side="pos" role="vecB" opacity={0.12} />
			<HalfPlane lesson={LESSON} id="plane" normal="n3" side="neg" role="muted" opacity={0.12} />
		</Show>
		{basis && <>
			<Arrow lesson={LESSON} id="e1" to={g.e1} role="muted" drag="none" />
			<Arrow lesson={LESSON} id="e2" to={g.e2} role="muted" drag="none" />
			{!tiny(g.legs[0]) && <Segment lesson={LESSON} id="e1s" a={O3} b={g.legs[0]} dashed role="vecC" />}
			{!tiny(g.legs[1]) && <Segment lesson={LESSON} id="e2s" a={g.legs[0]} b={g.ground} dashed role="vecC" />}
		</>}
		{!tiny(g.ground) && <Arrow lesson={LESSON} id="ground" to={g.ground} role="vecC" />}
		{!tiny(g.along) && <Segment lesson={LESSON} id="an" a={g.ground} b="a3" dashed role="light" />}
		<RightAngle lesson={LESSON} id="rt" at={g.ground} u={g.along} v={scale(g.ground, -1)} show={!tiny(g.along) && !tiny(g.ground)} />
		{refl && <Arrow lesson={LESSON} id="refl" to={g.refl} role="ink" />}
		<Arrow lesson={LESSON} id="n3" to="n3" role="vecB" label="n" />
		<Arrow lesson={LESSON} id="a3" to="a3" role="vecA" label="a" />
		{labels && <>
			<Label lesson={LESSON} id="a3" at="a3" role="vecA" offset={TIP}>a</Label>
			<Label lesson={LESSON} id="n3" at="n3" role="vecB" offset={TIP}>n</Label>
			{!tiny(g.ground) && <Label lesson={LESSON} id="ground" at={g.ground} role="vecC" away={g.a} gap={8}>{'a − (a·n)n'}</Label>}
			{!tiny(g.along) && <Label lesson={LESSON} id="an" at={mid(g.ground, g.a)} role="light" offset={[14, 0]}>{'(a·n)n'}</Label>}
			{refl && <Label lesson={LESSON} id="refl" at={g.refl} role="ink" offset={TIP}>r</Label>}
			{basis && <Label lesson={LESSON} id="e1" at={g.e1} role="muted" offset={[8, 12]}>e₁</Label>}
			{basis && <Label lesson={LESSON} id="e2" at={g.e2} role="muted" offset={[8, 12]}>e₂</Label>}
		</>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const tip = (name: string) => ({ mouse: t(locale, 'pj.hintTipMouse', { name }), touch: t(locale, 'pj.hintTipTouch', { name }) });
	const state = (v: Vals) => (derive(v).p.zeroB ? t(locale, 'pj.stateNoAxis') : null);
	if (scene === 1) {
		return {
			hints: {
				'a:tip': tip('a'), 'b:tip': tip('b'), 'b:pt': tip('b'),
				shadow: (v) => t(locale, 'pj.hintShadow', { s: fmt2(derive(v).p.s) }),
				'shadow:ray': t(locale, 'pj.hintRay'), axis: t(locale, 'pj.hintAxis'),
			},
			state,
		};
	}
	if (scene === 2) {
		return {
			hints: {
				'a:tip': tip('a'), 'b:tip': tip('b'), 'b:pt': tip('b'),
				proj: (v) => t(locale, 'pj.hintProj', { proj: fmtVec(derive(v).p.proj) }),
				perp: (v) => t(locale, 'pj.hintPerp', { perp: fmtVec(derive(v).p.perp) }),
				proj2: (v) => t(locale, 'pj.hintTile', { name: '|proj|²', value: fmt2(derive(v).p.proj2) }),
				perp2: (v) => t(locale, 'pj.hintTile', { name: '|perp|²', value: fmt2(derive(v).p.perp2) }),
				na2: (v) => t(locale, 'pj.hintTile', { name: '|a|²', value: fmt2(derive(v).p.na2) }),
			},
			state,
		};
	}
	if (scene === 3) {
		return {
			hints: {
				'c:tip': tip('a'),
				'u1:pt': { mouse: t(locale, 'pj.hintU1'), touch: t(locale, 'pj.hintU1') },
				'u2:pt': { mouse: t(locale, 'pj.hintU2'), touch: t(locale, 'pj.hintU2') },
				miss: (v) => t(locale, 'pj.hintMiss', { miss: fmt2(derive(v).q.miss) }),
			},
			state: (v) => (derive(v).q.ortho ? null : t(locale, 'pj.stateOblique')),
		};
	}
	return {
		hints: {
			'a3:tip': { mouse: t(locale, 'pj.hintTip3', { name: 'a' }), touch: t(locale, 'pj.hintTipTouch', { name: 'a' }) },
			'n3:tip': { mouse: t(locale, 'pj.hintN'), touch: t(locale, 'pj.hintN') },
			an: (v) => t(locale, 'pj.hintAn', { an: fmt2(derive(v).g.an) }),
			ground: (v) => t(locale, 'pj.hintGround', { g: fmtVec(derive(v).g.ground) }),
			plane: t(locale, 'pj.hintPlane'),
		},
	};
}

function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	const base = [ov('grid', 'pj.ovGrid'), ov('axes', 'pj.ovAxes'), ov('labels', 'pj.ovLabels')];
	if (scene === 1) return [...base, ov('line', 'pj.ovLine'), ov('angle', 'pj.ovAngle')];
	if (scene === 2) return [...base, ov('line', 'pj.ovLine'), ov('tiles', 'pj.ovTiles')];
	if (scene === 3) return [...base, ov('line', 'pj.ovAxisLines'), ov('coords', 'pj.ovCoords')];
	return [...base, ov('plane', 'pj.ovPlane'), ov('basis', 'pj.ovBasis'), ov('refl', 'pj.ovRefl', false)];
}

const CARD = {
	1: { id: 'PJ-1', name: 'pj.name1', mode: '2d', extent: 4.5 },
	2: { id: 'PJ-2', name: 'pj.name2', mode: '2d', extent: 5 },
	3: { id: 'PJ-3', name: 'pj.name3', mode: '2d', extent: 5 },
	4: { id: 'PJ-4', name: 'pj.name4', mode: '3d', extent: 3.5 },
} as const;

function Floor({ scene }: { scene: Scene }) {
	const axes2 = useOverlay('axes');
	const c = CARD[scene];
	return <Show ov="grid"><Grid lesson={LESSON} extent={c.extent} step={1} minor={c.mode === '2d' ? 0.5 : undefined} axes={axes2} ticks={c.mode === '2d' && axes2} /></Show>;
}

export function ProjScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const c = CARD[scene];
	return <ViewportCard lesson={LESSON} id={c.id} name={c.id + ' · ' + t(locale, c.name)} mode={c.mode} extent={c.extent}
		rows={rowsFor(scene, locale)} overlays={overlaysFor(scene, locale)} status={statusFor(scene, locale)} drawIn={scene === 1}>
		<Floor scene={scene} />
		{scene === 1 ? <Shadow /> : scene === 2 ? <Split /> : scene === 3 ? <Coords /> : <Plane />}
	</ViewportCard>;
}
