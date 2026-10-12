import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { NumGrid, type GridRow } from '../NumTable';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { alignedRows } from '../tex';
import { CHECKS, derive, fmt2, fmtDeg, fmtVec, INITIAL, LESSON, LINKS, texFmt } from './projection.math';
import type { Scene } from './projection.rows';

/**
 * Lesson 1.5 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store, the DOM-only controls
 * (formulas, the shadows table, challenges, readouts) and the lazy scene island. Everything three lives in
 * projection.impl.tsx. Reader-facing words come from the `pj` dictionary namespace; numbers from projection.math.ts.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const ProjScene = localized(lazyIsland(() => import('./projection.impl').then((m) => m.ProjScene)));

type Vals = Parameters<typeof derive>[0];

const tex = String.raw;
const A = tex`\mathbf{a}`, B = tex`\mathbf{b}`, BH = tex`\hat{\mathbf{b}}`, N = tex`\mathbf{n}`;
const abs = (x: string) => tex`\lvert ${x}\rvert`;
const PROJ = tex`\mathbf{proj}`, PERP = tex`\mathbf{perp}`;

/** Every number a formula shows as `\s{key}`; one model (derive). */
const nan2 = [NaN, NaN];
const derived: Record<string, (v: Vals) => number> = {
	s: (v) => derive(v).p.s, ab: (v) => derive(v).p.ab, nb: (v) => derive(v).p.nb, na: (v) => derive(v).p.na,
	bb: (v) => derive(v).p.bb, th: (v) => derive(v).p.th,
	bh1: (v) => (derive(v).p.bh ?? nan2)[0], bh2: (v) => (derive(v).p.bh ?? nan2)[1],
	b1: (v) => (v.b as number[])[0], b2: (v) => (v.b as number[])[1],
	p1: (v) => (derive(v).p.proj ?? nan2)[0], p2: (v) => (derive(v).p.proj ?? nan2)[1],
	pa1: (v) => (derive(v).p.projAlt ?? nan2)[0], pa2: (v) => (derive(v).p.projAlt ?? nan2)[1],
	r1: (v) => (derive(v).p.perp ?? nan2)[0], r2: (v) => (derive(v).p.perp ?? nan2)[1],
	pd: (v) => derive(v).p.perpDotB, na2: (v) => derive(v).p.na2, proj2: (v) => derive(v).p.proj2, perp2: (v) => derive(v).p.perp2,
	sh1: (v) => derive(v).q.sh[0], sh2: (v) => derive(v).q.sh[1],
	rb1: (v) => derive(v).q.rebuild[0], rb2: (v) => derive(v).q.rebuild[1],
	c1: (v) => (v.c as number[])[0], c2: (v) => (v.c as number[])[1],
	co1: (v) => derive(v).q.co[0], co2: (v) => derive(v).q.co[1],
	an: (v) => derive(v).g.an,
	g1: (v) => derive(v).g.ground[0], g2: (v) => derive(v).g.ground[1], g3: (v) => derive(v).g.ground[2],
	e1s: (v) => derive(v).g.sh[0], e2s: (v) => derive(v).g.sh[1],
	f1: (v) => derive(v).g.refl[0], f2: (v) => derive(v).g.refl[1], f3: (v) => derive(v).g.refl[2],
};
const formats = Object.fromEntries(Object.keys(derived).map((k) => [k, k === 'th' ? texFmt.deg : k === 'b1' || k === 'b2' || k === 'c1' || k === 'c2' ? texFmt.c : texFmt.n]));

/** A live value after a dot, a minus or inside a product is parenthesised when negative: "2·(−1.5)". */
const sv = (key: string, v: Vals) => (derived[key](v) < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);
const pair = (keys: string[]) => '(' + keys.map((k) => tex`\s{${k}}`).join(',\\ ') + ')';

/** Hover links: a shadow label lights its formula terms, cells and band; the tiles light their arrows. */
const HOT: HotLinkMap = {
	proj2: ['proj'], perp2: ['perp'], na2: ['a'], pd: ['perp', 'b'], perpDot: ['perp', 'b'],
	co1: ['co2'], co2: ['co1'], miss: ['rebuild'], rebuild: ['miss'],
	an: ['n3'], e1s: ['e1'], e2s: ['e2'],
};

// ---------------------------------------------------------------------------------------------------------------
// Formulas.

