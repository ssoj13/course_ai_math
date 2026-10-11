import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { NumGrid, type GridRow } from '../NumTable';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { alignedRows } from '../tex';
import { CHECKS, INITIAL, LESSON, LINKS, SPEC_TH, derive1, derive2, derive3, derive4, fmt, fmtDeg, tex3, texIn, texNum } from './basis.math';
import type { Scene } from './basis.rows';

/**
 * Lesson 1.6 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store with the orthonormal
 * link of BS-4, the DOM-only controls (formulas, the two-descriptions tables, the Gram matrix, readouts, challenges)
 * and the lazy scene island. Everything three lives in basis.impl.tsx. Reader-facing words come from the `bas`
 * dictionary namespace; numbers come from basis.math.ts, the one model the scenes draw.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const BasisScene = localized(lazyIsland(() => import('./basis.impl').then((m) => m.BasisScene)));

type Vals = Parameters<typeof derive1>[0];

const tex = String.raw;
const comp = (key: string, i: number) => (v: Vals) => (v[key] as readonly number[])[i];

/** Every number a formula shows as `\s{key}`; one model (basis.math.ts) behind all of them. */
const derived: Record<string, (v: Vals) => number> = {
	u1x: comp('u1', 0), u1y: comp('u1', 1), u2x: comp('u2', 0), u2y: comp('u2', 1), vx: comp('v', 0), vy: comp('v', 1),
	area: (v) => derive1(v).area, c1: (v) => derive1(v).c1, c2: (v) => derive1(v).c2,
	px: comp('p', 0), py: comp('p', 1),
	ru1x: (v) => derive3(v).u1[0], ru1y: (v) => derive3(v).u1[1], ru2x: (v) => derive3(v).u2[0], ru2y: (v) => derive3(v).u2[1],
	sc1: (v) => derive3(v).c1, sc2: (v) => derive3(v).c2, len2: (v) => derive3(v).len2, clen2: (v) => derive3(v).clen2,
	rd1: (v) => derive4(v).rd[0], rd2: (v) => derive4(v).rd[1], rd3: (v) => derive4(v).rd[2],
	q1: (v) => derive4(v).c?.[0] ?? NaN, q2: (v) => derive4(v).c?.[1] ?? NaN, q3: (v) => derive4(v).c?.[2] ?? NaN,
	gap: (v) => derive4(v).gap,
};
const FMT: Record<string, (x: number) => string> = {
	...Object.fromEntries(Object.keys(derived).map((k) => [k, texNum])),
	u1x: texIn, u1y: texIn, u2x: texIn, u2y: texIn, vx: texIn, vy: texIn, px: texIn, py: texIn,
	ru1x: tex3, ru1y: tex3, ru2x: tex3, ru2y: tex3,
	th: (x) => fmtDeg(x),
};

/** A live value after an operator is parenthesised when negative ("1·(−1)", not "1·−1"). */
const S = (v: Vals, key: string) => ((derived[key]?.(v) ?? (v[key] as number)) < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);
const U = (i: number | 'i') => tex`\mathbf{u}_${i}`;

/**
 * Hover maps: a hot id lights its linked ids. Table cells of a vector light the arrow they belong to; the dimension row
 * lights what the span is drawn as; a Gram cell lights its two rulers; the coordinates light the oblique walk.
 */
const HOT: HotLinkMap = {
	vx: ['v:x', 'v'], vy: ['v:y', 'v'], dim: ['lat_s', 'lat_t', 'span'],
	px: ['P'], py: ['P'], ax1: ['ru1'], ax2: ['ru2'],
	coords: ['q1', 'q2', 'q3'], ghost: ['gap'],
	...Object.fromEntries([1, 2, 3].flatMap((i) => [1, 2, 3].map((j) => [`g${i}${j}`, i === j ? [`r${i}`] : [`r${i}`, `r${j}`]]))),
};

// ---------------------------------------------------------------------------------------------------------------
// Formulas.

