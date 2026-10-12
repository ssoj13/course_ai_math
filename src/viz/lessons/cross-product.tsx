import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Challenge, type ChallengeProps } from '../Challenge';
import { CodeView } from '../CodeView';
import { NumGrid, type GridRow } from '../NumTable';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { alignedRows } from '../tex';
import type { Scene } from './cross-product.rows';
import { CHECKS, INITIAL, LESSON, derive, derive2, fmt, fmtVec, tex as texFmt, tri } from './cross-product.math';

/**
 * Lesson 1.8 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store, the DOM-only controls
 * (formulas, the criss-cross table, code, challenges, live readouts) and the lazy scene island. Everything three lives
 * in cross-product.impl.tsx. Reader-facing words come from the `cp` dictionary namespace; numbers come from
 * cross-product.math.ts, the one model the scenes draw.
 */
lessonStore(LESSON, INITIAL);

export const CrossScene = localized(lazyIsland(() => import('./cross-product.impl').then((m) => m.CrossScene)));

type Vals = Parameters<typeof derive>[0];

const tex = String.raw;
/** A live value after a minus or a dot is parenthesised when negative ("3·(−1)", not "3·−1"). */
const signed = (key: string, value: number) => (value < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);

/** Every number a formula shows as `\s{key}`, from the one model. */
const derived = {
	c1: (v: Vals) => derive(v).c[0], c2: (v: Vals) => derive(v).c[1], c3: (v: Vals) => derive(v).c[2],
	len: (v: Vals) => derive(v).len, area: (v: Vals) => derive(v).area,
	na: (v: Vals) => derive(v).na, nb: (v: Vals) => derive(v).nb, th: (v: Vals) => derive(v).th,
	ax: (v: Vals) => derive2(v).a[0], ay: (v: Vals) => derive2(v).a[1], bx: (v: Vals) => derive2(v).b[0], by: (v: Vals) => derive2(v).b[1],
	sa: (v: Vals) => derive2(v).sa,
	...Object.fromEntries([0, 1, 2].flatMap((i) => {
		const x = 'xyz'[i];
		return [
			['e1' + x, (v: Vals) => tri(v).e1[i]], ['e2' + x, (v: Vals) => tri(v).e2[i]],
			['n' + x, (v: Vals) => tri(v).n[i]], ['N' + x, (v: Vals) => tri(v).N?.[i] ?? NaN],
		];
	})),
	ndotl: (v: Vals) => tri(v).ndotl, bright: (v: Vals) => tri(v).bright,
};
const formats: Record<string, (x: number) => string> = Object.fromEntries(Object.keys(derived).map((k) => [k, k === 'th' ? texFmt.deg : texFmt.num]));

/**
 * Hover maps (transitive, see HotLinks): the result arrow, its length and the parallelogram's area are one number;
 * each component lights its two products in the table and its leg; the face, its normal and its brightness go together.
 */
const HOT: HotLinkMap = {
	c: ['len'], len: ['c', 'par', 'area'], area: ['len'], par: ['len'], perpA: ['c'], perpB: ['c'],
	c1: ['p1', 'm1', 'c:x'], p1: ['c1'], m1: ['c1'], 'c:x': ['c1'],
	c2: ['p2', 'm2', 'c:y'], p2: ['c2'], m2: ['c2'], 'c:y': ['c2'],
	c3: ['p3', 'm3', 'c:z'], p3: ['c3'], m3: ['c3'], 'c:z': ['c3'],
	sa: ['par2'], par2: ['sa'], turn: ['th2'], th2: ['turn'],
	e1: ['n'], e2: ['n'], n: ['N'], N: ['n'], NdotL: ['N', 'L'], bright: ['face', 'NdotL'], face: ['bright'],
};