function ProjFormula({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	const common = { lesson: LESSON, derived, fmt: formats, block: true };
	if (scene === 1) {
		const def = tex`\t{shadow}{${A}\cdot ${BH}}=\frac{${A}\cdot ${B}}{${abs(B)}}=\frac{\s{ab}}{\s{nb}}=\s{s}`;
		const geo = tex`\t{shadow}{${A}\cdot ${BH}}=${abs(A)}\cos\theta=\s{na}\cdot\cos\s{th}=\s{s}`;
		return <Formula {...common} label={t(locale, 'pj.formulaShadow')} tex={alignedRows([def, geo])}
			tips={{ shadow: { text: t(locale, 'pj.tipShadow') } }} />;
	}
	if (scene === 2) {
		const unit = [tex`\t{proj}{${PROJ}}=(${A}\cdot ${BH})\,${BH}`, tex`\quad=${sv('s', v)}\cdot ${pair(['bh1', 'bh2'])}=${pair(['p1', 'p2'])}`];
		const raw = [tex`\t{proj}{${PROJ}}=\frac{${A}\cdot ${B}}{${B}\cdot ${B}}\,${B}`, tex`\quad=\frac{\s{ab}}{\s{bb}}\,${pair(['b1', 'b2'])}=${pair(['pa1', 'pa2'])}`];
		const perp = tex`\t{perp}{${PERP}}=${A}-${PROJ}=${pair(['r1', 'r2'])},\quad \t{perpDot}{${PERP}\cdot ${B}}=\s{pd}`;
		const pyth = tex`\t{na2}{${abs(A)}^2}=\t{proj2}{${abs(PROJ)}^2}+\t{perp2}{${abs(PERP)}^2}:\;\s{na2}=\s{proj2}+\s{perp2}`;
		const pythNarrow = [tex`\t{na2}{${abs(A)}^2}=\t{proj2}{${abs(PROJ)}^2}+\t{perp2}{${abs(PERP)}^2}`, tex`\quad\s{na2}=\s{proj2}+\s{perp2}`];
		const perpNarrow = [tex`\t{perp}{${PERP}}=${A}-${PROJ}=${pair(['r1', 'r2'])}`, tex`\t{perpDot}{${PERP}\cdot ${B}}=\s{pd}`];
		return <Formula {...common} label={t(locale, 'pj.formulaProj')}
			tex={alignedRows([unit.join(''), raw.join(''), perp, pyth])}
			texNarrow={alignedRows([...unit, ...raw, ...perpNarrow, ...pythNarrow])}
			tips={{ proj: { text: t(locale, 'pj.tipProj') }, perp: { text: t(locale, 'pj.tipPerp') } }} />;
	}
	if (scene === 3) {
		const U1 = tex`\mathbf{u}_1`, U2 = tex`\mathbf{u}_2`;
		const shadows = tex`\t{sh1}{${A}\cdot ${U1}}=\s{sh1},\quad \t{sh2}{${A}\cdot ${U2}}=\s{sh2}`;
		const rebuild = tex`\t{miss}{(${A}\cdot ${U1})\,${U1}+(${A}\cdot ${U2})\,${U2}}=${pair(['rb1', 'rb2'])},\quad ${A}=${pair(['c1', 'c2'])}`;
		const coordsTex = tex`${A}=\t{co1}{\s{co1}}\,${U1}+\t{co2}{${sv('co2', v)}}\,${U2}`;
		const rebuildNarrow = [tex`\t{miss}{(${A}\cdot ${U1})\,${U1}+(${A}\cdot ${U2})\,${U2}}`, tex`\quad=${pair(['rb1', 'rb2'])}`, tex`${A}=${pair(['c1', 'c2'])}`];
		return <Formula {...common} label={t(locale, 'pj.formulaCoords')} tex={alignedRows([shadows, rebuild, coordsTex])}
			texNarrow={alignedRows([shadows, ...rebuildNarrow, coordsTex])} />;
	}
	const E1 = tex`\mathbf{e}_1`, E2 = tex`\mathbf{e}_2`;
	const an = tex`\t{an}{${A}\cdot ${N}}=\s{an}`;
	const ground = tex`\t{ground}{${A}-(${A}\cdot ${N})\,${N}}=${pair(['g1', 'g2', 'g3'])}`;
	const two = tex`(${A}\cdot ${E1})\,${E1}+(${A}\cdot ${E2})\,${E2}:\;\t{e1s}{${A}\cdot ${E1}}=\s{e1s},\;\t{e2s}{${A}\cdot ${E2}}=\s{e2s}`;
	const refl = tex`\t{refl}{\mathbf{r}=${A}-2\,(${A}\cdot ${N})\,${N}}=${pair(['f1', 'f2', 'f3'])}`;
	const twoNarrow = [tex`\t{e1s}{${A}\cdot ${E1}}=\s{e1s},\;\t{e2s}{${A}\cdot ${E2}}=\s{e2s}`];
	const groundNarrow = [tex`\t{ground}{${A}-(${A}\cdot ${N})\,${N}}`, tex`\quad=${pair(['g1', 'g2', 'g3'])}`];
	const reflNarrow = [tex`\t{refl}{\mathbf{r}=${A}-2\,(${A}\cdot ${N})\,${N}}`, tex`\quad=${pair(['f1', 'f2', 'f3'])}`];
	return <Formula {...common} label={t(locale, 'pj.formulaPlane')} tex={alignedRows([an, ground, two, refl])}
		texNarrow={alignedRows([an, ...groundNarrow, ...twoNarrow, ...reflNarrow])}
		tips={{ ground: { text: t(locale, 'pj.tipGround') }, refl: { text: t(locale, 'pj.tipRefl') } }} />;
}

/** PJ-3 table: per axis, the shadow a·uᵢ next to the real coordinate; equal exactly for perpendicular unit axes. */
function CoordTable() {
	const locale = useLocale(), q = derive(useVals(LESSON)).q;
	const rows: GridRow[] = [0, 1].map((i) => ({
		head: i === 0 ? 'u₁' : 'u₂',
		cells: { sh: { id: 'sh' + (i + 1), value: q.sh[i] }, co: { id: 'co' + (i + 1), value: q.co[i] } },
	}));
	return <NumGrid lesson={LESSON} caption={t(locale, 'pj.coordCaption')} corner="" rows={rows} fmt={fmt2}
		cols={[{ id: 'sh', label: 'a·uᵢ' }, { id: 'co', label: t(locale, 'pj.colCoord') }]} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Challenges and readouts.

function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'pj.ch1'), check: CHECKS.negative, success: t(locale, 'pj.ch1Success') },
			{ n: 2, text: t(locale, 'pj.ch2'), check: CHECKS.zero, hint: [t(locale, 'pj.ch2Hint')] },
			{ n: 3, text: t(locale, 'pj.ch3'), check: CHECKS.full, hint: [t(locale, 'pj.ch3Hint')] },
		];
	}
	if (scene === 2) return [{ n: 4, text: t(locale, 'pj.ch4'), check: CHECKS.equal, hint: [t(locale, 'pj.ch4Hint')] }];
	if (scene === 3) {
		return [
			{ n: 5, text: t(locale, 'pj.ch5'), check: CHECKS.turned, success: t(locale, 'pj.ch5Success') },
			{ n: 6, text: t(locale, 'pj.ch6'), check: CHECKS.oblique, success: t(locale, 'pj.ch6Success') },
		];
	}
	return [{ n: 7, text: t(locale, 'pj.ch7'), check: CHECKS.inPlane, hint: [t(locale, 'pj.ch7Hint')] }];
}

