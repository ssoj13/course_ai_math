/**
 * DotUnit entry (SSR-safe, AGENTS.md lesson pattern): the module-level store, the DOM controls that sit under the card
 * (timeline `unit`, the linked formula, the component fields, the product table, the code) and the lazy scene island.
 * Mounted by the Start-here pages (INTRO-1), the 1.4 teaser and the dev page. Words come from the `du` namespace.
 */
import { useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Timeline } from '../Timeline';
import { CodeView } from '../CodeView';
import { Challenge, type ChallengeProps } from '../Challenge';
import { NumGrid, type GridRow } from '../NumTable';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { FieldCell } from '../scene/NPanel';
import { alignedRows } from '../tex';
import { dotOf, INITIAL, LESSON, LINKS, N_MAX } from './dotunit.store';
import { compRows } from './dotunit.rows';
import { fmt, fmtShort } from './model';

lessonStore(LESSON, INITIAL, LINKS);

export const DotUnitScene = localized(lazyIsland(() => import('./unit.impl').then((m) => m.DotUnitCard)));

type Vals = Parameters<typeof dotOf>[0];
const tex = String.raw;

/** Step texts of the `unit` timeline as literal keys (the dictionary usage test scans for them), spec §2.4. */
const STEP_KEYS = ['du.unit0', 'du.unit1', 'du.unit2', 'du.unit3', 'du.unit4', 'du.unit5', 'du.unit6', 'du.unit7', 'du.unit8', 'du.unit9'] as const;
/** What each step lights (spec §2.4 "hot / marks"). */
const STEP_FOCUS = [undefined, 'shadow', 'dot', undefined, undefined, undefined, undefined, 'sum', 'proj', undefined];

/** A tile lights its two legs and the loop line of the code; the three faces of the number light each other. */
const LINKS_HOT: HotLinkMap = {
	...Object.fromEntries(Array.from({ length: N_MAX }, (_, i) => ['p' + i, ['prod', 'a' + i, 'b' + i]])),
	dot: ['sum'],
	sum: ['dot'],
	proj: ['dot'],
};

const dash = tex`\text{--}`;
const num = (x: number) => (Number.isFinite(x) ? fmt(x) : dash);
const derived: Record<string, (v: Vals) => number> = {
	len_a: (v) => dotOf(v).lenA,
	shadow: (v) => dotOf(v).shadow,
	// The printed numbers follow the chosen arithmetic / order (spec §1.3); |a| and the shadow are fp64 geometry.
	dot: (v) => dotOf(v).acc.value,
	...Object.fromEntries(Array.from({ length: N_MAX }, (_, i) => ['p' + i, (v: Vals) => dotOf(v).acc.prods[i] ?? 0])),
};
const formats: Record<string, (x: number) => string> = {
	len_a: num, shadow: num, dot: num,
	...Object.fromEntries(Array.from({ length: N_MAX }, (_, i) => ['p' + i, fmtShort])),
};

/** a·b = |a| s = Σ aᵢbᵢ with the live numbers (spec §2.1 "Formula"); `narrow` breaks it for phones. */
function formulaTex(n: number, prods: readonly number[], narrow: boolean): string {
	const sym = Array.from({ length: n }, (_, i) => tex`\t{p${i}}{a_{${i + 1}}b_{${i + 1}}}`);
	const val = Array.from({ length: n }, (_, i) => (prods[i] < 0 ? tex`(\s{p${i}})` : tex`\s{p${i}}`));
	const head = tex`\t{dot}{\mathbf{a}\cdot\mathbf{b}}=\t{len_a}{\lvert\mathbf{a}\rvert}\,\t{shadow}{s}`;
	const mid = tex`\quad=\s{len_a}\cdot\s{shadow}`;
	if (!narrow) return alignedRows([head + '=' + sym.join('+'), mid + '=' + val.join('+') + tex`=\s{dot}`]);
	const half = Math.ceil(n / 2);
	return alignedRows([
		head,
		tex`\quad=` + sym.slice(0, half).join('+') + (n > half ? '+' : ''),
		...(n > half ? [tex`\qquad` + sym.slice(half).join('+')] : []),
		mid + tex`=\s{dot}`,
	]);
}

function UnitFormula() {
	const locale = useLocale(), d = dotOf(useVals(LESSON)), n = d.a.length;
	return <Formula lesson={LESSON} block label={t(locale, 'du.formula')} derived={derived} fmt={formats}
		tex={formulaTex(n, d.acc.prods, false)} texNarrow={formulaTex(n, d.acc.prods, true)}
		tips={{ shadow: { text: t(locale, 'du.shadowTip') }, len_a: { text: t(locale, 'du.lenTip') }, dot: { text: t(locale, 'du.dotTip') } }} />;
}

/**
 * aᵢ, bᵢ, aᵢbᵢ and the additions as the machine executes them (spec §2.1 NumTable ids): sequential = the running sum
 * after each tile; pairwise = the merges in execution order (row k holds merge k). The last row is the result.
 */
