import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { NumGrid, type GridCol, type GridRow } from '../NumTable';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { alignedRows } from '../tex';
import { arrow2, CHECKS, fmt1, fmt2, fmtV, INITIAL, LESSON, LINKS, translate, trip } from './vector.math';
import type { Scene } from './vector.rows';

/**
 * Lesson 1.1 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store with VE-2's free-arrow link,
 * the DOM-only controls (formulas, component tables, readouts, challenges) and the lazy scene island. Everything three
 * lives in vector.impl.tsx. Reader-facing words come from the `ve` dictionary namespace.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const VectorScene = localized(lazyIsland(() => import('./vector.impl').then((m) => m.VectorScene)));

type Vals = Parameters<typeof arrow2>[0];
const tex = String.raw;
const XYZ = ['x', 'y', 'z'] as const;

/** `\s{key}` values of the formulas: every component of every vector the scenes draw, by axis letter. */
const comps = (prefix: string, get: (v: Vals) => readonly number[], dims = 2) =>
	Object.fromEntries(XYZ.slice(0, dims).map((a, i) => [prefix + a, (v: Vals) => get(v)[i]]));
const derived: Record<string, (v: Vals) => number> = {
	...comps('v', (v) => v.v as number[]),
	...comps('w', (v) => arrow2(v).w),
	...comps('h', (v) => arrow2(v).to),
	...comps('t', (v) => arrow2(v).from),
	...comps('p', (v) => translate(v).before[1]),
	...comps('q', (v) => translate(v).after[1]),
	...comps('T', (v) => translate(v).T),
	...comps('n', (v) => translate(v).N),
	...comps('m', (v) => translate(v).N2),
	wN: (v) => translate(v).asPoint,
	...comps('v3', (v) => v.v3 as number[], 3),
	...comps('e', (v) => trip(v).end),
	...comps('s', (v) => trip(v).start),
	...comps('d', (v) => trip(v).d),
	len: (v) => trip(v).len,
	...comps('d3', (v) => trip(v, true).d, 3),
	len3: (v) => trip(v, true).len,
};
const FMT: Record<string, (x: number) => string> = Object.fromEntries(Object.keys(derived).map((k) => [k, /^(n|m)[xy]$|^len/.test(k) ? fmt2 : k === 'wN' ? String : fmt1]));

/** A live value after a minus, a dot or under a square is parenthesised when negative: "4.0 − (−1.0)". */
const par = (key: string, value: number) => (value < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);
/** "(\s{px}, \s{py})" tuple of live components. */
const tuple = (prefix: string, dims = 2) => '(' + XYZ.slice(0, dims).map((a) => tex`\s{${prefix + a}}`).join(tex`,\;`) + ')';
/** The same tuple with each component a hover term `${prefix}x` (formula ↔ table ↔ leg). */
const terms = (prefix: string, dims = 2) => '(' + XYZ.slice(0, dims).map((a) => tex`\t{${prefix + a}}{\s{${prefix + a}}}`).join(tex`,\;`) + ')';

/**
 * Hover maps: a component term or table cell lights its dashed leg (`${arrow}:x`) and the leg lights them back;
 * point coordinates light their point.
 */
const HOT: HotLinkMap = {
	...Object.fromEntries([['v', 'v'], ['w', 'w'], ['v3', 'v3'], ['d', 'd'], ['d3', 'd3']].flatMap(([term, arrow]) =>
		XYZ.flatMap((a) => [[term + a, [arrow + ':' + a]], [arrow + ':' + a, [term + a]]]))),
	ax: ['A'], ay: ['A'], bx: ['B'], by: ['B'], a3x: ['A'], a3y: ['A'], a3z: ['A'], b3x: ['B'], b3y: ['B'], b3z: ['B'],
	T: ['obj'], obj: ['T'],
};

// ---------------------------------------------------------------------------------------------------------------
// Formulas.