export function readout(scene: Scene, locale: Locale, v: Vals): string {
	const { p, q, g } = derive(v);
	if (scene === 1 || scene === 2) {
		if (p.zeroB) return t(locale, 'pj.readNoAxis');
		if (scene === 1) {
			const key = p.sign === 'pos' ? 'pj.readPos' : p.sign === 'neg' ? 'pj.readNeg' : 'pj.readZero';
			return t(locale, key, { s: fmt2(p.s), th: fmtDeg(p.th) });
		}
		return t(locale, 'pj.readSplit', { proj: fmtVec(p.proj), alt: fmtVec(p.projAlt), perp: fmtVec(p.perp), dot: fmt2(p.perpDotB), na2: fmt2(p.na2), proj2: fmt2(p.proj2), perp2: fmt2(p.perp2) });
	}
	if (scene === 3) {
		return q.ortho
			? t(locale, 'pj.readOrtho', { sh1: fmt2(q.sh[0]), sh2: fmt2(q.sh[1]) })
			: t(locale, 'pj.readOblique', { sh1: fmt2(q.sh[0]), sh2: fmt2(q.sh[1]), co1: fmt2(q.co[0]), co2: fmt2(q.co[1]), miss: fmt2(q.miss) });
	}
	return t(locale, 'pj.readPlane', { an: fmt2(g.an), ground: fmtVec(g.ground), refl: fmtVec(g.refl) });
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	return <section className="not-content" data-pj-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		<ProjFormula scene={scene} />
		{scene === 3 && <CoordTable />}
		<p aria-live="polite">{readout(scene, locale, v)}</p>
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const ProjControls = localized(Controls);
