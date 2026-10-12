import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { alignedRows } from '../tex';
import { CHECKS, INITIAL, LESSON, SHIFT, SPEC_S, SPEC_T, derive1, derive2, fmt2, fmtV, tex1, tex2, type Mode } from './linear-transformations.math';
import type { Scene } from './linear-transformations.rows';

/**
 * Lesson 2.1 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store, the DOM-only controls
 * (live formulas, the map switch of LT-2, readouts, challenges) and the lazy scene island. Everything three lives in
 * linear-transformations.impl.tsx. Reader-facing words come from the `lt` dictionary namespace; numbers come from
 * linear-transformations.math.ts, the one model the scenes draw.
 */
lessonStore(LESSON, INITIAL);

export const LinearScene = localized(lazyIsland(() => import('./linear-transformations.impl').then((m) => m.LinearScene)));

type Vals = Parameters<typeof derive1>[0];

const tex = String.raw;

/** `\s{key}` values: x/y components of every vector a formula prints. */
const xy = (prefix: string, get: (v: Vals) => readonly number[]) => ({
	[prefix + 'x']: (v: Vals) => get(v)[0],
	[prefix + 'y']: (v: Vals) => get(v)[1],
});
const derived: Record<string, (v: Vals) => number> = {
	...xy('v', (v) => derive1(v).v), ...xy('m1', (v) => derive1(v).m1), ...xy('m2', (v) => derive1(v).m2), ...xy('av', (v) => derive1(v).avT),
	...xy('w', (v) => derive2(v).w), ...xy('tvw', (v) => derive2(v).tvw), ...xy('sum', (v) => derive2(v).sum),
	...xy('tsv', (v) => derive2(v).tsv), ...xy('stv', (v) => derive2(v).stv),
	gapAdd: (v) => derive2(v).gapAdd, gapScale: (v) => derive2(v).gapScale,
};
const FMT: Record<string, (x: number) => string> = {
	...Object.fromEntries(Object.keys(derived).map((k) => [k, tex1])),
	gapAdd: tex2, gapScale: tex2, t: tex2, s: tex1,
};

/** A live value after an operator is parenthesised when negative ("2·(−1)", not "2·−1"). */
const S = (v: Vals, key: string) => ((derived[key]?.(v) ?? (v[key] as number)) < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);
const pair = (prefix: string) => tex`(\s{${prefix}x},\s{${prefix}y})`;

/** Hover maps: a column lights its family of image grid lines; a test gap lights the two routes it compares. */
const HOT: HotLinkMap = {
	a1: ['lat1', 'walk1'], a2: ['lat2', 'walk2'], av: ['walk1', 'walk2'],
	gapAdd: ['sum', 'tvw'], gapScale: ['tsv', 'stv'],
};

// ---------------------------------------------------------------------------------------------------------------
// Formulas.

/** The rule of each LT-2 map, in TeX (no words: TeX is compared verbatim between the languages). */
const MAP_TEX: Record<Mode, string> = {
	0: tex`T(\mathbf{p})=A\mathbf{p}`,
	1: tex`T(\mathbf{p})=A\mathbf{p}+(${SHIFT[0]},${SHIFT[1]})`,
	2: tex`T(x,y)=\bigl(x,\;y+\tfrac{1}{4}x^2\bigr)`,
};

