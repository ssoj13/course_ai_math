/**
 * Einsum cards entry (SSR-safe): the three stores (playground, matrix x vector, ladder), their DOM controls (preset
 * buttons, extent fields, timelines, dive / back-up buttons, the row-vs-column readout) and the lazy scene islands.
 * Words come from the `du` namespace.
 */
import { useEffect, useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { useStore } from 'zustand';
import { offsets, transpose } from '../../core';
import { hotOff, hotOn, lessonStore, useStep, useVals } from '../store';
import { idxId } from './layout';
import { lazyIsland } from '../island';
import { Timeline } from '../Timeline';
import { FieldCell, type Row } from '../scene/NPanel';
import { EXT, extKey, LAD_INIT, LADDER, letters, MATVEC, mvData, MV_INIT, PLAY_IDS, playData, playLesson, presetOf } from './einsum.store';
import { columnView, fmtShort, PRESETS, type PresetId } from './model';

lessonStore(MATVEC, MV_INIT);
lessonStore(LADDER, LAD_INIT);

export const PlaygroundScene = localized(lazyIsland(() => import('./einsum.impl').then((m) => m.PlaygroundCard)));
export const MatvecScene = localized(lazyIsland(() => import('./einsum.impl').then((m) => m.MatvecCard)));
export const ColumnScene = localized(lazyIsland(() => import('./einsum.impl').then((m) => m.ColumnCard)));
export const LadderScene = localized(lazyIsland(() => import('./einsum.impl').then((m) => m.LadderCard)));

const PASS_KEYS = ['du.pass0', 'du.pass1', 'du.pass2', 'du.pass3', 'du.pass4'] as const;
const CV_KEYS = ['du.cv0', 'du.cv1', 'du.cv2', 'du.cv3'] as const;
const RUNG_KEYS = ['du.rung0', 'du.rung1', 'du.rung2', 'du.rung3', 'du.rung4'] as const;
const PRESET_KEYS: Record<PresetId, Parameters<typeof t>[1]> = {
	dot: 'du.presetDot', matvec: 'du.presetMatvec', matmul: 'du.presetMatmul', batched: 'du.presetBatched', attention: 'du.presetAttention', layer: 'du.presetLayer',
};

/**
 * Escape closes the dive (playground) or climbs one rung (ladder) while focus is in that card or its controls, and only
 * in the normal size mode (Escape there belongs to wide/full mode exit).
 */
function useEscape(scope: string, act: () => void) {
	useEffect(() => {
		const key = (e: KeyboardEvent) => {
			if (e.key !== 'Escape' || e.defaultPrevented || document.fullscreenElement) return;
			const el = document.activeElement;
			const card = el?.closest<HTMLElement>('.lab-vp-card');
			if (card && card.dataset.size !== 'normal') return;
			if (!el?.closest(`[data-size-id^="${scope}:"], [data-du-scope="${scope}"]`)) return;
			act();
		};
		window.addEventListener('keydown', key);
		return () => window.removeEventListener('keydown', key);
	}, [scope, act]);
}

function PresetButtons({ lesson }: { lesson: string }) {
	const locale = useLocale(), v = useVals(lesson), cur = presetOf(v);
	return <div role="radiogroup" aria-label={t(locale, 'du.presets')} className="lab-du-presets" style={{ display: 'flex', flexWrap: 'wrap', gap: '.4rem' }}>
		{PLAY_IDS.map((id, i) => (
			<button key={id} type="button" role="radio" aria-checked={cur === id} style={{ font: 'inherit', padding: '.2rem .6rem', fontWeight: cur === id ? 700 : 400 }}
				onClick={() => lessonStore(lesson).getState().setMany({ preset: i, sel: 0, dive: 0 })}>
				<code>{PRESETS[id].op}</code> {t(locale, PRESET_KEYS[id])}
			</button>
		))}
	</div>;
}

function Extents({ lesson }: { lesson: string }) {
	const locale = useLocale(), id = presetOf(useVals(lesson));
	const rows = useMemo<Row[]>(() => letters(id).map((l) => ({ id: 'ext_' + l, label: l, kind: 'num', key: extKey(id, l), spec: { ...EXT }, fmt: (x) => String(Math.round(x)), tip: { text: t(locale, 'du.extTip', { l }) } })), [id, locale]);
	return <div className="not-content" aria-label={t(locale, 'du.extents')} style={{ display: 'flex', flexWrap: 'wrap', gap: '.4rem' }}>
		{rows.map((r) => <FieldCell key={r.key} lesson={lesson} row={r} size="sm" />)}
	</div>;
}

function DiveBar({ lesson }: { lesson: string }) {
	const locale = useLocale(), v = useVals(lesson), b = playData(v), dive = (v.dive as number) === 1;
	const st = lessonStore(lesson).getState();
	const back = () => st.set('dive', 0);
	useEscape(lesson, back);
	if (!b.ok) return <p>{t(locale, 'du.tooLarge')}</p>;
	const c = Math.min(b.d.L.cells.n - 1, Math.max(0, Math.round(v.sel as number)));
	const rank = b.d.L.plan.kept.length;
	const cell = '(' + Array.from(b.d.L.cells.idx.subarray(c * rank, c * rank + rank)).join(', ') + ')';
	const sel: Row = { id: 'sel', label: t(locale, 'du.selCell'), kind: 'num', key: 'sel', spec: { min: 0, max: Math.max(1, b.d.L.cells.n - 1), step: 1 }, fmt: (x) => String(Math.round(x)), tip: { text: t(locale, 'du.selTip') } };
	return <div style={{ display: 'flex', flexWrap: 'wrap', gap: '.6rem', alignItems: 'center' }}>
		{!dive && b.d.L.cells.n > 1 && <FieldCell lesson={lesson} row={sel} size="sm" />}
		{dive
			? <button type="button" onClick={back}>{t(locale, 'du.backUp')}</button>
			: <button type="button" onClick={() => st.set('dive', 1)}>{t(locale, 'du.diveBtn', { cell: rank ? cell : '()' })}</button>}
		<span aria-live="polite">{dive ? t(locale, 'du.diveText', { cell: rank ? cell : '()', v: fmtShort(b.d.out[c]) }) : ''}</span>
	</div>;
}

/**
 * Memory layout (spec §1.6): each operand as its flat buffer, in buffer order, with its shape and strides from the core.
 * Hovering a buffer cell lights its element in the scene (and the scene lights the strip). `Aᵀ` shows A through a
 * transposed VIEW: the same cells in the same order, other strides, so the traversal numbers under the cells change
 * while nothing moves. At most MEM_MAX cells per operand are listed.
 */
const MEM_MAX = 48;
function MemoryStrip({ lesson }: { lesson: string }) {
	const locale = useLocale(), v = useVals(lesson), b = playData(v), hot = useStore(lessonStore(lesson), (st) => st.hot);
	if (!b.ok) return null;
	const d = b.d, wide = d.wide, tA = v.tA === 1;
	return <div className="lab-du-mem" aria-label={t(locale, 'du.memory')}>
		<label style={{ display: 'inline-flex', gap: '.4rem', alignItems: 'center' }}>
			<input type="checkbox" checked={tA} onChange={(e) => lessonStore(lesson).getState().set('tA', e.currentTarget.checked ? 1 : 0)} />
			{t(locale, 'du.transpose')}
		</label>
		{([0, 1] as const).map((o) => {
			const base = d.ts[o], view = o === 0 && tA ? transpose(base) : base;
			const name = (o === 0 ? 'A' : 'B') + (o === 0 && tA ? 'ᵀ' : '');
			// Traversal step at which a row-major walk over the view reaches each buffer position.
			const order = new Map<number, number>();
			Array.from(offsets(view.shape, view.strides)).forEach((off, k) => order.set(view.offset + off, k));
			const n = Math.min(base.data.length, MEM_MAX);
			return <div key={o} style={{ margin: '.4rem 0' }}>
				<div style={{ fontFamily: 'var(--lab-font-mono)', fontSize: '.8rem' }}>
					{t(locale, 'du.stridesOf', { name, shape: '(' + view.shape.join(', ') + ')', strides: '(' + view.strides.join(', ') + ')' })}
				</div>
				<div style={{ display: 'flex', flexWrap: 'wrap', gap: 2 }}>
					{Array.from({ length: n }, (_, k) => {
						// The element stored at buffer position k, as a multi-index of the ORIGINAL operand (its scene id).
						let r = k;
						const idx = base.strides.map((st) => { const q = Math.floor(r / st); r -= q * st; return q; });
						const id = (o === 0 ? 'a' : 'b') + idxId(idx, 0, idx.length, wide);
						return <span key={k} data-t={id} tabIndex={0} className={hot.has(id) ? 'is-hot' : undefined}
							onPointerEnter={() => hotOn(lesson, id)} onPointerLeave={() => hotOff(lesson, id)} onFocus={() => hotOn(lesson, id)} onBlur={() => hotOff(lesson, id)}
							style={{ display: 'inline-grid', minWidth: '2.2em', textAlign: 'center', fontFamily: 'var(--lab-font-mono)', fontSize: '.75rem', border: '1px solid var(--sl-color-gray-5)', padding: '1px 2px', background: hot.has(id) ? 'var(--viz-hot-bg)' : undefined }}>
							<span>{fmtShort(base.data[k])}</span>
							<span style={{ opacity: 0.6 }}>{order.get(k)}</span>
						</span>;
					})}
					{base.data.length > n && <span>{t(locale, 'du.memMore', { k: String(base.data.length - n) })}</span>}
				</div>
			</div>;
		})}
	</div>;
}

function Play({ preset }: { preset?: PresetId }) {
	const locale = useLocale(), lesson = playLesson(preset);
	return <section className="not-content" data-du-scope={lesson}>
		<PresetButtons lesson={lesson} />
		<Extents lesson={lesson} />
		<Timeline lesson={lesson} id="pass" steps={PASS_KEYS.map((k, i) => ({ id: String(i), text: t(locale, k) }))} />
		<DiveBar lesson={lesson} />
		<MemoryStrip lesson={lesson} />
	</section>;
}

/** Both views' y side by side: one from the row view's stacks, one from the column view's head-to-tail sum. */
function MvReadout() {
	const locale = useLocale(), b = mvData(useVals(MATVEC));
	if (!b.ok) return null;
	const row = Array.from(b.d.out).map(fmtShort).join(', ');
	const col = columnView(b.d.L.plan, b.d.ts).y.map(fmtShort).join(', ');
	return <p aria-live="polite">{t(locale, 'du.readoutMv', { row: '(' + row + ')', col: '(' + col + ')' })}</p>;
}

function Matvec({ view }: { view: 'rows' | 'columns' }) {
	const locale = useLocale();
	return <section className="not-content" data-du-scope={MATVEC}>
		{view === 'rows'
			? <Timeline lesson={MATVEC} id="pass" steps={PASS_KEYS.map((k, i) => ({ id: String(i), text: t(locale, k) }))} />
			: <Timeline lesson={MATVEC} id="cv" steps={CV_KEYS.map((k, i) => ({ id: String(i), text: t(locale, k) }))} />}
		<MvReadout />
	</section>;
}

function Ladder() {
	const locale = useLocale(), step = useStep(LADDER, 'ladder');
	const climb = () => {
		const st = lessonStore(LADDER).getState(), next = Math.min(RUNG_KEYS.length - 1, (st.step.ladder ?? 0) + 1);
		if (next >= 2) st.setMany({ ...LAD_INIT });
		st.setStep('ladder', next);
	};
	useEscape(LADDER, climb);
	return <section className="not-content" data-du-scope={LADDER}>
		<Timeline lesson={LADDER} id="ladder" steps={RUNG_KEYS.map((k, i) => ({ id: String(i), text: t(locale, k) }))} />
		<button type="button" aria-disabled={step >= RUNG_KEYS.length - 1} onClick={() => step < RUNG_KEYS.length - 1 && climb()}>{t(locale, 'du.backUp')}</button>
	</section>;
}

export type EinsumKind = 'playground' | 'rows' | 'columns' | 'ladder';
/** `preset` = the playground's starting preset (other kinds ignore it). */
function Controls({ kind, preset }: { kind: EinsumKind; preset?: PresetId; locale?: Locale }) {
	if (kind === 'playground') return <Play preset={preset} />;
	if (kind === 'ladder') return <Ladder />;
	return <Matvec view={kind} />;
}
export const EinsumControls = localized(Controls);
