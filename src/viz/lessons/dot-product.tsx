import { useEffect, useState } from 'react';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { hotOff, hotOn, lessonStore, useHot, useStep, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Timeline } from '../Timeline';
import { Challenge, type ChallengeProps } from '../Challenge';
import { CodeView } from '../CodeView';
import { NumGrid, type GridRow } from '../NumTable';
import { FieldCell } from '../scene/NPanel';
import { reducedMotion } from '../scene/motion';
import type { Marks } from '../marks';
import { COMP_COLS, INITIAL, LESSON, LINKS, STRIKE_GAP_MS, derive, derive3, lamp, fmt, fmtCos, fmtDeg, newestColumn, strikeTarget, tex as texFmt } from './dot-product.math';
import { facesFor, type Scene } from './dot-product.rows';

/**
 * Lesson 1.4 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store, the DOM-only controls
 * (formulas, the derivation timeline, component tables, the three-faces strip, code, challenges, readouts) and the
 * lazy scene island. Everything three lives in dot-product.impl.tsx. All reader-facing words come from the `dp`
 * dictionary namespace; numbers come from dot-product.math.ts, the one model the scenes draw.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const DotScene = localized(lazyIsland(() => import('./dot-product.impl').then((m) => m.DotScene)));

type Vals = Parameters<typeof derive>[0];

const tex = String.raw;
/** A formula row carrying a mark id (`data-m`), so it can be hidden / dimmed by step. */
const row = (key: string, body: string) => tex`\htmlData{m=${key}}{${body}}`;
const aligned = (rows: string[]) => tex`\begin{aligned}${rows.map((x) => '&' + x).join(tex`\\ `)}\end{aligned}`;
const A = tex`\mathbf{a}`, B = tex`\mathbf{b}`;
const abs = (x: string) => tex`\lvert ${x}\rvert`;

/** A live value after a minus or a dot is parenthesised when negative ("3·(−1)", not "3·−1"). */
const signed = (key: string, value: number) => (value < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);

// ---------------------------------------------------------------------------------------------------------------
// Derived values a formula may show as `\s{key}` (one model: derive / derive3 / lamp).

const D2 = ['dot', 'rhs', 'rect', 'na', 'nb', 'cosT', 'th', 'shadow', 'na2', 'nb2', 'namb2', 'a1', 'a2', 'b1', 'b2'] as const;
const D3 = { dot3: 'dot', rhs3: 'rhs', rect3: 'rect', th3: 'th' } as const;
const derived = {
	...Object.fromEntries(D2.map((k) => [k, (v: Vals) => derive(v)[k]])),
	...Object.fromEntries(Object.entries(D3).map(([k, f]) => [k, (v: Vals) => derive3(v)[f]])),
	...Object.fromEntries([0, 1, 2].flatMap((i) => [
		[`a3${i + 1}`, (v: Vals) => (v.a3 as number[])[i]],
		[`b3${i + 1}`, (v: Vals) => (v.b3 as number[])[i]],
	])),
	ndotl: (v: Vals) => lamp(v).ndotl,
	brightNL: (v: Vals) => lamp(v).brightNL,
	thNL: (v: Vals) => lamp(v).thNL,
};
const formats: Record<string, (x: number) => string> = Object.fromEntries(
	Object.keys(derived).map((k) => [k, k === 'cosT' || k === 'ndotl' || k === 'brightNL' ? texFmt.cos : k.startsWith('th') ? texFmt.deg : texFmt.num]),
);

/**
 * DP-1 formula marks for a step and the cancellation state (spec DP-1 timeline table): line reveals, `dim` for lines
 * the story has moved past, the |a|² pair struck first and the |b|² pair after it, and at step 7 the struck terms of
 * line 3 hidden. Strike marks wrap only line 3's occurrences, so lines 1-2 keep their terms.
 */
