import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useHot, useStep, useVal, useVals } from '../store';
import { add, norm, scale, sub, to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { Stroke } from '../scene/view';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Poly } from '../scene/Poly';
import { AngleArc } from '../scene/AngleArc';
import { RightAngle } from '../scene/RightAngle';
import { HalfPlane } from '../scene/HalfPlane';
import { LitSphere } from '../scene/LitSphere';
import { Circle } from '../scene/Circle';
import { Label } from '../scene/Label';
import { HOVER_RANK, Show, useHoverable, useOverlay } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { DotFormula } from './dot-product';
import { rowsFor, type Scene } from './dot-product.rows';
import { LESSON, SPHERE_R, derive, derive3, lamp, moveB, moveLight, moveP, fmt, fmtAngle, fmtCos } from './dot-product.math';

/**
 * Lesson 1.4 scenes: DP-1 derivation (2D, draws itself), DP-2 sign explorer with the area face (2D), DP-3 3D explorer,
 * DP-4 the N·L lamp. Loaded lazily by the entry (dot-product.tsx); shares its store, its one model
 * (dot-product.math.ts) and its hover ids. Scene code reads `shown` values (eased writes glide), the DOM reads `vals`.
 */

type Vals = Parameters<typeof derive>[0];

/** Home framing per scene (spec: DP-1 5 around [−0.5, −0.5], DP-2 4.5, DP-3 3.5, DP-4 4). */
const EXTENT: Record<Scene, number> = { 1: 5, 2: 4.5, 3: 3.5, 4: 4 };
const ORIGIN2: V = [0, 0];
const ORIGIN3: V = [0, 0, 0];
const TIP: readonly [number, number] = [10, -12];

const mid = (p: V, q: V) => scale(add(p, q), 0.5);

/** Tile fill of a cancelled square: the fill fades to 15 % of its own opacity, the outline stays (spec DP-1 step 6). */
const tileFill = (fade: number) => STYLE.fillA * (0.15 + 0.85 * fade);

/** Arrow names next to their tips; text labels follow the `labels` overlay. */
function Names({ aKey, bKey }: { aKey: string; bKey: string }) {
	return <Show ov="labels">
		<Label lesson={LESSON} id={aKey} at={aKey} role="vecA" offset={TIP}>a</Label>
		<Label lesson={LESSON} id={bKey} at={bKey} role="vecB" offset={TIP}>b</Label>
	</Show>;
}

/**
 * A rectangle's value label at its far corner (the corner opposite O / P), not its centroid: the centroid sits on the
 * shadow band's label (DP-2) and on P's arrows (DP-4). Follows the `labels` overlay.
 */
function RectLabel({ id, pts, at, text, role = 'vecA' }: { id: string; pts: readonly V[]; at?: V; text: string; role?: SceneRole }) {
	// Just beyond the far corner, along the diagonal from the centre, so the text clears the rectangle's glowing edges.
	const corner = pts[2], centre = scale(pts.reduce<V>((s, p) => add(s, p), pts[0].map(() => 0)), 1 / pts.length);
	const where = at ?? add(corner, scale(sub(corner, centre), 0.2));
	return <Show ov="labels"><Label lesson={LESSON} id={id} at={where} role={role} offset={[0, 0]}>{text}</Label></Show>;
}

/**
 * DP-2's boundary a·v = 0: a dashed muted line that gets the hot OUTLINE (not a new colour) while b lies on it, or
 * while it is hovered/linked (spec DP-2 `perp`).
 */