function BasisFormula({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	const common = { lesson: LESSON, derived, fmt: FMT, block: true };
	if (scene === 1) {
		const areaL = tex`\t{area}{u_{1x}\,u_{2y}-u_{1y}\,u_{2x}}`;
		const areaR = tex`=\s{u1x}\cdot ${S(v, 'u2y')}-${S(v, 'u1y')}\cdot ${S(v, 'u2x')}=\s{area}`;
		const comb = tex`\t{v}{\mathbf{v}}=\t{c1}{\s{c1}}\,\t{u1}{${U(1)}}+\t{c2}{${S(v, 'c2')}}\,\t{u2}{${U(2)}}`;
		const numsL = tex`(\s{vx},\s{vy})=${S(v, 'c1')}\cdot(\s{u1x},\s{u1y})`;
		const numsR = tex`+${S(v, 'c2')}\cdot(\s{u2x},\s{u2y})`;
		return <Formula {...common} label={t(locale, 'bas.formula1')}
			tex={alignedRows([['f1-area', areaL + areaR], ['f1-comb', comb], ['f1-nums', numsL + numsR]])}
			texNarrow={alignedRows([['f1-area', areaL], ['f1-area2', tex`\quad${areaR}`], ['f1-comb', comb], ['f1-nums', numsL], ['f1-nums2', tex`\quad${numsR}`]])} />;
	}
	if (scene === 3) {
		const th = tex`\t{th}{\theta}=\s{th}^\circ`;
		const c1a = tex`\t{sc1}{c_1}=\mathbf{P}\cdot\t{ru1}{${U(1)}}`;
		const c1b = tex`=${S(v, 'px')}\cdot ${S(v, 'ru1x')}+${S(v, 'py')}\cdot ${S(v, 'ru1y')}=\s{sc1}`;
		const c2a = tex`\t{sc2}{c_2}=\mathbf{P}\cdot\t{ru2}{${U(2)}}`;
		const c2b = tex`=${S(v, 'px')}\cdot ${S(v, 'ru2x')}+${S(v, 'py')}\cdot ${S(v, 'ru2y')}=\s{sc2}`;
		const len = tex`\t{clen2}{c_1^2+c_2^2}=\s{clen2}=\t{len2}{x^2+y^2}`;
		return <Formula {...common} label={t(locale, 'bas.formula3')} scrub={{ th: SPEC_TH }}
			tex={alignedRows([['f3-th', th], ['f3-c1', c1a + c1b], ['f3-c2', c2a + c2b], ['f3-len', len]])}
			texNarrow={alignedRows([['f3-th', th], ['f3-c1', c1a], ['f3-c1b', c1b], ['f3-c2', c2a], ['f3-c2b', c2b], ['f3-len', len]])} />;
	}
	const reads = [1, 2, 3].map((i) => tex`\t{rd${i}}{${U(i)}\cdot\mathbf{v}}=\s{rd${i}}`);
	const coords = tex`\t{coords}{(c_1,c_2,c_3)}=(\s{q1},\s{q2},\s{q3})`;
	const gap = tex`\t{gap}{\Bigl\lvert\textstyle\sum_i(${U('i')}\cdot\mathbf{v})\,${U('i')}-\mathbf{v}\Bigr\rvert}=\s{gap}`;
	return <Formula {...common} label={t(locale, 'bas.formula4')}
		tex={alignedRows([['f4-rd', reads.join(tex`,\quad `)], ['f4-c', coords], ['f4-gap', gap]])}
		texNarrow={alignedRows([...reads.map((r, i): [string, string] => ['f4-rd' + i, r]), ['f4-c', coords], ['f4-gap', gap]])} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Tables.

/** A table cell; a value that does not exist (lost coordinates) leaves the cell blank, the readout says why. */
const cell = (id: string, value: number) => (Number.isFinite(value) ? { id, value } : undefined);

/** BS-1 / BS-3: the same arrow described in two bases, one row per basis. */
function Descriptions({ scene }: { scene: 1 | 3 }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	if (scene === 1) {
		const z = derive1(v);
		const rows: GridRow[] = [
			{ head: 'e₁, e₂', cells: { a: cell('vx', z.v[0]), b: cell('vy', z.v[1]) } },
			{ head: 'u₁, u₂', cells: { a: cell('c1', z.c1), b: cell('c2', z.c2) } },
		];
		return <NumGrid lesson={LESSON} caption={t(locale, 'bas.descCaption1')} corner={t(locale, 'bas.colBasis')} rows={rows} fmt={fmt}
			cols={[{ id: 'a', label: t(locale, 'bas.colFirst') }, { id: 'b', label: t(locale, 'bas.colSecond') }]} />;
	}
	const z = derive3(v);
	const rows: GridRow[] = [
		{ head: 'e₁, e₂', cells: { a: cell('px', z.p[0]), b: cell('py', z.p[1]), s: cell('len2', z.len2) } },
		{ head: 'u₁, u₂', cells: { a: cell('sc1', z.c1), b: cell('sc2', z.c2), s: cell('clen2', z.clen2) } },
	];
	return <NumGrid lesson={LESSON} caption={t(locale, 'bas.descCaption3')} corner={t(locale, 'bas.colBasis')} rows={rows} fmt={fmt}
		cols={[{ id: 'a', label: t(locale, 'bas.colFirst') }, { id: 'b', label: t(locale, 'bas.colSecond') }, { id: 's', label: t(locale, 'bas.colSquares') }]} />;
}

const RULER_COLS = [{ id: '1', label: 'u₁' }, { id: '2', label: 'u₂' }, { id: '3', label: 'u₃' }];

/** BS-4: the Gram matrix BᵀB (entry ij = uᵢ·uⱼ) and the readings next to the true coordinates. */
function RulerTables() {
	const locale = useLocale();
	const z = derive4(useVals(LESSON));
	const gram: GridRow[] = [0, 1, 2].map((i) => ({
		head: 'u' + '₁₂₃'[i],
		cells: Object.fromEntries([0, 1, 2].map((j) => [String(j + 1), cell(`g${i + 1}${j + 1}`, z.g[i][j])])),
	}));
	const reads: GridRow[] = [
		{ head: 'uᵢ·v', cells: Object.fromEntries([0, 1, 2].map((i) => [String(i + 1), cell(`rd${i + 1}`, z.rd[i])])) },
		{ head: 'cᵢ', cells: Object.fromEntries([0, 1, 2].map((i) => [String(i + 1), cell(`q${i + 1}`, z.c ? z.c[i] : NaN)])) },
	];
	return <>
		<NumGrid lesson={LESSON} caption={t(locale, 'bas.gramCaption')} corner="·" rows={gram} cols={RULER_COLS} fmt={fmt} />
		<NumGrid lesson={LESSON} caption={t(locale, 'bas.readCaption')} corner="" rows={reads} cols={RULER_COLS} fmt={fmt} />
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Readouts and challenges.

const mark = (ok: boolean) => (ok ? '✓' : '✗');

/**
 * The live sentence under a scene and every sentence the same scene can produce (for StableText's reserved height).
 * Exported for the tests.
 */
export function readouts(scene: Scene, locale: Locale, v: Vals): { text: string; variants: string[] } {
	if (scene === 1) {
		const z = derive1(v);
		const plane = t(locale, 'bas.read1', { area: fmt(z.area), c1: fmt(z.c1), c2: fmt(z.c2) });
		const variants = [plane, plane + t(locale, 'bas.read1Flip'), t(locale, 'bas.read1LineOn'), t(locale, 'bas.read1LineOff'), t(locale, 'bas.read1Point')];
		const text = z.dim === 2 ? (z.area < 0 ? variants[1] : variants[0]) : z.dim === 1 ? (z.onLine ? variants[2] : variants[3]) : variants[4];
		return { text, variants };
	}
	if (scene === 2) {
		const z = derive2(v);
		const variants = [
			t(locale, 'bas.read2Space', { vol: fmt(z.vol) }), t(locale, 'bas.read2Plane'), t(locale, 'bas.read2Line'), t(locale, 'bas.read2Point'),
		];
		return { text: variants[3 - z.dim], variants };
	}
	if (scene === 3) {
		const z = derive3(v);
		const text = t(locale, 'bas.read3', { th: fmtDeg(z.th), c1: fmt(z.c1), c2: fmt(z.c2), len2: fmt(z.len2) });
		return { text, variants: [text] };
	}
	const z = derive4(v);
	const r = { r1: fmt(z.rd[0]), r2: fmt(z.rd[1]), r3: fmt(z.rd[2]) };
	const c = z.c ? { c1: fmt(z.c[0]), c2: fmt(z.c[1]), c3: fmt(z.c[2]) } : { c1: '–', c2: '–', c3: '–' };
	const checks = t(locale, 'bas.read4Checks', {
		n1: mark(z.unit[0]), n2: mark(z.unit[1]), n3: mark(z.unit[2]), p12: mark(z.perp[0]), p13: mark(z.perp[1]), p23: mark(z.perp[2]),
	});
	const variants = [
		t(locale, 'bas.read4Ortho', { ...c }), t(locale, 'bas.read4Oblique', { ...r, ...c, gap: fmt(z.gap) }), t(locale, 'bas.read4Flat'),
	].map((s) => checks + ' ' + s);
	const text = !z.c ? variants[2] : z.orthonormal ? variants[0] : variants[1];
	return { text, variants };
}

function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'bas.ch1'), check: CHECKS.collapse, hint: [t(locale, 'bas.ch1Hint')] },
			{ n: 2, text: t(locale, 'bas.ch2'), check: CHECKS.readsTwoOne, hint: [t(locale, 'bas.ch2Hint')] },
		];
	}
	if (scene === 2) {
		return [
			{ n: 3, text: t(locale, 'bas.ch3'), check: CHECKS.space, hint: [t(locale, 'bas.ch3Hint')] },
			{ n: 4, text: t(locale, 'bas.ch4'), check: CHECKS.otherPlane, hint: [t(locale, 'bas.ch4Hint')] },
		];
	}
	if (scene === 3) {
		return [
			{ n: 5, text: t(locale, 'bas.ch5'), check: CHECKS.onAxis, hint: [t(locale, 'bas.ch5Hint')] },
			{ n: 6, text: t(locale, 'bas.ch6'), check: CHECKS.swapped, success: t(locale, 'bas.ch6Success') },
		];
	}
	return [
		{ n: 7, text: t(locale, 'bas.ch7'), check: CHECKS.ghostMiss, hint: [t(locale, 'bas.ch7Hint')] },
		{ n: 8, text: t(locale, 'bas.ch8'), check: CHECKS.backOn, success: t(locale, 'bas.ch8Success') },
	];
}

/**
 * Live text whose box is as tall as its tallest variant, so a write in one card never moves the card being dragged
 * (all scenes share one store). Pure CSS, so the server render reserves it too.
 */
function StableText({ text, variants }: { text: string; variants: readonly string[] }) {
	return <div data-bas-readout="" style={{ display: 'grid' }}>
		{variants.map((x, i) => <p key={i} aria-hidden="true" style={{ gridArea: '1 / 1', visibility: 'hidden', margin: 0 }}>{x}</p>)}
		<p aria-live="polite" style={{ gridArea: '1 / 1', margin: 0 }}>{text}</p>
	</div>;
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	return <section className="not-content" data-bas-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		{scene !== 2 && <BasisFormula scene={scene} />}
		{(scene === 1 || scene === 3) && <Descriptions scene={scene} />}
		{scene === 4 && <RulerTables />}
		<StableText {...readouts(scene, locale, v)} />
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const BasisControls = localized(Controls);