export function marksFor(step: number, strike: number): Marks {
	return {
		l1: { hidden: step < 2, dim: step >= 5 },
		l2: { hidden: step < 3, dim: step >= 5 },
		l3: { hidden: step < 5, dim: step >= 7 },
		l4: { hidden: step < 7, dim: step >= 8 },
		l5: { hidden: step < 7 },
		'l5-rect': { hidden: step < 8 },
		'x-na2l': { struck: strike >= 1, hidden: step >= 7 },
		'x-na2r': { struck: strike >= 1, hidden: step >= 7 },
		'x-nb2l': { struck: strike >= 2, hidden: step >= 7 },
		'x-nb2r': { struck: strike >= 2, hidden: step >= 7 },
		// The table's sum cells of |a|² and |b|² are struck with their formula terms.
		na2: { struck: strike >= 1 },
		nb2: { struck: strike >= 2 },
	};
}

/** The DP-1 derivation, five lines (spec DP-1 "Formula"); `narrow` splits the long lines for < 420 px. */
function deriveTex(narrow: boolean): string {
	const l1 = [tex`\t{namb2}{${abs(A + '-' + B)}^2}=\t{na2_r}{${abs(A)}^2}+\t{nb2_r}{${abs(B)}^2}-\t{corr}{2\,${abs(A)}\,${abs(B)}\cos\theta}`];
	const l2 = narrow
		? [tex`\t{namb2}{${abs(A + '-' + B)}^2}=\t{expand}{\sum_i (a_i-b_i)^2}`, tex`\quad=\t{na2_l}{\sum_i a_i^2}-\t{cross}{2\sum_i a_i b_i}+\t{nb2_l}{\sum_i b_i^2}`]
		: [tex`\t{namb2}{${abs(A + '-' + B)}^2}=\t{expand}{\sum_i (a_i-b_i)^2}=\t{na2_l}{\sum_i a_i^2}-\t{cross}{2\sum_i a_i b_i}+\t{nb2_l}{\sum_i b_i^2}`];
	const left = tex`\htmlData{m=x-na2l}{\t{na2_l}{${abs(A)}^2}}\htmlData{m=x-nb2l}{{}+\t{nb2_l}{${abs(B)}^2}}-\t{cross}{2\sum_i a_i b_i}`;
	const right = tex`\htmlData{m=x-na2r}{\t{na2_r}{${abs(A)}^2}}\htmlData{m=x-nb2r}{{}+\t{nb2_r}{${abs(B)}^2}}-\t{corr}{2\,${abs(A)}\,${abs(B)}\cos\theta}`;
	const l3 = narrow ? [left, tex`\quad=${right}`] : [tex`${left}=${right}`];
	const l4 = [tex`\t{sum}{\sum_i a_i b_i}=\t{rhs}{${abs(A)}\,${abs(B)}\cos\theta}`];
	const dot = tex`\t{dot}{${A}\cdot ${B}}:=\t{sum}{\sum_i a_i b_i}=\s{dot}`;
	const rhs = tex`\t{rhs}{${abs(A)}\,${abs(B)}\cos\theta}=\s{rhs}`;
	const rect = tex`\htmlData{m=l5-rect}{\t{rect}{${abs(A)}\cdot(${abs(B)}\cos\theta)}=\s{rect}}`;
	const l5 = narrow ? [dot, rhs, rect] : [tex`${dot}\qquad ${rhs}\qquad ${rect}`];
	return aligned([...l1.map((x) => row('l1', x)), ...l2.map((x) => row('l2', x)), ...l3.map((x) => row('l3', x)), ...l4.map((x) => row('l4', x)), ...l5.map((x) => row('l5', x))]);
}

/**
 * The formula of a scene. `compact` is the viewport's formula plate: DP-1 shows the three squared sides (step-neutral,
 * so the plate never gives the derivation away), the others their headline number.
 */
