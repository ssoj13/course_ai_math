import { useStore } from 'zustand';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { NumTable } from '../NumTable';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { CHECKS, derive, fmt1, fmt2, fmt4, fmtInt, INITIAL, LESSON, LINKS, SPEC } from './squares-and-plane.math';
import type { Scene } from './squares-and-plane.rows';

/**
 * Lesson 0.G entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store with its linked
 * x <-> A and P writes, the DOM-only controls (formulas, the two-readings table, challenges, live readouts) and the
 * lazy scene islands. Everything three lives in squares-and-plane.impl.tsx. Reader-facing words come from the `sq`
 * dictionary namespace.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const SquaresScene = localized(lazyIsland(() => import('./squares-and-plane.impl').then((m) => m.SquaresScene)));

const tex = String.raw;

/** Every number a formula shows as `\s{key}`; one `derive()` for the lesson. */
const KEYS = ['x2', 'whole', 'partial', 'px', 'py', 'px2', 'py2', 'sum', 'sqrtA', 'check'] as const;
const derived = Object.fromEntries(KEYS.map((k) => [k, (v: Parameters<typeof derive>[0]) => derive(v)[k]]));
const FMT: Record<string, (v: number) => string> = {
	x: fmt1, x2: fmt2, whole: fmtInt, partial: fmt2, px: fmt1, py: fmt1, px2: fmt2, py2: fmt2, sum: fmt2, A: fmt2, sqrtA: fmt4, check: fmt2,
};

/** A formula row carrying a mark id (`data-m`), so it can be hidden or dimmed. */
const row = (key: string, body: string) => tex`\htmlData{m=${key}}{${body}}`;
const aligned = (rows: string[]) => tex`\begin{aligned}${rows.map((r) => '&' + r).join(tex`\\ `)}\end{aligned}`;

/**
 * Hover maps (storyboard §7.3-§7.5). The 0.G-1 and 0.G-3 squares are one square (|x| = √A), so their shared ids
 * light both. NumTable's component cells a0/a1 are the two readings px/py.
 */
const HOT: HotLinkMap = {
	x: ['side'], side: ['x'], x2: ['sq'], sq: ['x2'], A: ['sq'], sqrtA: ['side'], check: ['sq'],
	px: ['legx', 'footx', 'a0'], legx: ['px'], footx: ['px'], a0: ['px'],
	py: ['legy', 'footy', 'a1'], legy: ['py'], footy: ['py'], a1: ['py'],
	px2: ['legx'], py2: ['legy'], sum: ['px2', 'py2'],
};

/** A side written after a dot or an equals sign is parenthesised when negative: "(−3.0)·(−3.0)", never "−3.0·−3.0". */
const side = (x: number) => (x < 0 ? tex`(\s{x})` : tex`\s{x}`);

/**
 * The scene formula with its second line (whole tiles + partial area, shown while there is a partial area; the 0.G-2 squares line dims until a SQUARES
 * row, term or tile is hot).
 */
function SquaresFormula({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	const hot = useStore(lessonStore(LESSON), (s) => s.hot);
	const z = derive(v);
	if (scene === 1) {
		const first = tex`\t{x2}{x^2}=${side(z.x)}\cdot${side(z.x)}=\s{x2}`;
		return <Formula lesson={LESSON} label={t(locale, 'sq.formulaSquare')} block
			tex={aligned([row('sq-1', first), row('sq-2', tex`=\t{whole}{\s{whole}}+\t{partial}{\s{partial}}`)])}
			marks={{ 'sq-2': { hidden: !(z.partial > 0) } }}
			scrub={{ x: SPEC.x }} derived={derived} fmt={FMT}
			tips={{ whole: { text: t(locale, 'sq.tipWhole') }, partial: { text: t(locale, 'sq.tipPartial') } }} />;
	}
	if (scene === 2) {
		const first = tex`P=(\t{px}{\s{px}},\;\t{py}{\s{py}})`;
		const squares = ['px2', 'py2', 'sum'].some((id) => hot.has(id));
		return <Formula lesson={LESSON} label={t(locale, 'sq.formulaPoint')} block
			tex={aligned([first, row('sq-sum', tex`\t{px2}{x^2}+\t{py2}{y^2}=\s{px2}+\s{py2}=\t{sum}{\s{sum}}`)])}
			marks={{ 'sq-sum': { dim: !squares } }} derived={derived} fmt={FMT}
			tips={{ sum: { text: t(locale, 'sq.tipSum') } }} />;
	}
	const first = tex`\t{sqrtA}{\sqrt{\s{A}}}=\s{sqrtA}`;
	return <Formula lesson={LESSON} label={t(locale, 'sq.formulaRoot')} block
		tex={aligned([first, tex`\t{check}{(\sqrt{A})^2}=\s{check}`])}
		scrub={{ A: SPEC.A }} derived={derived} fmt={FMT}
		tips={{ check: { text: t(locale, 'sq.tipCheck') } }} />;
}

/** Challenge cards per scene (§7.3-§7.5); every predicate reads store values only (keyboard / typing solves it). */
function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'sq.ch1'), check: CHECKS.sixteen },
			{ n: 2, text: t(locale, 'sq.ch2'), check: CHECKS.negative, success: t(locale, 'sq.ch2Success') },
		];
	}
	if (scene === 2) {
		return [
			{ n: 3, text: t(locale, 'sq.ch3'), check: CHECKS.pointTyped, hint: [t(locale, 'sq.ch3Hint')] },
			{ n: 4, text: t(locale, 'sq.ch4'), check: CHECKS.sumOther, hint: [t(locale, 'sq.ch4Hint')] },
		];
	}
	return [
		{ n: 5, text: t(locale, 'sq.ch5'), check: CHECKS.wholeSide },
		{ n: 6, text: t(locale, 'sq.ch6'), check: CHECKS.sideHalf, hint: [t(locale, 'sq.ch6Hint')] },
	];
}

/** The live sentence under the formula (the canvas-free account of what the scene shows). */
export function readout(scene: Scene, locale: Locale, z: ReturnType<typeof derive>): string {
	if (scene === 1) {
		if (z.x === 0) return t(locale, 'sq.read1Zero');
		const tiles = t(locale, 'sq.read1', { x: fmt1(z.x), x2: fmt2(z.x2), whole: fmtInt(z.whole) })
			+ (z.partial > 0 ? t(locale, 'sq.read1Partial', { partial: fmt2(z.partial), pieces: fmtInt(z.partialPieces) }) : '.');
		return tiles + (z.x < 0 ? t(locale, 'sq.read1Negative') : '');
	}
	if (scene === 2) return t(locale, 'sq.read2', { px: fmt1(z.px), py: fmt1(z.py), px2: fmt2(z.px2), py2: fmt2(z.py2), sum: fmt2(z.sum) });
	if (z.A === 0) return t(locale, 'sq.read3Zero');
	return t(locale, 'sq.read3', { A: fmt2(z.A), sqrtA: fmt4(z.sqrtA), check: fmt2(z.check), whole: fmtInt(z.whole3) })
		+ (z.partial3 > 0 ? t(locale, 'sq.read3Partial', { partial: fmt2(z.partial3), pieces: fmtInt(z.partialPieces3) }) : '.');
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const z = derive(useVals(LESSON));
	return <section className="not-content" data-sq-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		<SquaresFormula scene={scene} />
		{scene === 2 && <NumTable lesson={LESSON} rows={[{ label: 'P', key: 'P' }]} op="components" labels={['x', 'y']} />}
		<p aria-live="polite">{readout(scene, locale, z)}</p>
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const SquaresControls = localized(Controls);
