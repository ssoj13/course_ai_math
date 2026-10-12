import { t } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Challenge } from '../Challenge';
import { isContiguous } from '../../core';
import { MemStrip } from './MemStrip';
import { fmtShape } from './tensor-cells';
import { BUF, CHECKS, chain, INITIAL, LESSON, LINE_BYTES, LINKS, N, OPS, RESHAPES, runCache, selected, SHAPES, walkOf } from './tensor-memory.math';
import './tensor-cells.css';
import './tensor-memory.css';

/**
 * Lesson 3.3 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store, the DOM-only controls under
 * each card (code buttons, the chain as Python, shape/strides/offset readouts, the buffer strips, challenges) and the
 * lazy scene islands (three lives in tensor-memory.impl.tsx). Words: the `mem` namespace; numbers: tensor-memory.math.ts.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const ViewScene = localized(lazyIsland(() => import('./tensor-memory.impl').then((m) => m.ViewScene)));
export const CacheScene = localized(lazyIsland(() => import('./tensor-memory.impl').then((m) => m.CacheScene)));

const set = (patch: Record<string, number>) => lessonStore(LESSON).getState().setMany(patch);

/** A row of mutually exclusive code buttons writing one store key. */
function Picks({ label, keyName, value, items }: { label: string; keyName: string; value: number; items: readonly string[] }) {
	return <div role="radiogroup" aria-label={label} className="mem-picks">
		<span className="mem-pick-label" aria-hidden="true">{label}</span>
		{items.map((text, i) => (
			<button key={i} type="button" role="radio" aria-checked={i === value} onClick={() => set({ [keyName]: i })}><code>{text}</code></button>
		))}
	</div>;
}

const reshapeCode = (s: readonly number[] | null) => (s ? `.reshape(${s.join(', ')})` : '—');

