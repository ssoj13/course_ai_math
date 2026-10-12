import { useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Challenge } from '../Challenge';
import { Timeline } from '../Timeline';
import { CHECKS, code, firstTie, INITIAL, LESSON, LINKS, model, orderRun, SOURCES, SUM_NS, type RModel, type SrcId } from './reductions.math';
import { fmtNum, fmtShape, unravel } from './tensor-ops';
import './tensor-road.css';
import './tensor-ops.css';

/**
 * Lesson 3.5 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store, the DOM-only controls (data
 * picker, the NumPy call and its result, max ties, the timeline, challenges, the summation-order experiment) and the
 * lazy scene island. Everything three lives in reductions.impl.tsx; every number comes from reductions.math.ts.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const ReduceScene = localized(lazyIsland(() => import('./reductions.impl').then((m) => m.ReduceScene)));

const SRC_NAME: Record<SrcId, Parameters<typeof t>[1]> = { sheet: 'rd.srcSheet', block: 'rd.srcBlock', gray: 'rd.srcGray' };
const SRC_TEXT: Record<SrcId, Parameters<typeof t>[1]> = { sheet: 'rd.dSheet', block: 'rd.dBlock', gray: 'rd.dGray' };

/** Up to this many result values are listed in the readout. */
const LIST_MAX = 12;

export function readout(m: RModel, locale: Locale): string {
	const vals = Array.from(m.r.data.slice(0, LIST_MAX), (v) => fmtNum(v)).join(', ') + (m.r.size > LIST_MAX ? ', …' : '');
	const ax = m.axis === null
		? t(locale, 'rd.readAll')
		: t(locale, 'rd.readAxis', { a: m.axis, pos: m.axes[0], neg: m.axes[0] - m.x.ndim, n: m.x.shape[m.axes[0]] });
	return t(locale, 'rd.read', { from: fmtShape(m.x.shape), to: fmtShape(m.out), ax, n: m.count[0], vals });
}

function Ties({ m }: { m: RModel }) {
	const locale = useLocale();
	if (m.op !== 2) return null;
	const tie = firstTie(m);
	if (!tie) return <p className="tr-note">{t(locale, 'rd.noTie')}</p>;
	const cells = tie.cells.map((k) => '(' + unravel(m.x.shape, k).join(', ') + ')').join(', ');
	return <p className="tr-note">{t(locale, 'rd.tie', { v: fmtNum(m.r.data[tie.cell]), k: tie.cells.length, cells, first: '(' + unravel(m.x.shape, tie.cells[0]).join(', ') + ')' })}</p>;
}

function Reduce() {
	const locale = useLocale();
	const m = model(useVals(LESSON));
	const steps = (['rd.step0', 'rd.step1', 'rd.step2'] as const).map((k, i) => ({ id: String(i), text: t(locale, k) }));
	return <section className="not-content tr-controls" data-to-controls="reduce">
		<Timeline lesson={LESSON} id="rd" steps={steps} />
		<div role="radiogroup" aria-label={t(locale, 'rd.srcLabel')} className="tr-picks">
			{SOURCES.map((id, i) => (
				<button key={id} type="button" role="radio" aria-checked={m.src === id}
					onClick={() => lessonStore(LESSON).getState().set('src', i)}>{t(locale, SRC_NAME[id])}</button>
			))}
		</div>
		<p className="tr-note">{t(locale, SRC_TEXT[m.src])}</p>
		<p className="tr-formula"><code>{code(m)}</code> : {fmtShape(m.x.shape)} → {fmtShape(m.out)}</p>
		<p aria-live="polite">{readout(m, locale)}</p>
		<Ties m={m} />
		<Challenge lesson={LESSON} n={1} text={t(locale, 'rd.ch1')} check={CHECKS.colSums} hint={[t(locale, 'rd.ch1Hint')]} success={t(locale, 'rd.ch1Success')} />
		<Challenge lesson={LESSON} n={2} text={t(locale, 'rd.ch2')} check={CHECKS.keep} hint={[t(locale, 'rd.ch2Hint')]} success={t(locale, 'rd.ch2Success')} />
		<Challenge lesson={LESSON} n={3} text={t(locale, 'rd.ch3')} check={CHECKS.brightest} hint={[t(locale, 'rd.ch3Hint')]} success={t(locale, 'rd.ch3Success')} />
		<Challenge lesson={LESSON} n={4} text={t(locale, 'rd.ch4')} check={CHECKS.negative} hint={[t(locale, 'rd.ch4Hint')]} success={t(locale, 'rd.ch4Success')} />
	</section>;
}

/** The machine layer: one sum of n copies of 0.1 in float32, in three orders (core `sum`). */
function Order() {
	const locale = useLocale();
	const sn = Math.round(useVals(LESSON).sn as number);
	const n = SUM_NS[sn];
	const run = useMemo(() => orderRun(n), [n]);
	const ideal = n / 10;
	const rows = [
		{ key: 'rd.oSeq', v: run.seq },
		{ key: 'rd.oTree', v: run.tree },
		{ key: 'rd.oWide', v: run.wide },
	] as const;
	const fmt = (x: number) => x.toLocaleString('en-US', { maximumFractionDigits: 6, useGrouping: false });
	return <section className="not-content tr-controls" data-to-controls="order">
		<div role="radiogroup" aria-label={t(locale, 'rd.oCount')} className="tr-picks">
			{SUM_NS.map((k, i) => (
				<button key={k} type="button" role="radio" aria-checked={i === sn}
					onClick={() => lessonStore(LESSON).getState().set('sn', i)}>{'n = ' + k.toLocaleString('en-US').replace(/,/g, ' ')}</button>
			))}
		</div>
		<div className="to-scroll">
			<table className="to-sums">
				<thead>
					<tr><th scope="col">{t(locale, 'rd.oHow')}</th><th scope="col">{t(locale, 'rd.oSum')}</th><th scope="col">{t(locale, 'rd.oMean')}</th><th scope="col">{t(locale, 'rd.oErr')}</th></tr>
				</thead>
				<tbody>
					{rows.map((r) => (
						<tr key={r.key}>
							<th scope="row">{t(locale, r.key)}</th>
							<td>{fmt(r.v.sum)}</td>
							<td>{fmt(r.v.mean)}</td>
							<td>{fmt(r.v.sum - ideal)}</td>
						</tr>
					))}
				</tbody>
			</table>
		</div>
		<p className="tr-note">{t(locale, 'rd.oNote', { n: fmt(n), ideal: fmt(ideal), exact: fmt(run.exact) })}</p>
	</section>;
}

function Controls({ part }: { part: 'reduce' | 'order' }) {
	return part === 'reduce' ? <Reduce /> : <Order />;
}
export const ReduceControls = localized(Controls);
