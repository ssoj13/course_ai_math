import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { NumGrid, type GridCol, type GridRow } from '../NumTable';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { alignedRows } from '../tex';
import { blend, combo, combo3, CHECKS, CORNER, fmt1, fmt2, fmtV, INITIAL, JAW, LESSON, MOUTH, RANGE_S, RANGE_ST, RANGE_W, scaled, SMILE, sum } from './vector-arithmetic.math';
import type { Scene } from './vector-arithmetic.rows';

/**
 * Lesson 1.2 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store, the DOM-only controls
 * (formulas with scrubbable coefficients, component tables, readouts, challenges) and the lazy scene island.
 * Everything three lives in vector-arithmetic.impl.tsx. Reader-facing words come from the `va` dictionary namespace.
 */
lessonStore(LESSON, INITIAL);

export const ArithmeticScene = localized(lazyIsland(() => import('./vector-arithmetic.impl').then((m) => m.ArithmeticScene)));

type Vals = Parameters<typeof sum>[0];
const tex = String.raw;
const XYZ = ['x', 'y', 'z'] as const;

const comps = (prefix: string, get: (v: Vals) => readonly number[], dims = 2) =>
	Object.fromEntries(XYZ.slice(0, dims).map((a, i) => [prefix + a, (v: Vals) => get(v)[i]]));
/** `\s{key}` values of the formulas, by vector and axis letter. */
const derived: Record<string, (v: Vals) => number> = {
	...comps('a', (v) => sum(v).a), ...comps('b', (v) => sum(v).b), ...comps('s', (v) => sum(v).ab),
	...comps('c', (v) => scaled(v).c), ...comps('k', (v) => scaled(v).sc),
	...comps('la', (v) => combo(v).a), ...comps('lb', (v) => combo(v).b), ...comps('r', (v) => combo(v).r),
	...comps('r3', (v) => combo3(v).r, 3),
	...comps('p', (v) => blend(v).corner),
};
/** Scalars (s, t, u, weights) print with their own precision; blended positions need two decimals. */
const FMT: Record<string, (x: number) => string> = {
	...Object.fromEntries(Object.keys(derived).map((k) => [k, /^p[xy]$/.test(k) ? fmt2 : fmt1])),
	s: fmt1, ls: fmt1, lt: fmt1, s3: fmt1, t3: fmt1, u3: fmt1, w1: fmt2, w2: fmt2,
};

/** A live value after a plus, a dot or a minus is parenthesised when negative: "1.0 + (−1.0)". */
const par = (key: string, value: number) => (value < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);
const tuple = (prefix: string, dims = 2) => '(' + XYZ.slice(0, dims).map((a) => tex`\s{${prefix + a}}`).join(tex`,\;`) + ')';
/** A constant vector of the face data, "(0.2,\; 0.3)". */
const constV = (p: readonly number[]) => '(' + p.map((x) => fmt1(x)).join(tex`,\;`) + ')';

/**
 * Hover maps: a sum cell lights the pair it adds (storyboard: "pairwise highlight"), an arrow lights its column, the
 * result lights the pieces it is made of.
 */
const pairs = (out: string, ins: readonly string[], dims = 2) =>
	Object.fromEntries(XYZ.slice(0, dims).map((a) => [out + a, ins.map((i) => i + a)]));
const column = (id: string, prefix: string, dims = 2) => [id, XYZ.slice(0, dims).map((a) => prefix + a)] as const;
const HOT: HotLinkMap = {
	...pairs('s', ['a', 'b']), ...pairs('k', ['c']), ...pairs('r', ['sa', 'tb']), ...pairs('r3', ['sa3', 'tb3', 'uc3'], 3),
	...Object.fromEntries([
		column('a', 'a'), column('b', 'b'), column('sum', 's'), column('c', 'c'), column('sc', 'k'),
		column('sa', 'sa'), column('tb', 'tb'), column('r', 'r'),
		column('sa3', 'sa3', 3), column('tb3', 'tb3', 3), column('uc3', 'uc3', 3), column('r3', 'r3', 3),
	]),
	cornerP: ['corner'], corner: ['cornerP'],
};

// ---------------------------------------------------------------------------------------------------------------
// Formulas.

