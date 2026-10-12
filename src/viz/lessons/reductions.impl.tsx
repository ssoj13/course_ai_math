/**
 * Lesson 3.5 scene (3.5-1) in the 6d Blender look, drawn with the DotUnit's instanced `Boxes` renderer: a tensor, the
 * axis that collapses and the result below it. The `rd` timeline slides every cell along the reduced axis into one
 * cell, then moves the result out in its own shape (keepdims or not). Numbers come from reductions.math.ts (the tensor
 * core); the choreography is reductions.scene.ts.
 */
import { useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useStep, useVals } from '../store';
import { ViewportCard } from '../scene/ViewportCard';
import { useOverlay } from '../scene/handles';
import type { Hint, StatusSpec } from '../scene/StatusBar';
import { Boxes } from '../dotunit/Boxes';
import { Frame } from '../dotunit/choreo';
import { Tag } from '../dotunit/scene';
import { code, LESSON, model, OP_NAMES, type RModel } from './reductions.math';
import { CAP, geo, linked, resultAt, writeFrame, writeIds, type Geo } from './reductions.scene';
import { reduceRows } from './reductions.rows';
import { fmtNum, fmtShape, unravel } from './tensor-ops';
import { useStepFrame } from './tensor-ops.view';

/** Duration (ms) of each step of the `rd` timeline: 0 = the input (a cut), 1 = the collapse, 2 = the result moves out. */
const STEP_MS = [0, 1100, 700];
/** Numbers are printed on the input cells up to this many, and on the result cells up to this many. */
const LABEL_MAX = 24;

function Reduce({ locale }: { locale: Locale }) {
	const vals = useVals(LESSON);
	const step = useStep(LESSON, 'rd');
	const m = useMemo(() => model(vals), [vals]);
	const g = useMemo(() => geo(m), [m]);
	const frame = useMemo(() => new Frame(CAP), []);
	const update = useStepFrame({
		lesson: LESSON, timeline: 'rd', ms: STEP_MS, frameKey: g.w + 'x' + g.h, dataKey: m,
		write: (s, u, hot) => {
			writeIds(m, frame);
			writeFrame(m, g, s, u, linked(m, hot), frame);
		},
	});
	return <>
		<Boxes lesson={LESSON} frame={frame} update={update} pad={2} />
		<Labels m={m} g={g} frame={frame} step={step} locale={locale} />
	</>;
}

function Labels({ m, g, frame, step, locale }: { m: RModel; g: Geo; frame: Frame; step: number; locale: Locale }) {
	const labels = useOverlay('labels');
	if (!labels) return null;
	const n = m.x.size;
	const centre = (i: number) => (): number[] | null => (i < frame.n && frame.alpha[i] > 0.2 ? [frame.c[3 * i], frame.c[3 * i + 1], frame.c[3 * i + 2]] : null);
	// While the result sits on the landing cells (step 1), their own input numbers would sit under the result's.
	const landing = new Set(Array.from(m.first));
	const captionAt = (P: Geo['input']) => () => [P.origin[0] + (P.w - 1) / 2, P.origin[1] - (P.h - 1) - 0.5, 0];
	const axisText = m.axis === null ? t(locale, 'rd.capAll') : t(locale, 'rd.capAxis', { a: m.axis });
	return <>
		{!g.image && n <= LABEL_MAX && Array.from({ length: n }, (_, k) => (
			<Tag key={'x' + k} lesson={LESSON} hot={'x' + k} small dense role={step >= 1 ? 'muted' : 'ink'} text={fmtNum(m.x.data[k])}
				at={step === 1 && landing.has(k) ? () => null : centre(k)} />
		))}
		{m.r.size <= LABEL_MAX && Array.from({ length: m.r.size }, (_, c) => (
			<Tag key={'r' + c} lesson={LESSON} hot={'r' + c} small={m.r.size > 8} role="ink" text={fmtNum(m.r.data[c])} at={centre(resultAt(m, c))} />
		))}
		<Tag lesson={LESSON} role="ink" text={'x ' + fmtShape(m.x.shape) + ' · ' + axisText} offset={[0, 18]} at={captionAt(g.input)} />
		{step >= 2 && <Tag lesson={LESSON} role="vecC" text={code(m) + ' → ' + fmtShape(m.out)} offset={[0, 18]} at={captionAt(g.output)} />}
	</>;
}

function statusFor(locale: Locale): StatusSpec {
	const cell = (id: string): Hint => (vals) => {
		const m = model(vals);
		const k = Number(id.slice(1));
		if (id[0] === 'x') {
			if (!(k < m.x.size)) return null;
			const c = m.group[k];
			return t(locale, 'rd.hintX', { idx: unravel(m.x.shape, k).join(', '), v: fmtNum(m.x.data[k]), r: unravel(m.out, c).join(', '), op: OP_NAMES[m.op] });
		}
		if (!(k < m.r.size)) return null;
		return t(locale, 'rd.hintR', { op: OP_NAMES[m.op], idx: unravel(m.out, k).join(', '), n: m.count[k], v: fmtNum(m.r.data[k]) });
	};
	return {
		hints: new Proxy({} as Record<string, Hint>, { get: (_, id) => (typeof id === 'string' && /^[xr]\d+$/.test(id) ? cell(id) : undefined) }),
		idle: { mouse: t(locale, 'rd.idleMouse'), touch: t(locale, 'rd.idleTouch') },
	};
}

export function ReduceScene() {
	const locale = useLocale();
	const m = model(useVals(LESSON));
	const g = geo(m);
	const rows = useMemo(() => reduceRows(locale), [locale]);
	const status = useMemo(() => statusFor(locale), [locale]);
	const extent = Math.max(4, 0.6 * Math.hypot(g.w + 3, g.h + 3));
	return <ViewportCard lesson={LESSON} id="3.5-1" name={'3.5-1 · ' + t(locale, 'rd.name')} mode="3d" extent={extent}
		rows={rows} status={status} overlays={[{ id: 'labels', label: t(locale, 'rd.ovLabels'), on: true }]}>
		<Reduce locale={locale} />
	</ViewportCard>;
}
