import { t, type Locale, type TextKey } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { cross, type V } from '../geom';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { AngleArc } from '../scene/AngleArc';
import { Label } from '../scene/Label';
import { Show, useOverlay, type Move } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { GROUP_KEY, TOY_KEY, rowsFor, type Scene } from './cosine-similarity.rows';
import { LESSON, RANGE_S, TOY, dot, fmt2, fmtCos, fmtDeg, len, scaled, toy } from './cosine-similarity.math';
import { GROUP_ROLE, PIC, PIC_ORIGIN, WORDS, pairOf, picPos, picture, type Group } from './cosine-similarity.data';

/**
 * Lesson 1.7 scenes: COS-1 the toy meaning space (2D, three draggable word arrows), COS-2 scale invariance (2D, a
 * and s·b), COS-3 the real table projected to 3D for display (62 words; the selected pair as arrows). Loaded lazily
 * by the entry (cosine-similarity.tsx); shares its store, model and hover ids.
 */

type Vals = Parameters<typeof toy>[0];

const EXTENT: Record<Scene, number> = { 1: 1.2, 2: 9, 3: PIC };
const O2: V = [0, 0];
const TIP: readonly [number, number] = [10, -12];
const ROLE = { cat: 'vecA', dog: 'vecB', car: 'vecC' } as const;

/**
 * The home view frames the drawn figure (contentBox), which for COS-1 is only the first quadrant: a toy arrow dragged
 * to negative x left the view, and the feature names at the axis ends fell under the N-panel. This invisible box
 * declares the region the reader should see; it never draws or takes pointer events.
 */
function HomeBox({ min, max }: { min: readonly [number, number]; max: readonly [number, number] }) {
	const size: [number, number, number] = [max[0] - min[0], max[1] - min[1], 1e-3];
	return <mesh visible={false} raycast={() => null} position={[(min[0] + max[0]) / 2, (min[1] + max[1]) / 2, 0]}>
		<boxGeometry args={size} />
	</mesh>;
}

// ---------------------------------------------------------------------------------------------------------------
// COS-1: the toy space.

