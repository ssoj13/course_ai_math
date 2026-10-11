import { useEffect, useRef } from 'react';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useStep, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Timeline } from '../Timeline';
import { NumGrid, type GridRow } from '../NumTable';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { alignedRows } from '../tex';
import { rich } from '../rich';
import { CHECKS, cosSin, derive, fmt2, fmt3, fmtDeg, INITIAL, LESSON, rad, SPEC, SPECIAL, tex2, tex3, texDeg } from './unit-circle.math';
import type { Scene } from './unit-circle.rows';

/**
 * Lesson 0.4 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store, the DOM-only
 * controls (formulas, the quarter-turn timeline, the unroll buttons, the special-angle table, challenges, live
 * readouts) and the lazy scene islands. Everything three lives in unit-circle.impl.tsx. Reader-facing words come
 * from the `uc` dictionary namespace.
 */
lessonStore(LESSON, INITIAL);

export const UnitScene = localized(lazyIsland(() => import('./unit-circle.impl').then((m) => m.UnitScene)));

const tex = String.raw;
type Vals = Parameters<typeof derive>[0];

/** Every number a formula shows as `\s{key}`; one `derive()` for the lesson (`deg` = theta without the degree sign). */
const KEYS = ['cos', 'sin', 'cos2', 'sin2', 'one', 'rad', 'turn', 'mir'] as const;
const derived = {
	...Object.fromEntries(KEYS.map((k) => [k, (v: Vals) => derive(v)[k]])),
	deg: (v: Vals) => derive(v).th,
};
const FMT: Record<string, (v: number) => string> = {
	th: texDeg, deg: (v) => String(Math.round(v)), mir: texDeg,
	cos: tex2, sin: tex2, cos2: tex2, sin2: tex2, one: tex2, rad: tex3, turn: tex3,
};
const SCRUB = { th: SPEC.th };

/**
 * Hover maps: a term lights its pictures in every card (shadow bands, legs, feet, traced curves, graph drops) and
 * the pictures light the term. One map for the lesson; HotLinks expands it transitively.
 */
const HOT: HotLinkMap = {
	cos: ['cosb', 'cosl', 'cosf', 'cosc', 'cosd'], cosb: ['cos'], cosl: ['cos'], cosf: ['cos'], cosc: ['cos'], cosd: ['cos'],
	sin: ['sinb', 'sinl', 'sinf', 'sinc', 'sind', 'carry'], sinb: ['sin'], sinl: ['sin'], sinf: ['sin'], sinc: ['sin'], sind: ['sin'], carry: ['sin'],
	cos2: ['cos'], sin2: ['sin'], one: ['circle'],
	rad: ['arc'], arc: ['rad'], turn: ['arc'],
	mir: ['thq', 'radq'], thq: ['mir'], radq: ['mir'], Q: ['radq'], P: ['radius'],
};

const rows = (r: string[]) => alignedRows(r);

