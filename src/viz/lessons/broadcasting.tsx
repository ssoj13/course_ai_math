import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { roleVar } from '../palette';
import { Challenge } from '../Challenge';
import { Timeline } from '../Timeline';
import { rich } from '../rich';
import { CHECKS, LESSON, LINKS, INITIAL, model, OPS, PRESETS, shapesPatch, tintModel, type Align, type BModel, type PresetId } from './broadcasting.math';
import { fmtShape } from './tensor-ops';
import './tensor-road.css';
import './tensor-ops.css';

/**
 * Lesson 3.4 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store, the DOM-only controls (presets,
 * the right-aligned shape table, the stride-0 view readout, the timeline, challenges) and the lazy scene islands.
 * Everything three lives in broadcasting.impl.tsx; every number comes from broadcasting.math.ts (the tensor core).
 */
lessonStore(LESSON, INITIAL, LINKS);

export const RuleScene = localized(lazyIsland(() => import('./broadcasting.impl').then((m) => m.RuleScene)));
export const TintScene = localized(lazyIsland(() => import('./broadcasting.impl').then((m) => m.TintScene)));

const PRESET_NAME: Record<PresetId, Parameters<typeof t>[1]> = {
	same: 'bc.pSame', row: 'bc.pRow', col: 'bc.pCol', outer: 'bc.pOuter', scalar: 'bc.pScalar', block: 'bc.pBlock', bad: 'bc.pBad',
};

const KIND: Record<Align['kind'], Parameters<typeof t>[1]> = {
	same: 'bc.kSame', stretchA: 'bc.kStretchA', stretchB: 'bc.kStretchB', newA: 'bc.kNewA', newB: 'bc.kNewB', bad: 'bc.kBad',
};

const same = (a: readonly number[], b: readonly number[]) => a.length === b.length && a.every((n, i) => n === b[i]);

function Presets({ m }: { m: BModel }) {
	const locale = useLocale();
	return <div role="radiogroup" aria-label={t(locale, 'bc.presets')} className="tr-picks">
		{(Object.keys(PRESETS) as PresetId[]).map((id) => {
			const p = PRESETS[id];
			return <button key={id} type="button" role="radio" aria-checked={same(m.sa, p.a) && same(m.sb, p.b)}
				onClick={() => lessonStore(LESSON).getState().setMany(shapesPatch(p.a, p.b))}>{t(locale, PRESET_NAME[id])}</button>;
		})}
	</div>;
}

/** The rule as a table: shapes written right-aligned, one column per axis counted from the right, and the verdict. */
function AlignTable({ m }: { m: BModel }) {
	const locale = useLocale();
	const cell = (n: number | null, role: 'vecA' | 'vecB' | 'vecC', bad: boolean) => (
		<td className={bad ? 'is-bad' : undefined} style={bad ? undefined : { color: roleVar(role) }}>{n ?? ''}</td>
	);
	return <div className="to-scroll">
		<table className="to-table">
			<thead>
				<tr><th>{t(locale, 'bc.tAxis')}</th>{m.cols.map((c) => <th key={c.axis} scope="col">{c.axis}</th>)}</tr>
			</thead>
			<tbody>
				<tr><th scope="row">A</th>{m.cols.map((c) => <td key={c.axis} style={{ color: roleVar('vecA') }} className={c.kind === 'bad' ? 'is-bad' : undefined}>{c.a ?? ''}</td>)}</tr>
				<tr><th scope="row">B</th>{m.cols.map((c) => <td key={c.axis} style={{ color: roleVar('vecB') }} className={c.kind === 'bad' ? 'is-bad' : undefined}>{c.b ?? ''}</td>)}</tr>
				<tr><th scope="row">{t(locale, 'bc.tOut')}</th>{m.cols.map((c) => cell(c.out, 'vecC', c.kind === 'bad'))}</tr>
				<tr className="to-verdict"><th scope="row" />{m.cols.map((c) => <td key={c.axis}>{t(locale, KIND[c.kind], { a: c.a ?? 1, b: c.b ?? 1 })}</td>)}</tr>
			</tbody>
		</table>
	</div>;
}

/** What broadcasting costs in memory: the stride-0 views of A and B (core broadcastTo), or why there is no result. */
function Views({ m }: { m: BModel }) {
	const locale = useLocale();
	if (!m.ok) {
		const c = m.cols.find((x) => x.kind === 'bad')!;
		return <div className="to-err" role="status">
			<p>{rich(t(locale, 'bc.errBox', { axis: c.axis, a: c.a!, b: c.b! }))}</p>
			<p>{t(locale, 'bc.errCore')} <code>ShapeError: {m.coreMsg}</code></p>
		</div>;
	}
	const row = (name: string, o: NonNullable<BModel['opA']>) => (
		<span key={name}>{t(locale, 'bc.viewOf', { name, shape: fmtShape(o.t.shape), out: fmtShape(m.out!), strides: fmtShape(o.view.strides), k: o.repeats })}</span>
	);
	return <div className="tr-hud">
		{row('A', m.opA!)}
		{row('B', m.opB!)}
	</div>;
}

export function readout(m: BModel, locale: Locale): string {
	if (!m.ok) return t(locale, 'bc.readErr', { sa: fmtShape(m.sa), sb: fmtShape(m.sb) });
	return t(locale, 'bc.readOk', { sa: fmtShape(m.sa), op: OPS[m.op], sb: fmtShape(m.sb), out: fmtShape(m.out!), n: m.C!.size, stored: m.A.size + m.B.size });
}

function Rule() {
	const locale = useLocale();
	const m = model(useVals(LESSON));
	const steps = (['bc.step0', 'bc.step1', 'bc.step2'] as const).map((k, i) => ({ id: String(i), text: t(locale, k) }));
	return <section className="not-content tr-controls" data-to-controls="rule">
		<Timeline lesson={LESSON} id="bc" steps={steps} />
		<Presets m={m} />
		<AlignTable m={m} />
		<Views m={m} />
		<p aria-live="polite">{readout(m, locale)}</p>
		<Challenge lesson={LESSON} n={1} text={t(locale, 'bc.ch1')} check={CHECKS.column} hint={[t(locale, 'bc.ch1Hint')]} success={t(locale, 'bc.ch1Success')} />
		<Challenge lesson={LESSON} n={2} text={t(locale, 'bc.ch2')} check={CHECKS.error} hint={[t(locale, 'bc.ch2Hint')]} success={t(locale, 'bc.ch2Success')} />
		<Challenge lesson={LESSON} n={3} text={t(locale, 'bc.ch3')} check={CHECKS.outer} hint={[t(locale, 'bc.ch3Hint')]} success={t(locale, 'bc.ch3Success')} />
	</section>;
}

function Tint() {
	const locale = useLocale();
	const m = tintModel(useVals(LESSON));
	const b = m.opB!;
	return <section className="not-content tr-controls" data-to-controls="tint">
		<div className="tr-hud">
			<span>{t(locale, 'bc.viewOf', { name: t(locale, 'bc.nameTint'), shape: fmtShape(b.t.shape), out: fmtShape(m.out!), strides: fmtShape(b.view.strides), k: b.repeats })}</span>
		</div>
		<Challenge lesson={LESSON} n={4} text={t(locale, 'bc.ch4')} check={CHECKS.noRed} hint={[t(locale, 'bc.ch4Hint')]} success={t(locale, 'bc.ch4Success')} />
	</section>;
}

function Controls({ part }: { part: 'rule' | 'tint' }) {
	return part === 'rule' ? <Rule /> : <Tint />;
}
export const BcastControls = localized(Controls);