export function DotFormula({ scene, compact = false }: { scene: Scene; compact?: boolean }) {
	const locale = useLocale(), v = useVals(LESSON), step = useStep(LESSON, 'derive');
	const common = { lesson: LESSON, derived, fmt: formats };
	if (scene === 1) {
		if (compact) {
			return <Formula {...common} label={t(locale, 'dp.sidesFormula')}
				tex={aligned([tex`\t{namb2}{${abs(A + '-' + B)}^2}=\s{namb2},\;\t{th}{\theta=\s{th}}`, tex`\t{na2}{${abs(A)}^2}=\s{na2},\;\t{nb2}{${abs(B)}^2}=\s{nb2}`])} />;
		}
		return <Formula {...common} block label={t(locale, 'dp.derivationFormula')} tex={deriveTex(false)} texNarrow={deriveTex(true)}
			marks={marksFor(step, v.strike as number)}
			tips={{
				corr: { text: t(locale, 'dp.corrTip'), tex: tex`2\,${abs(A)}\,${abs(B)}\cos\theta` },
				cross: { text: t(locale, 'dp.crossTip'), tex: tex`2\sum_i a_i b_i` },
				expand: { text: t(locale, 'dp.expandTip'), tex: tex`(a_i-b_i)^2=a_i^2-2a_ib_i+b_i^2` },
			}} />;
	}
	if (scene === 2) {
		const z = derive(v);
		if (compact) return <Formula {...common} label={t(locale, 'dp.signFormula')} tex={tex`\t{dot}{${A}\cdot ${B}}=\s{dot},\;\t{th}{\theta=\s{th}}`} />;
		// Each route as [definition = live inputs, = result]; phones put the result on its own line.
		const routes = [
			[tex`\t{dot}{${A}\cdot ${B}}=\t{sum}{\s{a1}\cdot ${signed('b1', z.b1)}+\s{a2}\cdot ${signed('b2', z.b2)}}`, tex`=\s{dot}`],
			[tex`\t{rhs}{${abs(A)}\,${abs(B)}\cos\theta}=\s{na}\cdot\s{nb}\cdot ${signed('cosT', z.cosT)}`, tex`=\s{rhs}`],
			[tex`\t{rect}{${abs(A)}\cdot(${abs(B)}\cos\theta)}=\s{na}\cdot ${signed('shadow', z.shadow)}`, tex`=\s{rect}`],
		];
		const angle = tex`\t{th}{\theta=\s{th}}`;
		const lines = [...routes.map(([l, r]) => l + r), angle];
		const narrow = [...routes.flatMap(([l, r]) => [l, tex`\quad${r}`]), angle];
		return <Formula {...common} block label={t(locale, 'dp.signFormula')} tex={aligned(lines)} texNarrow={aligned(narrow)}
			tips={{ shadow: { text: t(locale, 'dp.shadowTip') }, rect: { text: t(locale, 'dp.rectTip') } }} />;
	}
	if (scene === 3) {
		if (compact) return <Formula {...common} label={t(locale, 'dp.formula3')} tex={tex`\t{dot3}{${A}\cdot ${B}}=\s{dot3},\;\t{th3}{\theta=\s{th3}}`} />;
		const b3 = v.b3 as number[];
		const term = (i: number) => tex`\s{a3${i}}\cdot ${signed('b3' + i, b3[i - 1])}`;
		const terms = [1, 2, 3].map(term).join('+');
		const lines = [
			tex`\t{dot3}{${A}\cdot ${B}}=\t{sum3}{${terms}}=\s{dot3}`,
			tex`\t{rhs3}{${abs(A)}\,${abs(B)}\cos\theta}=\s{rhs3}`,
			tex`\t{rect3}{${abs(A)}\cdot(${abs(B)}\cos\theta)}=\s{rect3}`,
			tex`\t{th3}{\theta=\s{th3}}`,
		];
		// Phones: the three products take two lines (one sum3 term each, so both halves light together).
		const narrow = [tex`\t{dot3}{${A}\cdot ${B}}=\s{dot3}`, tex`\quad=\t{sum3}{${term(1)}+${term(2)}}`, tex`\qquad+\t{sum3}{${term(3)}}`, ...lines.slice(1)];
		return <Formula {...common} block label={t(locale, 'dp.formula3')} tex={aligned(lines)} texNarrow={aligned(narrow)} />;
	}
	const z = lamp(v);
	if (compact) return <Formula {...common} label={t(locale, 'dp.lampFormula')} tex={tex`\t{brightNL}{k}=\max(0,\t{NdotL}{\mathbf{N}\cdot\mathbf{L}})=\s{brightNL}`} />;
	const lines = [
		tex`\t{brightNL}{k}=\max\bigl(0,\;\t{NdotL}{\mathbf{N}\cdot\mathbf{L}}\bigr)=\max(0,\;\s{ndotl})=\s{brightNL}`,
		tex`\t{NdotL}{\mathbf{N}\cdot\mathbf{L}}=${abs(tex`\mathbf{N}`)}\,${abs(tex`\mathbf{L}`)}\cos\theta=1\cdot 1\cdot\cos\s{thNL}=\s{ndotl}`,
		tex`\t{rectNL}{${abs(tex`\mathbf{N}`)}\cdot(${abs(tex`\mathbf{L}`)}\cos\theta)}=1\cdot ${signed('ndotl', z.ndotl)}`,
	];
	const narrow = [
		tex`\t{brightNL}{k}=\max\bigl(0,\;\t{NdotL}{\mathbf{N}\cdot\mathbf{L}}\bigr)`, tex`\quad=\max(0,\;\s{ndotl})=\s{brightNL}`,
		tex`\t{NdotL}{\mathbf{N}\cdot\mathbf{L}}=1\cdot 1\cdot\cos\s{thNL}=\s{ndotl}`,
		lines[2],
	];
	return <Formula {...common} block label={t(locale, 'dp.lampFormula')} tex={aligned(lines)} texNarrow={aligned(narrow)} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Hover links (spec DP-1/DP-2/DP-4 hover maps, §4 "alias groups").

/** The three faces of one number, per scene: hovering any member lights all of them (the `faces` alias of the spec). */
const FACES = ['dot', 'sum', 'rhs', 'rect', 'bright'];
const FACES3 = ['dot3', 'sum3', 'rhs3', 'rect3'];
const FACES_NL = ['NdotL', 'rectNL', 'brightNL'];
const group = (ids: string[], extra: string[]) => Object.fromEntries(ids.map((id) => [id, [...ids.filter((x) => x !== id), ...extra]]));

/**
 * One-hop hover links: a hot id lights the ids listed here, and nothing beyond (no transitive chains, so `cross`
 * lighting `dot` does not drag in every face). Virtual focus ids of the DP-1 timeline (`s3`, `s4`) light a step's
 * several objects at once. The engine still lights one id per hover (spec §4); the lesson expands it here.
 */
export const HOVER_LINKS: Readonly<Record<string, readonly string[]>> = {
	...group(FACES, ['a', 'b', 'shadow']),
	...group(FACES3, ['a3', 'b3', 'shadow3']),
	...group(FACES_NL, ['N', 'L_dir', 'thNL']),
	rhs: [...FACES.filter((x) => x !== 'rhs'), 'a', 'b', 'shadow', 'th'],
	a1: ['a:x'], a2: ['a:y'], b1: ['b:x'], b2: ['b:y'],
	a31: ['a3:x'], a32: ['a3:y'], a33: ['a3:z'], b31: ['b3:x'], b32: ['b3:y'], b33: ['b3:z'],
	p31: ['a3:x', 'b3:x'], p32: ['a3:y', 'b3:y'], p33: ['a3:z', 'b3:z'],
	d1: ['amb'], d2: ['amb'],
	na2: ['na2_l', 'na2_r'], na2_l: ['na2', 'na2_r'], na2_r: ['na2', 'na2_l'],
	nb2: ['nb2_l', 'nb2_r'], nb2_l: ['nb2', 'nb2_r'], nb2_r: ['nb2', 'nb2_l'],
	corr: ['th'], cross: ['th', 'p1', 'p2', 'dot'],
	hp_pos: ['sgn_pos'], hp_neg: ['sgn_neg'], perp: ['sgn_zero', 'badge'],
	sgn_pos: ['hp_pos'], sgn_neg: ['hp_neg'], sgn_zero: ['perp', 'badge'],
	aa: ['a', 'na'],
	L: ['L_dir', 'light'], L_dir: ['L'], term: ['rt'],
	s3: ['amb', 'expand', 'd1sq', 'd2sq'], s4: ['cross', 'p1', 'p2', 'dot'],
};

function SemanticLinks() {
	useEffect(() => {
		const store = lessonStore(LESSON);
		let written: ReadonlySet<string> | null = null, writing = false;
		const sync = () => {
			const st = store.getState();
			// Our own expansion comes back through the subscription (synchronously, while setHot runs, and once more
			// afterwards): leave it alone, so links stay one hop.
			if (writing || st.hot === written) return;
			const ids = new Set(st.hot);
			for (const id of st.hot) for (const linked of HOVER_LINKS[id] ?? []) ids.add(linked);
			if (ids.size === st.hot.size) return;
			writing = true;
			try { st.setHot([...ids], st.hotSource ?? 'interaction'); } finally { writing = false; }
			written = store.getState().hot;
		};
		sync();
		return store.subscribe(sync);
	}, []);
	return null;
}

/**
 * The step-6 cancellation (spec DP-1 step 6): the |a|² pair is struck as the step opens, the |b|² pair STRIKE_GAP_MS
 * later; the matching tiles fade with the shared spring (`fade_*` eased, scenes draw `shown`). A step change cancels a
 * pending second strike (the cleanup runs first), so going back or jumping never applies an obsolete write. Reduced
 * motion: both pairs at once.
 */
function StrikeClock() {
	const step = useStep(LESSON, 'derive');
	useEffect(() => {
		const st = lessonStore(LESSON).getState();
		const target = strikeTarget(step, reducedMotion());
		const write = (strike: number) => st.setMany({ strike, fade_a: strike >= 1 ? 0 : 1, fade_b: strike >= 2 ? 0 : 1 }, 'cut', ['fade_a', 'fade_b']);
		write(target.now);
		if (target.later === null) return;
		const later = target.later;
		const timer = setTimeout(() => write(later), STRIKE_GAP_MS);
		return () => clearTimeout(timer);
	}, [step]);
	return null;
}

// ---------------------------------------------------------------------------------------------------------------
// Tables.

/** Plain numbers for table cells: up to 4 decimals, no trailing zeros, `−` sign, never -0. */
const fmtCell = (x: number) => String(+x.toFixed(4) + 0).replace('-', '−');

/** DP-1 "comp": two component rows and the sums, columns revealed by step (spec DP-1 NumTable). */
function CompTable() {
	const locale = useLocale(), v = useVals(LESSON), step = useStep(LESSON, 'derive'), z = derive(v);
	const labels: Record<string, string> = { a: 'aᵢ', b: 'bᵢ', d: 'aᵢ − bᵢ', dsq: '(aᵢ − bᵢ)²', asq: 'aᵢ²', bsq: 'bᵢ²', p: 'aᵢbᵢ' };
	const cols = COMP_COLS.filter((c) => step >= c.from).map((c) => ({ id: c.id, label: labels[c.id] }));
	const cell = (id: string, value: number) => ({ id, value });
	const rows: GridRow[] = [
		{ head: '1', cells: { a: cell('a1', z.a1), b: cell('b1', z.b1), d: cell('d1', z.d1), dsq: cell('d1sq', z.d1sq), asq: cell('a1sq', z.a1sq), bsq: cell('b1sq', z.b1sq), p: cell('p1', z.p1) } },
		{ head: '2', cells: { a: cell('a2', z.a2), b: cell('b2', z.b2), d: cell('d2', z.d2), dsq: cell('d2sq', z.d2sq), asq: cell('a2sq', z.a2sq), bsq: cell('b2sq', z.b2sq), p: cell('p2', z.p2) } },
		// Sums: the three squares from step 1, the cross sum only from step 4 (spec: "+ aᵢbᵢ (no sum yet)" at step 3).
		{ head: 'Σ', cells: { dsq: cell('namb2', z.namb2), asq: cell('na2', z.na2), bsq: cell('nb2', z.nb2), p: step >= 4 ? cell('dot', z.dot) : undefined } },
	];
	return <NumGrid lesson={LESSON} caption={t(locale, 'dp.compCaption')} corner="i" cols={cols} rows={rows}
		marks={marksFor(step, v.strike as number)} fmt={fmtCell} reveal={newestColumn(step)} />;
}

/** DP-3 "comp3": three component rows with their products and the sum. */
function Comp3Table() {
	const locale = useLocale(), v = useVals(LESSON), z = derive3(v), a = v.a3 as number[], b = v.b3 as number[];
	const rows: GridRow[] = [0, 1, 2].map((i) => ({
		head: String(i + 1),
		cells: { a: { id: `a3${i + 1}`, value: a[i] }, b: { id: `b3${i + 1}`, value: b[i] }, p: { id: `p3${i + 1}`, value: z.p[i] } },
	}));
	rows.push({ head: 'Σ', cells: { p: { id: 'dot3', value: z.dot } } });
	return <NumGrid lesson={LESSON} caption={t(locale, 'dp.comp3Caption')} corner="i" rows={rows} fmt={fmtCell}
		cols={[{ id: 'a', label: 'aᵢ' }, { id: 'b', label: 'bᵢ' }, { id: 'p', label: 'aᵢbᵢ' }]} />;
}

// ---------------------------------------------------------------------------------------------------------------
// DP-2 three-faces strip and the sign words.

/** One sign word; hovering it lights its half-plane or the boundary (spec DP-2 hover map). */
function SignWord({ id, text, active }: { id: string; text: string; active: boolean }) {
	const hot = useHot(LESSON, id);
	return <span data-t={id} data-active={active ? '' : undefined} tabIndex={0}
		// The strip is drawn in the viewport's dark chrome in both site themes, so the word uses the viz tokens too.
		style={{ padding: '.15rem .5rem', borderRadius: 4, border: '1px solid var(--viz-chrome-2)', fontFamily: 'var(--lab-font-mono)', fontWeight: active ? 700 : 400,
			background: active ? 'var(--viz-field-hover)' : 'transparent', color: 'var(--viz-ink)', outline: hot ? '1px solid var(--viz-hot)' : undefined }}
		onPointerEnter={() => hotOn(LESSON, id)} onPointerLeave={() => hotOff(LESSON, id)} onFocus={() => hotOn(LESSON, id)} onBlur={() => hotOff(LESSON, id)}>{text}</span>;
}

function FacesStrip() {
	const locale = useLocale(), z = derive(useVals(LESSON));
	const sign = z.dot > 1e-9 ? 'pos' : z.dot < -1e-9 ? 'neg' : 'zero';
	// The cells are N-panel fields, drawn in the viewport's dark chrome (their colours are calibrated for it).
	return <div className="not-content" data-dp-faces="" style={{ background: 'var(--viz-chrome)', color: 'var(--viz-ink)', padding: '.4rem', borderRadius: 6 }}>
		<div style={{ display: 'flex', flexWrap: 'wrap', gap: '.5rem' }}>
			{facesFor(locale).map((r) => <FieldCell key={r.id} lesson={LESSON} row={r} size="lg" />)}
		</div>
		<p style={{ display: 'flex', flexWrap: 'wrap', gap: '.5rem', alignItems: 'center' }} aria-live="polite">
			<span>{t(locale, 'dp.signLabel')}</span>
			<SignWord id="sgn_pos" text={t(locale, 'dp.sgnPos')} active={sign === 'pos'} />
			<SignWord id="sgn_zero" text={t(locale, z.zeroA || z.zeroB ? 'dp.sgnZero' : 'dp.sgnPerp')} active={sign === 'zero'} />
			<SignWord id="sgn_neg" text={t(locale, 'dp.sgnNeg')} active={sign === 'neg'} />
		</p>
		<p>{t(locale, 'dp.facesNote')}</p>
	</div>;
}

// ---------------------------------------------------------------------------------------------------------------
// Code.

const NUMPY = 'dot = a[0]*b[0] + a[1]*b[1] + a[2]*b[2]\ndot = (a*b).sum()\ndot = a @ b';
const GLSL3 = 'float d = dot(a, b);\nfloat c = d / (length(a) * length(b));\nfloat th = acos(clamp(c, -1.0, 1.0));';
const GLSL4 = 'vec3 N = normalize(P - center);\nvec3 L = normalize(lightPos - P);\nfloat ndotl = dot(N, L);\nfloat k = max(0.0, ndotl);\ncolor = albedo * k;';

/** DP-3 code in two tabs (NumPy, GLSL), lines linked by id (spec DP-3 CodeView). */
function CodeTabs() {
	const locale = useLocale(), [tab, setTab] = useState<'numpy' | 'glsl'>('numpy');
	const tabs = [['numpy', 'NumPy'], ['glsl', 'GLSL']] as const;
	return <div className="not-content" data-dp-code="">
		<div role="tablist" aria-label={t(locale, 'dp.codeTabs')} style={{ display: 'flex', gap: '.4rem' }}>
			{tabs.map(([id, label]) => <button key={id} type="button" role="tab" aria-selected={tab === id} onClick={() => setTab(id)}
				style={{ font: 'inherit', padding: '.2rem .7rem', fontWeight: tab === id ? 700 : 400 }}>{label}</button>)}
		</div>
		<div role="tabpanel">
			{tab === 'numpy'
				? <CodeView lesson={LESSON} lang="python" code={NUMPY} lines={{ 1: 'sum3', 2: 'sum3', 3: 'dot3' }} />
				: <CodeView lesson={LESSON} lang="glsl" code={GLSL3} lines={{ 1: 'dot3', 2: 'cosT3', 3: 'th3' }} />}
		</div>
	</div>;
}

// ---------------------------------------------------------------------------------------------------------------
// Timeline, challenges, readouts.

/** DP-1 step texts as literal dictionary keys (the dictionary usage test scans for them). */
const STEP_KEYS = ['dp.step0', 'dp.step1', 'dp.step2', 'dp.step3', 'dp.step4', 'dp.step5', 'dp.step6', 'dp.step7', 'dp.step8'] as const;
/** The focus id of each step (spec "lit"); `s3` / `s4` are virtual ids expanded by HOVER_LINKS. Step 6 lights nothing. */
const STEP_FOCUS = ['amb', 'namb2', 'corr', 's3', 's4', 'namb2', undefined, 'dot', 'rect'];

/** Challenge cards per scene (spec DP-1..DP-4); predicates read store values only, so they work from the keyboard. */
function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	const d = (x: Vals) => derive(x), d3 = (x: Vals) => derive3(x), l = (x: Vals) => lamp(x);
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'dp.ch1'), check: (x) => d(x).perp, hint: [t(locale, 'dp.ch1Hint')] },
			{ n: 2, text: t(locale, 'dp.ch2'), check: (x) => d(x).dot < -1e-9, success: t(locale, 'dp.ch2Success') },
			{ n: 3, text: t(locale, 'dp.ch3'), check: (x) => { const z = d(x); return Math.abs(z.dot - z.na2) < 1e-9 && z.na > 0; }, success: t(locale, 'dp.ch3Success') },
		];
	}
	if (scene === 2) {
		return [
			{ n: 4, text: t(locale, 'dp.ch4'), check: (x) => d(x).perp, hint: [t(locale, 'dp.ch4Hint'), t(locale, 'dp.ch4Hint2')] },
			{ n: 5, text: t(locale, 'dp.ch5'), check: (x) => { const z = d(x); return Math.abs(z.nb - 2) < 0.05 && z.cosT > 0.995; } },
			{ n: 6, text: t(locale, 'dp.ch6'), check: (x) => { const z = d(x); return Math.abs(z.nb - 2) < 0.05 && z.cosT < -0.995; } },
			{ n: 7, text: t(locale, 'dp.ch7'), check: (x) => Math.abs(d(x).cosT - 0.5) < 0.01 },
		];
	}
	if (scene === 3) {
		return [
			{ n: 8, text: t(locale, 'dp.ch8'), check: (x) => d3(x).perp },
			{ n: 9, text: t(locale, 'dp.ch9'), check: (x) => d3(x).dot < -1e-9 && (x.b3 as number[])[2] > 0 },
		];
	}
	return [
		{ n: 10, text: t(locale, 'dp.ch10'), check: (x) => Math.abs(l(x).ndotl) < 0.02, hint: [t(locale, 'dp.ch10Hint')] },
		{ n: 11, text: t(locale, 'dp.ch11'), check: (x) => l(x).ndotl > 0.995 },
		{ n: 12, text: t(locale, 'dp.ch12'), check: (x) => x.nl_raw === 1 && l(x).ndotl < -0.995 },
	];
}

