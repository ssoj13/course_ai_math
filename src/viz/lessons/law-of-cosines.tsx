import { useEffect, useState } from 'react';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useStep, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Timeline } from '../Timeline';
import { Challenge, type ChallengeProps } from '../Challenge';
import { CodeView } from '../CodeView';
import { Ledger } from '../Ledger';
import { reducedMotion } from '../scene/motion';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import type { Marks } from '../marks';
import { alignedRows, type AlignedRow } from '../tex';
import { LESSON, INITIAL, derive, solveIK, fmt, fmtCos, fmtAngle, cutProgress, cutElapsed, cutMoves, CUT_PIECES, CUT_LENGTHS, PROOF_PIECES, pieceShown, shownPiece } from './law-of-cosines.math';
import { SCRUB } from './law-of-cosines.rows';

/**
 * Lesson 0.5 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store, the DOM-only
 * controls (formulas, timelines, transport, challenges, readouts) and the lazy scene islands. Everything three
 * lives in law-of-cosines.impl.tsx. All reader-facing words come from the `lc` dictionary namespace.
 */
lessonStore(LESSON, INITIAL);

export const LawDetail = localized(lazyIsland(() => import('./law-of-cosines.impl').then((m) => m.LawDetail)));
export const LawScene = localized(lazyIsland(() => import('./law-of-cosines.impl').then((m) => m.LawScene)));

type Scene = 0 | 1 | 2 | 3;
type Vals = Parameters<typeof derive>[0];
type Derived = ReturnType<typeof derive>;

const tex = String.raw;
const deg = (x: number) => fmtAngle(x) + tex`^\circ`;

/** Every derived number a formula may show as `\s{key}`; one `derive()` per lesson (spec §0.1). */
const KEYS = ['a', 'b', 'th', 'cos', 'c', 'a2', 'b2', 'c2', 'corr', 'strip_a', 'strip_b', 'R1', 'R2', 'R1c', 'R2c', 'p', 'q', 'ab', 'rest', 'rest2', 'rise', 'rise2', 'bcos2', 'bsin2'] as const;
const derived = Object.fromEntries(KEYS.map((k) => [k, (v: Vals) => derive(v)[k]]));
const formats = Object.fromEntries(KEYS.map((k) => [k, k === 'cos' ? fmtCos : k === 'th' ? deg : fmt]));

/** A formula row carrying a mark id (`data-m`): dimmed by step, or left out of the TeX while hidden (alignedRows). */
const row = (key: string, body: string): AlignedRow => [key, body];
const aligned = (rows: readonly AlignedRow[], marks?: Marks) => alignedRows(rows, marks);

/** Formula marks per scene and step (spec step tables): line reveals, `dim` at 90°, the struck identity. */
export function marksFor(scene: Scene, step: number, th: number): Marks {
	const zero = th === 90;
	if (scene === 0) {
		return {
			'cut-0': { hidden: step < 9 }, 'cut-1': { hidden: step < 10 }, 'cut-2': { hidden: step < 10 }, 'cut-3': { hidden: step < 11 }, 'cut-4': { hidden: step < 11 },
			strip_a: { dim: zero }, strip_b: { dim: zero }, corr: { dim: zero },
		};
	}
	if (scene === 2) {
		return {
			'proof-0': { hidden: step < 4, dim: step === 9 }, 'proof-1': { hidden: step < 6, dim: step === 9 }, 'proof-2': { hidden: step < 7, dim: step === 9 }, 'proof-3': { hidden: step < 8 },
			'leg-rest': { dim: step === 4 }, 'leg-rise': { dim: step === 4 }, ident: { struck: step >= 8 }, corr: { dim: step === 9 && zero },
		};
	}
	return { corr: { dim: zero } };
}