function LinearFormula({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	const common = { lesson: LESSON, derived, fmt: FMT, block: true };
	if (scene === 1) {
		const head = tex`\t{av}{T(\mathbf{v})}=\t{walk1}{\s{vx}\,\t{a1}{T(\mathbf{e}_1)}}+\t{walk2}{${S(v, 'vy')}\,\t{a2}{T(\mathbf{e}_2)}}`;
		const nums = tex`=${S(v, 'vx')}\cdot${pair('m1')}+${S(v, 'vy')}\cdot${pair('m2')}`;
		const out = tex`=${pair('av')}`;
		const col = (id: string, p: string) => tex`\t{${id}}{\begin{matrix}\s{${p}x}\\ \s{${p}y}\end{matrix}}`;
		const mat = tex`A=\left[\,${col('a1', 'm1')}\quad${col('a2', 'm2')}\,\right],\qquad t=\s{t}`;
		return <Formula {...common} label={t(locale, 'lt.formula1')} scrub={{ t: SPEC_T }}
			tex={alignedRows([['f1-head', head], ['f1-nums', nums + out], ['f1-mat', mat]])}
			texNarrow={alignedRows([['f1-head', head], ['f1-nums', nums], ['f1-out', out], ['f1-mat', mat]])} />;
	}
	const z = derive2(v);
	const add1 = tex`\t{tvw}{T(\mathbf{v}+\mathbf{w})}=${pair('tvw')}`;
	const add2 = tex`\t{sum}{T(\mathbf{v})+T(\mathbf{w})}=${pair('sum')}`;
	const gapA = tex`\t{gapAdd}{\bigl|T(\mathbf{v}+\mathbf{w})-T(\mathbf{v})-T(\mathbf{w})\bigr|}=\s{gapAdd}`;
	const sc1 = tex`\t{tsv}{T(s\mathbf{v})}=${pair('tsv')},\quad s=\s{s}`;
	const sc2 = tex`\t{stv}{s\,T(\mathbf{v})}=${pair('stv')}`;
	const gapS = tex`\t{gapScale}{\bigl|T(s\mathbf{v})-s\,T(\mathbf{v})\bigr|}=\s{gapScale}`;
	return <Formula {...common} label={t(locale, 'lt.formula2')} scrub={{ s: SPEC_S }}
		tex={alignedRows([['f2-map', MAP_TEX[z.mode]], ['f2-add', add1 + tex`,\quad ` + add2], ['f2-gapA', gapA], ['f2-sc', sc1 + tex`,\quad ` + sc2], ['f2-gapS', gapS]])}
		texNarrow={alignedRows([['f2-map', MAP_TEX[z.mode]], ['f2-add1', add1], ['f2-add2', add2], ['f2-gapA', gapA], ['f2-sc1', sc1], ['f2-sc2', sc2], ['f2-gapS', gapS]])} />;
}

// ---------------------------------------------------------------------------------------------------------------
// The map switch of LT-2 (the N-panel's `mode` row writes the same key).

function ModeSwitch() {
	const locale = useLocale();
	const mode = useVals(LESSON).mode as number;
	const set = (m: number) => lessonStore(LESSON).getState().set('mode', m);
	return <fieldset data-lt-modes="" style={{ display: 'flex', flexWrap: 'wrap', gap: '.25rem 1rem', border: 0, padding: 0, margin: '.5rem 0' }}>
		<legend style={{ padding: 0, marginBottom: '.25rem' }}>{t(locale, 'lt.modeLegend')}</legend>
		{([0, 1, 2] as const).map((m) => <label key={m} style={{ display: 'inline-flex', gap: '.35rem', alignItems: 'center' }}>
			<input type="radio" name="lt-mode" checked={mode === m} onChange={() => set(m)} />
			{t(locale, (['lt.mode0Long', 'lt.mode1Long', 'lt.mode2Long'] as const)[m])}
		</label>)}
	</fieldset>;
}

// ---------------------------------------------------------------------------------------------------------------
// Readouts and challenges.

/**
 * The live sentence under a scene and every sentence the same scene can produce (StableText reserves the tallest).
 * Exported for the tests.
 */
export function readouts(scene: Scene, locale: Locale, v: Vals): { text: string; variants: string[] } {
	if (scene === 1) {
		const z = derive1(v);
		const base = t(locale, 'lt.read1', { a1: fmtV(z.m1), a2: fmtV(z.m2), v: fmtV(z.v), av: fmtV(z.avT) });
		const variants = [base, base + t(locale, 'lt.read1Mirror'), base + t(locale, 'lt.read1Flat')];
		return { text: z.flat ? variants[2] : z.detT < 0 ? variants[1] : variants[0], variants };
	}
	const z = derive2(v);
	const g = { gap: fmt2(z.gapAdd), gs: fmt2(z.gapScale), o: fmtV(z.origin) };
	const variants = [
		t(locale, 'lt.read2Linear'),
		t(locale, 'lt.read2Shift', g),
		t(locale, 'lt.read2Bend', g),
		t(locale, 'lt.read2BendLucky', g),
	];
	const text = z.mode === 0 ? variants[0] : z.mode === 1 ? variants[1] : z.addOk ? variants[3] : variants[2];
	return { text, variants };
}

function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'lt.ch1'), check: CHECKS.mirror, hint: [t(locale, 'lt.ch1Hint')], success: t(locale, 'lt.ch1Success') },
			{ n: 2, text: t(locale, 'lt.ch2'), check: CHECKS.squash, hint: [t(locale, 'lt.ch2Hint')], success: t(locale, 'lt.ch2Success') },
		];
	}
	return [
		{ n: 3, text: t(locale, 'lt.ch3'), check: CHECKS.lucky, hint: [t(locale, 'lt.ch3Hint')], success: t(locale, 'lt.ch3Success') },
		{ n: 4, text: t(locale, 'lt.ch4'), check: CHECKS.shiftScale, hint: [t(locale, 'lt.ch4Hint')], success: t(locale, 'lt.ch4Success') },
	];
}

/**
 * Live text whose box is as tall as its tallest variant, so a write in one card never moves the card being dragged
 * (both scenes share one store). Pure CSS, so the server render reserves it too.
 */
function StableText({ text, variants }: { text: string; variants: readonly string[] }) {
	return <div data-lt-readout="" style={{ display: 'grid' }}>
		{variants.map((x, i) => <p key={i} aria-hidden="true" style={{ gridArea: '1 / 1', visibility: 'hidden', margin: 0 }}>{x}</p>)}
		<p aria-live="polite" style={{ gridArea: '1 / 1', margin: 0 }}>{text}</p>
	</div>;
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	return <section className="not-content" data-lt-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		{scene === 2 && <ModeSwitch />}
		<LinearFormula scene={scene} />
		<StableText {...readouts(scene, locale, v)} />
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const LinearControls = localized(Controls);
