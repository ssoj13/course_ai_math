import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useStep, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Timeline } from '../Timeline';
import { NumGrid, type GridRow } from '../NumTable';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { alignedRows } from '../tex';
import { CHECKS, derive, fmt2, fmt3, fmtVec, INITIAL, LESSON, N_BARS, sumOf, texFmt, WALK } from './length.math';
import type { Scene } from './length.rows';

/**
 * Lesson 1.3 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store, the DOM-only controls
 * (timelines, formulas, Σ tables, challenges, readouts) and the lazy scene island. Everything three lives in
 * length.impl.tsx. Reader-facing words come from the `ln` dictionary namespace; numbers from length.math.ts.
 */
lessonStore(LESSON, INITIAL);

export const LengthScene = localized(lazyIsland(() => import('./length.impl').then((m) => m.LengthScene)));

type Vals = Parameters<typeof derive>[0];

const tex = String.raw;
const V = tex`\mathbf{v}`;
const abs = (x: string) => tex`\lvert ${x}\rvert`;

/** Component i (1-based) of a vector key, for `\s{}`. */
const comp = (key: string, i: number) => (v: Vals) => (v[key] as number[])[i - 1];

/** Every number a formula shows as `\s{key}`: components (1 decimal), squares and sums (2), unit components (3). */
const derived: Record<string, (v: Vals) => number> = {
	v1: comp('v', 1), v2: comp('v', 2),
	q1: (v) => derive(v).s.sq[0], q2: (v) => derive(v).s.sq[1],
	s2: (v) => derive(v).s.s2, len: (v) => derive(v).s.len,
	v31: comp('v3', 1), v32: comp('v3', 2), v33: comp('v3', 3),
	q31: (v) => derive(v).s3.sq[0], q32: (v) => derive(v).s3.sq[1], q33: (v) => derive(v).s3.sq[2],
	// Running sum after the two floor legs (x, then z) of the 3D walk.
	r32: (v) => derive(v).s3.sq[0] + derive(v).s3.sq[2],
	s23: (v) => derive(v).s3.s2, len3: (v) => derive(v).s3.len,
	w1: comp('w', 1), w2: comp('w', 2), nw: (v) => derive(v).n.n,
	h1: (v) => derive(v).n.hatC[0], h2: (v) => derive(v).n.hatC[1], hl: (v) => derive(v).n.hatLen,
	w31: comp('w3', 1), w32: comp('w3', 2), w33: comp('w3', 3), nw3: (v) => derive(v).n3.n,
	h31: (v) => derive(v).n3.hatC[0], h32: (v) => derive(v).n3.hatC[1], h33: (v) => derive(v).n3.hatC[2], hl3: (v) => derive(v).n3.hatLen,
	su2: (v) => derive(v).su.s2, lenu: (v) => derive(v).su.len,
};
const COMPS = new Set(['v1', 'v2', 'v31', 'v32', 'v33', 'w1', 'w2', 'w31', 'w32', 'w33']);
const UNITS = new Set(['h1', 'h2', 'hl', 'h31', 'h32', 'h33', 'hl3']);
const formats = Object.fromEntries(Object.keys(derived).map((k) => [k, COMPS.has(k) ? texFmt.c : UNITS.has(k) ? texFmt.u : texFmt.s]));

/** A live value under a square or after a minus is parenthesised when negative: "(−3.0)²", never "−3.0²". */
const paren = (key: string, value: number) => (value < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);

/**
 * Hover links (storyboard 1.3): a running-sum cell lights the tiles it has added so far, the total lights every tile,
 * |v| lights the arrow; a bar lights its two table cells and they light the bar back.
 */
const BARS = Array.from({ length: N_BARS }, (_, i) => i);
const HOT: HotLinkMap = {
	run1: ['sq1'], run2: ['sq1', 'sq2'], s2: ['sq1', 'sq2'], len: ['v'],
	trun1: ['tq1'], trun2: ['tq1', 'tq3', 'd1'], trun3: ['tq1', 'tq2', 'tq3'], d1: ['trun2'],
	s2_3: ['tq1', 'tq2', 'tq3'], len3: ['v3'],
	nw: ['w'], hat: ['hatPt'], hatPt: ['hat'], nw3: ['w3'], hat3: ['hatPt'],
	su2: BARS.map((i) => 'usq' + i), lenu: BARS.map((i) => 'u' + i),
	...Object.fromEntries(BARS.flatMap((i) => [['u' + i, ['uc' + i, 'usq' + i]], ['uc' + i, ['u' + i]], ['usq' + i, ['u' + i]]])),
};

// ---------------------------------------------------------------------------------------------------------------
// Formulas.

/**
 * LN-1 / LN-1-3D: |v|² = Σ v_i², unfolded one component per step: line 2 lists the squares revealed so far, line 3
 * their values and the running sum, the last step adds the root. Terms carry the tile ids, so hovering lights tiles.
 */