export function LawFormula({ scene, compact = false }: { scene: Scene; compact?: boolean }) {
	const locale = useLocale(), v = useVals(LESSON), step = useStep(LESSON, scene === 2 ? 'proof' : 'cut');
	if (scene === 3) {
		const ids = ['d', 'cosraw', 'cosc', 'phi'] as const;
		return <Formula lesson={LESSON} block label={t(locale, 'lc.ikFormula')}
			tex={aligned([
				tex`\t{d}{d}=\sqrt{x^2+y^2}=\s{d}`,
				tex`\cos\t{phi}{\varphi}=\t{cosphi}{\frac{L_1^2+L_2^2-d^2}{2L_1L_2}}=\s{cosraw}`,
				tex`\t{clamp}{\operatorname{clamp}}\;\to\;\t{cosc}{\s{cosc}},\quad\t{phi}{\varphi}=\arccos(\s{cosc})=\s{phi}`,
			])}
			texNarrow={aligned([
				tex`\t{d}{d}=\sqrt{x^2+y^2}=\s{d}`,
				tex`\cos\t{phi}{\varphi}=\t{cosphi}{\frac{L_1^2+L_2^2-d^2}{2L_1L_2}}`,
				tex`\t{cosraw}{\s{cosraw}}\;\to\;\t{clamp}{\operatorname{clamp}}\;\to\;\t{cosc}{\s{cosc}}`,
				tex`\t{phi}{\varphi}=\arccos(\s{cosc})=\s{phi}`,
			])}
			derived={Object.fromEntries(ids.map((k) => [k, (x: Vals) => solveIK(x)[k]]))}
			fmt={{ d: fmt, cosraw: fmtCos, cosc: fmtCos, phi: deg }} />;
	}
	if (compact) {
		return <Formula lesson={LESSON} label={t(locale, 'lc.triangleValues')}
			tex={aligned([tex`\t{a}{a=\s{a}},\;\t{b}{b=\s{b}},\;\t{th}{\theta=\s{th}}`, tex`\t{c}{c=\s{c}},\;\t{c2}{c^2=\s{c2}}`])}
			scrub={SCRUB} derived={derived} fmt={formats} />;
	}
	// A negative live value after a minus or a dot is parenthesised (audit M-2): "− 2·(−6.00)", not "− 2·−6.00".
	const z = derive(v);
	const signed = (key: 'strip_a' | 'cos') => (z[key] < 0 ? tex`(\s{${key}})` : tex`\s{${key}}`);
	const cut = [
		row('cut-0', tex`\t{c2}{c^2}=\t{R1}{a\cdot DB}+\t{R2}{b\cdot EA}`),
		row('cut-1', tex`=(\t{a2}{a^2}-\t{strip_a}{a\cdot\t{p}{b\cos\theta}})+(\t{b2}{b^2}-\t{strip_b}{b\cdot\t{q}{a\cos\theta}})`),
		row('cut-2', tex`=\t{a2}{a^2}+\t{b2}{b^2}\t{corr}{-2ab\cos\theta}`),
		row('cut-3', tex`\s{c2}=\s{a2}+\s{b2}-2\cdot${signed('strip_a')}`),
		row('cut-4', tex`\t{ab}{\mathbf{a}\cdot\mathbf{b}}=\t{strip_a}{\lvert\mathbf{a}\rvert\,(b\cos\theta)}=\s{strip_a}`),
	];
	const proof = [
		row('proof-0', tex`\t{c2}{c^2}=\htmlData{m=leg-rest}{\t{rest2}{(a-b\cos\theta)^2}}+\htmlData{m=leg-rise}{\t{rise2}{(b\sin\theta)^2}}`),
		row('proof-1', tex`=\t{a2}{a^2}\t{corr}{-2ab\cos\theta}+\t{bcos2}{b^2\cos^2\theta}+\t{bsin2}{b^2\sin^2\theta}`),
		row('proof-2', tex`=\t{a2}{a^2}+\t{b2}{b^2\,\t{ident}{(\cos^2\theta+\sin^2\theta)}}\t{corr}{-2ab\cos\theta}`),
		row('proof-3', tex`=\t{a2}{a^2}+\t{b2}{b^2}\t{corr}{-2ab\cos\theta}`),
	];
	// Narrow layouts split the widest lines in two; the second half keeps the same mark id so it reveals together.
	const narrow = scene === 0
		? [cut[0], row('cut-1', tex`=(\t{a2}{a^2}-\t{strip_a}{a\cdot\t{p}{b\cos\theta}})`), row('cut-1', tex`\quad+(\t{b2}{b^2}-\t{strip_b}{b\cdot\t{q}{a\cos\theta}})`), ...cut.slice(2)]
		: [
			row('proof-0', tex`\t{c2}{c^2}=\htmlData{m=leg-rest}{\t{rest2}{(a-b\cos\theta)^2}}`), row('proof-0', tex`\quad+\htmlData{m=leg-rise}{\t{rise2}{(b\sin\theta)^2}}`),
			row('proof-1', tex`=\t{a2}{a^2}\t{corr}{-2ab\cos\theta}`), row('proof-1', tex`\quad+\t{bcos2}{b^2\cos^2\theta}+\t{bsin2}{b^2\sin^2\theta}`),
			row('proof-2', tex`=\t{a2}{a^2}+\t{b2}{b^2\,\t{ident}{(\cos^2\theta+\sin^2\theta)}}`), row('proof-2', tex`\t{corr}{-2ab\cos\theta}`),
			proof[3],
		];
	const ledger = [
		tex`\t{c2}{c^2}=\t{a2}{a^2}+\t{b2}{b^2}\t{corr}{-2ab\cos\theta}`,
		tex`\s{c2}=\s{a2}+\s{b2}\t{corr}{-2\cdot\s{a}\cdot\s{b}\cdot${signed('cos')}}`,
		tex`\t{c}{c=\s{c}},\quad\t{th}{\theta=\s{th}},\;\cos\theta=\s{cos}`,
	];
	const ledgerNarrow = [ledger[0], tex`\s{c2}=\s{a2}+\s{b2}`, tex`\t{corr}{-2\cdot\s{a}\cdot\s{b}\cdot${signed('cos')}}`, tex`\t{c}{c=\s{c}},\quad\t{th}{\theta=\s{th}}`, tex`\cos\theta=\s{cos}`];
	const marks = marksFor(scene, step, z.th);
	return <Formula lesson={LESSON} block label={t(locale, 'lc.derivation')}
		tex={aligned(scene === 0 ? cut : scene === 2 ? proof : ledger, marks)}
		texNarrow={aligned(scene === 1 ? ledgerNarrow : narrow, marks)}
		marks={marks} derived={derived} fmt={formats} scrub={SCRUB}
		tips={{
			corr: { text: t(locale, 'lc.corrTip'), tex: '-2ab\\cos\\theta' },
			p: { text: t(locale, 'lc.pTip') },
			q: { text: t(locale, 'lc.qTip') },
		}} />;
}

