/**
 * Lesson 3.6 scene (3.6-1, the S3 light box) in the 6d Blender look: the reader's einsum drawn by the DotUnit's
 * generic renderer (`EinsumView`: operand faces, one shaft per output cell, tiles that drop into stacks on the output
 * face), with the index letters written on the block's edges and a click on a cell diving into its dot product
 * (`DotStatic`, the DotUnit face 2). Numbers and layout come from einsum.math.ts (the tensor core).
 */
import { useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { lessonStore, useStep, useVals } from '../store';
import { ViewportCard } from '../scene/ViewportCard';
import { Label } from '../scene/Label';
import { useOverlay } from '../scene/handles';
import type { Hint, StatusSpec } from '../scene/StatusBar';
import { DotStatic, EinsumView, Floor, overlays, pickHints } from '../dotunit/einsum.impl';
import { diveVectors } from '../dotunit/einsum.store';
import { Tag } from '../dotunit/scene';
import { axisTags, lab, LESSON, roleOf, type Lab } from './einsum.math';
import { labRows } from './einsum.rows';

type Ok = Extract<Lab, { ok: true }>;

/** Index letters on the block edges; the summed letters glow while the shafts run (pass steps 1 and 2). */
function AxisTags({ b }: { b: Ok }) {
	const labels = useOverlay('labels');
	const step = useStep(LESSON, 'pass');
	const tags = useMemo(() => axisTags(b.d), [b.d]);
	if (!labels) return null;
	return <>{tags.map((tg, i) => {
		const summed = tg.role === 'contracted' || tg.role === 'reduced';
		return <Tag key={i + tg.text} lesson={LESSON} hot={'l_' + tg.letter} text={tg.text} small
			role={summed ? (step === 1 || step === 2 ? 'hot' : 'light') : tg.role === 'batch' ? 'vecC' : 'muted'} at={() => tg.at} />;
	})}</>;
}

function LabBody() {
	const locale = useLocale(), v = useVals(LESSON), b = lab(v);
	if (!b.ok) return <Label lesson={LESSON} id="err" at={[0, 0, 0]} role="hot">{t(locale, 'es.sceneErr')}</Label>;
	const c = Math.min(b.d.L.cells.n - 1, Math.max(0, Math.round(v.sel as number)));
	if (v.dive === 1) {
		const pair = diveVectors(b.d, c);
		return <DotStatic lesson={LESSON} a={pair.a} b={pair.b} step={9} frameKey={'dive:' + b.op + JSON.stringify(b.plan.dims) + c} />;
	}
	return <>
		<EinsumView lesson={LESSON} d={b.d} timeline="pass" focus={c} frameKey={b.op + JSON.stringify(b.plan.dims)}
			onCell={(cell) => lessonStore(LESSON).getState().setMany({ sel: cell, dive: 1 })} />
		<AxisTags b={b} />
	</>;
}

function statusFor(locale: Locale): StatusSpec {
	const letter = (l: string): Hint => (vals) => {
		const b = lab(vals);
		if (!b.ok || !b.plan.inputs.join('').includes(l)) return null;
		return t(locale, 'es.hintLetter', { l, n: b.plan.dims[l], what: t(locale, ROLE_HINT[roleOf(b.plan, l)]) });
	};
	const dive = pickHints(locale);
	return {
		hints: new Proxy({} as Record<string, Hint>, { get: (_, id) => (typeof id !== 'string' ? undefined : id.startsWith('l_') ? letter(id.slice(2)) : dive[id]) }),
		state: (v) => (lab(v).ok ? null : t(locale, 'es.sceneErr')),
		idle: (v) => {
			const b = lab(v);
			if (!b.ok) return null;
			if (v.dive === 1) return t(locale, 'du.diveIdle');
			return b.plan.summed.length ? t(locale, 'du.summed', { letters: b.plan.summed.join(', ') }) : t(locale, 'du.outer');
		},
	};
}
const ROLE_HINT = { batch: 'es.rBatch', free: 'es.rFree', contracted: 'es.rContracted', reduced: 'es.rReduced' } as const;

export function LabScene() {
	const locale = useLocale();
	const rows = useMemo(() => labRows(locale), [locale]);
	const status = useMemo(() => statusFor(locale), [locale]);
	const b = lab(useVals(LESSON));
	const extent = b.ok ? Math.max(4, 0.55 * Math.hypot(b.d.L.hi[0] - b.d.L.lo[0], b.d.L.hi[1] - b.d.L.lo[1], b.d.L.hi[2] - b.d.L.lo[2])) : 4;
	return <ViewportCard lesson={LESSON} id="3.6-1" name={'3.6-1 · ' + t(locale, 'es.name')} mode="3d" extent={extent}
		rows={rows} status={status} overlays={overlays(locale)}>
		<Floor lesson={LESSON} extent={8} />
		<LabBody />
	</ViewportCard>;
}
