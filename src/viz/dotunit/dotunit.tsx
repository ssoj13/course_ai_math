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
	dot: (v) => dotOf(v).dot,
	...Object.fromEntries(Array.from({ length: N_MAX }, (_, i) => ['p' + i, (v: Vals) => dotOf(v).prods[i] ?? 0])),
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
		tex={formulaTex(n, d.prods, false)} texNarrow={formulaTex(n, d.prods, true)}
		tips={{ shadow: { text: t(locale, 'du.shadowTip') }, len_a: { text: t(locale, 'du.lenTip') }, dot: { text: t(locale, 'du.dotTip') } }} />;
}

/** aᵢ, bᵢ, aᵢbᵢ and the running sum per component; the last row is the stack's height (spec §2.1 NumTable ids). */
function UnitTable() {
	const locale = useLocale(), d = dotOf(useVals(LESSON));
	const rows: GridRow[] = d.a.map((x, i) => ({
		head: String(i + 1),
		cells: { a: { id: 'a' + i, value: x }, b: { id: 'b' + i, value: d.b[i] }, p: { id: 'p' + i, value: d.prods[i] }, run: { id: 'run' + i, value: d.run[i] } },
	}));
	rows.push({ head: 'Σ', cells: { p: { id: 'sum', value: d.dot } } });
	return <NumGrid lesson={LESSON} caption={t(locale, 'du.tableCaption')} corner="i" rows={rows} fmt={fmtShort}
		cols={[{ id: 'a', label: 'aᵢ' }, { id: 'b', label: 'bᵢ' }, { id: 'p', label: 'aᵢbᵢ' }, { id: 'run', label: t(locale, 'du.running') }]} />;
}

const CODE = 's = 0.0\nfor i in range(n):\n    s += a[i] * b[i]';

function Fields() {
	const locale = useLocale(), n = (useVals(LESSON).n as number) ?? 3;
	const rows = useMemo(() => compRows(locale, n), [locale, n]);
	return <div className="not-content" data-du-fields="">{rows.map((r) => <FieldCell key={r.id + n} lesson={LESSON} row={r} size="sm" />)}</div>;
}

export type UnitPart = 'timeline' | 'formula' | 'fields' | 'table' | 'code';
const ALL: readonly UnitPart[] = ['timeline', 'formula', 'fields', 'table', 'code'];

function Controls({ parts = ALL }: { parts?: readonly UnitPart[]; locale?: Locale }) {
	const locale = useLocale();
	const has = (p: UnitPart) => parts.includes(p);
	return <section className="not-content" data-du-controls="">
		<HotLinks lesson={LESSON} links={LINKS_HOT} />
		{has('timeline') && <Timeline lesson={LESSON} id="unit" steps={STEP_KEYS.map((k, i) => ({ id: String(i), text: t(locale, k), focus: STEP_FOCUS[i] }))} />}
		{has('formula') && <UnitFormula />}
		{has('fields') && <Fields />}
		{has('table') && <UnitTable />}
		{has('code') && <CodeView lesson={LESSON} lang="python" code={CODE} lines={{ 1: 'sum', 2: 'n', 3: 'prod' }} />}
	</section>;
}
export const DotUnitControls = localized(Controls);