/** The CP-1 formula for the current order: U × W with U, W = a, b (or b, a when swapped). */
function crossTex(swap: boolean, narrow: boolean): string {
	const [u, w] = swap ? ['b', 'a'] : ['a', 'b'];
	const U = tex`\mathbf{${u}}`, W = tex`\mathbf{${w}}`;
	const comp = [
		tex`\t{c1}{${u}_y ${w}_z - ${u}_z ${w}_y}`,
		tex`\t{c2}{${u}_z ${w}_x - ${u}_x ${w}_z}`,
		tex`\t{c3}{${u}_x ${w}_y - ${u}_y ${w}_x}`,
	];
	const len = tex`\t{len}{\lvert ${U}\times ${W}\rvert}=\s{len}`;
	const area = tex`\t{area}{\lvert\mathbf{a}\rvert\,\lvert\mathbf{b}\rvert\sin\theta}=\s{na}\cdot\s{nb}\cdot\sin\s{th}=\s{area}`;
	if (!narrow) {
		return alignedRows([
			tex`\t{c}{${U}\times ${W}}=(${comp.join(tex`,\;`)})`,
			tex`\phantom{${U}\times ${W}}=(\s{c1},\;\s{c2},\;\s{c3})`,
			tex`${len}\qquad ${area}`,
		]);
	}
	// Phones: one component per line, then the two routes to the same number; the sine route breaks before its result
	// (on one line it was the formula that still scrolled sideways at 360 px).
	return alignedRows([
		...comp.map((c, i) => tex`(${U}\times ${W})_${'xyz'[i]}=${c}=\s{c${i + 1}}`),
		len,
		tex`\t{area}{\lvert\mathbf{a}\rvert\,\lvert\mathbf{b}\rvert\sin\theta}=\s{na}\cdot\s{nb}\cdot\sin\s{th}`,
		tex`\quad=\s{area}`,
	]);
}

function CrossFormula({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	const common = { lesson: LESSON, derived, fmt: formats, block: true };
	if (scene === 1) {
		const swap = derive(v).swap;
		return <Formula {...common} label={t(locale, 'cp.formula1')} tex={crossTex(swap, false)} texNarrow={crossTex(swap, true)}
			tips={{
				c1: { text: t(locale, 'cp.tipC1') }, c2: { text: t(locale, 'cp.tipC2') }, c3: { text: t(locale, 'cp.tipC3') },
				area: { text: t(locale, 'cp.tipArea') },
			}} />;
	}
	if (scene === 2) {
		const z = derive2(v);
		const body = tex`\t{sa}{a_x b_y - a_y b_x}`;
		const values = tex`\s{ax}\cdot ${signed('by', z.b[1])}-${signed('ay', z.a[1])}\cdot ${signed('bx', z.b[0])}=\s{sa}`;
		return <Formula {...common} label={t(locale, 'cp.formula2')} tex={alignedRows([tex`${body}=${values}`])}
			texNarrow={alignedRows([body, tex`\quad=${values}`])} tips={{ sa: { text: t(locale, 'cp.tipSigned') } }} />;
	}
	const e = (k: string) => tex`(\s{${k}x},\;\s{${k}y},\;\s{${k}z})`;
	const light = tex`\t{bright}{k}=\max\bigl(0,\;\t{NdotL}{\mathbf{N}\cdot\mathbf{L}}\bigr)=\max(0,\;\s{ndotl})=\s{bright}`;
	const lines = (narrow: boolean) => alignedRows([
		tex`\t{e1}{\mathbf{e}_1}=${narrow ? '' : 'P_1-P_0='}${e('e1')}`,
		tex`\t{e2}{\mathbf{e}_2}=${narrow ? '' : 'P_2-P_0='}${e('e2')}`,
		tex`\t{n}{\mathbf{e}_1\times\mathbf{e}_2}=${e('n')}`,
		tex`\t{N}{\mathbf{N}}=${e('N')}`,
		...(narrow ? [tex`\t{bright}{k}=\max(0,\;\t{NdotL}{\mathbf{N}\cdot\mathbf{L}})`, tex`\quad=\max(0,\;\s{ndotl})=\s{bright}`] : [light]),
	]);
	return <Formula {...common} label={t(locale, 'cp.formula3')} tex={lines(false)} texNarrow={lines(true)}
		tips={{ N: { text: t(locale, 'cp.tipN') }, NdotL: { text: t(locale, 'cp.tipNdotL') } }} />;
}

/** Plain numbers for table cells: up to 4 decimals, no trailing zeros, never -0. */
const fmtCell = (x: number) => String(+x.toFixed(4) + 0);

/** CP-1 "criss-cross": per component, the two products and their difference (the component itself). */
function CrissCross() {
	const locale = useLocale(), z = derive(useVals(LESSON));
	const rows: GridRow[] = [0, 1, 2].map((i) => ({
		head: 'xyz'[i],
		cells: {
			plus: { id: 'p' + (i + 1), value: z.plus[i] },
			minus: { id: 'm' + (i + 1), value: z.minus[i] },
			c: { id: 'c' + (i + 1), value: z.c[i] },
		},
	}));
	return <NumGrid lesson={LESSON} caption={t(locale, 'cp.tableCaption')} corner="" rows={rows} fmt={fmtCell}
		cols={[{ id: 'plus', label: t(locale, 'cp.colPlus') }, { id: 'minus', label: t(locale, 'cp.colMinus') }, { id: 'c', label: t(locale, 'cp.colC') }]} />;
}

const GLSL1 = '// 6 multiplies, 3 subtractions, no sqrt\nvec3 c = vec3(a.y*b.z - a.z*b.y,\n              a.z*b.x - a.x*b.z,\n              a.x*b.y - a.y*b.x);\nfloat area = length(c);';
const GLSL3 = 'vec3 e1 = P1 - P0;\nvec3 e2 = P2 - P0;\nvec3 N = normalize(cross(e1, e2));\nfloat k = max(0.0, dot(N, L));\ncolor = albedo * k;';

function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'cp.ch1'), check: CHECKS.plusZ, hint: [t(locale, 'cp.ch1Hint')], success: t(locale, 'cp.ch1Success') },
			{ n: 2, text: t(locale, 'cp.ch2'), check: CHECKS.parallel, hint: [t(locale, 'cp.ch2Hint')] },
			{ n: 3, text: t(locale, 'cp.ch3'), check: CHECKS.areaSix, hint: [t(locale, 'cp.ch3Hint')] },
			{ n: 4, text: t(locale, 'cp.ch4'), check: CHECKS.swapped },
		];
	}
	if (scene === 2) {
		return [
			{ n: 5, text: t(locale, 'cp.ch5'), check: CHECKS.negative },
			{ n: 6, text: t(locale, 'cp.ch6'), check: CHECKS.six, hint: [t(locale, 'cp.ch6Hint')] },
			{ n: 7, text: t(locale, 'cp.ch7'), check: CHECKS.line },
		];
	}
	return [
		{ n: 8, text: t(locale, 'cp.ch8'), check: CHECKS.black, success: t(locale, 'cp.ch8Success') },
		{ n: 9, text: t(locale, 'cp.ch9'), check: CHECKS.turnAway, hint: [t(locale, 'cp.ch9Hint')] },
	];
}

