/**
 * Lesson 3.4 scenes in the 6d Blender look, drawn with the DotUnit's instanced `Boxes` renderer (one draw call):
 * - 3.4-1 (`RuleScene`): A op B = C for any two shapes of rank <= 3. A and B are drawn inside the result's shape:
 *   their own elements solid, the stride-0 repeats faint; the `bc` timeline stretches, then fills C. Shapes that do not
 *   broadcast draw A and B in their own shapes and the error.
 * - 3.4-2 (`TintScene`): a 4 x 4 RGB picture times a tint (3,): the three tint numbers fill the three channel images.
 * Every number comes from broadcasting.math.ts (the tensor core); the choreography is broadcasting.scene.ts.
 */
import { useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useStep, useVals } from '../store';
import { ViewportCard } from '../scene/ViewportCard';
import { useOverlay } from '../scene/handles';
import type { Hint, StatusSpec } from '../scene/StatusBar';
import type { SceneRole } from '../palette';
import { Boxes } from '../dotunit/Boxes';
import { Frame } from '../dotunit/choreo';
import { Tag } from '../dotunit/scene';
import { LESSON, model, OPS, tintModel, type BModel } from './broadcasting.math';
import { CAP, geo, linked, PANEL_GAP, writeFrame, writeIds, type Geo, type Look } from './broadcasting.scene';
import { ruleRows, tintRows } from './broadcasting.rows';
import { fmtNum, fmtShape, unravel } from './tensor-ops';
import { useStepFrame } from './tensor-ops.view';

/** Duration (ms) of each step of the `bc` timeline: 0 is the stored state (a cut), 1 the stretch, 2 the fill. */
const STEP_MS = [0, 900, 700];
/** Numbers are printed on every cell up to this many cells per panel. */
const LABEL_MAX = 24;

type Vals = Parameters<typeof model>[0];

/**
 * The figure: all cells, their numbers, the panel captions and the operator signs. The model is built here, inside the
 * Stage, from the SHOWN values, so an eased value (a typed tint, an arrow-key step) glides on screen.
 */
function Bcast({ of, look, timeline, caps, locale }: {
	of: (v: Vals) => BModel; look: Look; timeline: boolean; caps: (m: BModel, step: number) => string[]; locale: Locale;
}) {
	const vals = useVals(LESSON);
	const step = useStep(LESSON, 'bc');
	const m = useMemo(() => of(vals), [of, vals]);
	const g = useMemo(() => geo(m, look), [m, look]);
	const frame = useMemo(() => new Frame(CAP), []);
	const update = useStepFrame({
		lesson: LESSON, timeline: timeline ? 'bc' : null, ms: STEP_MS, frameKey: g.w + 'x' + g.h, dataKey: m,
		write: (s, u, hot) => {
			writeIds(g, frame);
			writeFrame(m, g, look, s, u, linked(m, hot), frame);
		},
	});
	return <>
		<Boxes lesson={LESSON} frame={frame} update={update} pad={1.4} />
		<Labels m={m} g={g} frame={frame} look={look} caps={caps(m, timeline ? step : 2)} locale={locale} />
	</>;
}

function Labels({ m, g, frame, look, caps, locale }: {
	m: BModel; g: Geo; frame: Frame; look: Look; caps: readonly string[]; locale: Locale;
}) {
	const labels = useOverlay('labels');
	if (!labels) return null;
	const centre = (i: number) => (): number[] | null => (i < frame.n && frame.alpha[i] > 0.2 ? [frame.c[3 * i], frame.c[3 * i + 1], frame.c[3 * i + 2]] : null);
	const midY = -(g.h - 1) / 2;
	const roles: SceneRole[] = look === 'image' ? ['ink', 'ink', 'ink'] : ['vecA', 'vecB', 'vecC'];
	return <>
		{look === 'num' && g.panels.map((P, p) => {
			const cnt = P.pos.length / 3;
			if (cnt > LABEL_MAX) return null;
			return Array.from({ length: cnt }, (_, k) => {
				const i = g.base[p] + k;
				const o = p === 0 ? m.opA : p === 1 ? m.opB : null;
				const v = o ? o.t.data[o.t.offset + o.src[k]] : p === 2 ? m.C!.data[k] : (p === 0 ? m.A : m.B).data[k];
				const repeat = o !== null && o.real[k] === 0;
				return <Tag key={p + ':' + k} lesson={LESSON} hot={'abc'[p] + k} small dense role={repeat ? 'muted' : 'ink'} text={fmtNum(v)} at={centre(i)} />;
			});
		})}
		{g.panels.map((P, p) => (
			<Tag key={'cap' + p} lesson={LESSON} role={roles[p]} text={caps[p]} offset={[0, 18]}
				at={() => [P.origin[0] + (P.w - 1) / 2, P.origin[1] - (P.h - 1) - 0.5, 0]} />
		))}
		{g.panels.slice(0, -1).map((P, p) => (
			<Tag key={'sign' + p} lesson={LESSON} role="ink" text={p === 0 ? OPS[m.op] : '='} at={() => [P.origin[0] + P.w - 0.5 + PANEL_GAP / 2, midY, 0]} />
		))}
		{!m.ok && <Tag lesson={LESSON} role="hot" text={t(locale, 'bc.sceneErr')} offset={[0, 44]} at={() => [(g.w - 1) / 2, -(g.h - 1) - 0.5, 0]} />}
	</>;
}