/** Timeline texts, as literal dictionary keys (the dictionary usage test scans for them). */
const CUT_KEYS = ['lc.cut0', 'lc.cut1', 'lc.cut2', 'lc.cut3', 'lc.cut4', 'lc.cut5', 'lc.cut6', 'lc.cut7', 'lc.cut8', 'lc.cut9', 'lc.cut10', 'lc.cut11'] as const;
const PROOF_KEYS = ['lc.proof0', 'lc.proof1', 'lc.proof2', 'lc.proof3', 'lc.proof4', 'lc.proof5', 'lc.proof6', 'lc.proof7', 'lc.proof8', 'lc.proof9'] as const;
/** The step's focus id (spec "lit"). Proof step 5 lights both legs through the `legs` link below. */
const CUT_FOCUS = ['sq_c', 'p', 'strip_a', 'strip_b', 'R1', 'drop_c', 'R1', 'R1', 'R1c', 'R2c', 'corr', 'ab'];
const PROOF_FOCUS = ['c', 'drop_a', 'p', 'rest', 'c2', 'legs', 'corr', 'ident', 'b2', 'corr'];

/**
 * Cut step texts built from the current values (review I1): step 0 keeps the spec's hook and states the live
 * numbers, branching on a square corner, missing area or extra area; edge cases append a sentence.
 */