function ToySpace() {
	const locale = useLocale();
	return <>
		<HomeBox min={[-1, -0.2]} max={[1.2, 1.2]} />
		{TOY.map((k) => <Arrow key={k} lesson={LESSON} id={k} to={k} role={ROLE[k]} label={t(locale, TOY_KEY[k])} />)}
		<Show ov="angle">
			{/* Unlabelled arcs: the numbers live in the panel and the status line. On a phone the narrow cat-dog
			    wedge pushed its label onto the word names, and the names were then moved away from their arrows. */}
			<AngleArc lesson={LESSON} id="thCD" at={O2} from="cat" to="dog" r={0.32} role="vecB" />
			<AngleArc lesson={LESSON} id="thCC" at={O2} from="cat" to="car" r={0.55} role="vecC" />
		</Show>
		<Show ov="labels">
			{TOY.map((k) => <Label key={k} lesson={LESSON} id={k} at={k} role={ROLE[k]} offset={TIP}>{t(locale, TOY_KEY[k])}</Label>)}
			<Label lesson={LESSON} id="axFurry" at={[1.0, 0]} role="axisX" offset={[0, 14]}>{t(locale, 'cs.axFurry')}</Label>
			<Label lesson={LESSON} id="axWheels" at={[0, 1.1]} role="axisY" offset={[-40, 0]}>{t(locale, 'cs.axWheels')}</Label>
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// COS-2: scale invariance.

const snapS = (s: number) => Math.round(s / RANGE_S.step) * RANGE_S.step;

/**
 * Dragging the tip of s·b changes only s: the pointer is projected onto the line of b (s = p·b / b·b), so the
 * direction cannot change by accident. b's direction is edited in the panel.
 */
function moveS(vals: Vals, raw: V): Move {
	const b = vals.sb as number[];
	const bb = dot(b, b);
	if (bb === 0) return { set: {} };
	const s = Math.max(RANGE_S.min, Math.min(RANGE_S.max, snapS(dot(raw, b) / bb)));
	return { set: { s } };
}

function ScaleSpace() {
	const locale = useLocale(), v = useVals(LESSON), z = scaled(v);
	const b = v.sb as number[], nb = len(b);
	// The line of b through the origin, long enough to leave the view: where s·b can go.
	const reach = nb > 0 ? b.map((x) => (x / nb) * 12) : null;
	const sTip = (vals: Vals) => scaled(vals).sb;
	// The distance label beside the middle of the dashed gap, pushed off it on the side away from the origin.
	const a = v.sa as number[];
	const gapMid = z.sb.map((x, k) => (x + a[k]) / 2);
	const d = a.map((x, k) => x - z.sb[k]);
	const n = dot([-d[1], d[0]], gapMid) >= 0 ? [-d[1], d[0]] : [d[1], -d[0]];
	const gapAway = gapMid.map((x, k) => x - n[k]);
	return <>
		<Show ov="line">{reach && <Segment lesson={LESSON} id="lineB" a={O2} b={reach} dashed role="muted" />}</Show>
		<Show ov="gap"><Segment lesson={LESSON} id="gap" a="sa" b={z.sb} dashed role="light" /></Show>
		<Arrow lesson={LESSON} id="sa" to="sa" role="vecA" label="a" />
		<Arrow lesson={LESSON} id="sbArrow" to={z.sb} role="vecB" />
		<Point lesson={LESSON} id="sTip" at={sTip} move={moveS} keys={['s']} role="vecB" label="s·b" />
		<Show ov="angle"><AngleArc lesson={LESSON} id="th" at={O2} from="sa" to={z.sb} r={1.4} /></Show>
		<Show ov="labels">
			<Label lesson={LESSON} id="sa" at="sa" role="vecA" offset={TIP}>a</Label>
			<Label lesson={LESSON} id="sTip" at={z.sb} role="vecB" offset={[12, 14]}>{'s·b'}</Label>
			<Show ov="gap">{z.dist > 0.2 && <Label lesson={LESSON} id="gap" at={gapMid} away={gapAway} gap={6} role="light">{'|a − s·b| = ' + fmt2(z.dist)}</Label>}</Show>
			{/* The cosine sits in a fixed corner, not on the arc: a 16° wedge has no room for it, and a fixed place makes
			    "this number does not move while s changes" visible. */}
			<Label lesson={LESSON} id="th" at={[6.5, 0.9]} role={Number.isFinite(z.cos) ? 'vecB' : 'hot'}>{Number.isFinite(z.cos) ? 'cos θ = ' + fmtCos(z.cos) : t(locale, 'cs.zeroBadge')}</Label>
		</Show>
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// COS-3: the real table, projected.

/** Clicking a word point makes it the second word of the pair (the first becomes the old second). */
function pick(i: number) {
	const st = lessonStore(LESSON).getState();
	const [, j] = pairOf(st.vals);
	if (i !== j) st.set('pair', [j, i]);
}

function Projected() {
	const v = useVals(LESSON), [i, j] = pairOf(v), pic = picture([i, j]), names = useOverlay('names');
	const n = cross(pic.da, pic.db);
	const normal = len(n) > 1e-9 ? n : [0, 1, 0];
	return <>
		{WORDS.map((w, k) => <group key={w.word} onClick={(e) => { e.stopPropagation(); pick(k); }}>
			<Point lesson={LESSON} id={'w' + k} at={picPos(k)} drag={false} role={GROUP_ROLE[w.group as Group]} size={k === i || k === j ? 9 : 6} />
		</group>)}
		<Point lesson={LESSON} id="origin" at={PIC_ORIGIN} drag={false} role="ink" size={5} />
		<Arrow lesson={LESSON} id="pairA" from={PIC_ORIGIN} to={pic.a} role="vecA" />
		<Arrow lesson={LESSON} id="pairB" from={PIC_ORIGIN} to={pic.b} role="vecB" />
		<Show ov="angle">
			{/* The drawn angle is read in the panel (θ drawn): two near-parallel projections leave no wedge for a label. */}
			<AngleArc lesson={LESSON} id="thPic" at={PIC_ORIGIN} from={pic.da} to={pic.db} r={0.5} normal={normal} />
		</Show>
		<Show ov="labels">
			<Label lesson={LESSON} id="pairA" at={pic.a} role="vecA" offset={TIP}>{WORDS[i].word}</Label>
			<Label lesson={LESSON} id="pairB" at={pic.b} role="vecB" offset={TIP}>{WORDS[j].word}</Label>
			<Label lesson={LESSON} id="origin" at={PIC_ORIGIN} role="ink" offset={[-12, 14]}>0</Label>
		</Show>
		{names && WORDS.map((w, k) => (k === i || k === j ? null
			: <Label key={w.word} lesson={LESSON} id={'w' + k} at={picPos(k)} role="muted" offset={[8, -8]}>{w.word}</Label>))}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards.

function statusFor(scene: Scene, locale: Locale): StatusSpec {
	if (scene === 1) {
		const tipHint = (k: (typeof TOY)[number]) => ({ mouse: t(locale, 'cs.hintToy', { name: t(locale, TOY_KEY[k]) }), touch: t(locale, 'cs.hintToyTouch', { name: t(locale, TOY_KEY[k]) }) });
		return {
			hints: {
				'cat:tip': tipHint('cat'), 'dog:tip': tipHint('dog'), 'car:tip': tipHint('car'),
				thCD: (v) => t(locale, 'cs.hintArc', { cos: fmtCos(toy(v).cosCD), th: fmtDeg(toy(v).thCD) }),
				thCC: (v) => t(locale, 'cs.hintArc', { cos: fmtCos(toy(v).cosCC), th: fmtDeg(toy(v).thCC) }),
			},
			state: (v) => (toy(v).zero.length ? t(locale, 'cs.stateZero') : null),
		};
	}
	if (scene === 2) {
		return {
			hints: {
				'sa:tip': { mouse: t(locale, 'cs.hintA'), touch: t(locale, 'cs.hintATouch') },
				'sTip:pt': { mouse: t(locale, 'cs.hintS'), touch: t(locale, 'cs.hintSTouch') },
				gap: (v) => t(locale, 'cs.hintGap', { dist: fmt2(scaled(v).dist) }),
				th: (v) => t(locale, 'cs.hintTh', { cos: fmtCos(scaled(v).cos) }),
				lineB: t(locale, 'cs.hintLine'),
			},
			state: (v) => (Number.isFinite(scaled(v).cos) ? null : t(locale, 'cs.stateZero')),
		};
	}
	const hints: StatusSpec['hints'] = Object.fromEntries(WORDS.map((w, k) => [
		'w' + k, { mouse: t(locale, 'cs.hintWord', { word: w.word, group: t(locale, GROUP_KEY[w.group as Group]) }), touch: t(locale, 'cs.hintWordTouch', { word: w.word }) },
	]));
	return {
		hints: { ...hints, thPic: (v) => t(locale, 'cs.hintPic', { th: fmtDeg(picture(pairOf(v)).th) }), origin: t(locale, 'cs.hintOrigin') },
		idle: t(locale, 'cs.idle3'),
	};
}

function overlaysFor(scene: Scene, locale: Locale) {
	const ov = (id: string, key: TextKey, on = true) => ({ id, label: t(locale, key), on });
	if (scene === 1) return [ov('grid', 'cs.ovGrid'), ov('labels', 'cs.ovLabels'), ov('angle', 'cs.ovAngle')];
	if (scene === 2) return [ov('grid', 'cs.ovGrid'), ov('labels', 'cs.ovLabels'), ov('angle', 'cs.ovAngle'), ov('gap', 'cs.ovGap'), ov('line', 'cs.ovLine')];
	return [ov('grid', 'cs.ovFloor'), ov('labels', 'cs.ovLabels'), ov('angle', 'cs.ovAngle'), ov('names', 'cs.ovNames', false)];
}

const NAME_KEYS = { 1: 'cs.name1', 2: 'cs.name2', 3: 'cs.name3' } as const;

function Floor({ scene }: { scene: Scene }) {
	return <Show ov="grid">
		{scene === 1 ? <Grid lesson={LESSON} extent={EXTENT[1]} step={0.5} minor={0.1} />
			: scene === 2 ? <Grid lesson={LESSON} extent={EXTENT[2]} step={1} />
				: <Grid lesson={LESSON} extent={EXTENT[3]} step={1} ticks={false} />}
	</Show>;
}

export function CosScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	return <ViewportCard lesson={LESSON} id={'COS-' + scene} name={'COS-' + scene + ' · ' + t(locale, NAME_KEYS[scene])}
		mode={scene === 3 ? '3d' : '2d'} extent={EXTENT[scene]} center={scene === 2 ? [2.5, 2.5] : scene === 1 ? [0.4, 0.4] : undefined}
		rows={rowsFor(scene, locale)} overlays={overlaysFor(scene, locale)} status={statusFor(scene, locale)} drawIn={scene === 1}>
		<Floor scene={scene} />
		{scene === 1 ? <ToySpace /> : scene === 2 ? <ScaleSpace /> : <Projected />}
	</ViewportCard>;
}