/** The live sentence under a scene: the canvas-free account of what it shows. */
export function readout(scene: Scene, locale: Locale, v: Vals): string {
	if (scene === 1) {
		const z = derive(v), name = z.swap ? 'b × a' : 'a × b';
		if (z.zero) return t(locale, 'cp.read1Zero', { name });
		if (z.parallel) return t(locale, 'cp.read1Par', { name });
		const base = t(locale, 'cp.read1', { name, c: fmtVec(z.c), len: fmt(z.len), area: fmt(z.area), ca: fmt(z.ca), cb: fmt(z.cb) });
		return base + (z.swap ? t(locale, 'cp.read1Swap') : '');
	}
	if (scene === 2) {
		const z = derive2(v), vars = { p: fmt(z.p), q: fmt(z.q), sa: fmt(z.sa), abs: fmt(Math.abs(z.sa)) };
		return t(locale, z.orient === 'ccw' ? 'cp.read2Ccw' : z.orient === 'cw' ? 'cp.read2Cw' : 'cp.read2Line', vars);
	}
	const z = tri(v);
	if (!z.N) return t(locale, 'cp.read3Flat');
	const order = z.rev ? 'A → C → B' : 'A → B → C';
	return t(locale, 'cp.read3', { order, e1: fmtVec(z.e1), e2: fmtVec(z.e2), n: fmtVec(z.n), N: fmtVec(z.N), ndotl: fmt(z.ndotl), bright: fmt(z.bright) })
		+ t(locale, z.ndotl > 0 ? 'cp.read3Lit' : 'cp.read3Dark')
		+ t(locale, z.cull ? 'cp.read3Cull' : 'cp.read3NoCull');
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON);
	return <section className="not-content" data-cp-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		<CrossFormula scene={scene} />
		{scene === 1 && <><CrissCross /><CodeView lesson={LESSON} lang="glsl" code={GLSL1} lines={{ 2: 'c1', 3: 'c2', 4: 'c3', 5: 'len' }} /></>}
		{scene === 3 && <CodeView lesson={LESSON} lang="glsl" code={GLSL3} lines={{ 1: 'e1', 2: 'e2', 3: 'N', 4: 'bright', 5: 'face' }} />}
		<p aria-live="polite">{readout(scene, locale, v)}</p>
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const CrossControls = localized(Controls);