/**
 * The live sentence under a scene: the two (or three) routes to the number, or why a route has nothing to compute
 * (ruling 2: a zero b still has a shadow and an area of 0; a zero a has no axis at all; Σaᵢbᵢ never fails).
 * `variants` are every sentence the same numbers can produce, for StableText's height reservation.
 */
export function readouts(scene: Scene, locale: Locale, v: Vals): { text: string; variants: string[] } {
	if (scene === 4) {
		const z = lamp(v);
		const base = t(locale, 'dp.lampReadout', { ndotl: fmtCos(z.ndotl), bright: fmtCos(z.brightNL), th: fmtDeg(z.thNL) });
		const state = Math.abs(z.ndotl) < 0.02 ? 'dp.lampTerminator' : z.ndotl < 0 ? (z.raw ? 'dp.lampBehindRaw' : 'dp.lampBehind') : 'dp.lampLit';
		const states = ['dp.lampTerminator', 'dp.lampBehindRaw', 'dp.lampBehind', 'dp.lampLit'] as const;
		return { text: base + t(locale, state), variants: states.map((s) => base + t(locale, s)) };
	}
	const z = scene === 3 ? derive3(v) : derive(v);
	const dot = fmt(z.dot);
	const zeroA = t(locale, scene === 2 ? 'dp.zeroA2' : 'dp.zeroA', { dot });
	const routes = t(locale, 'dp.routes', { dot, rhs: fmt(z.rhs), rect: fmt(z.rect), th: fmtDeg(z.th) });
	const variants = [t(locale, 'dp.zeroBoth', { dot }), zeroA, t(locale, 'dp.zeroB', { dot }), routes + t(locale, 'dp.perpNote'), routes + t(locale, 'dp.negNote')];
	const text = z.zeroA && z.zeroB ? variants[0] : z.zeroA ? variants[1] : z.zeroB ? variants[2]
		: routes + (z.perp ? t(locale, 'dp.perpNote') : z.dot < 0 ? t(locale, 'dp.negNote') : '');
	return { text, variants };
}
export const readout = (scene: Scene, locale: Locale, v: Vals): string => readouts(scene, locale, v).text;