function UnitTable() {
	const locale = useLocale(), v = useVals(LESSON), d = dotOf(v), pair = v.du_pair === 1;
	const merge = (i: number) => (i === 0 ? (pair ? undefined : d.acc.prods[0]) : d.acc.merges[i - 1]?.value);
	const rows: GridRow[] = d.a.map((x, i) => {
		const m = merge(i);
		return {
			head: String(i + 1),
			cells: { a: { id: 'a' + i, value: x }, b: { id: 'b' + i, value: d.b[i] }, p: { id: 'p' + i, value: d.acc.prods[i] }, run: m === undefined ? undefined : { id: 'run' + i, value: m } },
		};
	});
	rows.push({ head: 'Σ', cells: { p: { id: 'sum', value: d.acc.value } } });
	return <NumGrid lesson={LESSON} caption={t(locale, 'du.tableCaption')} corner="i" rows={rows} fmt={fmtShort}
		cols={[{ id: 'a', label: 'aᵢ' }, { id: 'b', label: 'bᵢ' }, { id: 'p', label: 'aᵢbᵢ' }, { id: 'run', label: t(locale, pair ? 'du.merges' : 'du.running') }]} />;
}

const SEQ = 's = 0.0\nfor i in range(n):\n    s += a[i] * b[i]';
const PAIR = 'p = [a[i] * b[i] for i in range(n)]\nwhile len(p) > 1:  # pairwise, like BLAS and GPUs\n    p = [p[i] + p[i + 1] if i + 1 < len(p) else p[i] for i in range(0, len(p), 2)]';

/**
 * The loop with a fourth line that follows the animation (spec §2.1 CodeView): while the column passes or a tile drops,
 * line 3 is linked to that tile and the comment shows that step's numbers; otherwise it shows the result. Always four
 * lines, so the block never changes height.
 */
function UnitCode() {
	const v = useVals(LESSON), d = dotOf(v), pair = v.du_pair === 1, cur = Math.round(v.du_cur as number);
	const p = d.acc.prods;
	const live = cur < 0 || cur >= p.length ? `# s = ${fmtShort(d.acc.value)}`
		: pair ? `# tile ${cur + 1}: ${fmtShort(d.a[cur])} * ${fmtShort(d.b[cur])} = ${fmtShort(p[cur])}`
		: `# i = ${cur}: s = ${fmtShort(cur === 0 ? 0 : cur === 1 ? p[0] : d.acc.merges[cur - 2].value!)} + ${fmtShort(p[cur])} = ${fmtShort(cur === 0 ? p[0] : d.acc.merges[cur - 1].value!)}`;
	const tile = cur >= 0 ? 'p' + cur : 'prod';
	const lines = pair ? { 1: tile, 2: 'n', 3: 'sum', 4: 'sum' } : { 1: 'sum', 2: 'n', 3: tile, 4: 'sum' };
	return <CodeView lesson={LESSON} lang="python" code={(pair ? PAIR : SEQ) + '\n' + live} lines={lines} />;
}

/** Challenges (1.4 spec "make the stack negative", "make the shadow zero", plus three on tiles, n and rounding). */
function challengesFor(locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	const nz = (xs: readonly number[]) => xs.some((x) => x !== 0);
	return [
		{ n: 1, text: t(locale, 'du.ch1'), check: (v) => dotOf(v).dot < -1e-9 },
		{ n: 2, text: t(locale, 'du.ch2'), hint: [t(locale, 'du.ch2Hint')], check: (v) => { const d = dotOf(v); return d.lenA > 0 && nz(d.b) && Math.abs(d.dot) < 1e-9; } },
		{ n: 3, text: t(locale, 'du.ch3'), check: (v) => { const d = dotOf(v); return d.prods.every((x) => x > 0) && d.dot < 1; } },
		{ n: 4, text: t(locale, 'du.ch4'), check: (v) => { const d = dotOf(v); return d.a.length === 4 && [...d.a, ...d.b].every((x) => x !== 0) && Math.abs(d.dot) < 1e-9; } },
		{ n: 5, text: t(locale, 'du.ch5'), hint: [t(locale, 'du.ch5Hint')], success: t(locale, 'du.ch5Success'), check: (v) => v.du_fp32 === 1 && dotOf(v).acc.err !== 0 },
	];
}
function Fields() {
	const locale = useLocale(), n = (useVals(LESSON).n as number) ?? 3;
	const rows = useMemo(() => compRows(locale, n), [locale, n]);
	return <div className="not-content" data-du-fields="">{rows.map((r) => <FieldCell key={r.id + n} lesson={LESSON} row={r} size="sm" />)}</div>;
}

export type UnitPart = 'timeline' | 'formula' | 'fields' | 'table' | 'code' | 'challenges';
const ALL: readonly UnitPart[] = ['timeline', 'formula', 'fields', 'table', 'code', 'challenges'];

function Controls({ parts = ALL }: { parts?: readonly UnitPart[]; locale?: Locale }) {
	const locale = useLocale();
	const has = (p: UnitPart) => parts.includes(p);
	return <section className="not-content" data-du-controls="">
		<HotLinks lesson={LESSON} links={LINKS_HOT} />
		{has('timeline') && <Timeline lesson={LESSON} id="unit" steps={STEP_KEYS.map((k, i) => ({ id: String(i), text: t(locale, k), focus: STEP_FOCUS[i] }))} />}
		{has('formula') && <UnitFormula />}
		{has('fields') && <Fields />}
		{has('table') && <UnitTable />}
		{has('code') && <UnitCode />}
		{has('challenges') && challengesFor(locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const DotUnitControls = localized(Controls);
