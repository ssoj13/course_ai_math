import { useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { roleVar } from '../palette';
import { Challenge, type ChallengeProps } from '../Challenge';
import { FieldCell, type Row } from '../scene/NPanel';
import { CHECKS, EXAMPLES, fmtInt, fmtShape, fmtVal, indexOf, INITIAL, kKey, LESSON, LINKS, memView, model, selectPatch, type ExampleId, type Model } from './tensor-road.math';
import { MemStrip, Tuple } from './MemStrip';
import './tensor-road.css';

/**
 * Lesson 3.1 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store with its one link group, the
 * DOM-only controls (shape readout, real-data picker, index fields, the memory strip, challenges) and the lazy scene
 * island. Everything three lives in tensor-road.impl.tsx. Words come from the `tr` dictionary namespace; every number
 * comes from tensor-road.math.ts, the model the scene draws.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const RoadScene = localized(lazyIsland(() => import('./tensor-road.impl').then((m) => m.RoadScene)));

const EX_NAME: Record<ExampleId, Parameters<typeof t>[1]> = {
	loss: 'tr.exLoss', pixel: 'tr.exPixel', word: 'tr.exWord', gray: 'tr.exGray', verts: 'tr.exVerts',
	rgb: 'tr.exRgb', text: 'tr.exText', anim: 'tr.exAnim', batch: 'tr.exBatch', video: 'tr.exVideo',
};
const EX_TEXT: Record<ExampleId, Parameters<typeof t>[1]> = {
	loss: 'tr.dLoss', pixel: 'tr.dPixel', word: 'tr.dWord', gray: 'tr.dGray', verts: 'tr.dVerts',
	rgb: 'tr.dRgb', text: 'tr.dText', anim: 'tr.dAnim', batch: 'tr.dBatch', video: 'tr.dVideo',
};

/** Shape, rank and element count of what the card shows, in the axis colours of its rulers. */
function Hud({ m }: { m: Model }) {
	const locale = useLocale();
	const product = m.rank === 0 ? t(locale, 'tr.emptyProduct') : m.shape.join(' · ') + ' = ' + fmtInt(m.size);
	return <div className="tr-hud">
		<span>{t(locale, 'tr.hudShape')} <Tuple nums={m.shape} roles={m.role} /></span>
		<span>{t(locale, 'tr.hudRank')} <b>{m.rank}</b></span>
		<span>{t(locale, 'tr.hudElements')} {product}</span>
	</div>;
}

/** The real-data examples of the current rank (data mode), and what the shown one is. */
function Examples({ m }: { m: Model }) {
	const locale = useLocale();
	const list = EXAMPLES[m.rank];
	if (!m.ex) return m.noExample ? <p className="tr-note">{t(locale, 'tr.noExample')}</p> : null;
	const ex = m.ex;
	return <div className="tr-examples">
		{list.length > 1 && <div role="radiogroup" aria-label={t(locale, 'tr.examples')} className="tr-picks">
			{list.map((e, i) => (
				<button key={e.id} type="button" role="radio" aria-checked={e.id === ex.id}
					onClick={() => lessonStore(LESSON).getState().set('pick', i)}>{t(locale, EX_NAME[e.id])}</button>
			))}
		</div>}
		<p className="tr-note">
			<b>{t(locale, EX_NAME[ex.id])}.</b> {t(locale, EX_TEXT[ex.id])}
			{ex.of && <> {t(locale, 'tr.sliceOf', { k: ex.shape.at(-1)!, n: ex.of.at(-1)!, full: fmtShape(ex.of) })}</>}
		</p>
	</div>;
}

/** One index field per axis (shape order), named by the axis letter of an example or `i0, i1, ...` on the road. */
function IndexFields({ m }: { m: Model }) {
	const locale = useLocale();
	const rows = useMemo<Row[]>(() => m.shape.map((n, a) => ({
		id: 'idx' + a, label: m.names ? m.names[a] : 'i' + a, kind: 'num', key: kKey(m.rank - 1 - a),
		spec: { min: 0, max: Math.max(1, n - 1), step: 1 }, fmt: fmtInt,
		tip: { text: t(locale, 'tr.tipIdx', { a, max: n - 1 }) },
	})), [m.shape, m.names, m.rank, locale]);
	if (!rows.length) return null;
	return <div className="tr-index" role="group" aria-label={t(locale, 'tr.indexLabel')}>
		<span aria-hidden="true">x[</span>
		{rows.map((r) => <FieldCell key={r.id + r.key} lesson={LESSON} row={r} size="sm" />)}
		<span aria-hidden="true">]</span>
	</div>;
}

export function readout(m: Model, locale: Locale): string {
	if (m.rank === 0) return t(locale, 'tr.readScalar', { v: fmtVal(m, m.value) });
	return t(locale, 'tr.read', { rank: m.rank, n: fmtInt(m.size), shape: fmtShape(m.shape), name: 'x', idx: m.idx.join(', '), v: fmtVal(m, m.value), off: m.offset });
}

const ROAD_CHALLENGES = (locale: Locale): Omit<ChallengeProps, 'lesson'>[] => [
	{ n: 1, text: t(locale, 'tr.ch1'), check: CHECKS.rank4of24, hint: [t(locale, 'tr.ch1Hint')] },
	{ n: 2, text: t(locale, 'tr.ch2'), check: CHECKS.offset17, hint: [t(locale, 'tr.ch2Hint')] },
	{ n: 3, text: t(locale, 'tr.ch3'), check: CHECKS.green25, hint: [t(locale, 'tr.ch3Hint')], success: t(locale, 'tr.ch3Success') },
];

/** Under the road card: what the card shows, the data picker, the index of the selected cell, the readout, tasks. */
function Road() {
	const locale = useLocale();
	const m = model(useVals(LESSON));
	return <section className="not-content tr-controls" data-tr-controls="road">
		<Hud m={m} />
		<Examples m={m} />
		<IndexFields m={m} />
		<p aria-live="polite">{readout(m, locale)}</p>
		{ROAD_CHALLENGES(locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}

/**
 * The flat buffer: one strip of cells in address order, the view's shape and strides, the address formula and the walk
 * order. `.T` reverses shape and strides: no cell moves, the walk (bottom numbers) jumps instead.
 */
function Memory() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const m = model(v);
	const tr = v.tr === 1;
	const view = memView(m, tr);
	const roles = tr ? [...m.role].reverse() : m.role;
	const select = (p: number) => lessonStore(LESSON).getState().setMany(selectPatch(m, p));
	const terms = view.idx.map((i, a) => <span key={a}>
		{a > 0 && ' + '}
		<span style={{ color: roleVar(roles[a]) }}>{i}</span>·{view.strides[a]}
	</span>);
	return <section className="not-content tr-controls" data-tr-controls="memory">
		<label className="tr-check">
			<input type="checkbox" checked={tr} onChange={(e) => lessonStore(LESSON).getState().set('tr', e.currentTarget.checked ? 1 : 0)} />
			{t(locale, 'tr.memTranspose')}
		</label>
		<div className="tr-hud">
			<span><code>{tr ? 'x.T' : 'x'}</code></span>
			<span>{t(locale, 'tr.hudShape')} <Tuple nums={view.shape} roles={roles} /></span>
			<span>{t(locale, 'tr.hudStrides')} <Tuple nums={view.strides} roles={roles} /></span>
			<span>{view.contiguous ? t(locale, 'tr.memContig') : t(locale, 'tr.memNotContig')}</span>
		</div>
		<p className="tr-formula">
			{t(locale, 'tr.memAddress')} = {m.rank ? terms : '0'} = <b>{m.offset}</b>
		</p>
		<MemStrip lesson={LESSON} label={t(locale, 'tr.memTitle')} n={m.size} center={m.offset} sel={m.offset} onSelect={select}
			more={(k) => t(locale, 'tr.memMore', { k })}
			cell={(p) => ({
				text: fmtVal(m, m.data[p]), step: view.step[p], hot: 'c' + p,
				label: t(locale, 'tr.hintCell', { name: 'x', idx: indexOf(m, p).join(', '), v: fmtVal(m, m.data[p]), off: p }),
			})} />
		<p className="tr-note">{t(locale, 'tr.memLegend')}</p>
		<Challenge lesson={LESSON} n={4} text={t(locale, 'tr.ch4')} check={CHECKS.transposeSecond} hint={[t(locale, 'tr.ch4Hint')]} success={t(locale, 'tr.ch4Success')} />
	</section>;
}

function Controls({ part }: { part: 'road' | 'memory' }) {
	return part === 'road' ? <Road /> : <Memory />;
}
export const RoadControls = localized(Controls);