function cutTexts(locale: Locale, z: Derived): string[] {
	const intro = z.th === 90 ? t(locale, 'lc.cutZero')
		: z.corr < 0 ? t(locale, 'lc.cutMissing', { missing: fmt(-z.corr) })
		: t(locale, 'lc.cutExtra', { extra: fmt(z.corr) });
	const nums = { a2: fmt(z.a2), b2: fmt(z.b2), pyth: fmt(z.pyth), c2: fmt(z.c2) };
	return CUT_KEYS.map((key, i) =>
		t(locale, key, nums)
		+ (i === 0 ? intro : '')
		+ (i === 4 && (z.R1 < 0 || z.R2 < 0) ? t(locale, 'lc.cut4Debt') : '')
		+ (i >= 5 && i <= 9 && z.degenerate ? t(locale, 'lc.cutNoSquare') : ''));
}

function proofTexts(locale: Locale, z: Derived): string[] {
	return PROOF_KEYS.map((key, i) =>
		t(locale, key, { th: fmtAngle(z.th) })
		+ (i === 0 ? t(locale, z.th === 90 ? 'lc.proofRight' : 'lc.proofNotRight') : '')
		+ (i === 2 && z.th === 90 ? t(locale, 'lc.proof2Zero') : '')
		+ (i === 4 && (z.th === 0 || z.th === 180) ? t(locale, 'lc.proof4Flat') : ''));
}

/**
 * Directional hover links (spec hover maps), expanded by the shared HotLinks. `legs` is a virtual focus id of proof
 * step 5 that lights both leg tiles at once.
 */
const LINKS: HotLinkMap = {
	sq_a: ['a2'], a2: ['sq_a'], sq_b: ['b2'], b2: ['sq_b'], sq_c: ['c2'], c2: ['sq_c'],
	p: ['drop_a', 'D'], q: ['drop_b', 'E'], strip_a: ['p'], strip_b: ['q'],
	th: ['cos'], corr: ['strip_a', 'strip_b', 'p', 'q'], ab: ['strip_a'],
	R1c: ['R1'], R2c: ['R2'],
	legs: ['rest2', 'rise2'], ident: ['bcos2', 'bsin2'], rise2: ['bsin2'], bsin2: ['rise2'],
	cosraw: ['cosphi'], cosphi: ['cosraw', 'l1', 'l2', 'd'], cosc: ['clamp'], clamp: ['cosc', 'gap', 'reach_max'],
	hand: ['ik_hand'], ik_hand: ['hand'], l1: ['ik_l1'], ik_l1: ['l1'], l2: ['ik_l2'], ik_l2: ['l2'],
};

const TRANSPORTS = ['cut', 'proof_fade'] as const;
type Transport = typeof TRANSPORTS[number];

/**
 * One requestAnimationFrame services both lesson motions (cut pieces, proof tile fade), each with its own elapsed
 * time, so pausing one never stops the other. A step change starts a transition from the current placement
 * (never from the previous step: a reader may change step mid-motion); resuming after a pause or scrub continues
 * from the paused progress (`cutElapsed`). Reduced motion cuts every transport to its target.
 */