/** The scene formula of each card; wide and narrow layouts (the narrow one splits long lines). */
function UnitFormula({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const common = { lesson: LESSON, block: true, scrub: SCRUB, derived, fmt: FMT };
	const P = tex`P=(\t{cos}{\cos\theta},\;\t{sin}{\sin\theta})`;
	if (scene === 1) {
		const th = tex`\t{th}{\theta}=\s{th}`;
		const pyth = tex`\t{cos2}{\cos^2\theta}+\t{sin2}{\sin^2\theta}`;
		const sum = tex`=\s{cos2}+\s{sin2}=\t{one}{\s{one}}`;
		return <Formula {...common} label={t(locale, 'uc.formulaPoint')}
			tex={rows([tex`${th},\quad ${P}=(\s{cos},\;\s{sin})`, pyth + sum])}
			texNarrow={rows([th, P, tex`=(\s{cos},\;\s{sin})`, pyth, sum])}
			tips={{ one: { text: t(locale, 'uc.tipOne') } }} />;
	}
	if (scene === 2) {
		const lines = [
			tex`\t{th}{\theta}=\s{th}`,
			tex`\t{rad}{\theta}=\s{deg}\cdot\frac{\pi}{180}=\s{rad}`,
			tex`\t{arc}{s}=r\cdot\theta=1\cdot\s{rad}=\s{rad}`,
			tex`\t{turn}{\frac{\theta}{2\pi}}=\s{turn}`,
		];
		return <Formula {...common} label={t(locale, 'uc.formulaRad')} tex={rows(lines)}
			tips={{ arc: { text: t(locale, 'uc.tipArc') }, turn: { text: t(locale, 'uc.tipTurn') } }} />;
	}
	if (scene === 3) {
		const cos = tex`\cos\s{th}=\t{cos}{\s{cos}}`;
		const sin = tex`\sin\s{th}=\t{sin}{\s{sin}}`;
		return <Formula {...common} label={t(locale, 'uc.formulaWaves')} tex={rows([tex`${cos},\quad ${sin}`])} texNarrow={rows([cos, sin])} />;
	}
	const Q = tex`Q=(\t{sin}{\sin\theta},\;\t{cos}{\cos\theta})`;
	const mir = tex`\t{th}{\theta}=\s{th},\quad\t{mir}{90^\circ-\theta}=\s{mir}`;
	const swap = tex`\cos(\t{mir}{90^\circ-\theta})=\t{sin}{\sin\theta}`;
	return <Formula {...common} label={t(locale, 'uc.formulaMirror')}
		tex={rows([tex`${P}=(\s{cos},\;\s{sin})`, tex`${Q}=(\s{sin},\;\s{cos})`, tex`${mir},\quad ${swap}`])}
		texNarrow={rows([P, tex`=(\s{cos},\;\s{sin})`, Q, tex`=(\s{sin},\;\s{cos})`, mir, swap])}
		tips={{ mir: { text: t(locale, 'uc.tipMir') } }} />;
}

/** Challenge cards per scene; every predicate reads store values only (keyboard or typing solves it). */
function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'uc.ch1'), check: CHECKS.cosHalfNeg, hint: [t(locale, 'uc.ch1Hint')], success: t(locale, 'uc.ch1Success') },
			{ n: 2, text: t(locale, 'uc.ch2'), check: CHECKS.down },
			{ n: 3, text: t(locale, 'uc.ch3'), check: CHECKS.cosTwin, hint: [t(locale, 'uc.ch3Hint')] },
		];
	}
	if (scene === 2) {
		return [
			{ n: 4, text: t(locale, 'uc.ch4'), check: CHECKS.fullTurnFlat, success: t(locale, 'uc.ch4Success') },
			{ n: 5, text: t(locale, 'uc.ch5'), check: CHECKS.oneRadian, hint: [t(locale, 'uc.ch5Hint')] },
		];
	}
	if (scene === 3) {
		return [
			{ n: 6, text: t(locale, 'uc.ch6'), check: CHECKS.crossFirst },
			{ n: 7, text: t(locale, 'uc.ch7'), check: CHECKS.crossSecond, hint: [t(locale, 'uc.ch7Hint')] },
		];
	}
	return [{ n: 8, text: t(locale, 'uc.ch8'), check: CHECKS.onMirror }];
}

/** The live sentence under the formula (the canvas-free account of what the scene shows). */
export function readout(scene: Scene, locale: Locale, z: ReturnType<typeof derive>): string {
	const nums = { th: fmtDeg(z.th), cos: fmt2(z.cos), sin: fmt2(z.sin), rad: fmt3(z.rad), turn: fmt3(z.turn), mir: fmtDeg(z.mir) };
	if (scene === 1) {
		const where = z.quad === 0 ? t(locale, 'uc.read1Axis') : t(locale, 'uc.read1Quad', {
			q: String(z.quad), sx: z.cos > 0 ? '+' : '−', sy: z.sin > 0 ? '+' : '−',
		});
		return t(locale, 'uc.read1', nums) + where;
	}
	if (scene === 2) return t(locale, 'uc.read2', nums) + t(locale, z.roll === 1 ? 'uc.read2Flat' : z.roll === 0 ? 'uc.read2Round' : 'uc.read2Mid');
	if (scene === 3) return t(locale, 'uc.read3', nums);
	return t(locale, 'uc.read4', nums) + (z.th === 45 || z.th === 225 ? t(locale, 'uc.read4Same') : '');
}

/** The quarter-turn steps of UC-3 (literal dictionary keys: the usage test scans for them). */
const SWEEP_KEYS = ['uc.sweep0', 'uc.sweep1', 'uc.sweep2', 'uc.sweep3', 'uc.sweep4'] as const;

