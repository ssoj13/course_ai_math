/**
 * The DotUnit card (docs/design/intro-and-dotunit.md §2): one computation with two faces. Face 1 (steps 0-2) is the
 * projection: arrows a and b, b's shadow on a's line, |a| × shadow. Face 2 (steps 3-9) is the machine: the components
 * unfold into axis-coloured legs, a's legs lie in a row, b's stand in a column that passes the row, matching legs
 * multiply into tiles (area aᵢbᵢ, hatched when negative), the tiles drop into a stack, and the stack ends exactly as
 * tall as the projection bar |a| · shadow. Face 1 uses engine primitives; face 2 is one instanced `Boxes` draw driven
 * by the pure choreography (choreo.ts). Every number comes from the tensor core through `dotOf` (model.ts).
 * Loaded lazily by the entry (dotunit.tsx).
 */
import { useMemo, useRef } from 'react';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { add, scale, sub, type V } from '../geom';
import { lessonStore, useStep, useVals } from '../store';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { AngleArc } from '../scene/AngleArc';
import { RightAngle } from '../scene/RightAngle';
import { Label } from '../scene/Label';
import { Show, useOverlay, useStage } from '../scene/handles';
import type { Hint, StatusSpec } from '../scene/StatusBar';
import { Boxes } from './Boxes';
import { DOT_BAR, dotCap, dotFrame, dotGeom, dotIds, Frame, unitMs, type DotGeom } from './choreo';
import { dotOf, LESSON, pad3 } from './dotunit.store';
import { unitRows } from './dotunit.rows';
import { fmt, fmtShort } from './model';
import { Tag, useClock } from './scene';

const O: V = [0, 0, 0];
const FRONT = [0, 0, 1] as const;
const TIP: readonly [number, number] = [10, -12];

const cross = (a: V, b: V): V => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const len = (a: V) => Math.hypot(...a);

/** Face 1 (spec §2.2): arrows, the dropped perpendicular, the shadow on a's line, the ruler line and the angle. */
function Face1({ step }: { step: number }) {
	const locale = useLocale(), v = useVals(LESSON), d = dotOf(v), labels = useOverlay('labels');
	const a3 = pad3(d.a), b3 = pad3(d.b), n = d.a.length;
	// Only n <= 3 has an honest 3D picture of the shadow; above it face 1 shows the first three components (spec §2.1).
	const honest = n <= 3 && d.lenA > 0;
	const ahat = honest ? scale(a3, 1 / d.lenA) : null;
	const foot = ahat ? scale(ahat, d.shadow) : null;
	const normal = (() => {
		const c = cross(a3, b3), l = len(c);
		if (l > 1e-9) return scale(c, 1 / l);
		return Math.abs(a3[1]) < 0.9 * (len(a3) || 1) ? [0, 1, 0] : [1, 0, 0];
	})();
	return <>
		<Arrow lesson={LESSON} id="a" to="du_a3" role="vecA" label="a" />
		<Arrow lesson={LESSON} id="b" to="du_b3" role="vecB" label="b" />
		<Show ov="labels">
			<Label lesson={LESSON} id="a" at="du_a3" role="vecA" offset={TIP}>a</Label>
			<Label lesson={LESSON} id="b" at="du_b3" role="vecB" offset={TIP}>b</Label>
		</Show>
		<Show ov="angle">{len(a3) > 0 && len(b3) > 0 && <AngleArc lesson={LESSON} id="theta" at={O} from="du_a3" to="du_b3" r={0.6} normal={normal}
			label={labels ? (deg) => 'θ = ' + Math.round(deg) + '°' : undefined} />}</Show>
		{!honest && n > 3 && labels && <Label lesson={LESSON} id="more" at={O} role="muted" offset={[0, 26]}>{t(locale, 'du.more', { k: String(n - 3) })}</Label>}
		{foot && step >= 1 && <>
			<Segment lesson={LESSON} id="shadow" a={O} b={foot} band role="vecC" />
			<Segment lesson={LESSON} id="perp" a="du_b3" b={foot} dashed role="muted" />
			<Point lesson={LESSON} id="foot" at={foot} drag={false} role="vecC" size={7} />
			{len(sub(b3, foot)) > 1e-6 && <RightAngle lesson={LESSON} id="rt" at={foot} u={ahat!} v={sub(b3, foot)} show />}
			<Show ov="labels"><Label lesson={LESSON} id="shadow" at={scale(foot, 0.5)} role="vecC" offset={[0, 16]}>{t(locale, 'du.shadow') + ' = ' + fmt(d.shadow)}</Label></Show>
		</>}
		{ahat && step >= 2 && <>
			<Segment lesson={LESSON} id="ruler" a={scale(ahat, -1.5)} b={scale(ahat, d.lenA + 1.5)} dashed role="muted" />
			<Show ov="labels">
				<Label lesson={LESSON} id="len_a" at={scale(a3, 0.5)} role="vecA" offset={[-12, -16]}>{'|a| = ' + fmt(d.lenA)}</Label>
				<Label lesson={LESSON} id="dot" at={add(a3, scale(ahat, 0.4))} role="ink" offset={[12, 14]}>{'a·b = ' + fmt(d.proj)}</Label>
			</Show>
		</>}
	</>;
}

