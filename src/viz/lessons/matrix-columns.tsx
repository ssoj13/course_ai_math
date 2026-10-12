import { useStore } from 'zustand';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { hotOff, hotOn, lessonStore, useStep, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { NumGrid, type GridRow } from '../NumTable';
import { Timeline } from '../Timeline';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { alignedRows } from '../tex';
import type { Marks } from '../marks';
import {
	CHECKS, INITIAL, LESSON, LINKS, PRESETS, STEP_TRANSFER, STEPS, derive1, derive2, fmt1, fmt2, fmtV, isIdentity, tex1, tex2, type Preset,
} from './matrix-columns.math';
import type { Scene } from './matrix-columns.rows';

/**
 * Lesson 2.2 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store (with the lesson-1.6 orthonormal
 * link on the rows of R), the timeline-driven transfer, the DOM-only controls (live formulas, the products grids, the
 * Gram heatmap, ready-made frames, readouts, challenges) and the lazy scene island. Everything three lives in
 * matrix-columns.impl.tsx. Reader-facing words come from the `mc` dictionary namespace.
 */
const store = lessonStore(LESSON, INITIAL, LINKS);

/**
 * The transfer follows the timeline: entering the transfer step eases `tr` to 1, going back before it eases it to 0.
 * Only crossing that boundary writes, so a transfer the reader scrubbed by hand is not undone by other steps.
 * Client only (the server never changes steps).
 */
if (typeof window !== 'undefined') {
	store.subscribe((st, prev) => {
		const now = st.step.rv ?? 0;
		const was = prev.step.rv ?? 0;
		if (now === was) return;
		const want = now >= STEP_TRANSFER ? 1 : 0;
		if (want !== (was >= STEP_TRANSFER ? 1 : 0)) st.set('tr', want, 'ease');
	});
}

export const MatrixScene = localized(lazyIsland(() => import('./matrix-columns.impl').then((m) => m.MatrixScene)));

type Vals = Parameters<typeof derive1>[0];

const tex = String.raw;
const SUB = ['₁', '₂', '₃'];

const xy = (prefix: string, get: (v: Vals) => readonly number[]) => ({
	[prefix + 'x']: (v: Vals) => get(v)[0],
	[prefix + 'y']: (v: Vals) => get(v)[1],
});
/** `\s{key}` values of the MC-1 formula. */
const derived: Record<string, (v: Vals) => number> = {
	...xy('m1', (v) => derive1(v).m1), ...xy('m2', (v) => derive1(v).m2), ...xy('x', (v) => derive1(v).x), ...xy('y', (v) => derive1(v).y),
	y1: (v) => derive1(v).yRows[0], y2: (v) => derive1(v).yRows[1],
};
const FMT: Record<string, (x: number) => string> = {
	...Object.fromEntries(Object.keys(derived).map((k) => [k, tex1])),
	y1: tex2, y2: tex2,
};
const S = (v: Vals, key: string) => (derived[key](v) < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);

/**
 * Hover maps. MC-1: a product cell lights the column piece it belongs to and the output leg its row adds into (the
 * same number is part of both readings). MC-2: a product cell lights its row's shadow and its column's piece; a Gram
 * cell lights its two rulers.
 */
const HOT: HotLinkMap = {
	m1: ['w1'], m2: ['w2'], y1: ['y:x'], y2: ['y:y'],
	p11: ['w1', 'y:x'], p12: ['w2', 'y:x'], p21: ['w1', 'y:y'], p22: ['w2', 'y:y'],
	...Object.fromEntries([1, 2, 3].flatMap((i) => [1, 2, 3].map((j) => [`P${i}${j}`, [`rd${i}`, `cw${j}`]]))),
	...Object.fromEntries([1, 2, 3].flatMap((i) => [1, 2, 3].map((j) => [`g${i}${j}`, i === j ? [`q${i}`] : [`q${i}`, `q${j}`]]))),
	yv: ['rd1', 'rd2', 'rd3', 'cw1', 'cw2', 'cw3'],
};

// ---------------------------------------------------------------------------------------------------------------
// MC-1: formula and products.

function ColumnsFormula() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const row = (i: 1 | 2, a: string) => tex`\t{y${i}}{y_${i}}=\t{p${i}1}{\s{m1${a}}\cdot ${S(v, 'xx')}}+\t{p${i}2}{\s{m2${a}}\cdot ${S(v, 'xy')}}=\s{y${i}}`;
	const cols = tex`\t{y}{M\mathbf{x}}=\t{w1}{${S(v, 'xx')}\cdot(\s{m1x},\s{m1y})}+\t{w2}{${S(v, 'xy')}\cdot(\s{m2x},\s{m2y})}`;
	const out = tex`=(\s{yx},\s{yy})`;
	const col = (id: string, p: string) => tex`\t{${id}}{\begin{matrix}\s{${p}x}\\ \s{${p}y}\end{matrix}}`;
	const mat = tex`M=\left[\,${col('m1', 'm1')}\quad${col('m2', 'm2')}\,\right]`;
	return <Formula lesson={LESSON} derived={derived} fmt={FMT} block label={t(locale, 'mc.formula1')}
		tex={alignedRows([['f1-mat', mat], ['f1-r1', row(1, 'x')], ['f1-r2', row(2, 'y')], ['f1-cols', cols + out]])}
		texNarrow={alignedRows([['f1-mat', mat], ['f1-r1', row(1, 'x')], ['f1-r2', row(2, 'y')], ['f1-cols', cols], ['f1-out', tex`\quad${out}`]])} />;
}