function VectorFormula({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	const common = { lesson: LESSON, derived, fmt: FMT, block: true };
	if (scene === 1) return <Formula {...common} label={t(locale, 've.formula1')} tex={tex`\mathbf{v}=${terms('v')}`} />;
	if (scene === 2) {
		const a = arrow2(v);
		if (a.anchor) return <Formula {...common} label={t(locale, 've.formula2Anchored')} tex={tex`\t{head}{P}=(0,\;0)+\mathbf{v}=${terms('w')}`} />;
		const end = tex`\t{head}{${tuple('h')}}`, start = tex`\t{tail}{${tuple('t')}}`;
		return <Formula {...common} label={t(locale, 've.formula2')}
			tex={tex`\mathbf{v}=${end}-${start}=${terms('w')}`}
			texNarrow={alignedRows([tex`\mathbf{v}=${end}-${start}`, tex`\quad=${terms('w')}`])} />;
	}
	if (scene === 3) {
		const T = tex`\t{T}{${tuple('T')}}`;
		const p = tex`\t{newP1}{P_1'}=\t{oldP1}{${tuple('p')}}+1\cdot${T}`, n = tex`\t{N2}{\mathbf{N}'}=\t{N}{${tuple('n')}}+\s{wN}\cdot${T}`;
		return <Formula {...common} label={t(locale, 've.formula3')}
			tex={alignedRows([tex`${p}=${tuple('q')}`, tex`${n}=${tuple('m')}`])}
			texNarrow={alignedRows([p, tex`\quad=${tuple('q')}`, n, tex`\quad=${tuple('m')}`])} />;
	}
	if (scene === 4) return <Formula {...common} label={t(locale, 've.formula4')} tex={tex`\mathbf{v}=${terms('v3', 3)}`} />;
	if (scene === 5) {
		const z = trip(v);
		const [E, S] = z.rev ? ['A', 'B'] : ['B', 'A'];
		const diff = '(' + (['x', 'y'] as const).map((a, i) => tex`\t{d${a}}{\s{e${a}}-${par('s' + a, z.start[i])}}`).join(tex`,\;`) + ')';
		const len = tex`\t{len}{\lvert\mathbf{v}\rvert}=\sqrt{${par('dx', z.d[0])}^2+${par('dy', z.d[1])}^2}=\s{len}`;
		return <Formula {...common} label={t(locale, 've.formula5')}
			tex={alignedRows([tex`\mathbf{v}=${E}-${S}=${diff}=${tuple('d')}`, len])}
			texNarrow={alignedRows([tex`\mathbf{v}=${E}-${S}=${diff}`, tex`\quad=${tuple('d')}`, len])} />;
	}
	return <Formula {...common} label={t(locale, 've.formula6')}
		tex={alignedRows([tex`\mathbf{v}=B-A=${terms('d3', 3)}`, tex`\t{len3}{\lvert\mathbf{v}\rvert}=\s{len3}`])} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Component tables: one row per axis, one column per vector, cells carrying the hover ids.

function grid(dims: number, cols: readonly { col: string; prefix: string; value: (i: number) => number }[]): GridRow[] {
	return XYZ.slice(0, dims).map((a, i) => ({ head: a, cells: Object.fromEntries(cols.map((c) => [c.col, { id: c.prefix + a, value: c.value(i) }])) }));
}

function VectorTable({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	const show = (cols: GridCol[], rows: GridRow[]) =>
		<NumGrid lesson={LESSON} caption={t(locale, scene === 5 || scene === 6 ? 've.tableTrip' : 've.tableList')} corner="" cols={cols} rows={rows} fmt={fmt1} />;
	if (scene === 1 || scene === 4) {
		const p = (scene === 1 ? v.v : v.v3) as number[];
		return show([{ id: 'v', label: 'v' }], grid(p.length, [{ col: 'v', prefix: scene === 1 ? 'v' : 'v3', value: (i) => p[i] }]));
	}
	if (scene === 2) {
		const a = arrow2(v);
		return show([{ id: 'h', label: t(locale, 've.colEnd') }, { id: 't', label: t(locale, 've.colStart') }, { id: 'w', label: 'v' }],
			grid(2, [{ col: 'h', prefix: 'h', value: (i) => a.to[i] }, { col: 't', prefix: 't', value: (i) => a.from[i] }, { col: 'w', prefix: 'w', value: (i) => a.w[i] }]));
	}
	const three = scene === 6, z = trip(v, three), pre = three ? '3' : '';
	const B = { col: 'B', prefix: 'b' + pre, value: (i: number) => z.B[i] }, A = { col: 'A', prefix: 'a' + pre, value: (i: number) => z.A[i] };
	const D = { col: 'd', prefix: 'd' + pre, value: (i: number) => z.d[i] };
	const cols = z.rev ? [{ id: 'A', label: 'A' }, { id: 'B', label: 'B' }, { id: 'd', label: 'A − B' }] : [{ id: 'B', label: 'B' }, { id: 'A', label: 'A' }, { id: 'd', label: 'B − A' }];
	return show(cols, grid(three ? 3 : 2, [B, A, D]));
}

// ---------------------------------------------------------------------------------------------------------------
// Readouts and challenges.

/** The live sentence under a scene: the canvas-free account of what it shows. */
export function readout(scene: Scene, locale: Locale, v: Vals): string {
	if (scene === 1 || scene === 4) {
		const p = (scene === 1 ? v.v : v.v3) as number[];
		if (p.every((x) => x === 0)) return t(locale, 've.readZero');
		const [x, y, z] = p.map(fmt1);
		return scene === 1 ? t(locale, 've.read1', { v: fmtV(p), x, y }) : t(locale, 've.read4', { v: fmtV(p), x, y, z: z! });
	}
	if (scene === 2) {
		const a = arrow2(v);
		if (a.anchor) return t(locale, 've.read2Anchored', { p: fmtV(a.w) });
		return t(locale, 've.read2', { start: fmtV(a.from), end: fmtV(a.to), v: fmtV(a.w) });
	}
	if (scene === 3) {
		const z = translate(v);
		if (z.asPoint) return t(locale, 've.read3AsPoint', { T: fmtV(z.T), n: fmtV(z.N2, fmt2), len: fmt2(z.lenN2) });
		return t(locale, 've.read3', { T: fmtV(z.T), n: fmtV(z.N, fmt2) });
	}
	const z = trip(v, scene === 6);
	if (z.len < 1e-9) return t(locale, 've.readSame');
	if (z.rev) return t(locale, 've.read5Back', { d: fmtV(z.d), len: fmt2(z.len) });
	return t(locale, 've.read5', { d: fmtV(z.d), len: fmt2(z.len) });
}

function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	switch (scene) {
		case 1: return [
			{ n: 1, text: t(locale, 've.ch1'), check: CHECKS.v1 },
			{ n: 2, text: t(locale, 've.ch2'), check: CHECKS.down, hint: [t(locale, 've.ch2Hint')] },
		];
		case 2: return [
			{ n: 3, text: t(locale, 've.ch3'), check: CHECKS.slide, hint: [t(locale, 've.ch3Hint')], success: t(locale, 've.ch3Success') },
			{ n: 4, text: t(locale, 've.ch4'), check: CHECKS.position, success: t(locale, 've.ch4Success') },
		];
		case 3: return [
			{ n: 5, text: t(locale, 've.ch5'), check: CHECKS.move33, success: t(locale, 've.ch5Success') },
			{ n: 6, text: t(locale, 've.ch6'), check: CHECKS.bug, success: t(locale, 've.ch6Success') },
		];
		case 4: return [
			{ n: 7, text: t(locale, 've.ch7'), check: CHECKS.up, hint: [t(locale, 've.ch7Hint')] },
			{ n: 8, text: t(locale, 've.ch8'), check: CHECKS.flat, hint: [t(locale, 've.ch7Hint')] },
		];
		case 5: return [
			{ n: 9, text: t(locale, 've.ch9'), check: CHECKS.back, hint: [t(locale, 've.ch9Hint')] },
			{ n: 10, text: t(locale, 've.ch10'), check: CHECKS.origin, hint: [t(locale, 've.ch10Hint')], success: t(locale, 've.ch10Success') },
		];
		default: return [{ n: 11, text: t(locale, 've.ch11'), check: CHECKS.trip3, hint: [t(locale, 've.ch11Hint')] }];
	}
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	return <section className="not-content" data-ve-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the first scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		<VectorFormula scene={scene} />
		{scene !== 3 && <VectorTable scene={scene} />}
		<p aria-live="polite">{readout(scene, locale, v)}</p>
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const VectorControls = localized(Controls);
