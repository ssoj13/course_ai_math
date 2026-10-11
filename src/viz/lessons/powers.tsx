import { t, type Locale, type TextKey } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Timeline } from '../Timeline';
import { alignedRows } from '../tex';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import {
	CHECKS, derive, fmt2, fmt4, fmtG, fmtAmt, fmtInt, fmtSci, fmtVal, fmtX, INITIAL, LESSON, LINKS, PRESETS, RUNGS, rungId, SPEC, sup,
} from './powers.math';
import { powLabel, type Scene } from './powers.rows';

/**
 * Lesson 0.2 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store with its linked
 * x <-> A <-> V writes and the n / k timeline adapters, the DOM-only controls (formulas, power chips, timelines,
 * presets, challenges, live readouts) and the lazy scene islands. Everything three lives in powers.impl.tsx.
 * Reader-facing words come from the `pw` dictionary namespace.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const PowersScene = localized(lazyIsland(() => import('./powers.impl').then((m) => m.PowersScene)));

const tex = String.raw;
type Vals = Parameters<typeof derive>[0];

/** Every number a formula shows as `\s{key}` that is not a store key; one `derive()` for the lesson. */
const KEYS = ['x2', 'x3', 'xn', 'sqrtA', 'check2', 'cbrtV', 'check3', 'cur', 'prevval', 'y', 'yinv'] as const;
const derived = Object.fromEntries(KEYS.map((k) => [k, (v: Vals) => derive(v)[k]]));
const FMT: Record<string, (v: number) => string> = {
	x: fmtX, A: fmtAmt, V: fmtAmt, x2: fmtAmt, x3: fmtAmt, xn: fmtAmt, n: fmtInt, k: fmtInt, base: fmtVal, sqrtA: fmt4, check2: fmtAmt,
	cbrtV: fmt4, check3: fmtAmt, cur: fmtVal, prevval: fmtVal, g: fmtG, t: fmt2, y: fmt4, yinv: fmt4,
};

/**
 * Hover maps (storyboard §7.7-§7.11): the field, the scene object and the formula term of one number light together.
 * Rung ids p3 .. pm3 are shared by the rung, its label and its row, so they need no entry.
 */
const HOT: HotLinkMap = {
	x: ['side', 'top', 'edgez', 'edgey'], side: ['x'], top: ['x'], edgez: ['x'], edgey: ['x'],
	x2: ['sq'], sq: ['x2'], A: ['sq', 'root'], sqrtA: ['rootside'], rootside: ['sqrtA', 'cbrtV'],
	xn: ['obj'], obj: ['xn'], V: ['root'], cbrtV: ['rootside'], check2: ['root'], check3: ['root'], root: ['A', 'V'],
	cur: [], prevval: [], g: ['curve'], curve: ['g'], y: ['pt'], pt: ['y'], inv: ['yinv'], yinv: ['inv'],
};