/** Under 3.3-1: choose x, the view and the reshape; read the chain, the strides, the address and both buffers. */
function View() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const c = chain(v);
	const sel = selected(c.res, v.sel as number);
	const onX = walkOf(c.onX);
	const yes = (b: boolean) => t(locale, b ? 'mem.yes' : 'mem.no');
	const terms = sel.idx.map((i, a) => `${i}·${c.res.strides[a]}`);
	return <section className="not-content mem-controls">
		<Picks label={t(locale, 'mem.pickShape')} keyName="shp" value={v.shp as number} items={SHAPES.map(fmtShape)} />
		<Picks label={t(locale, 'mem.pickView')} keyName="op" value={v.op as number} items={OPS.map((o) => o.code.replace('y = ', ''))} />
		<Picks label={t(locale, 'mem.pickReshape')} keyName="re" value={v.re as number} items={RESHAPES.map(reshapeCode)} />
		<pre className="ts-box mem-code"><code>{c.code.join('\n')}</code></pre>
		<table className="mem-table">
			<thead><tr><th></th><th>{t(locale, 'mem.colShape')}</th><th>{t(locale, 'mem.colStrides')}</th><th>{t(locale, 'mem.colOffset')}</th><th>{t(locale, 'mem.colContig')}</th><th>{t(locale, 'mem.colShares')}</th></tr></thead>
			<tbody>
				<tr><th>x</th><td>{fmtShape(c.x.shape)}</td><td>{fmtShape(c.x.strides)}</td><td>{c.x.offset}</td><td>{yes(isContiguous(c.x))}</td><td>—</td></tr>
				<tr><th>y</th><td>{fmtShape(c.res.shape)}</td><td>{fmtShape(c.res.strides)}</td><td>{c.res.offset}</td><td>{yes(isContiguous(c.res))}</td><td>{yes(c.res.data === c.x.data)}</td></tr>
			</tbody>
		</table>
		{c.refused && <p className="mem-refused" role="status">{t(locale, c.refused === 'size' ? 'mem.refusedSize' : 'mem.refusedContig', { n: c.res.size })}</p>}
		<p className="ts-box" aria-live="polite">
			y[{sel.idx.join(', ')}] → {t(locale, 'mem.address')} = {c.res.offset}{terms.map((s) => ' + ' + s).join('')} = <b>{sel.addr}</b>
			{' '}{t(locale, c.copied ? 'mem.inCopy' : 'mem.inX')} · {t(locale, 'mem.value')} {sel.value}
		</p>
		<MemStrip lesson={LESSON} label={t(locale, 'mem.stripX')} n={BUF} center={c.copied ? 12 : sel.addr} sel={c.copied ? null : sel.addr}
			more={(k) => t(locale, 'mem.more', { k })}
			cell={(p) => ({
				text: String(c.x.data[p]), step: onX.step[p] >= 0 ? onX.step[p] : null, dim: onX.step[p] < 0, hot: 'a' + p,
				label: onX.step[p] >= 0 ? t(locale, 'mem.hintReached', { p, k: onX.step[p] }) : t(locale, 'mem.hintSkipped', { p }),
			})} />
		{c.copied && <MemStrip lesson={LESSON} label={t(locale, 'mem.stripCopy')} n={c.res.size} center={sel.addr} sel={sel.addr}
			more={(k) => t(locale, 'mem.more', { k })}
			onSelect={(q) => set({ sel: q })}
			cell={(q) => ({ text: String(c.res.data[q]), step: q, hot: 'a' + c.res.data[q], label: t(locale, 'mem.hintCopy', { q, v: c.res.data[q] }) })} />}
		<p className="mem-note">{t(locale, c.copied ? 'mem.noteCopy' : 'mem.noteView')}</p>
		<Challenge lesson={LESSON} n={1} text={t(locale, 'mem.ch1')} check={CHECKS.copyThenView} hint={[t(locale, 'mem.ch1Hint')]} success={t(locale, 'mem.ch1Success')} />
		<Challenge lesson={LESSON} n={2} text={t(locale, 'mem.ch2')} check={CHECKS.negativeStride} hint={[t(locale, 'mem.ch2Hint')]} success={t(locale, 'mem.ch2Success')} />
		<Challenge lesson={LESSON} n={3} text={t(locale, 'mem.ch3')} check={CHECKS.fThenT} hint={[t(locale, 'mem.ch3Hint')]} success={t(locale, 'mem.ch3Success')} />
	</section>;
}

/** Under 3.3-2: the whole walk's misses for both orders at this cache size, and a task. */
function Cache() {
	const locale = useLocale();
	const lines = Math.round(useVals(LESSON).lines as number);
	const rowMiss = runCache(false, lines).misses[N * N - 1], colMiss = runCache(true, lines).misses[N * N - 1];
	return <section className="not-content mem-controls">
		<table className="mem-table">
			<thead><tr><th>{t(locale, 'mem.colWalk')}</th><th>{t(locale, 'mem.colMisses')}</th><th>{t(locale, 'mem.colBytes')}</th></tr></thead>
			<tbody>
				<tr><th>{t(locale, 'mem.walkRow')}</th><td>{rowMiss}</td><td>{rowMiss * LINE_BYTES}</td></tr>
				<tr><th>{t(locale, 'mem.walkCol')}</th><td>{colMiss}</td><td>{colMiss * LINE_BYTES}</td></tr>
			</tbody>
		</table>
		<p className="mem-note">{t(locale, 'mem.noteCache', { n: lines })}</p>
		<Challenge lesson={LESSON} n={4} text={t(locale, 'mem.ch4')} check={CHECKS.columnFits} hint={[t(locale, 'mem.ch4Hint')]} success={t(locale, 'mem.ch4Success')} />
	</section>;
}

function Controls({ part }: { part: 'view' | 'cache' }) {
	return part === 'view' ? <View /> : <Cache />;
}
export const MemControls = localized(Controls);
