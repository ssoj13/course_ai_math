import { useEffect, type CSSProperties } from 'react';
import { useStore } from 'zustand';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { NumGrid, type GridCol, type GridRow } from '../NumTable';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { reducedMotion } from '../scene/motion';
import { rich } from '../rich';
import {
	basis, CHECKS, CLOCK, coefA, coefB, derive, deriveEpi, fmt2, fmtInt, gram, INITIAL, INITIAL_CLOCK, LESSON, LINKS, M,
	N, SHAPES, SIGNALS, SPEC, type Derived,
} from './waves-dft.math';
import type { Scene } from './waves-dft.rows';

/**
 * Lesson 1.9 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level stores (the lesson and its
 * epicycle clock), the DOM-only controls (preset buttons, formulas, the orthogonality grid, the stick table, the
 * transport, challenges, live readouts) and the lazy scene island. Everything three lives in waves-dft.impl.tsx.
 * Reader-facing words come from the `wd` dictionary namespace.
 */
lessonStore(LESSON, INITIAL, LINKS);
lessonStore(CLOCK, INITIAL_CLOCK);

export const WavesScene = localized(lazyIsland(() => import('./waves-dft.impl').then((m) => m.WavesScene)));

const tex = String.raw;

/** Every number a formula shows as `\s{key}`. */
const DERIVED_KEYS = ['dot', 'rr', 'coef', 'energy', 'specEnergy', 'err'] as const;
const derived = Object.fromEntries(DERIVED_KEYS.map((k) => [k, (v: Parameters<typeof derive>[0]) => derive(v)[k]]));
const epiDerived = { epiErr: (v: Parameters<typeof derive>[0]) => deriveEpi(v).err };
const FMT: Record<string, (v: number) => string> = {
	k: fmtInt, K: fmtInt, S: fmtInt, dot: fmt2, rr: fmtInt, coef: fmt2, energy: fmt2, specEnergy: fmt2, err: fmt2, epiErr: fmt2,
};

/**
 * Hover maps: a sample, its ruler value and their product light together; the dot product lights every product; an
 * energy term lights the other one (they are the same number).
 */
const HOT: HotLinkMap = {
	...Object.fromEntries(Array.from({ length: N }, (_, n) => [
		['x' + n, ['r' + n, 'p' + n, 'b' + n]],
		['r' + n, ['x' + n, 'p' + n]],
		['p' + n, ['x' + n, 'r' + n]],
	]).flat()),
	dot: Array.from({ length: N }, (_, n) => 'p' + n),
	energy: ['specEnergy'],
	specEnergy: ['energy'],
	err: Array.from({ length: N }, (_, n) => 'e' + n),
};

const aligned = (rows: string[]) => tex`\begin{aligned}${rows.map((r) => '&' + r).join(tex`\\ `)}\end{aligned}`;