function TransportClock() {
	const store = lessonStore(LESSON);
	useEffect(() => {
		let raf = 0, disposed = false;
		const elapsed: Record<Transport, number> = { cut: 0, proof_fade: 0 }, last: Record<Transport, number> = { cut: 0, proof_fade: 0 };
		const stop = () => { cancelAnimationFrame(raf); raf = 0; };
		const ensure = () => { if (!disposed && !raf && TRANSPORTS.some((k) => store.getState().vals[k + '_play'])) raf = requestAnimationFrame(tick); };
		function tick(now: number) {
			raf = 0;
			if (disposed) return;
			const st = store.getState(), update: Record<string, number> = {};
			for (const k of TRANSPORTS) {
				if (!st.vals[k + '_play']) continue;
				elapsed[k] += Math.max(0, now - last[k]);
				last[k] = now;
				const p = cutProgress(elapsed[k], st.vals[k + '_phases'] as number), from = st.vals[k + '_from'] as number, to = st.vals[k + '_to'] as number;
				update[k + '_progress'] = p;
				update[k + '_position'] = from + (to - from) * p;
				if (p === 1) update[k + '_play'] = 0;
			}
			st.setMany(update);
			ensure();
		}
		const off = store.subscribe((st, prev) => {
			for (const k of TRANSPORTS) {
				const timeline = k === 'cut' ? 'cut' : 'proof';
				if (st.step[timeline] !== prev.step[timeline]) {
					// The fade runs between proof steps 7 and 8 only: its target is 1 (faded) from step 8 on.
					const step = st.step[timeline] ?? 0, from = prev.vals[k + '_position'] as number, to = k === 'cut' ? step : step >= 8 ? 1 : 0;
					const moves = k === 'cut' ? cutMoves(from, to, step) : step >= 7 && from !== to, play = moves && !reducedMotion();
					elapsed[k] = 0;
					last[k] = performance.now();
					st.setMany({
						[k + '_from']: from, [k + '_to']: to, [k + '_phases']: k === 'cut' && to === 9 ? 3 : 1,
						[k + '_progress']: play ? 0 : 1, [k + '_position']: play ? from : to, [k + '_play']: play ? 1 : 0,
					});
				} else if (st.vals[k + '_play'] && !prev.vals[k + '_play']) {
					elapsed[k] = cutElapsed(st.vals[k + '_progress'] as number, st.vals[k + '_phases'] as number);
					last[k] = performance.now();
				}
			}
			if (TRANSPORTS.some((k) => store.getState().vals[k + '_play'])) ensure(); else stop();
		});
		const media = matchMedia('(prefers-reduced-motion: reduce)');
		const reduce = () => {
			if (!media.matches) return;
			stop();
			const st = store.getState();
			st.setMany(Object.fromEntries(TRANSPORTS.flatMap((k) => [[k + '_play', 0], [k + '_progress', 1], [k + '_position', st.vals[k + '_to']]])));
		};
		media.addEventListener('change', reduce);
		return () => {
			disposed = true;
			stop();
			off();
			store.getState().setMany({ cut_play: 0, proof_fade_play: 0 });
			media.removeEventListener('change', reduce);
		};
	}, [store]);
	return null;
}

/** Pause / resume and a progress slider for one transport; shown only while the selected step animates. */
function MotionTransport({ kind }: { kind: Transport }) {
	const locale = useLocale(), v = useVals(LESSON), step = useStep(LESSON, kind === 'cut' ? 'cut' : 'proof'), store = lessonStore(LESSON);
	const [reduced, setReduced] = useState(false);
	useEffect(() => {
		const media = matchMedia('(prefers-reduced-motion: reduce)'), sync = () => setReduced(media.matches);
		sync();
		media.addEventListener('change', sync);
		return () => media.removeEventListener('change', sync);
	}, []);
	const from = v[kind + '_from'] as number, to = v[kind + '_to'] as number, progress = v[kind + '_progress'] as number, playing = !!v[kind + '_play'];
	if (kind === 'cut' ? !cutMoves(from, to, step) : !(step >= 7 && from !== to)) return null;
	const scrub = (p: number) => store.getState().setMany({ [kind + '_play']: 0, [kind + '_progress']: p, [kind + '_position']: from + (to - from) * p });
	const isCut = kind === 'cut';
	return <div data-lc-transport={isCut ? '' : undefined} data-lc-proof-transport={isCut ? undefined : ''} className="not-content">
		<p>{t(locale, isCut ? 'lc.cutTransport' : 'lc.fadeTransport')}</p>
		<button type="button" disabled={reduced || (!playing && progress === 1)} onClick={() => store.getState().set(kind + '_play', playing ? 0 : 1)}>
			{t(locale, playing ? 'lc.pause' : 'lc.resume')}
		</button>{' '}
		<label style={{ display: 'grid', gridTemplateColumns: 'minmax(0,1fr)', maxWidth: '100%' }}>{t(locale, 'lc.progress')}
			<input style={{ width: '100%', minWidth: 0, boxSizing: 'border-box' }} type="range" min={0} max={1} step={.01} value={progress}
				aria-label={t(locale, isCut ? 'lc.cutProgressAria' : 'lc.fadeProgressAria')} aria-valuetext={Math.round(progress * 100) + '%'}
				onChange={(e) => scrub(Number(e.currentTarget.value))} />
		</label>
		<output aria-live={playing ? 'off' : 'polite'}>{Math.round(progress * 100) + '%'}</output>
	</div>;
}