/**
 * UC-3 timeline: each step is a quarter turn; changing the step sweeps theta there (eased, so both curves visibly
 * trace). Only a CHANGE of step moves theta: on mount the reader's current angle stays.
 */
function Sweep() {
	const locale = useLocale();
	const step = useStep(LESSON, 'sweep');
	const th = useVals(LESSON).th as number;
	const last = useRef(step);
	useEffect(() => {
		if (last.current === step) return;
		last.current = step;
		lessonStore(LESSON).getState().set('th', step * 90, 'ease');
	}, [step]);
	// The other direction: theta brought onto a quarter turn by any other control shows that quarter's step text.
	// `last` is updated first, so the step effect above does not write theta back.
	useEffect(() => {
		if (th % 90 !== 0 || th / 90 === last.current) return;
		last.current = th / 90;
		lessonStore(LESSON).getState().setStep('sweep', th / 90);
	}, [th]);
	return <Timeline lesson={LESSON} id="sweep" steps={SWEEP_KEYS.map((key, i) => ({ id: 'q' + i, text: t(locale, key) }))} />;
}

const BUTTON: React.CSSProperties = { font: 'inherit', padding: '.3rem .6rem', marginRight: '.5rem', marginBottom: '.4rem' };
/** A pressed button (the current state) carries the hot outline, so the state is not told by `aria-pressed` alone. */
const look = (pressed: boolean): React.CSSProperties => (pressed ? { ...BUTTON, outline: '2px solid var(--viz-hot)', fontWeight: 700 } : BUTTON);

/** UC-2: roll the arc out onto the ruler and back (eased: the arc visibly straightens). */
function RollButtons() {
	const locale = useLocale();
	const roll = useVals(LESSON).roll as number;
	const set = (v: number) => lessonStore(LESSON).getState().set('roll', v, 'ease');
	return <div role="group" aria-label={t(locale, 'uc.rollGroup')} style={{ margin: '.6rem 0' }}>
		<button type="button" style={look(roll === 1)} aria-pressed={roll === 1} onClick={() => set(1)}>{t(locale, 'uc.unroll')}</button>
		<button type="button" style={look(roll === 0)} aria-pressed={roll === 0} onClick={() => set(0)}>{t(locale, 'uc.rollBack')}</button>
	</div>;
}

/**
 * UC-4: the special angles as buttons (eased jumps of theta) and as a table whose row lights up while theta sits on
 * it: the other rows dim. The cells carry term ids, so hovering one lights it like any linked number.
 */
function SpecialTable() {
	const locale = useLocale();
	const th = useVals(LESSON).th as number;
	const set = (d: number) => lessonStore(LESSON).getState().set('th', d, 'ease');
	const onRow = (SPECIAL as readonly number[]).includes(th);
	const marks: Record<string, { dim: boolean }> = {};
	const grid: GridRow[] = SPECIAL.map((d) => {
		const [c, s] = cosSin(d);
		const dim = onRow && d !== th;
		for (const col of ['rad', 'cos', 'sin']) marks[col + '@' + d] = { dim };
		return { head: fmtDeg(d), cells: { rad: { id: 'rad@' + d, value: rad(d) }, cos: { id: 'cos@' + d, value: c }, sin: { id: 'sin@' + d, value: s } } };
	});
	return <>
		<div role="group" aria-label={t(locale, 'uc.snapGroup')} style={{ margin: '.6rem 0' }}>
			{SPECIAL.map((d) => <button key={d} type="button" style={look(th === d)} aria-pressed={th === d} onClick={() => set(d)}>{fmtDeg(d)}</button>)}
		</div>
		<NumGrid lesson={LESSON} caption={t(locale, 'uc.tableCaption')} corner="θ" marks={marks} fmt={fmt3} rows={grid}
			cols={[{ id: 'rad', label: t(locale, 'uc.colRad') }, { id: 'cos', label: 'cos θ' }, { id: 'sin', label: 'sin θ' }]} />
	</>;
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const z = derive(useVals(LESSON));
	return <section className="not-content" data-uc-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		{scene === 2 && <RollButtons />}
		<UnitFormula scene={scene} />
		{scene === 3 && <Sweep />}
		{scene === 4 && <SpecialTable />}
		<p aria-live="polite">{rich(readout(scene, locale, z))}</p>
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const UnitControls = localized(Controls);