function WavesFormula({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const z = derive(useVals(LESSON));
	if (scene === 1) {
		const fn = z.sine ? tex`\sin` : tex`\cos`;
		const rulerTex = tex`r_n=${fn}\!\left(2\pi\cdot\s{k}\cdot n/16\right)`;
		const dotTex = tex`\t{dot}{x\cdot r}=\sum_{n=0}^{15}x_n\,r_n=\s{dot}`;
		const coefTex = z.rr > 0 ? tex`\t{coef}{\tfrac{x\cdot r}{r\cdot r}}=\tfrac{\s{dot}}{\s{rr}}=\s{coef}` : tex`r\cdot r=0`;
		return <Formula lesson={LESSON} label={t(locale, 'wd.formulaDot')} block tex={aligned([rulerTex, dotTex, coefTex])}
			scrub={{ k: SPEC.k }} derived={derived} fmt={FMT} />;
	}
	if (scene === 2) {
		const left = tex`\t{energy}{\sum_n x_n^2}=\s{energy}`;
		const right = tex`\t{specEnergy}{\sum_k\tfrac{(x\cdot r_k)^2}{r_k\cdot r_k}}=\s{specEnergy}`;
		return <Formula lesson={LESSON} label={t(locale, 'wd.formulaSpectrum')} block
			tex={left + '=' + right} texNarrow={aligned([left, right])} derived={derived} fmt={FMT} />;
	}
	// scene 3
	const sum = tex`\hat x=\sum_{k=0}^{\s{K}-1}\Big(\tfrac{x\cdot c_k}{c_k\cdot c_k}\,c_k+\tfrac{x\cdot s_k}{s_k\cdot s_k}\,s_k\Big)`;
	const err = tex`\t{err}{\lvert x-\hat x\rvert^2}=\s{err}`;
	return <Formula lesson={LESSON} label={t(locale, 'wd.formulaRebuild')} block tex={aligned([sum, err])}
		scrub={{ K: SPEC.K }} derived={derived} fmt={FMT} />;
}

function EpiFormula() {
	const locale = useLocale();
	const stick = tex`\varphi_j+2\pi k_j t`;
	const wide = tex`\mathbf{p}(t)=\mathbf{c}_0+\sum_{j=1}^{\s{S}}r_j\big(\cos(${stick}),\,\sin(${stick})\big)`;
	const err = tex`\t{epiErr}{\overline{\lvert\mathbf{p}-\mathbf{z}\rvert^2}}=\s{epiErr}`;
	const narrow = [tex`\mathbf{p}(t)=\mathbf{c}_0+\sum_{j=1}^{\s{S}}r_j\,\mathbf{u}_j(t)`, tex`\mathbf{u}_j=\big(\cos(${stick}),\,\sin(${stick})\big)`, err];
	return <Formula lesson={LESSON} label={t(locale, 'wd.formulaEpi')} block tex={aligned([wide, err])} texNarrow={aligned(narrow)}
		scrub={{ S: SPEC.S }} derived={epiDerived} fmt={FMT} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Buttons (page-side DOM, Starlight colors)

const ROW: CSSProperties = { display: 'flex', flexWrap: 'wrap', gap: '.4rem', alignItems: 'center', margin: '.6rem 0' };
const btn = (on: boolean): CSSProperties => ({
	font: 'inherit', fontSize: '.9rem', padding: '.25rem .7rem', borderRadius: '.4rem', cursor: 'pointer',
	border: '1px solid ' + (on ? 'var(--sl-color-accent)' : 'var(--sl-color-gray-5)'),
	background: on ? 'var(--sl-color-accent-low)' : 'transparent', color: 'var(--sl-color-white)', fontWeight: on ? 600 : 400,
});

/** A row of mutually exclusive choices (aria-pressed buttons). */
function Choices({ label, items, current, pick }: { label: string; items: readonly string[]; current: number; pick: (i: number) => void }) {
	return <div role="group" aria-label={label} style={ROW}>
		<span>{label}</span>
		{items.map((text, i) => <button key={i} type="button" aria-pressed={current === i} style={btn(current === i)} onClick={() => pick(i)}>{text}</button>)}
	</div>;
}

const SIGNAL_KEYS = { mix: 'wd.sigMix', pure: 'wd.sigPure', between: 'wd.sigBetween', fast: 'wd.sigFast', step: 'wd.sigStep' } as const;
const SHAPE_KEYS = { heart: 'wd.shapeHeart', star: 'wd.shapeStar', square: 'wd.shapeSquare', eight: 'wd.shapeEight' } as const;

function SignalPresets() {
	const locale = useLocale();
	const src = useVals(LESSON).src as number;
	return <>
		<Choices label={t(locale, 'wd.presetsLabel')} items={SIGNALS.map((id) => t(locale, SIGNAL_KEYS[id]))} current={src}
			pick={(i) => lessonStore(LESSON).getState().set('src', i, 'ease')} />
		{src < 0 && <p style={{ margin: 0 }}>{t(locale, 'wd.sigEdited')}</p>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Orthogonality grid and stick table

function GramGrid() {
	const locale = useLocale();
	const b = basis();
	const g = gram();
	const cols: GridCol[] = b.map((r) => ({ id: r.id, label: (r.sine ? 'sin' : 'cos') + r.k }));
	const rows: GridRow[] = b.map((r, i) => ({
		head: (r.sine ? 'sin' : 'cos') + r.k,
		cells: Object.fromEntries(b.map((c, j) => [c.id, { id: 'g' + r.id + '-' + c.id, value: g[i][j] }])),
	}));
	return <NumGrid lesson={LESSON} caption={t(locale, 'wd.gramCaption')} corner="·" cols={cols} rows={rows} fmt={fmtInt} />;
}

/** How many sticks the table lists (the chain may use up to 63). */
const TABLE_STICKS = 8;

function StickTable() {
	const locale = useLocale();
	const d = deriveEpi(useVals(LESSON));
	const cols: GridCol[] = [
		{ id: 'k', label: t(locale, 'wd.colTurns') },
		{ id: 'r', label: t(locale, 'wd.colLength') },
		{ id: 'phi', label: t(locale, 'wd.colAngle') },
	];
	const rows: GridRow[] = d.e.list.slice(0, TABLE_STICKS).map((st, j) => ({
		head: String(j + 1),
		cells: {
			k: { id: 'stk' + j, value: st.k },
			r: { id: 'str' + j, value: st.r },
			// Whole degrees: the angle of a real stick is exact to far below a degree.
			phi: { id: 'stp' + j, value: Math.round((st.phase * 180) / Math.PI) + 0 },
		},
	}));
	const marks = Object.fromEntries(d.e.list.slice(0, TABLE_STICKS).flatMap((_, j) => (j < d.S ? [] : ['stk', 'str', 'stp'].map((c) => [c + j, { dim: true }]))));
	const fmt = (v: number) => (Number.isInteger(v) ? String(v) : fmt2(v));
	return <NumGrid lesson={LESSON} caption={t(locale, 'wd.sticksCaption', { x: fmt2(d.e.pin[0]), y: fmt2(d.e.pin[1]) })} corner="#"
		cols={cols} rows={rows} marks={marks} fmt={fmt} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Epicycle transport: drawing presets, draw mode, play/pause/step/scrub

/** One full loop of the pen, in milliseconds. */
const LOOP_MS = 10000;

/** Advances the clock while `play` is on (one requestAnimationFrame loop; stops when paused or unmounted). */
function EpiClock() {
	useEffect(() => {
		const clock = lessonStore(CLOCK);
		let raf = 0;
		let last = 0;
		const tick = (now: number) => {
			const st = clock.getState();
			if (!st.vals.play) {
				raf = 0;
				return;
			}
			const dt = last ? Math.min(100, now - last) : 0;
			last = now;
			st.set('t', ((st.vals.t as number) + dt / LOOP_MS) % 1);
			raf = requestAnimationFrame(tick);
		};
		const sync = () => {
			if (clock.getState().vals.play && !raf) {
				last = 0;
				raf = requestAnimationFrame(tick);
			}
		};
		sync();
		const off = clock.subscribe(sync);
		return () => {
			off();
			cancelAnimationFrame(raf);
			clock.getState().set('play', 0);
		};
	}, []);
	return null;
}

function Transport() {
	const locale = useLocale();
	const clock = lessonStore(CLOCK);
	const time = useStore(clock, (s) => s.vals.t as number);
	const playing = useStore(clock, (s) => s.vals.play === 1);
	const set = (p: Record<string, number>) => clock.getState().setMany(p);
	return <div role="group" aria-label={t(locale, 'wd.transport')} style={ROW}>
		<button type="button" style={btn(playing)} aria-pressed={playing} onClick={() => set({ play: playing ? 0 : 1 })}>
			{t(locale, playing ? 'wd.pause' : 'wd.play')}
		</button>
		{/* One step = one sample of the drawing: the pen lands exactly on the next drawn point. */}
		<button type="button" style={btn(false)} onClick={() => set({ play: 0, t: (Math.round(time * M) + 1) % M / M })}>{t(locale, 'wd.step')}</button>
		<label style={{ display: 'flex', alignItems: 'center', gap: '.4rem', flex: '1 1 12rem', minWidth: 0 }}>
			{t(locale, 'wd.time')}
			<input type="range" min={0} max={1} step={0.001} value={time} style={{ flex: 1, minWidth: 0 }}
				aria-valuetext={t(locale, 'wd.timeOut', { t: fmt2(time) })}
				onChange={(e) => set({ play: 0, t: Number(e.currentTarget.value) })} />
		</label>
		<output aria-live="off" style={{ fontVariantNumeric: 'tabular-nums' }}>{t(locale, 'wd.timeOut', { t: fmt2(time) })}</output>
	</div>;
}

function ShapePresets() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const shape = v.shape as number;
	const drawing = v.draw === 1;
	const store = lessonStore(LESSON);
	return <>
		<div style={ROW}>
			<Choices label={t(locale, 'wd.shapesLabel')} items={SHAPES.map((id) => t(locale, SHAPE_KEYS[id]))} current={drawing ? -2 : shape}
				pick={(i) => {
					store.getState().setMany({ shape: i, draw: 0 }, reducedMotion() ? 'cut' : 'ease');
					lessonStore(CLOCK).getState().set('t', 0);
				}} />
			<button type="button" style={btn(drawing || shape === -1)} aria-pressed={drawing} onClick={() => store.getState().set('draw', drawing ? 0 : 1)}>
				{t(locale, drawing ? 'wd.drawCancel' : 'wd.drawOwn')}
			</button>
		</div>
		{drawing && <p style={{ margin: 0 }}>{t(locale, 'wd.drawHint')}</p>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Challenges and readouts

function challengesFor(scene: Scene, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'wd.ch1'), check: CHECKS.strongest, hint: [t(locale, 'wd.ch1Hint')] },
			{ n: 2, text: t(locale, 'wd.ch2'), check: CHECKS.alias, hint: [t(locale, 'wd.ch2Hint')], success: t(locale, 'wd.ch2Success') },
		];
	}
	if (scene === 3) return [{ n: 3, text: t(locale, 'wd.ch3'), check: CHECKS.exactEarly, hint: [t(locale, 'wd.ch3Hint')] }];
	if (scene === 4) {
		return [
			{ n: 4, text: t(locale, 'wd.ch4'), check: CHECKS.heart, hint: [t(locale, 'wd.ch4Hint')] },
			{ n: 5, text: t(locale, 'wd.ch5'), check: CHECKS.drawn, success: t(locale, 'wd.ch5Success') },
		];
	}
	return [];
}

/** The live sentence under the formula (the canvas-free account of what the scene shows). */
export function readout(scene: Scene, locale: Locale, z: Derived): string {
	if (scene === 1) {
		const fn = z.sine ? 'sin' : 'cos';
		if (z.rr === 0) return t(locale, 'wd.read1Empty', { k: z.k });
		const base = t(locale, 'wd.read1', { fn, k: z.k, dot: fmt2(z.dot), rr: fmtInt(z.rr), coef: fmt2(z.coef) });
		const perp = Math.abs(z.dot) < 1e-9 ? t(locale, 'wd.read1Perp') : '';
		const story = z.src === SIGNALS.indexOf('fast') ? t(locale, 'wd.readFast') : z.src === SIGNALS.indexOf('between') ? t(locale, 'wd.readBetween') : '';
		return base + perp + story;
	}
	if (scene === 2) {
		const b = z.bins[z.k];
		return t(locale, 'wd.read2', { k: z.k, a: fmt2(coefA(b)), b: fmt2(coefB(b)), amp: fmt2(z.amp), e1: fmt2(z.energy), e2: fmt2(z.specEnergy) });
	}
	if (z.K === 0) return t(locale, 'wd.read3None', { err: fmt2(z.err) });
	return t(locale, 'wd.read3', { last: z.K - 1, err: fmt2(z.err) }) + (z.err < 1e-9 ? t(locale, 'wd.read3Exact') : '');
}

function epiReadout(locale: Locale, d: ReturnType<typeof deriveEpi>): string {
	return t(locale, 'wd.read4', { S: d.S, err: fmt2(d.err), rel: fmt2(100 * d.rel) });
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	const z = derive(v);
	return <section className="not-content" data-wd-controls={scene}>
		{/* Hover links and the clock are lesson-wide; they mount once each. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		{scene === 4 && <EpiClock />}
		{(scene === 1 || scene === 3) && <SignalPresets />}
		{scene === 4 ? <><ShapePresets /><Transport /><EpiFormula /></> : <WavesFormula scene={scene} />}
		<p aria-live="polite">{rich(scene === 4 ? epiReadout(locale, deriveEpi(v)) : readout(scene, locale, z))}</p>
		{scene === 2 && <GramGrid />}
		{scene === 4 && <StickTable />}
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const WavesControls = localized(Controls);