/** Challenge cards per scene (spec §1.2–§1.5); predicates read store values only, so they work from the keyboard. */
function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 0) {
		return [
			{ n: 1, text: t(locale, 'lc.ch1'), check: (x) => x.th === 90, hint: [t(locale, 'lc.ch1Hint')] },
			{ n: 2, text: t(locale, 'lc.ch2'), check: (x) => (x.th as number) > 90 },
			{ n: 3, text: t(locale, 'lc.ch3'), check: (x) => { const d = derive(x); return d.p > d.a + 1e-9 || d.q > d.b + 1e-9; }, hint: [t(locale, 'lc.ch3Hint')] },
			{ n: 4, text: t(locale, 'lc.ch4'), check: (x) => Math.abs((x.a as number) - (x.b as number)) < 1e-9, success: t(locale, 'lc.ch4Success') },
		];
	}
	if (scene === 1) {
		return [
			{ n: 5, text: t(locale, 'lc.ch5'), check: (x) => x.th === 90 },
			// Impossible by design: success fires at the limit and the card explains the triangle inequality.
			{ n: 6, text: t(locale, 'lc.ch6'), check: (x) => x.th === 180, success: t(locale, 'lc.ch6Success') },
			{ n: 7, text: t(locale, 'lc.ch7'), check: (x) => x.a === 3 && x.b === 4 && x.th === 90 },
		];
	}
	if (scene === 2) {
		// The current step must be the expansion (not the historical maximum): a reader who jumps to the end is sent back to look.
		return [{ n: 8, text: t(locale, 'lc.ch8'), check: (_x, s) => s.proof === 6, hint: [t(locale, 'lc.ch8Hint')] }];
	}
	return [
		{ n: 9, text: t(locale, 'lc.ch9'), check: (x) => { const d = solveIK(x); return Math.abs(d.d - d.reachMax) < .05 && d.d <= d.reachMax + 1e-9; } },
		{ n: 10, text: t(locale, 'lc.ch10'), check: (x) => Math.abs(solveIK(x).cosraw) > 1, success: t(locale, 'lc.ch10Success') },
		{ n: 11, text: t(locale, 'lc.ch11'), check: (x) => Math.abs(solveIK(x).cosc) < 1e-9, hint: [t(locale, 'lc.ch11Hint'), t(locale, 'lc.ch11Hint2')] },
	];
}