/** The four products mᵢⱼxⱼ: a row adds up to yᵢ (row view), a column is xⱼ·colⱼ (column view). */
function Products1() {
	const locale = useLocale();
	const z = derive1(useVals(LESSON));
	const rows: GridRow[] = [0, 1].map((i) => ({
		head: 'row' + SUB[i],
		cells: { a: { id: `p${i + 1}1`, value: z.prods[i][0] }, b: { id: `p${i + 1}2`, value: z.prods[i][1] }, s: { id: `y${i + 1}`, value: z.yRows[i] } },
	}));
	return <NumGrid lesson={LESSON} caption={t(locale, 'mc.prodCaption1')} corner="" rows={rows} fmt={fmt2}
		cols={[{ id: 'a', label: 'mᵢ₁·x₁' }, { id: 'b', label: 'mᵢ₂·x₂' }, { id: 's', label: 'Σ = yᵢ' }]} />;
}

// ---------------------------------------------------------------------------------------------------------------
// MC-2: products, Gram heatmap, ready-made frames.

const COLS3 = [{ id: '1', label: '·v₁' }, { id: '2', label: '·v₂' }, { id: '3', label: '·v₃' }, { id: 's', label: 'Σ = uᵢ·v' }];

function Products3() {
	const locale = useLocale();
	const z = derive2(useVals(LESSON));
	const step = useStep(LESSON, 'rv');
	const rows: GridRow[] = [0, 1, 2].map((i) => ({
		head: 'u' + SUB[i],
		cells: {
			...Object.fromEntries([0, 1, 2].map((j) => [String(j + 1), { id: `P${i + 1}${j + 1}`, value: z.prods[i][j] }])),
			s: { id: `rd${i + 1}`, value: z.y[i] },
		},
	}));
	// Rows 1..3 of the timeline: the row being read stands out, the others are dimmed.
	const marks: Marks = step >= 1 && step <= 3
		? Object.fromEntries([1, 2, 3].filter((i) => i !== step).flatMap((i) => [1, 2, 3].map((j) => [`P${i}${j}`, { dim: true }]).concat([[`rd${i}`, { dim: true }]])))
		: {};
	return <NumGrid lesson={LESSON} caption={t(locale, 'mc.prodCaption3')} corner="" rows={rows} cols={COLS3} fmt={fmt2} marks={marks} />;
}

/**
 * The Gram matrix G = RRᵀ (gᵢⱼ = uᵢ·uⱼ) as a heatmap: each cell is tinted by how far it is from the identity's entry
 * (0 = untinted), so an orthonormal frame shows a clean table and every leak between rulers glows. Cells hover-link
 * to their rulers like any table cell.
 */
function GramHeat() {
	const locale = useLocale();
	const z = derive2(useVals(LESSON));
	const hot = useStore(store, (st) => st.hot);
	const ok = isIdentity(z.g);
	return <div className="lab-num-grid not-content" data-mc-gram="">
		<p className="lab-num-grid-caption" style={{ margin: '.4rem 0' }}>{t(locale, ok ? 'mc.gramOk' : 'mc.gramOff')}</p>
		<table className="lab-num-table" aria-label={t(locale, 'mc.gramCaption')}>
			<thead><tr><th scope="col">·</th>{[0, 1, 2].map((j) => <th key={j} scope="col">{'u' + SUB[j]}</th>)}</tr></thead>
			<tbody>{z.g.map((r, i) => <tr key={i}><th scope="row">{'u' + SUB[i]}</th>{r.map((x, j) => {
				const id = `g${i + 1}${j + 1}`;
				const off = Math.min(1, Math.abs(x - (i === j ? 1 : 0)));
				// Tint strength = distance from the identity entry, capped at 1 (a leak of 1 or more is fully lit).
				const bg = off < 1e-6 ? undefined : `color-mix(in srgb, var(--viz-hot) ${Math.round(12 + 48 * off)}%, transparent)`;
				return <td key={j} data-t={id} tabIndex={0} className={hot.has(id) ? 'is-hot' : ''} style={{ background: bg }}
					onPointerEnter={() => hotOn(LESSON, id)} onPointerLeave={() => hotOff(LESSON, id)}
					onFocus={() => hotOn(LESSON, id)} onBlur={() => hotOff(LESSON, id)}>{fmt2(x)}</td>;
			})}</tr>)}</tbody>
		</table>
	</div>;
}