/** The scene formulas (§7.7-§7.11 "Formula"); live numbers are `\s{}` terms, ids are `\t{}` terms. */
function PowersFormula({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const z = derive(useVals(LESSON));
	if (scene === 1) {
		return <Formula lesson={LESSON} label={t(locale, 'pw.formulaSquare')} block
			tex={alignedRows([tex`\t{x2}{x^2}=\s{x}\cdot\s{x}=\s{x2}`, tex`\s{x}=\t{sqrtA}{\sqrt{\s{A}}}`])}
			scrub={{ x: SPEC.x, A: SPEC.A }} derived={derived} fmt={FMT}
			tips={{ sqrtA: { text: t(locale, 'pw.tipRootLine') } }} />;
	}
	if (scene === 2) {
		// The expansion has as many factors as the power: x, x·x, x·x·x.
		const factors = Array.from({ length: z.n }, () => tex`\s{x}`).join(tex`\cdot`);
		return <Formula lesson={LESSON} label={t(locale, 'pw.formulaPower')} block
			tex={alignedRows([tex`\t{xn}{x^{\s{n}}}=\s{xn}`, tex`=${factors}`])}
			scrub={{ n: SPEC.n, x: SPEC.x }} derived={derived} fmt={FMT}
			tips={{ xn: { text: t(locale, 'pw.tipXn') } }} />;
	}
	if (scene === 3) {
		// Line 2 (the division that led here) does not exist on the top rung.
		const rows = alignedRows([
			tex`\t{cur}{\s{base}^{\s{k}}}=\s{cur}`,
			['pw-prev', tex`=\t{prevval}{\s{base}^{${z.k + 1}}}/\s{base}=\s{prevval}/\s{base}`],
		], { 'pw-prev': { hidden: z.k === 3 } });
		return <Formula lesson={LESSON} label={t(locale, 'pw.formulaLadder')} block tex={rows}
			scrub={{ base: SPEC.base, k: SPEC.k }} derived={derived} fmt={FMT}
			tips={{ prevval: { text: t(locale, 'pw.tipPrev') } }} />;
	}
	if (scene === 4) {
		return z.d3 === 1
			? <Formula lesson={LESSON} label={t(locale, 'pw.formulaCbrt')} block
				tex={alignedRows([tex`\t{cbrtV}{\s{V}^{1/3}}=\s{cbrtV}`, tex`\t{check3}{(\s{cbrtV})^3}=\s{check3}`])}
				scrub={{ V: SPEC.V }} derived={derived} fmt={FMT} tips={{ check3: { text: t(locale, 'pw.tipCheck3') } }} />
			: <Formula lesson={LESSON} label={t(locale, 'pw.formulaSqrt')} block
				tex={alignedRows([tex`\t{sqrtA}{\s{A}^{1/2}}=\sqrt{\s{A}}=\s{sqrtA}`, tex`\t{check2}{(\s{sqrtA})^2}=\s{check2}`])}
				scrub={{ A: SPEC.A }} derived={derived} fmt={FMT} tips={{ check2: { text: t(locale, 'pw.tipCheck2') } }} />;
	}
	return <Formula lesson={LESSON} label={t(locale, 'pw.formulaPow')} block
		tex={alignedRows([tex`\t{y}{\s{t}^{\s{g}}}=\s{y}`, tex`\t{yinv}{(\s{t}^{\s{g}})^{1/\s{g}}}=\s{yinv}`])}
		scrub={{ t: SPEC.t, g: SPEC.g }} derived={derived} fmt={FMT} tips={{ yinv: { text: t(locale, 'pw.tipYinv') } }} />;
}

/** 0.2-2 power chips x¹ x² x³: buttons writing the authoritative n (the timeline mirrors it). */
function PowerChips() {
	const locale = useLocale();
	const n = useVals(LESSON).n as number;
	return <div className="lab-timeline-controls not-content" role="group" aria-label={t(locale, 'pw.chipsLabel')} data-pw-chips="">
		{[1, 2, 3].map((p) => <button key={p} type="button" aria-pressed={n === p} onClick={() => lessonStore(LESSON).getState().set('n', p)}>{powLabel(p)}</button>)}
	</div>;
}

/** 0.2-5 presets: decode 2.2, encode 1/2.2, specular 8 and 32, written exactly (not rounded to the field step). */
function Presets() {
	const locale = useLocale();
	const g = useVals(LESSON).g as number;
	return <div className="lab-timeline-controls not-content" role="group" aria-label={t(locale, 'pw.presetsLabel')} data-pw-presets="">
		{PRESETS.map((p) => <button key={p.id} type="button" aria-pressed={g === p.g} onClick={() => lessonStore(LESSON).getState().set('g', p.g, 'ease')}>
			{'g = ' + p.label}
		</button>)}
	</div>;
}

/** Timeline texts with live numbers (§7.8 power, §7.9 ladder). */
function powerSteps(locale: Locale) {
	return [1, 2, 3].map((n) => ({ id: 'n' + n, focus: 'xn', text: t(locale, n === 1 ? 'pw.step1' : n === 2 ? 'pw.step2' : 'pw.step3') }));
}
function ladderSteps(locale: Locale, z: ReturnType<typeof derive>) {
	const b = fmtVal(z.base);
	const v = (j: number) => fmtVal(z.base ** j);
	const keys = ['pw.ladder0', 'pw.ladder1', 'pw.ladder2', 'pw.ladder3', 'pw.ladder4', 'pw.ladder5', 'pw.ladder6'] as const;
	return RUNGS.map((j, i) => ({
		id: 'k' + j, focus: rungId(j),
		text: t(locale, keys[i], { b, p3: v(3), p2: v(2), p1: v(1), pm1: v(-1), pm2: v(-2), pm3: v(-3) }),
	}));
}

/** Challenge cards per scene (§7.7-§7.11); every predicate reads store values only (typing alone solves it). */
function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	// Keys are literals (the dictionary test finds every used key by its literal).
	const c = (n: number, check: (v: Vals) => boolean, text: TextKey, hint?: TextKey, success?: TextKey): Omit<ChallengeProps, 'lesson'> => ({
		n, check,
		text: t(locale, text),
		hint: hint ? [t(locale, hint)] : undefined,
		success: success ? t(locale, success) : undefined,
	});
	if (scene === 1) return [c(1, CHECKS.fourTiles, 'pw.ch1'), c(2, CHECKS.typedRoot, 'pw.ch2', 'pw.ch2Hint'), c(3, CHECKS.notTyped, 'pw.ch3', 'pw.ch3Hint')];
	if (scene === 2) return [c(4, CHECKS.cube27, 'pw.ch4'), c(5, CHECKS.cubeSmaller, 'pw.ch5', 'pw.ch5Hint', 'pw.ch5Success'), c(6, CHECKS.twoLayers, 'pw.ch6')];
	if (scene === 3) return [c(7, CHECKS.oneTile, 'pw.ch7'), c(8, CHECKS.quarter, 'pw.ch8'), c(9, CHECKS.otherBase, 'pw.ch9', undefined, 'pw.ch9Success')];
	if (scene === 4) return [c(10, CHECKS.wholeEdge, 'pw.ch10', 'pw.ch10Hint'), c(11, CHECKS.edgeTwo, 'pw.ch11', 'pw.ch11Hint')];
	return [c(12, CHECKS.diagonal, 'pw.ch12'), c(13, CHECKS.quarterOut, 'pw.ch13'), c(14, CHECKS.highlight, 'pw.ch14', undefined, 'pw.ch14Success')];
}