/** The live sentence under the formulas: IK state for LC-3, signed areas for the triangle scenes. */
function readout(scene: Scene, locale: Locale, z: Derived, ik: ReturnType<typeof solveIK>): string {
	if (scene === 3) {
		const state = ik.zero ? 'lc.ikZero' : ik.outside ? 'lc.ikOutside' : ik.inside ? 'lc.ikInside' : 'lc.ikReachable';
		return t(locale, 'lc.ikReadout', { wrist: ik.W.map(fmt).join(', '), target: ik.T.map(fmt).join(', ') }) + t(locale, state);
	}
	const areas = t(locale, 'lc.areasReadout', { a2: fmt(z.a2), b2: fmt(z.b2), strip: fmt(z.strip_a), r1: fmt(z.R1), r2: fmt(z.R2), c2: fmt(z.c2) });
	return areas + (z.degenerate ? t(locale, 'lc.noSquare') : z.DB < 0 ? t(locale, 'lc.dbNegative') : z.th === 90 ? t(locale, 'lc.zeroCorr') : '');
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale(), v = useVals(LESSON), z = derive(v), ik = solveIK(v);
	const cut = useStep(LESSON, 'cut'), proof = useStep(LESSON, 'proof');
	const challenges = challengesFor(scene, locale);
	return <section className="not-content" data-lc-controls={scene}>
		{/* Links and the clock are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 0 && <><HotLinks lesson={LESSON} links={LINKS} /><TransportClock /></>}
		{scene !== 3 && <LawFormula scene={scene} compact />}
		{scene === 0 && <MotionTransport kind="cut" />}
		{scene === 2 && <MotionTransport kind="proof_fade" />}
		{scene === 0 && <Timeline lesson={LESSON} id="cut" steps={cutTexts(locale, z).map((text, i) => ({ id: String(i), text, phases: i === 9 ? 3 : 1, focus: CUT_FOCUS[i] }))} />}
		{scene === 2 && <Timeline lesson={LESSON} id="proof" steps={proofTexts(locale, z).map((text, i) => ({ id: String(i), text, focus: PROOF_FOCUS[i] }))} />}
		<div style={scene === 1 ? { display: 'flex', flexWrap: 'wrap', gap: '1rem', alignItems: 'flex-start' } : undefined}>
			<div style={{ minWidth: 0, flex: '1 1 260px' }}><LawFormula scene={scene} /></div>
			{scene === 1 && <Ledger lesson={LESSON} label={t(locale, 'lc.ledger')} scale={4.5} max={64} fmt={fmt} zeroLabel={t(locale, 'lc.pythagoras')}
				items={[
					{ id: 'a2', label: 'a²', role: 'vecA', value: (x) => derive(x).a2 },
					{ id: 'b2', label: 'b²', role: 'vecB', value: (x) => derive(x).b2 },
					{ id: 'corr', label: '−2ab cos θ', role: 'vecC', value: (x) => derive(x).corr },
				]}
				total={{ id: 'c2', label: 'c²', value: (x) => derive(x).c2 }} />}
		</div>
		<p aria-live="polite">{readout(scene, locale, z, ik)}</p>
		{scene === 3 && <CodeView lesson={LESSON} lang="ts"
			code={'d = Math.hypot(hand[0], hand[1]);\nc = (L1*L1 + L2*L2 - d*d) / (2*L1*L2);\nc = Math.max(-1, Math.min(1, c));\nphi = Math.acos(c);'}
			lines={{ 1: 'd', 2: 'cosphi', 3: 'clamp', 4: 'phi' }} />}
		{challenges.map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
		<span hidden data-current-cut={cut} data-current-proof={proof} />
	</section>;
}
export const LawControls = localized(Controls);

/**
 * Selector and Fit button of a detail view; the readout repeats the shown piece's value in words. The selector shows
 * the piece the view draws: the reader's choice while the overview draws it, otherwise piece 0 (`shownPiece`), so
 * going back a step never leaves an empty detail; the stored choice returns when its step comes back.
 */
function DetailControls({ proof = false }: { proof?: boolean }) {
	const locale = useLocale(), v = useVals(LESSON), step = useStep(LESSON, proof ? 'proof' : 'cut'), store = lessonStore(LESSON), z = derive(v);
	const key = proof ? 'proof_detail' : 'detail', pieces = proof ? PROOF_PIECES : CUT_PIECES, fade = v.proof_fade_position as number;
	const shown = shownPiece(proof, (v[key + '_part'] as number) ?? 0, step, fade), piece = pieces[shown];
	const value = z[piece.key];
	const sentence = !proof && shown >= CUT_LENGTHS ? (value === 0 ? 'lc.zeroLength' : 'lc.sameLength') : value === 0 ? 'lc.zeroArea' : 'lc.sameArea';
	return <section className="not-content" data-lc-detail-controls={proof ? undefined : ''} data-lc-proof-detail-controls={proof ? '' : undefined}>
		<p>{t(locale, 'lc.detailIntro')}</p>
		<label>{t(locale, 'lc.inspect')}<select value={shown} onChange={(e) => store.getState().set(key + '_part', Number(e.currentTarget.value))}>
			{pieces.map(({ label }, i) => <option key={label} value={i} disabled={!pieceShown(proof, i, step, fade)}>{label}</option>)}
		</select></label>{' '}
		<button type="button" onClick={() => store.getState().set(key + '_fit', ((store.getState().vals[key + '_fit'] as number) ?? 0) + 1)}>{t(locale, 'lc.fit')}</button>
		<p aria-live="polite">
			{proof && step < 4 ? t(locale, 'lc.tilesLater') : ''}
			{piece.id === 'tri'
				? t(locale, 'lc.triDetail', { a: fmt(z.a), b: fmt(z.b), c: fmt(z.c), th: fmtAngle(z.th) })
				: t(locale, 'lc.partValue', { label: piece.label, value: fmt(value) }) + t(locale, sentence)}
		</p>
	</section>;
}
export const LawDetailControls = localized(DetailControls);