const PRESET_NAMES: Readonly<Record<Preset, Parameters<typeof t>[1]>> = {
	ortho: 'mc.presetOrtho', oblique: 'mc.presetOblique', unitOblique: 'mc.presetUnitOblique', mirror: 'mc.presetMirror', duplicate: 'mc.presetDuplicate',
};

function Presets() {
	const locale = useLocale();
	return <div data-mc-presets="" style={{ display: 'flex', flexWrap: 'wrap', gap: '.4rem', alignItems: 'center', margin: '.5rem 0' }}>
		<span>{t(locale, 'mc.presetLabel')}</span>
		{(Object.keys(PRESET_NAMES) as Preset[]).map((k) => <button key={k} type="button" className="lab-button"
			onClick={() => store.getState().setMany(PRESETS[k])}>{t(locale, PRESET_NAMES[k])}</button>)}
	</div>;
}

// ---------------------------------------------------------------------------------------------------------------
// Timeline, readouts and challenges.

const STEP_KEYS = ['mc.step0', 'mc.step1', 'mc.step2', 'mc.step3', 'mc.step4', 'mc.step5'] as const;
const STEP_FOCUS = ['q1', 'rd1', 'rd2', 'rd3', 'yv', 'cw1'];
if (STEP_KEYS.length !== STEPS) throw new Error('mcols: one timeline text per step');

function RowsTimeline() {
	const locale = useLocale();
	const z = derive2(useVals(LESSON));
	const vars = { v: fmtV(z.v), r1: fmt2(z.y[0]), r2: fmt2(z.y[1]), r3: fmt2(z.y[2]), y: fmtV(z.y) };
	const steps = STEP_KEYS.map((key, i) => ({ id: 's' + i, text: t(locale, key, vars), focus: STEP_FOCUS[i] }));
	return <Timeline lesson={LESSON} id="rv" steps={steps} />;
}

/** The live sentence under a scene and every sentence it can produce (StableText reserves the tallest). */
export function readouts(scene: Scene, locale: Locale, v: Vals): { text: string; variants: string[] } {
	if (scene === 1) {
		const z = derive1(v);
		const text = t(locale, 'mc.read1', { y1: fmt2(z.yRows[0]), y2: fmt2(z.yRows[1]), x1: fmt1(z.x[0]), x2: fmt1(z.x[1]), y: fmtV(z.y) });
		return { text, variants: [text] };
	}
	const z = derive2(v);
	const g = { y: fmtV(z.y), c: z.c ? fmtV(z.c) : '–', gap: fmt2(z.gap) };
	const variants = [t(locale, 'mc.read2Rot', g), t(locale, 'mc.read2Mirror', g), t(locale, 'mc.read2Bent', g), t(locale, 'mc.read2Flat', g)];
	const text = z.c === null ? variants[3] : z.orthonormal ? (z.vol > 0 ? variants[0] : variants[1]) : variants[2];
	return { text, variants };
}

function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'mc.ch1'), check: CHECKS.column2, hint: [t(locale, 'mc.ch1Hint')], success: t(locale, 'mc.ch1Success') },
			{ n: 2, text: t(locale, 'mc.ch2'), check: CHECKS.lost, hint: [t(locale, 'mc.ch2Hint')], success: t(locale, 'mc.ch2Success') },
		];
	}
	return [
		{ n: 3, text: t(locale, 'mc.ch3'), check: CHECKS.ghostMiss, hint: [t(locale, 'mc.ch3Hint')], success: t(locale, 'mc.ch3Success') },
		{ n: 4, text: t(locale, 'mc.ch4'), check: CHECKS.mirror, hint: [t(locale, 'mc.ch4Hint')], success: t(locale, 'mc.ch4Success') },
	];
}

function StableText({ text, variants }: { text: string; variants: readonly string[] }) {
	return <div data-mc-readout="" style={{ display: 'grid' }}>
		{variants.map((x, i) => <p key={i} aria-hidden="true" style={{ gridArea: '1 / 1', visibility: 'hidden', margin: 0 }}>{x}</p>)}
		<p aria-live="polite" style={{ gridArea: '1 / 1', margin: 0 }}>{text}</p>
	</div>;
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	return <section className="not-content" data-mc-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		{scene === 1 && <><ColumnsFormula /><Products1 /></>}
		{scene === 2 && <><RowsTimeline /><Presets /><Products3 /><GramHeat /></>}
		<StableText {...readouts(scene, locale, v)} />
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const MatrixControls = localized(Controls);