/** The live sentence under the formula (the canvas-free account of what the scene shows). */
export function readout(scene: Scene, locale: Locale, z: ReturnType<typeof derive>): string {
	if (scene === 1) {
		if (z.x === 0) return t(locale, 'pw.read1Zero');
		return t(locale, 'pw.read1', { x: fmtX(z.x), x2: fmtAmt(z.x2), whole: fmtInt(z.whole) })
			+ (z.partial > 0 ? t(locale, 'pw.read1Partial', { partial: fmtAmt(z.partial) }) : '.');
	}
	if (scene === 2) {
		if (z.x === 0) return t(locale, 'pw.read2Zero');
		const head = t(locale, 'pw.read2', { pow: powLabel(z.n), x: fmtX(z.x), xn: fmtAmt(z.xn), whole: fmtInt(z.wholeN) })
			+ (z.partialN > 0 ? t(locale, 'pw.read2Partial', { partial: fmtAmt(z.partialN) }) : '.');
		return z.n === 3 ? head + t(locale, 'pw.read2Layers', { layers: fmtInt(z.layers), slab: fmtInt(z.layers * z.layers) }) : head;
	}
	if (scene === 3) {
		const b = fmtVal(z.base);
		if (z.k === 0) return t(locale, 'pw.read3One', { b });
		return z.k > 0
			? t(locale, 'pw.read3', { b, k: sup(z.k), cur: fmtVal(z.cur) })
			: t(locale, 'pw.read3Fill', { b, k: sup(z.k), cur: fmtVal(z.cur) });
	}
	if (scene === 4) {
		return z.d3 === 1
			? t(locale, 'pw.read4Cube', { V: fmtAmt(z.V), cbrtV: fmt4(z.cbrtV), check3: fmtAmt(z.check3) })
			: t(locale, 'pw.read4Square', { A: fmtAmt(z.A), sqrtA: fmt4(z.sqrtA), check2: fmtAmt(z.check2) });
	}
	const key = z.g > 1 ? 'pw.read5Below' : z.g < 1 ? 'pw.read5Above' : 'pw.read5Diag';
	return t(locale, key, { t: fmt2(z.t), g: fmtG(z.g), y: fmtSci(z.y) });
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const z = derive(useVals(LESSON));
	return <section className="not-content" data-pw-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		{scene === 2 && <PowerChips />}
		{scene === 5 && <Presets />}
		<PowersFormula scene={scene} />
		{scene === 2 && <Timeline lesson={LESSON} id="power" steps={powerSteps(locale)} />}
		{scene === 3 && <Timeline lesson={LESSON} id="ladder" steps={ladderSteps(locale, z)} />}
		<p aria-live="polite">{readout(scene, locale, z)}</p>
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const PowersControls = localized(Controls);