/** Number labels of face 2: leg values, tile products, the running sum on the stack and the projection bar's value. */
function Face2Tags({ g, f, clock }: { g: DotGeom; f: Frame; clock: { step: number; t: number } }) {
	const labels = useOverlay('labels');
	if (!labels) return null;
	const n = g.n;
	const centre = (k: number, dy = 0): number[] | null => (f.alpha[k] > 0.5 ? [f.c[3 * k], f.c[3 * k + 1] + dy, f.c[3 * k + 2]] : null);
	const face2 = () => clock.step >= 4;
	return <>
		{g.d.a.map((x, i) => <Tag key={'a' + i} lesson={LESSON} hot={'a' + i} role="ink" small text={fmtShort(x)} offset={[0, 14]} at={() => (face2() ? centre(i) : null)} />)}
		{g.d.b.map((x, i) => <Tag key={'b' + i} lesson={LESSON} hot={'b' + i} role="ink" small text={fmtShort(x)} offset={[-16, 0]} at={() => (face2() ? centre(n + i) : null)} />)}
		{g.d.prods.map((p, i) => <Tag key={'p' + i} lesson={LESSON} hot={'p' + i} role="ink" text={fmtShort(p)} at={() => (clock.step >= 5 ? centre(2 * n + i) : null)} />)}
		<Tag lesson={LESSON} hot="sum" role="ink" text={'Σ = ' + fmt(g.d.dot)} offset={[-44, -10]}
			at={() => (clock.step > 7 || (clock.step === 7 && clock.t >= 1) ? [g.stackX + 0.5, g.run[n - 1], 0] : null)} />
		<Tag lesson={LESSON} hot="proj" role="vecC" text={'|a|·s = ' + fmt(g.proj)} offset={[52, -10]}
			at={() => (clock.step >= 8 ? [g.barX + 0.3, f.c[3 * DOT_BAR(n) + 1] * 2, 0] : null)} />
	</>;
}

function Face2() {
	const v = useVals(LESSON), d = dotOf(v), n = d.a.length;
	const g = useMemo(() => dotGeom(d), [d]);
	const frame = useMemo(() => {
		const f = new Frame(dotCap(n));
		dotIds(n, f);
		return f;
	}, [n]);
	const { clock, tick } = useClock(LESSON, 'unit', (s) => unitMs(s, n));
	const reg = useStage();
	const geo = useRef(g);
	const dirty = useRef(true);
	if (geo.current !== g) {
		geo.current = g;
		dirty.current = true;
	}
	/** Step whose framing was last requested (face 2 is framed from the front; face 1 gets the home view back). */
	const framed = useRef(-1);
	const update = () => {
		const changed = tick();
		if (!changed && !dirty.current) return 'still' as const;
		dirty.current = false;
		dotFrame(geo.current, clock.step, clock.t, frame);
		// Camera choreography (spec §2.4 step 3): smooth view to the front as face 2 starts, and View All from the front
		// again once a step's motion has played (the row, the stack and the bar widen the figure).
		const key = clock.step * 2 + (clock.t >= 1 ? 1 : 0);
		if (clock.step >= 3) {
			if (key !== framed.current) {
				framed.current = key;
				reg.nav?.frame(FRONT);
			}
		} else if (framed.current >= 6) {
			framed.current = -1;
			reg.nav?.reset();
		}
		return clock.t < 1 ? ('moving' as const) : ('moved' as const);
	};
	return <>
		<Boxes lesson={LESSON} frame={frame} update={update} />
		<Face2Tags g={g} f={frame} clock={clock} />
	</>;
}

function Floor() {
	const axes = useOverlay('axes');
	return <Show ov="grid"><Grid lesson={LESSON} extent={6} step={1} axes={axes} /></Show>;
}

function Unit() {
	const step = useStep(LESSON, 'unit');
	return <>
		{step <= 3 && <Face1 step={step} />}
		<Face2 />
	</>;
}

/** Status hints (spec §2.2, §2.3): the tips, every leg and tile, and the "aha" line from step 8 on. */
export function unitStatus(locale: Locale): StatusSpec {
	const hints: Record<string, Hint> = {
		'a:tip': { mouse: t(locale, 'du.hintTip', { name: 'a' }), touch: t(locale, 'du.hintTipTouch', { name: 'a' }) },
		'b:tip': { mouse: t(locale, 'du.hintTip', { name: 'b' }), touch: t(locale, 'du.hintTipTouch', { name: 'b' }) },
		shadow: t(locale, 'du.hintShadow'),
		proj: t(locale, 'du.hintProj'),
		sum: t(locale, 'du.hintSum'),
	};
	for (let i = 0; i < 8; i++) {
		hints['p' + i] = (v) => {
			const d = dotOf(v);
			return i < d.a.length ? t(locale, 'du.hintTile', { a: fmtShort(d.a[i]), b: fmtShort(d.b[i]), p: fmtShort(d.prods[i]) }) : null;
		};
		hints['a' + i] = (v) => (i < dotOf(v).a.length ? t(locale, 'du.hintLeg', { name: 'a', i: String(i + 1), x: fmtShort(dotOf(v).a[i]) }) : null);
		hints['b' + i] = (v) => (i < dotOf(v).b.length ? t(locale, 'du.hintLeg', { name: 'b', i: String(i + 1), x: fmtShort(dotOf(v).b[i]) }) : null);
	}
	return { hints, idle: () => ((lessonStore(LESSON).getState().step.unit ?? 0) >= 8 ? t(locale, 'du.sameHeight') : null) };
}

export function DotUnitCard({ card = 'DU' }: { card?: string }) {
	const locale = useLocale();
	const status = useMemo(() => unitStatus(locale), [locale]);
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	return <ViewportCard lesson={LESSON} id={card} name={card + ' · ' + t(locale, 'du.name')} mode="3d" extent={4}
		rows={unitRows(locale)} status={status} drawIn
		overlays={[ov('grid', 'du.ovGrid'), ov('axes', 'du.ovAxes'), ov('labels', 'du.ovLabels'), ov('angle', 'du.ovAngle')]}>
		<Floor />
		<Unit />
	</ViewportCard>;
}