/**
 * Live text whose box is as tall as its tallest variant. The four scenes share one store, so a drag in DP-2 rewrites
 * DP-1's readout above it; if that paragraph grew by a line (the "perpendicular" note), the card being dragged would
 * jump under the pointer (measured: a 0.7-unit overshoot). All variants sit in one grid cell, invisible and hidden from
 * assistive tech; only the current text is visible and announced. Pure CSS, so the server render reserves it too.
 */
function StableText({ text, variants }: { text: string; variants: readonly string[] }) {
	return <div data-dp-readout="" style={{ display: 'grid' }}>
		{variants.map((x, i) => <p key={i} aria-hidden="true" style={{ gridArea: '1 / 1', visibility: 'hidden', margin: 0 }}>{x}</p>)}
		<p aria-live="polite" style={{ gridArea: '1 / 1', margin: 0 }}>{text}</p>
	</div>;
}
function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON), step = useStep(LESSON, 'derive');
	return <section className="not-content" data-dp-controls={scene}>
		{/* Links and the strike clock are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <><SemanticLinks /><StrikeClock /></>}
		{scene === 1 && <Timeline lesson={LESSON} id="derive" steps={STEP_KEYS.map((key, i) => ({ id: String(i), text: t(locale, key), focus: STEP_FOCUS[i] }))} />}
		{scene === 1
			? <div style={{ position: 'relative' }}>
				{/* The hidden lines keep their space (no layout shift when they appear); until the first one does, the
				    empty box says what will fill it instead of looking broken. */}
				{step < 2 && <p data-dp-later="" style={{ position: 'absolute', inset: '0 0 auto 0', margin: 0, color: 'var(--sl-color-gray-3)' }}>{t(locale, 'dp.formulaLater')}</p>}
				<DotFormula scene={1} />
			</div>
			: <DotFormula scene={scene} />}
		{scene === 1 && <CompTable />}
		{scene === 2 && <FacesStrip />}
		{scene === 3 && <><Comp3Table /><CodeTabs /></>}
		{scene === 4 && <CodeView lesson={LESSON} lang="glsl" code={GLSL4} lines={{ 1: 'N', 2: 'L', 3: 'NdotL', 4: 'brightNL', 5: 'sphere' }} />}
		<StableText {...readouts(scene, locale, v)} />
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
		<span hidden data-current-derive={step} />
	</section>;
}
export const DotControls = localized(Controls);