function sumTex(three: boolean, step: number, v: Vals): string {
	const order: readonly number[] = three ? WALK : [0, 1];
	const n = order.length;
	const comps = (three ? v.v3 : v.v) as number[];
	const tile = (i: number) => (three ? 'tq' : 'sq') + i;
	const cKey = (i: number) => (three ? 'v3' + i : 'v' + i);
	const qKey = (i: number) => (three ? 'q3' + i : 'q' + i);
	const total = three ? 's23' : 's2';
	const s2Id = three ? 's2_3' : 's2', lenId = three ? 'len3' : 'len', lenKey = three ? 'len3' : 'len';
	const shown = Math.min(step, n);
	const rows: string[] = [tex`\t{${s2Id}}{${abs(V)}^2}=\sum_{i=1}^{${n}} v_i^2`];
	if (shown >= 1) {
		// Components in walk order (3D: x, z, then y): a sum does not care about the order of its terms.
		const idx = order.slice(0, shown).map((k) => k + 1);
		rows.push(tex`\quad=` + idx.map((i) => tex`\t{${tile(i)}}{${paren(cKey(i), comps[i - 1])}^2}`).join('+'));
		const sum = idx.map((i) => tex`\t{${tile(i)}}{\s{${qKey(i)}}}`).join('+');
		// Until every square is in, the running total is "so far"; at the last component it is |v|².
		const end = shown === n ? tex`=\t{${s2Id}}{\s{${total}}}` : shown > 1 ? tex`=\s{r32}` : '';
		rows.push(tex`\quad=` + sum + end);
	}
	if (step > n) rows.push(tex`\t{${lenId}}{${abs(V)}}=\sqrt{\s{${total}}}=\s{${lenKey}}`);
	return alignedRows(rows);
}

function normTex(three: boolean, narrow: boolean): string {
	const k = three ? ['w31', 'w32', 'w33'] : ['w1', 'w2'];
	const h = three ? ['h31', 'h32', 'h33'] : ['h1', 'h2'];
	const nw = three ? 'nw3' : 'nw', hl = three ? 'hl3' : 'hl', hatId = three ? 'hat3' : 'hat', lenId = three ? 'hatLen3' : 'hatLen';
	const vecOf = (keys: string[]) => '(' + keys.map((x) => tex`\s{${x}}`).join(',\\ ') + ')';
	const def = tex`\hat{${V}}=\frac{${V}}{\t{${nw}}{${abs(V)}}}`;
	const values = tex`\frac{${vecOf(k)}}{\t{${nw}}{\s{${nw}}}}`;
	const result = tex`\t{${hatId}}{${vecOf(h)}}`;
	const check = tex`\t{${lenId}}{${abs(tex`\hat{${V}}`)}}=\s{${hl}}`;
	return narrow
		? alignedRows([def, tex`\quad=${values}`, tex`\quad=${result}`, check])
		: alignedRows([tex`${def}=${values}=${result}`, check]);
}

const uTex = tex`\t{lenu}{\lvert\mathbf{u}\rvert}=\sqrt{\sum_{i=1}^{8} u_i^2}=\sqrt{\t{su2}{\s{su2}}}=\s{lenu}`;