/** Status line for a hovered cell (`a<k>`, `b<k>`, `c<k>`), from the model of the current values. */
function hints(locale: Locale, of: (vals: Vals) => BModel, names: readonly [string, string]): StatusSpec['hints'] {
	const cell = (id: string): Hint => (vals) => {
		const m = of(vals);
		const k = Number(id.slice(1)), which = id[0];
		if (!m.ok) {
			const T = which === 'a' ? m.A : m.B;
			if (which === 'c' || !(k < T.size)) return null;
			return t(locale, 'bc.hintOwn', { name: which === 'a' ? names[0] : names[1], idx: unravel(T.shape, k).join(', '), v: fmtNum(T.data[k]) });
		}
		if (!(k < m.C!.size)) return null;
		const atIdx = unravel(m.out!, k).join(', ');
		const a = m.opA!, b = m.opB!;
		const va = a.t.data[a.src[k]], vb = b.t.data[b.src[k]];
		if (which === 'c') return t(locale, 'bc.hintC', { at: atIdx, a: fmtNum(va), op: OPS[m.op], b: fmtNum(vb), c: fmtNum(m.C!.data[k]) });
		const o = which === 'a' ? a : b, name = which === 'a' ? names[0] : names[1];
		const idx = unravel(o.t.shape, o.src[k]).join(', ');
		const v = fmtNum(o.t.data[o.src[k]]);
		return o.real[k] ? t(locale, 'bc.hintReal', { name, idx, v, p: o.src[k] }) : t(locale, 'bc.hintRepeat', { name, at: atIdx, idx, v });
	};
	return new Proxy({} as Record<string, Hint>, { get: (_, id) => (typeof id === 'string' && /^[abc]\d+$/.test(id) ? cell(id) : undefined) });
}

const extentOf = (g: Geo) => Math.max(4, 0.6 * Math.hypot(g.w + 3, g.h + 3));
const overlays = (locale: Locale) => [{ id: 'labels', label: t(locale, 'bc.ovLabels'), on: true }];

export function RuleScene() {
	const locale = useLocale();
	const m = model(useVals(LESSON));
	const rows = useMemo(() => ruleRows(locale), [locale]);
	const status = useMemo<StatusSpec>(() => ({
		hints: hints(locale, model, ['A', 'B']),
		idle: { mouse: t(locale, 'bc.idleMouse'), touch: t(locale, 'bc.idleTouch') },
		state: (vals) => {
			const mm = model(vals);
			return mm.ok ? null : t(locale, 'bc.stateErr', { sa: fmtShape(mm.sa), sb: fmtShape(mm.sb) });
		},
	}), [locale]);
	const caps = (m: BModel, step: number) => [
		t(locale, 'bc.capA', { shape: fmtShape(m.sa) }),
		m.ok && step >= 1 && fmtShape(m.sb) !== fmtShape(m.out!) ? t(locale, 'bc.capBto', { shape: fmtShape(m.sb), out: fmtShape(m.out!) }) : t(locale, 'bc.capB', { shape: fmtShape(m.sb) }),
		m.ok ? t(locale, 'bc.capC', { op: OPS[m.op], shape: fmtShape(m.out!) }) : '',
	];
	return <ViewportCard lesson={LESSON} id="3.4-1" name={'3.4-1 · ' + t(locale, 'bc.name1')} mode="3d" extent={extentOf(geo(m, 'num'))}
		rows={rows} status={status} overlays={overlays(locale)}>
		<Bcast of={model} look="num" timeline caps={caps} locale={locale} />
	</ViewportCard>;
}

export function TintScene() {
	const locale = useLocale();
	const m = tintModel(useVals(LESSON));
	const rows = useMemo(() => tintRows(locale), [locale]);
	const names = useMemo(() => [t(locale, 'bc.namePic'), t(locale, 'bc.nameTint')] as const, [locale]);
	const status = useMemo<StatusSpec>(() => ({
		hints: hints(locale, tintModel, names),
		idle: { mouse: t(locale, 'bc.idleTint'), touch: t(locale, 'bc.idleTouch') },
	}), [locale, names]);
	const caps = (m: BModel) => [
		t(locale, 'bc.capPic', { shape: fmtShape(m.sa) }),
		t(locale, 'bc.capTint', { shape: fmtShape(m.sb), out: fmtShape(m.out!) }),
		t(locale, 'bc.capOut', { shape: fmtShape(m.out!) }),
	];
	return <ViewportCard lesson={LESSON} id="3.4-2" name={'3.4-2 · ' + t(locale, 'bc.name2')} mode="3d" extent={extentOf(geo(m, 'image'))}
		rows={rows} status={status} overlays={overlays(locale)}>
		<Bcast of={tintModel} look="image" timeline={false} caps={caps} locale={locale} />
	</ViewportCard>;
}