function ArithmeticFormula({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	const common = { lesson: LESSON, derived, fmt: FMT, block: true };
	if (scene === 1) {
		const z = sum(v);
		const comp = '(' + (['x', 'y'] as const).map((a, i) => tex`\t{s${a}}{\s{a${a}}+${par('b' + a, z.b[i])}}`).join(tex`,\;`) + ')';
		const head = tex`\t{sum}{\mathbf{a}+\mathbf{b}}`;
		return <Formula {...common} label={t(locale, 'va.formula1')} tex={tex`${head}=${comp}=${tuple('s')}`}
			texNarrow={alignedRows([tex`${head}=${comp}`, tex`\quad=${tuple('s')}`])} />;
	}
	if (scene === 2) {
		const z = scaled(v);
		const comp = '(' + (['x', 'y'] as const).map((a, i) => tex`\t{k${a}}{${par('s', z.s)}\cdot ${par('c' + a, z.c[i])}}`).join(tex`,\;`) + ')';
		const head = tex`\t{sc}{s\,\mathbf{a}}=${par('s', z.s)}\cdot${tuple('c')}`;
		return <Formula {...common} label={t(locale, 'va.formula2')} scrub={{ s: RANGE_S }}
			tex={alignedRows([tex`${head}`, tex`\quad=${comp}=${tuple('k')}`])}
			texNarrow={alignedRows([head, tex`\quad=${comp}`, tex`\quad=${tuple('k')}`])} />;
	}
	if (scene === 3) {
		const z = combo(v);
		const sa = tex`\t{sa}{\s{ls}\cdot${tuple('la')}}`, tb = tex`\t{tb}{${par('lt', z.t)}\cdot${tuple('lb')}}`;
		const head = tex`\t{r}{\mathbf{r}}=s\,\mathbf{a}+t\,\mathbf{b}`;
		return <Formula {...common} label={t(locale, 'va.formula3')} scrub={{ ls: RANGE_ST, lt: RANGE_ST }}
			tex={alignedRows([head, tex`\quad=${sa}+${tb}`, tex`\quad=${tuple('r')}`])}
			texNarrow={alignedRows([head, tex`\quad=${sa}`, tex`\qquad+${tb}`, tex`\quad=${tuple('r')}`])} />;
	}
	if (scene === 4) {
		const z = combo3(v);
		const body = tex`\t{sa3}{\s{s3}\,\mathbf{a}}+\t{tb3}{${par('t3', z.t)}\,\mathbf{b}}+\t{uc3}{${par('u3', z.u)}\,\mathbf{c}}`;
		return <Formula {...common} label={t(locale, 'va.formula4')} scrub={{ s3: RANGE_ST, t3: RANGE_ST, u3: RANGE_ST }}
			tex={alignedRows([tex`\t{r3}{\mathbf{r}}=${body}`, tex`\quad=${tuple('r3', 3)}`])} />;
	}
	const z = blend(v);
	const nums = tex`${constV(MOUTH[CORNER])}+\t{d1}{\s{w1}\cdot${constV(SMILE[CORNER])}}+\t{d2}{${par('w2', z.w2)}\cdot${constV(JAW[CORNER])}}`;
	return <Formula {...common} label={t(locale, 'va.formula5')} scrub={{ w1: RANGE_W, w2: RANGE_W }}
		tex={alignedRows([tex`\t{cornerP}{P'}=P+w_1\,\boldsymbol{\Delta}_1+w_2\,\boldsymbol{\Delta}_2`, tex`\quad=${nums}`, tex`\quad=${tuple('p')}`])}
		texNarrow={alignedRows([tex`\t{cornerP}{P'}=P+w_1\,\boldsymbol{\Delta}_1+w_2\,\boldsymbol{\Delta}_2`, tex`\quad=${constV(MOUTH[CORNER])}`,
			tex`\qquad+\t{d1}{\s{w1}\cdot${constV(SMILE[CORNER])}}`, tex`\qquad+\t{d2}{${par('w2', z.w2)}\cdot${constV(JAW[CORNER])}}`, tex`\quad=${tuple('p')}`])} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Component tables.

function ArithmeticTable({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	const table = (cols: readonly (GridCol & { prefix: string; p: readonly number[] })[]) => {
		const dims = cols[0].p.length;
		const rows: GridRow[] = XYZ.slice(0, dims).map((a, i) => ({ head: a, cells: Object.fromEntries(cols.map((c) => [c.id, { id: c.prefix + a, value: c.p[i] }])) }));
		return <NumGrid lesson={LESSON} caption={t(locale, 'va.table')} corner="" cols={cols} rows={rows} fmt={fmt1} />;
	};
	if (scene === 1) {
		const z = sum(v);
		return table([{ id: 'a', label: 'a', prefix: 'a', p: z.a }, { id: 'b', label: 'b', prefix: 'b', p: z.b }, { id: 's', label: 'a + b', prefix: 's', p: z.ab }]);
	}
	if (scene === 2) {
		const z = scaled(v);
		return table([{ id: 'c', label: 'a', prefix: 'c', p: z.c }, { id: 'k', label: 's·a', prefix: 'k', p: z.sc }]);
	}
	if (scene === 3) {
		const z = combo(v);
		return table([{ id: 'sa', label: 's·a', prefix: 'sa', p: z.sa }, { id: 'tb', label: 't·b', prefix: 'tb', p: z.tb }, { id: 'r', label: 'r', prefix: 'r', p: z.r }]);
	}
	const z = combo3(v);
	return table([{ id: 'sa3', label: 's·a', prefix: 'sa3', p: z.sa }, { id: 'tb3', label: 't·b', prefix: 'tb3', p: z.tb },
		{ id: 'uc3', label: 'u·c', prefix: 'uc3', p: z.uc }, { id: 'r3', label: 'r', prefix: 'r3', p: z.r }]);
}

// ---------------------------------------------------------------------------------------------------------------
// Readouts and challenges.

/** The live sentence under a scene: the canvas-free account of what it shows. */
export function readout(scene: Scene, locale: Locale, v: Vals): string {
	if (scene === 1) {
		const z = sum(v);
		if (z.ab.every((x) => Math.abs(x) < 1e-9)) return t(locale, 'va.read1Zero', { a: fmtV(z.a), b: fmtV(z.b) });
		return t(locale, 'va.read1', { a: fmtV(z.a), b: fmtV(z.b), ab: fmtV(z.ab), nab: fmt2(z.nab), sum: fmt2(z.na + z.nb) });
	}
	if (scene === 2) {
		const z = scaled(v);
		if (z.sense === 0) return t(locale, 'va.read2Zero');
		return t(locale, z.sense > 0 ? 'va.read2' : 'va.read2Neg', { s: fmt1(z.s), sc: fmtV(z.sc), f: fmt1(z.factor) });
	}
	if (scene === 3) {
		const z = combo(v);
		const base = t(locale, 'va.read3', { s: fmt1(z.s), t: fmt1(z.t), r: fmtV(z.r) });
		return base + (z.parallel ? t(locale, 'va.read3Line') : z.zero ? t(locale, 'va.read3Zero') : '');
	}
	if (scene === 4) {
		const z = combo3(v);
		return t(locale, 'va.read4', { r: fmtV(z.r) }) + (z.onPlane ? t(locale, 'va.read4Plane') : '');
	}
	const z = blend(v);
	return t(locale, 'va.read5', { p: fmtV(z.corner, fmt2), w1: fmt2(z.w1), w2: fmt2(z.w2) });
}

function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	switch (scene) {
		case 1: return [
			{ n: 1, text: t(locale, 'va.ch1'), check: CHECKS.cancel, success: t(locale, 'va.ch1Success') },
			{ n: 2, text: t(locale, 'va.ch2'), check: CHECKS.aligned, hint: [t(locale, 'va.ch2Hint')], success: t(locale, 'va.ch2Success') },
		];
		case 2: return [
			{ n: 3, text: t(locale, 'va.ch3'), check: CHECKS.flip },
			{ n: 4, text: t(locale, 'va.ch4'), check: CHECKS.zero, success: t(locale, 'va.ch4Success') },
		];
		case 3: return [
			{ n: 5, text: t(locale, 'va.ch5'), check: CHECKS.reach, hint: [t(locale, 'va.ch5Hint')] },
			{ n: 6, text: t(locale, 'va.ch6'), check: CHECKS.line, hint: [t(locale, 'va.ch6Hint')], success: t(locale, 'va.ch6Success') },
		];
		case 4: return [
			{ n: 7, text: t(locale, 'va.ch7'), check: CHECKS.up, hint: [t(locale, 'va.ch7Hint')] },
			{ n: 8, text: t(locale, 'va.ch8'), check: CHECKS.plane, success: t(locale, 'va.ch8Success') },
		];
		default: return [
			{ n: 9, text: t(locale, 'va.ch9'), check: CHECKS.level, hint: [t(locale, 'va.ch9Hint')] },
			{ n: 10, text: t(locale, 'va.ch10'), check: CHECKS.frown, success: t(locale, 'va.ch10Success') },
		];
	}
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	return <section className="not-content" data-va-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the first scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		<ArithmeticFormula scene={scene} />
		{scene !== 5 && <ArithmeticTable scene={scene} />}
		<p aria-live="polite">{readout(scene, locale, v)}</p>
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const ArithmeticControls = localized(Controls);