function Boundary({ a, b, lit }: { a: V; b: V; lit: boolean }) {
	const pal = usePalette(), hot = useHot(LESSON, 'perp'), hover = useHoverable(LESSON, 'perp', HOVER_RANK.guide);
	return <group {...hover}><Stroke points={[to3(a), to3(b)]} color={pal.muted} hot={hot || lit} width={STYLE.edge} halo={false} dashPx={[7, 5]} order={1} /></group>;
}
/** Shadow of b on the line of a: the soft band O → foot and the dashed drop from b's tip (one id: `shadow`). */
function Shadow({ id, foot, tipKey, zero }: { id: string; foot: V; tipKey: string; zero: V }) {
	return <>
		<Segment lesson={LESSON} id={id} a={zero} b={foot} band role="muted" />
		<Segment lesson={LESSON} id={id} a={tipKey} b={foot} dashed role="muted" />
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// DP-1: the derivation.

function Derivation() {
	const v = useVals(LESSON), z = derive(v), step = useStep(LESSON, 'derive'), labels = useOverlay('labels'), legs = useOverlay('legs');
	const fade = { a: v.fade_a as number, b: v.fade_b as number };
	const square = (key: 'sq_a' | 'sq_b' | 'sq_amb') => (vals: Vals) => derive(vals)[key]!;
	// A cancelled square (step 6) keeps its outline but drops its label: the struck pair is gone from the equation, and its
	// label would sit on top of the rectangle's label inside the square on a.
	const strike = v.strike as number;
	const sqLabel = (text: string, key: 'na2' | 'nb2' | 'namb2', gone = false) => (labels && !gone ? (vals: Vals) => text + ' = ' + fmt(derive(vals)[key]) : undefined);
	return <>
		<Show ov="squares">{step >= 1 && <>
			{z.sq_a && <Poly lesson={LESSON} id="na2" timeline="derive" pts={square('sq_a')} role="vecA" fill={tileFill(fade.a)} label={sqLabel('|a|²', 'na2', strike >= 1)} />}
			{z.sq_b && <Poly lesson={LESSON} id="nb2" timeline="derive" pts={square('sq_b')} role="vecB" fill={tileFill(fade.b)} label={sqLabel('|b|²', 'nb2', strike >= 2)} />}
			{z.sq_amb && <Poly lesson={LESSON} id="namb2" timeline="derive" pts={square('sq_amb')} role="vecC" label={sqLabel('|a − b|²', 'namb2')} />}
		</>}</Show>
		<Show ov="rect">{z.rectPts && <>
			{/* Collapsed (zero width, hidden) before step 8, so stepping to 8 grows it from O along a with the spring. */}
			<Poly lesson={LESSON} id="rect" timeline="derive" role="vecA" fill={0.28} hatched={(vals) => derive(vals).shadow < 0}
				pts={(vals, s) => { const r = derive(vals).rectPts!; return s >= 8 ? r : [r[0], r[0], r[3], r[3]]; }} />
			{step >= 8 && <RectLabel id="rect" pts={z.rectPts} text={'a·b = ' + fmt(z.rect)} />}
			{step >= 8 && <Shadow id="shadow" foot={z.foot!} tipKey="b" zero={ORIGIN2} />}
		</>}</Show>
		<Arrow lesson={LESSON} id="a" to="a" role="vecA" legs={legs} />
		<Arrow lesson={LESSON} id="b" to="b" role="vecB" legs={legs} />
		<Arrow lesson={LESSON} id="amb" from="b" to="a" role="vecC" drag="none" label="a − b" />
		<Names aKey="a" bKey="b" />
		<Show ov="labels">{z.namb2 > 0 && <Label lesson={LESSON} id="amb" at={mid(z.a, z.b)} role="vecC" offset={[14, -10]}>a − b</Label>}</Show>
		<Show ov="angle"><AngleArc lesson={LESSON} id="th" at={ORIGIN2} from="a" to="b" r={0.7} label={labels ? (d) => 'θ = ' + fmtAngle(d) + '°' : undefined} /></Show>
		<RightAngle lesson={LESSON} id="rt" at={ORIGIN2} u="a" v="b" show={z.perp} />
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// DP-2: the sign explorer.

/**
 * A far corner of a half-plane for its "a·v > 0" / "a·v < 0" label: diagonally between the line of a and the boundary,
 * on the side away from b for the positive label and toward b for the negative one, so neither sits on an arrow.
 */
function corner(z: ReturnType<typeof derive>, sign: 1 | -1): V {
	const k = 0.55 * EXTENT[2];
	return add(scale(z.ahat!, sign * k), scale(z.h!, sign * k));
}

function Sign() {
	const locale = useLocale(), v = useVals(LESSON), z = derive(v), labels = useOverlay('labels'), legs = useOverlay('legs');
	const ext = EXTENT[2];
	return <>
		{z.ahat && <>
			<Show ov="halfplanes">
				<HalfPlane lesson={LESSON} id="hp_pos" normal="a" side="pos" role="vecA" />
				<HalfPlane lesson={LESSON} id="hp_neg" normal="a" side="neg" role="muted" />
				<Show ov="labels">
					<Label lesson={LESSON} id="hp_pos" at={corner(z, 1)} role="vecA">{'a·v > 0'}</Label>
					<Label lesson={LESSON} id="hp_neg" at={corner(z, -1)} role="muted">{'a·v < 0'}</Label>
				</Show>
			</Show>
			<Show ov="perp">
				{/* The boundary a·v = 0: every direction perpendicular to a. Lit while b lies on it. */}
				<Boundary a={scale([z.ahat[1], -z.ahat[0]], ext)} b={scale([-z.ahat[1], z.ahat[0]], ext)} lit={z.perp} />
				<Show ov="labels"><Label lesson={LESSON} id="perp" at={scale([-z.ahat[1], z.ahat[0]], 0.85 * ext)} role="muted" offset={[0, -10]}>a·v = 0</Label></Show>
			</Show>
			<Show ov="shadow"><Shadow id="shadow" foot={z.foot!} tipKey="b" zero={ORIGIN2} />
				<Show ov="labels"><Label lesson={LESSON} id="shadow" at={mid(ORIGIN2, z.foot!)} role="muted" offset={[0, 14]}>{'|b| cos θ = ' + fmt(z.shadow)}</Label></Show>
			</Show>
			<Show ov="rect">
				<Poly lesson={LESSON} id="rect" step={0} role="vecA" fill={0.28} pts={(vals) => derive(vals).rectPts!} hatched={(vals) => derive(vals).shadow < 0} />
				<RectLabel id="rect" pts={z.rectPts!} text={'a·b = ' + fmt(z.rect)} />
			</Show>
		</>}
		<Arrow lesson={LESSON} id="a" to="a" role="vecA" legs={legs} />
		<Arrow lesson={LESSON} id="b" to="b" role="vecB" legs={legs} move={moveB} />
		<Names aKey="a" bKey="b" />
		<Show ov="angle"><AngleArc lesson={LESSON} id="th" at={ORIGIN2} from="a" to="b" r={0.6} label={labels ? (d) => 'θ = ' + fmtAngle(d) + '°' : undefined} /></Show>
		<RightAngle lesson={LESSON} id="rt" at={ORIGIN2} u="a" v="b" show={z.perp} />
		{z.perp && <Label lesson={LESSON} id="badge" at="b" role="hot" offset={[16, 16]}>{t(locale, 'dp.badge')}</Label>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// DP-3: the same in 3D.

function Explorer3() {
	const locale = useLocale(), v = useVals(LESSON), z = derive3(v), labels = useOverlay('labels'), legs = useOverlay('legs');
	return <>
		{z.ahat && <>
			<Show ov="disc">
				{/* The plane a·v = 0 as a disc: the face seen from a's side is tinted with a, the other face muted. */}
				<HalfPlane lesson={LESSON} id="perp3" normal="a3" side="pos" role="vecA" opacity={0.12} />
				<HalfPlane lesson={LESSON} id="perp3" normal="a3" side="neg" role="muted" opacity={0.12} />
			</Show>
			<Show ov="rect">
				<Shadow id="shadow3" foot={z.foot!} tipKey="b3" zero={ORIGIN3} />
				<Poly lesson={LESSON} id="rect3" step={0} role="vecA" fill={0.28} pts={(vals) => derive3(vals).rectPts!} hatched={(vals) => derive3(vals).shadow < 0} />
				<RectLabel id="rect3" pts={z.rectPts!} text={'a·b = ' + fmt(z.rect)} />
			</Show>
		</>}
		<Arrow lesson={LESSON} id="a3" to="a3" role="vecA" legs={legs} label="a" />
		<Arrow lesson={LESSON} id="b3" to="b3" role="vecB" legs={legs} label="b" />
		<Names aKey="a3" bKey="b3" />
		<Show ov="angle"><AngleArc lesson={LESSON} id="th3" at={ORIGIN3} from="a3" to="b3" r={0.6} normal={z.arcRef} label={labels ? (d) => 'θ = ' + fmtAngle(d) + '°' : undefined} /></Show>
		<RightAngle lesson={LESSON} id="rt3" at={ORIGIN3} u="a3" v="b3" show={z.perp} />
		{z.perp && <Label lesson={LESSON} id="badge3" at="b3" role="hot" offset={[16, 16]}>{t(locale, 'dp.badge')}</Label>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// DP-4: the lamp.

/**
 * The lowest point of the terminator circle (world y down): the label sits on the ring, never inside the sphere, and away
 * from the lamp's own label (the lamp is usually above the sphere).
 */
function termLow(term: ReturnType<typeof lamp>['term']): V {
	const n = term.n, down = [n[1] * n[0], n[1] * n[1] - 1, n[1] * n[2]], len = norm(down);
	const dir = len > 1e-9 ? scale(down, 1 / len) : [1, 0, 0];
	return add(scale(n, norm(term.c) + 0.01), scale(dir, term.r));
}

function Lamp() {
	const locale = useLocale(), v = useVals(LESSON), z = lamp(v), raw = useVal(LESSON, 'nl_raw', 0), labels = useOverlay('labels');
	const tipN = add(z.P, scale(z.N, 1.2)), tipL = add(z.P, scale(z.L, 1.2));
	const onTerm = Math.abs(z.ndotl) < 0.02;
	return <>
		<LitSphere lesson={LESSON} id="sphere" center={ORIGIN3} r={SPHERE_R} light="nl_light" mode={raw ? 'raw' : 'lambert'} roles={{ base: 'vecC', neg: 'axisZ' }} />
		<Show ov="terminator">
			{/* The horizon of the point light, lifted 0.01 off the surface so the dashes are not half-buried. */}
			<Circle lesson={LESSON} id="term" center={(vals) => { const term = lamp(vals).term; return scale(term.n, norm(term.c) + 0.01); }}
				r={(vals) => lamp(vals).term.r} normal={(vals) => lamp(vals).term.n} role={onTerm ? 'hot' : 'muted'} dashPx={[7, 5]} />
			<Show ov="labels"><Label lesson={LESSON} id="term" at={termLow(z.term)} role="muted" offset={[0, 12]}>N·L = 0</Label></Show>
		</Show>
		<Show ov="normal"><Arrow lesson={LESSON} id="N" from={z.P} to={tipN} role="vecB" />
			<Show ov="labels"><Label lesson={LESSON} id="N" at={tipN} role="vecB" offset={TIP}>N</Label></Show>
		</Show>
		<Show ov="light"><Arrow lesson={LESSON} id="L_dir" from={z.P} to={tipL} role="vecA" />
			<Show ov="labels"><Label lesson={LESSON} id="L_dir" at={tipL} role="vecA" offset={TIP}>L</Label></Show>
		</Show>
		<Show ov="rays">
			<Segment lesson={LESSON} id="rays" a="nl_light" b={z.P} dashed role="light" />
		</Show>
		<Show ov="angle"><AngleArc lesson={LESSON} id="thNL" at={z.P} from={z.N} to={z.L} r={0.5} normal={z.arcRef} label={labels ? (d) => 'θ = ' + fmtAngle(d) + '°' : undefined} /></Show>
		<RightAngle lesson={LESSON} id="rt" at={z.P} u={z.N} v={z.L} show={Math.abs(z.ndotl) < 1e-9} />
		<Show ov="rect">
			<Poly lesson={LESSON} id="rectNL" step={0} role="vecB" fill={0.28} pts={(vals) => lamp(vals).rectPts} hatched={(vals) => lamp(vals).ndotl < 0} />
			{/* Controller ruling (DP-4 crowding): the label sits beyond the rectangle's far corner, along h away from P. */}
			<RectLabel id="rectNL" role="vecB" pts={z.rectPts} at={add(z.rectPts[2], scale(sub(z.rectPts[3], z.P), 0.25))} text={'N·L = ' + fmtCos(z.ndotl)} />
		</Show>
		<Point lesson={LESSON} id="light" at="nl_light" move={moveLight} role="light" size={14} label={t(locale, 'dp.light')} />
		<Point lesson={LESSON} id="P" at="nl_pt" move={moveP} role="ink" label="P" />
		<Show ov="labels">
			<Label lesson={LESSON} id="light" at="nl_light" role="light" offset={TIP}>{t(locale, 'dp.light')}</Label>
			<Label lesson={LESSON} id="P" at="nl_pt" role="ink" offset={[-14, 16]}>P</Label>
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

/** Status bar hints per scene (spec status lines); values are read live from the store. */
function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const tipHint = (name: string) => ({ mouse: t(locale, 'dp.hintTipMouse', { name }), touch: t(locale, 'dp.hintTipTouch', { name }) });
	if (scene === 1) {
		const sq = (label: string, key: 'na2' | 'nb2' | 'namb2') => (v: Vals) => t(locale, 'dp.hintSquare', { label, value: fmt(derive(v)[key]) });
		return {
			hints: {
				'a:tip': tipHint('a'), 'b:tip': tipHint('b'), amb: t(locale, 'dp.hintAmb'),
				na2: sq('|a|²', 'na2'), nb2: sq('|b|²', 'nb2'), namb2: sq('|a − b|²', 'namb2'),
				rect: (v) => t(locale, 'dp.hintRect', { value: fmt(derive(v).rect) }),
				shadow: t(locale, 'dp.hintShadow'),
			},
			idle: t(locale, 'dp.idle1'),
		};
	}
	if (scene === 2) {
		return {
			hints: {
				'a:tip': { mouse: t(locale, 'dp.hintATurn'), touch: t(locale, 'dp.hintATurnTouch') },
				'b:tip': { mouse: t(locale, 'dp.hintBSnap'), touch: t(locale, 'dp.hintBSnapTouch') },
				hp_pos: t(locale, 'dp.hintPos'), hp_neg: t(locale, 'dp.hintNeg'), perp: t(locale, 'dp.hintPerp'),
				rect: (v) => t(locale, 'dp.hintArea', { value: fmt(derive(v).rect) }),
				shadow: t(locale, 'dp.hintShadow'),
			},
			state: (v) => (derive(v).zeroA ? t(locale, 'dp.stateNoA') : null),
		};
	}
	if (scene === 3) {
		return {
			hints: {
				'a3:tip': { mouse: t(locale, 'dp.hintTip3', { name: 'a' }), touch: t(locale, 'dp.hintTipTouch', { name: 'a' }) },
				'b3:tip': { mouse: t(locale, 'dp.hintTip3', { name: 'b' }), touch: t(locale, 'dp.hintTipTouch', { name: 'b' }) },
				perp3: t(locale, 'dp.hintDisc'),
				rect3: (v) => t(locale, 'dp.hintArea', { value: fmt(derive3(v).rect) }),
				shadow3: t(locale, 'dp.hintShadow'),
			},
		};
	}
	return {
		hints: {
			'light:pt': { mouse: t(locale, 'dp.hintLight'), touch: t(locale, 'dp.hintLightTouch') },
			'P:pt': { mouse: t(locale, 'dp.hintP'), touch: t(locale, 'dp.hintPTouch') },
			term: t(locale, 'dp.hintTerm'),
			rectNL: (v) => t(locale, 'dp.hintRectNL', { value: fmtCos(lamp(v).ndotl) }),
		},
	};
}

/** Overlay toggles per scene (spec "Overlays"); each with its default. */
function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	if (scene === 1) return [ov('grid', 'dp.ovGrid'), ov('axes', 'dp.ovAxes'), ov('legs', 'dp.ovLegs'), ov('labels', 'dp.ovLabels'), ov('angle', 'dp.ovAngle'), ov('squares', 'dp.ovSquares'), ov('rect', 'dp.ovRect')];
	if (scene === 2) return [ov('grid', 'dp.ovGrid'), ov('axes', 'dp.ovAxes'), ov('labels', 'dp.ovLabels'), ov('halfplanes', 'dp.ovHalfPlanes'), ov('perp', 'dp.ovPerp'), ov('shadow', 'dp.ovShadow'), ov('rect', 'dp.ovRectOnly'), ov('angle', 'dp.ovAngle'), ov('legs', 'dp.ovLegs', false)];
	if (scene === 3) return [ov('grid', 'dp.ovFloor'), ov('axes', 'dp.ovAxes'), ov('legs', 'dp.ovLegs'), ov('labels', 'dp.ovLabels'), ov('disc', 'dp.ovDisc'), ov('rect', 'dp.ovRect'), ov('angle', 'dp.ovAngle')];
	return [ov('terminator', 'dp.ovTerminator'), ov('normal', 'dp.ovNormal'), ov('light', 'dp.ovLightArrow'), ov('angle', 'dp.ovAngle'), ov('rect', 'dp.ovRectNL'), ov('labels', 'dp.ovLabels'), ov('rays', 'dp.ovRays', false)];
}

const NAME_KEYS = { 1: 'dp.name1', 2: 'dp.name2', 3: 'dp.name3', 4: 'dp.name4' } as const;

/** Floor/plane grid with the `axes` overlay deciding whether the Blender-coloured axes are drawn. */
function Floor({ scene }: { scene: Scene }) {
	const axes = useOverlay('axes');
	return <Show ov="grid"><Grid lesson={LESSON} extent={EXTENT[scene]} step={1} axes={axes} ticks={scene !== 3 && axes} /></Show>;
}

export function DotScene({ scene }: { scene: Scene }) {
	const locale = useLocale(), raw = useVal(LESSON, 'nl_raw', 0);
	return <ViewportCard lesson={LESSON} id={'DP-' + scene} name={'DP-' + scene + ' · ' + t(locale, NAME_KEYS[scene])}
		mode={scene >= 3 ? '3d' : '2d'} extent={EXTENT[scene]} center={scene === 1 ? [-0.5, -0.5] : undefined}
		rows={rowsFor(scene, locale, raw)} overlays={overlaysFor(scene, locale)} status={statusFor(scene, locale)}
		instruments formula={<DotFormula scene={scene} compact />} drawIn={scene === 1}>
		{scene !== 4 && <Floor scene={scene} />}
		{scene === 1 ? <Derivation /> : scene === 2 ? <Sign /> : scene === 3 ? <Explorer3 /> : <Lamp />}
	</ViewportCard>;
}