function LengthFormula({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	const step = useStep(LESSON, scene === 2 ? 'sum3' : 'sum2');
	const common = { lesson: LESSON, derived, fmt: formats, block: true };
	if (scene === 1 || scene === 2) {
		const three = scene === 2;
		return <Formula {...common} label={t(locale, 'ln.formulaSum')} tex={sumTex(three, step, v)}
			tips={{ [three ? 's2_3' : 's2']: { text: t(locale, 'ln.tipS2') } }} />;
	}
	if (scene === 3 || scene === 4) {
		const three = scene === 4;
		return <Formula {...common} label={t(locale, 'ln.formulaNorm')} tex={normTex(three, false)} texNarrow={normTex(three, true)}
			tips={{ [three ? 'nw3' : 'nw']: { text: t(locale, 'ln.tipDivisor') } }} />;
	}
	return <Formula {...common} label={t(locale, 'ln.formulaU')} tex={uTex} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Σ tables.

/** LN-1 / LN-1-3D: component, square, running sum per row, revealed by the timeline; the root row at the end. */
function SumTable({ three }: { three: boolean }) {
	const locale = useLocale(), v = useVals(LESSON), step = useStep(LESSON, three ? 'sum3' : 'sum2');
	const comps = (three ? v.v3 : v.v) as number[];
	const order: readonly number[] = three ? WALK : [0, 1];
	// Rows in walk order, so "sum so far" reads top to bottom; the head keeps each component's own index i.
	const s = sumOf(order.map((k) => comps[k]));
	const p = three ? 't' : '';
	const tile = (i: number) => (three ? 'tq' : 'sq') + i;
	const rows: GridRow[] = order.map((k, j) => {
		const i = k + 1, shown = step >= j + 1;
		return {
			head: String(i),
			cells: {
				c: { id: p + 'c' + i, value: comps[k] },
				sq: shown ? { id: tile(i), value: s.sq[j] } : undefined,
				run: shown ? { id: p + 'run' + (j + 1), value: s.run[j] } : undefined,
			},
		};
	});
	if (step > s.comps.length) rows.push({ head: '√', cells: { run: { id: three ? 'len3' : 'len', value: s.len } } });
	return <NumGrid lesson={LESSON} caption={t(locale, 'ln.sumCaption')} corner="i" rows={rows} fmt={(x) => fmt2(x)}
		cols={[{ id: 'c', label: 'vᵢ' }, { id: 'sq', label: 'vᵢ²' }, { id: 'run', label: t(locale, 'ln.colRun') }]} />;
}

/** LN-3: the eight components and their squares, with Σ and the root. */
function BarTable() {
	const locale = useLocale(), s = derive(useVals(LESSON)).su;
	const cols = [...BARS.map((i) => ({ id: String(i), label: String(i + 1) })), { id: 'sum', label: 'Σ' }, { id: 'len', label: '√Σ' }];
	const rows: GridRow[] = [
		{ head: 'uᵢ', cells: Object.fromEntries(BARS.map((i) => [String(i), { id: 'uc' + i, value: s.comps[i] }])) },
		{ head: 'uᵢ²', cells: { ...Object.fromEntries(BARS.map((i) => [String(i), { id: 'usq' + i, value: s.sq[i] }])), sum: { id: 'su2', value: s.s2 }, len: { id: 'lenu', value: s.len } } },
	];
	return <NumGrid lesson={LESSON} caption={t(locale, 'ln.barCaption')} corner="i" cols={cols} rows={rows} fmt={(x) => fmt2(x)} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Timelines, challenges, readouts.

const STEPS2 = ['ln.s2a', 'ln.s2b', 'ln.s2c', 'ln.s2d'] as const;
const FOCUS2 = ['v', 'sq1', 'sq2', 's2'];
const STEPS3 = ['ln.s3a', 'ln.s3b', 'ln.s3c', 'ln.s3d', 'ln.s3e'] as const;
const FOCUS3 = ['v3', 'tq1', 'trun2', 'tq2', 's2_3'];

function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) return [{ n: 1, text: t(locale, 'ln.ch1'), check: CHECKS.five, hint: [t(locale, 'ln.ch1Hint')] }];
	if (scene === 2) return [{ n: 2, text: t(locale, 'ln.ch2'), check: CHECKS.ones3, success: t(locale, 'ln.ch2Success') }];
	if (scene === 3) {
		return [
			{ n: 3, text: t(locale, 'ln.ch3'), check: CHECKS.sixEight, hint: [t(locale, 'ln.ch3Hint')] },
			{ n: 4, text: t(locale, 'ln.ch4'), check: CHECKS.zero, success: t(locale, 'ln.ch4Success') },
		];
	}
	if (scene === 4) return [{ n: 5, text: t(locale, 'ln.ch5'), check: CHECKS.onSphere }];
	return [
		{ n: 6, text: t(locale, 'ln.ch6'), check: CHECKS.four, hint: [t(locale, 'ln.ch6Hint')] },
		{ n: 7, text: t(locale, 'ln.ch7'), check: CHECKS.ones8, success: t(locale, 'ln.ch7Success') },
	];
}

/** The live sentence under a scene (the canvas-free account of what it shows). */
export function readout(scene: Scene, locale: Locale, v: Vals): string {
	const z = derive(v);
	if (scene === 1 || scene === 2) {
		const s = scene === 2 ? z.s3 : z.s;
		return t(locale, 'ln.readSum', { v: fmtVec(s.comps), squares: s.sq.map(fmt2).join(' + '), s2: fmt2(s.s2), len: fmt2(s.len) });
	}
	if (scene === 3 || scene === 4) {
		const n = scene === 4 ? z.n3 : z.n;
		if (n.zero) return t(locale, 'ln.readZero');
		return t(locale, 'ln.readNorm', { v: fmtVec(n.w), n: fmt2(n.n), hat: fmtVec(n.hatC, fmt3), len: fmt3(n.hatLen), k: fmt2(n.k), mid: fmt2(n.midLen) });
	}
	return t(locale, 'ln.readBars', { s2: fmt2(z.su.s2), len: fmt2(z.su.len) });
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	return <section className="not-content" data-ln-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		{scene === 1 && <Timeline lesson={LESSON} id="sum2" steps={STEPS2.map((key, i) => ({ id: String(i), text: t(locale, key), focus: FOCUS2[i] }))} />}
		{scene === 2 && <Timeline lesson={LESSON} id="sum3" steps={STEPS3.map((key, i) => ({ id: String(i), text: t(locale, key), focus: FOCUS3[i] }))} />}
		<LengthFormula scene={scene} />
		{(scene === 1 || scene === 2) && <SumTable three={scene === 2} />}
		{scene === 5 && <BarTable />}
		<p aria-live="polite">{readout(scene, locale, v)}</p>
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const LengthControls = localized(Controls);
