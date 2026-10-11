import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import { roleVar } from '../palette';
import { rich } from '../rich';
import {
	CHECKS, INITIAL, LESSON, analogy, cosine, dot, dotMatrix, fmt2, fmtCos, fmtDeg, fmtInt, len, nearest, products, rank, scaled, tex as texFmt, toy,
} from './cosine-similarity.math';
import {
	ANALOGIES, DOCS, GROUPS, GROUP_ROLE, LLM, NEIGHBOURS, PROV, QUERIES, SENTENCE, VECS, WORDS, pairOf, type Group,
} from './cosine-similarity.data';
import { DOC_KEY, GROUP_KEY, NOTE_KEY, Q_KEY, TOY_KEY, type Scene } from './cosine-similarity.rows';
import './cosine-similarity.css';

/**
 * Lesson 1.7 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store, the DOM-only
 * controls under each scene, and the DOM widgets of the embedding sections (table lookup, neighbours, analogies,
 * semantic search, attention preview). Everything three lives in cosine-similarity.impl.tsx. Reader-facing words come
 * from the `cs` dictionary namespace; numbers come from cosine-similarity.math.ts and the real table in
 * cosine-similarity.data.ts.
 */
lessonStore(LESSON, INITIAL);

export const CosScene = localized(lazyIsland(() => import('./cosine-similarity.impl').then((m) => m.CosScene)));

const store = () => lessonStore(LESSON).getState();
const tex = String.raw;
type Vals = Parameters<typeof toy>[0];

// ---------------------------------------------------------------------------------------------------------------
// Heatmap drawing: one canvas helper for the three tables (embedding rows, cosine matrix, products strip).

/** Diverging colours from the page theme (cosine-similarity.css): negative, zero, positive as [r, g, b]. */
type Ramp = [number[], number[], number[]];

const hex = (s: string): number[] => {
	const m = /^#?([0-9a-f]{6})$/i.exec(s.trim());
	if (!m) throw new Error(`cosine-similarity: colour "${s}" is not #rrggbb`);
	const n = parseInt(m[1], 16);
	return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};

function readRamp(el: Element): Ramp {
	const cs = getComputedStyle(el);
	return [hex(cs.getPropertyValue('--cs-neg')), hex(cs.getPropertyValue('--cs-mid')), hex(cs.getPropertyValue('--cs-pos'))];
}

/** Colour of `x` in [−1, 1] (clipped): zero is the page's neutral, the ends are the two hues. */
export function rampColor(r: Ramp, x: number): string {
	const t = Math.max(-1, Math.min(1, Number.isFinite(x) ? x : 0));
	const end = t < 0 ? r[0] : r[2], k = Math.abs(t);
	const c = r[1].map((m, i) => Math.round(m + (end[i] - m) * k));
	return `rgb(${c[0]},${c[1]},${c[2]})`;
}

/**
 * The page theme's ramp, re-read when Starlight flips `data-theme` on <html> (canvas pixels do not follow CSS
 * variables on their own). Null before mount (server render draws nothing; the box is reserved by CSS).
 */
function useRamp(ref: React.RefObject<Element | null>): Ramp | null {
	const [ramp, setRamp] = useState<Ramp | null>(null);
	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		const read = () => setRamp(readRamp(el));
		read();
		const mo = new MutationObserver(read);
		mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
		return () => mo.disconnect();
	}, [ref]);
	return ramp;
}

interface HeatProps {
	/** Row-major values; each row the same length. */
	cells: readonly (readonly number[])[];
	/** Value mapped to the full hue (|x| ≥ scale saturates). */
	scale: number;
	/** Cell aspect: the canvas is `cols` x `rows` cells stretched to the CSS box (aspect-ratio set by the caller). */
	label: string;
	/** Outline: a row, or a cell [row, col]. */
	mark?: { row: number; col?: number };
	onPick?: (row: number, col: number, how: 'hover' | 'click') => void;
	className?: string;
	keys?: (e: React.KeyboardEvent) => void;
}

/**
 * A heatmap on a canvas at device resolution. Pointer position → cell for hover/tap; the caller supplies keyboard
 * handling and the text alternative. Redrawn when the cells, the mark, the ramp or the box size change.
 */
function Heat({ cells, scale, label, mark, onPick, className, keys }: HeatProps) {
	const ref = useRef<HTMLCanvasElement>(null);
	const ramp = useRamp(ref);
	const [size, setSize] = useState<[number, number] | null>(null);
	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		const ro = new ResizeObserver(() => setSize([el.clientWidth, el.clientHeight]));
		ro.observe(el);
		return () => ro.disconnect();
	}, []);
	useEffect(() => {
		const el = ref.current;
		if (!el || !ramp || !size) return;
		const dpr = window.devicePixelRatio || 1;
		const [w, h] = size;
		el.width = Math.max(1, Math.round(w * dpr));
		el.height = Math.max(1, Math.round(h * dpr));
		const g = el.getContext('2d');
		if (!g) return;
		const rows = cells.length, cols = cells[0]?.length ?? 0;
		const cw = el.width / cols, ch = el.height / rows;
		for (let i = 0; i < rows; i++) {
			for (let j = 0; j < cols; j++) {
				g.fillStyle = rampColor(ramp, cells[i][j] / scale);
				// Cells overlap by a device pixel so no hairline gaps show between fractional cell edges.
				g.fillRect(Math.floor(j * cw), Math.floor(i * ch), Math.ceil(cw) + 1, Math.ceil(ch) + 1);
			}
		}
		if (mark) {
			const ink = getComputedStyle(el).getPropertyValue('--cs-mark').trim() || '#000000';
			g.strokeStyle = ink;
			g.lineWidth = Math.max(1, 1.5 * dpr);
			if (mark.col === undefined) g.strokeRect(0.5, mark.row * ch + 0.5, el.width - 1, Math.max(1, ch - 1));
			else g.strokeRect(mark.col * cw, mark.row * ch, Math.max(cw, 2 * dpr), Math.max(ch, 2 * dpr));
		}
	}, [cells, scale, mark?.row, mark?.col, ramp, size]);
	const cellAt = (e: React.PointerEvent<HTMLCanvasElement>): [number, number] => {
		const r = e.currentTarget.getBoundingClientRect();
		const rows = cells.length, cols = cells[0]?.length ?? 0;
		const i = Math.min(rows - 1, Math.max(0, Math.floor(((e.clientY - r.top) / r.height) * rows)));
		const j = Math.min(cols - 1, Math.max(0, Math.floor(((e.clientX - r.left) / r.width) * cols)));
		return [i, j];
	};
	return <canvas ref={ref} className={'cs-heat ' + (className ?? '')} role="img" aria-label={label} tabIndex={keys ? 0 : undefined}
		onKeyDown={keys}
		onPointerMove={onPick && ((e) => { if (e.pointerType === 'mouse') onPick(...cellAt(e), 'hover'); })}
		onPointerDown={onPick && ((e) => onPick(...cellAt(e), 'click'))} />;
}

/** Group colour key of the scene points and the heatmap edges. */
function Legend() {
	const locale = useLocale();
	return <ul className="cs-legend">
		{GROUPS.map((g) => <li key={g}><span className="cs-swatch" style={{ background: roleVar(GROUP_ROLE[g]) }} />{t(locale, GROUP_KEY[g])}</li>)}
	</ul>;
}

/** A select of table words (the one input every embedding widget shares). */
function WordSelect({ value, onChange, label, id }: { value: number; onChange: (i: number) => void; label: string; id?: string }) {
	return <label className="cs-field">
		<span>{label}</span>
		<select id={id} value={value} onChange={(e) => onChange(Number(e.target.value))}>
			{WORDS.map((w, i) => <option key={w.word} value={i}>{w.word}</option>)}
		</select>
	</label>;
}

/** Horizontal cosine bar in [−1, 1] around a centre line, with its number. */
function CosBar({ cos }: { cos: number }) {
	const x = Number.isFinite(cos) ? Math.max(-1, Math.min(1, cos)) : 0;
	return <span className="cs-bar" aria-hidden="true">
		<span className={x < 0 ? 'cs-bar-fill is-neg' : 'cs-bar-fill'} style={{ left: x < 0 ? `${50 + 50 * x}%` : '50%', width: `${50 * Math.abs(x)}%` }} />
	</span>;
}

// ---------------------------------------------------------------------------------------------------------------
// Scene controls (formula, readout, challenges) under COS-1 and COS-2; heatmap + products under COS-3.

const derivedToy = {
	dotCD: (v: Vals) => toy(v).dotCD, nCat: (v: Vals) => toy(v).nCat, nDog: (v: Vals) => toy(v).nDog, cosCD: (v: Vals) => toy(v).cosCD,
};
const derivedScale = {
	dotS: (v: Vals) => scaled(v).dot, na: (v: Vals) => scaled(v).na, nsb: (v: Vals) => scaled(v).nsb, cos: (v: Vals) => scaled(v).cos, dist: (v: Vals) => scaled(v).dist,
};
const FMT: Record<string, (x: number) => string> = {
	dotCD: texFmt.num, nCat: texFmt.num, nDog: texFmt.num, cosCD: texFmt.cos,
	dotS: texFmt.num, na: texFmt.num, nsb: texFmt.num, cos: texFmt.cos, dist: texFmt.num, s: texFmt.num,
};

/** Formula term ↔ scene object ↔ N-panel row. */
const HOT: HotLinkMap = {
	cosCD: ['thCD'], thCD: ['cosCD'], cos: ['th'], th: ['cos'], dist: ['gap'], gap: ['dist'],
};

const frac = (num: string, den: string) => tex`\dfrac{${num}}{${den}}`;
const A = tex`\mathbf{a}`, B = tex`\mathbf{b}`;
const abs = (x: string) => tex`\lvert ${x}\rvert`;

function ToyFormula() {
	const locale = useLocale();
	return <Formula lesson={LESSON} label={t(locale, 'cs.formulaToy')} block derived={derivedToy} fmt={FMT}
		tex={tex`\t{cosCD}{\cos\theta}=${frac(tex`\mathbf{a}\cdot\mathbf{b}`, tex`${abs(A)}\,${abs(B)}`)}=${frac(tex`\s{dotCD}`, tex`\s{nCat}\cdot\s{nDog}`)}=\s{cosCD}`}
		tips={{ cosCD: { text: t(locale, 'cs.tipCos') } }} />;
}

function ScaleFormula() {
	const locale = useLocale();
	const sb = tex`s\,${B}`;
	return <Formula lesson={LESSON} label={t(locale, 'cs.formulaScale')} block derived={derivedScale} fmt={FMT} scrub={{ s: { min: 0, max: 2, step: 0.05 } }}
		tex={tex`\begin{aligned}&\t{cos}{\cos\theta}=${frac(tex`${A}\cdot ${sb}`, tex`${abs(A)}\,${abs(sb)}`)}=${frac(tex`\s{dotS}`, tex`\s{na}\cdot\s{nsb}`)}=\s{cos}\qquad s=\s{s}\\ &\t{dist}{${abs(tex`${A}-${sb}`)}}=\s{dist}\end{aligned}`}
		texNarrow={tex`\begin{aligned}&\t{cos}{\cos\theta}=${frac(tex`\s{dotS}`, tex`\s{na}\cdot\s{nsb}`)}=\s{cos}\\ &s=\s{s}\qquad \t{dist}{${abs(tex`${A}-${sb}`)}}=\s{dist}\end{aligned}`}
		tips={{ cos: { text: t(locale, 'cs.tipCosScale') }, dist: { text: t(locale, 'cs.tipDist') } }} />;
}

/** The live sentence under a scene (the canvas-free account of what it shows). */
export function readout(scene: 1 | 2, locale: Locale, v: Vals): string {
	if (scene === 1) {
		const z = toy(v);
		if (z.zero.length) return t(locale, 'cs.read1Zero', { name: z.zero.map((k) => t(locale, TOY_KEY[k])).join(', ') });
		return t(locale, 'cs.read1', { cd: fmtCos(z.cosCD), cc: fmtCos(z.cosCC), dc: fmtCos(z.cosDC), th: fmtDeg(z.thCD) });
	}
	const z = scaled(v);
	if (!Number.isFinite(z.cos)) return t(locale, len(v.sa as number[]) === 0 ? 'cs.read2ZeroA' : 'cs.read2Zero', { dist: fmt2(z.dist) });
	return t(locale, 'cs.read2', { s: fmt2(v.s as number), cos: fmtCos(z.cos), dist: fmt2(z.dist) });
}

function challengesFor(scene: 1 | 2, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'cs.ch1'), check: CHECKS.catCar, success: t(locale, 'cs.ch1Success') },
			{ n: 2, text: t(locale, 'cs.ch2'), check: CHECKS.perp, hint: [t(locale, 'cs.ch2Hint')] },
		];
	}
	return [
		{ n: 3, text: t(locale, 'cs.ch3'), check: CHECKS.farSame, hint: [t(locale, 'cs.ch3Hint')] },
		{ n: 4, text: t(locale, 'cs.ch4'), check: CHECKS.zero, success: t(locale, 'cs.ch4Success') },
	];
}

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	if (scene === 3) return <PairControls />;
	return <section className="not-content cs-controls" data-cs-controls={scene}>
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		{scene === 1 ? <><p className="cs-note">{t(locale, 'cs.toyWho')}</p><ToyFormula /></> : <ScaleFormula />}
		<p aria-live="polite">{readout(scene, locale, v)}</p>
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const CosControls = localized(Controls);

/** Every pairwise cosine of the table words, in the full 300 dimensions (computed once per page). */
const COS = (() => {
	const u = VECS.map((r) => { const n = len(r); return r.map((x) => x / n); });
	return u.map((a) => u.map((b) => dot(a, b)));
})();

/** The COS-3 controller: cosine heatmap of the 62 words, the two chosen words' 300 products, two selects. */
function PairControls() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const [i, j] = pairOf(v);
	const setPair = (a: number, b: number) => store().set('pair', [a, b]);
	const a = VECS[i], b = VECS[j];
	const prod = useMemo(() => [products(a, b)], [a, b]);
	const sum = dot(a, b), c = cosine(a, b);
	const max = Math.max(...prod[0].map(Math.abs));
	const keys = (e: React.KeyboardEvent) => {
		const step: Record<string, [number, number]> = { ArrowUp: [-1, 0], ArrowDown: [1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1] };
		const d = step[e.key];
		if (!d) return;
		e.preventDefault();
		const n = WORDS.length;
		setPair((i + d[0] + n) % n, (j + d[1] + n) % n);
	};
	return <section className="not-content cs-controls" data-cs-controls={3}>
		<Legend />
		<div className="cs-pair-grid">
			<figure className="cs-fig">
				<div className="cs-heat-box">
					<div className="cs-edge cs-edge-top" aria-hidden="true">{WORDS.map((w) => <span key={w.word} style={{ background: roleVar(GROUP_ROLE[w.group as Group]) }} />)}</div>
					<div className="cs-edge cs-edge-left" aria-hidden="true">{WORDS.map((w) => <span key={w.word} style={{ background: roleVar(GROUP_ROLE[w.group as Group]) }} />)}</div>
					<Heat className="cs-heat-square" cells={COS} scale={1} label={t(locale, 'cs.heatLabel')} mark={{ row: i, col: j }} keys={keys}
						onPick={(r, col) => setPair(r, col)} />
				</div>
				<figcaption>{t(locale, 'cs.heatCaption')}</figcaption>
			</figure>
			<div className="cs-pair-side">
				<WordSelect label={t(locale, 'cs.wordA')} value={i} onChange={(x) => setPair(x, j)} />
				<WordSelect label={t(locale, 'cs.wordB')} value={j} onChange={(x) => setPair(i, x)} />
				<p className="cs-big" aria-live="polite">{t(locale, 'cs.pairRead', { a: WORDS[i].word, b: WORDS[j].word, cos: fmtCos(c), th: fmtDeg(Math.acos(c) * 180 / Math.PI) })}</p>
			</div>
		</div>
		<figure className="cs-fig">
			<Heat className="cs-heat-strip" cells={prod} scale={max || 1} label={t(locale, 'cs.prodLabel', { a: WORDS[i].word, b: WORDS[j].word })} />
			<figcaption>{rich(t(locale, 'cs.prodCaption', { sum: fmt2(sum), na: fmt2(len(a)), nb: fmt2(len(b)), cos: fmtCos(c) }))}</figcaption>
		</figure>
	</section>;
}

// ---------------------------------------------------------------------------------------------------------------
// Widget: the embedding table and the lookup (token id → row).

const ALL_ROWS = [...WORDS, ...SENTENCE.filter((w, k, s) => s.indexOf(w) === k && w.group === 'sentence')];

function Lookup() {
	const locale = useLocale();
	const [sel, setSel] = useState(ALL_ROWS.findIndex((w) => w.word === 'cat'));
	const cells = useMemo(() => ALL_ROWS.map((w) => w.v), []);
	const w = ALL_ROWS[sel];
	const head = w.v.slice(0, 6).map(fmt2).join(', ');
	return <section className="not-content cs-widget" aria-labelledby="cs-lookup-h">
		<h3 id="cs-lookup-h" className="cs-widget-title">{t(locale, 'cs.lookupTitle')}</h3>
		<div className="cs-tokens" role="group" aria-label={t(locale, 'cs.lookupSentence')}>
			{SENTENCE.map((s, k) => {
				const at = ALL_ROWS.indexOf(s);
				return <button type="button" key={k} className={at === sel ? 'cs-token is-on' : 'cs-token'} aria-pressed={at === sel} onClick={() => setSel(at)}>
					<span className="cs-token-word">{s.word}</span><span className="cs-token-id">#{fmtInt(s.row)}</span>
				</button>;
			})}
		</div>
		<label className="cs-field">
			<span>{t(locale, 'cs.lookupPick')}</span>
			<select value={sel} onChange={(e) => setSel(Number(e.target.value))}>
				{ALL_ROWS.map((x, k) => <option key={x.word} value={k}>{x.word}</option>)}
			</select>
		</label>
		<p className="cs-row-read" aria-live="polite">{rich(t(locale, 'cs.lookupRead', { word: w.word, row: fmtInt(w.row), rows: fmtInt(PROV.words.table_rows), dims: PROV.words.dims, head }))}</p>
		<figure className="cs-fig">
			<Heat className="cs-heat-table" cells={cells} scale={1} label={t(locale, 'cs.tableLabel', { n: ALL_ROWS.length, dims: PROV.words.dims })} mark={{ row: sel }}
				onPick={(r, _c, how) => { if (how === 'click') setSel(r); }} />
			<figcaption>{t(locale, 'cs.tableCaption', { n: ALL_ROWS.length, dims: PROV.words.dims })}</figcaption>
		</figure>
		<p className="cs-note">{rich(t(locale, 'cs.llmRead', {
			model: LLM.model, rows: fmtInt(LLM.rows), dims: fmtInt(LLM.dims), text: LLM.text,
			ids: LLM.tokens.map((k) => `${k.piece.replace(/^Ġ/, '␣')} → ${fmtInt(k.id)}`).join(', '),
		}))}</p>
	</section>;
}
export const CosLookup = localized(Lookup);

// ---------------------------------------------------------------------------------------------------------------
// Widget: nearest neighbours, live while the reader edits one coordinate.

const K_NN = 8;

/** The coordinate with the largest magnitude: editing it moves the word the most per step. */
const strongest = (v: readonly number[]) => v.reduce((best, x, k) => (Math.abs(x) > Math.abs(v[best]) ? k : best), 0);

function Neighbours() {
	const locale = useLocale();
	const uid = useId();
	const [w, setW] = useState(() => WORDS.findIndex((x) => x.word === 'cat'));
	const [dim, setDim] = useState(() => strongest(VECS[w]));
	const [val, setVal] = useState(() => VECS[w][dim]);
	const pick = (i: number) => { setW(i); const d = strongest(VECS[i]); setDim(d); setVal(VECS[i][d]); };
	const chooseDim = (d: number) => { const k = Math.max(0, Math.min(PROV.words.dims - 1, Math.round(d))); setDim(k); setVal(VECS[w][k]); };
	const q = useMemo(() => VECS[w].map((x, k) => (k === dim ? val : x)), [w, dim, val]);
	const hits = nearest(VECS, q, K_NN, new Set([w]));
	const orig = VECS[w][dim];
	const full = NEIGHBOURS[WORDS[w].word];
	return <section className="not-content cs-widget" aria-labelledby={uid + 'h'}>
		<h3 id={uid + 'h'} className="cs-widget-title">{t(locale, 'cs.nnTitle')}</h3>
		<div className="cs-row-inputs">
			<WordSelect label={t(locale, 'cs.nnWord')} value={w} onChange={pick} />
			<label className="cs-field">
				<span>{t(locale, 'cs.nnDim')}</span>
				<input type="number" min={0} max={PROV.words.dims - 1} step={1} value={dim} onChange={(e) => chooseDim(Number(e.target.value))} />
			</label>
			<label className="cs-field cs-field-wide">
				<span>{t(locale, 'cs.nnVal', { dim, orig: fmt2(orig) })}</span>
				<input type="range" min={-3} max={3} step={0.01} value={val} onChange={(e) => setVal(Number(e.target.value))} aria-valuetext={fmt2(val)} />
			</label>
			<button type="button" className="cs-btn" onClick={() => setVal(orig)} disabled={val === orig}>{t(locale, 'cs.reset')}</button>
		</div>
		<p className="cs-note" aria-live="polite">{t(locale, val === orig ? 'cs.nnSame' : 'cs.nnEdited', { word: WORDS[w].word, dim, val: fmt2(val), orig: fmt2(orig) })}</p>
		<ol className="cs-list">
			{hits.map((h) => <li key={h.i}>
				<span className="cs-list-word"><span className="cs-swatch" style={{ background: roleVar(GROUP_ROLE[WORDS[h.i].group as Group]) }} />{WORDS[h.i].word}</span>
				<CosBar cos={h.cos} /><span className="cs-num">{fmtCos(h.cos)}</span>
			</li>)}
		</ol>
		{full && <p className="cs-note">{t(locale, 'cs.nnFull', { word: WORDS[w].word, list: full.map((x) => `${x.word} ${fmtCos(x.cos)}`).join(', ') })}</p>}
	</section>;
}
export const CosNeighbours = localized(Neighbours);

// ---------------------------------------------------------------------------------------------------------------
// Widget: analogy arithmetic a − b + c.

function Analogy() {
	const locale = useLocale();
	const uid = useId();
	const at = (a: { a: string; b: string; c: string }) => [a.a, a.b, a.c].map((x) => WORDS.findIndex((w) => w.word === x));
	const [abc, setAbc] = useState(() => at(ANALOGIES[0]));
	const [exclude, setExclude] = useState(true);
	const preset = ANALOGIES.findIndex((p) => at(p).every((x, k) => x === abc[k]));
	const hits = analogy(VECS, abc[0], abc[1], abc[2], 5, exclude);
	const set = (k: number, i: number) => setAbc(abc.map((x, m) => (m === k ? i : x)));
	const names = abc.map((i) => WORDS[i].word);
	return <section className="not-content cs-widget" aria-labelledby={uid + 'h'}>
		<h3 id={uid + 'h'} className="cs-widget-title">{t(locale, 'cs.anTitle')}</h3>
		<div className="cs-presets" role="group" aria-label={t(locale, 'cs.anPresets')}>
			{ANALOGIES.map((p, k) => <button type="button" key={k} className={k === preset ? 'cs-chip is-on' : 'cs-chip'} aria-pressed={k === preset} onClick={() => setAbc(at(p))}>
				{p.a} − {p.b} + {p.c}
			</button>)}
		</div>
		<div className="cs-row-inputs">
			<WordSelect label="a" value={abc[0]} onChange={(i) => set(0, i)} />
			<span className="cs-op" aria-hidden="true">−</span>
			<WordSelect label="b" value={abc[1]} onChange={(i) => set(1, i)} />
			<span className="cs-op" aria-hidden="true">+</span>
			<WordSelect label="c" value={abc[2]} onChange={(i) => set(2, i)} />
			<label className="cs-check"><input type="checkbox" checked={exclude} onChange={(e) => setExclude(e.target.checked)} />{t(locale, 'cs.anExclude')}</label>
		</div>
		<p className="cs-note" aria-live="polite">{t(locale, 'cs.anRead', { a: names[0], b: names[1], c: names[2], top: hits[0] ? WORDS[hits[0].i].word : '–' })}</p>
		<ol className="cs-list">
			{hits.map((h) => <li key={h.i} className={abc.includes(h.i) ? 'is-input' : undefined}>
				<span className="cs-list-word">{WORDS[h.i].word}{abc.includes(h.i) && <em className="cs-tag">{t(locale, 'cs.anInput')}</em>}</span>
				<CosBar cos={h.cos} /><span className="cs-num">{fmtCos(h.cos)}</span>
			</li>)}
		</ol>
		{preset >= 0 && <>
			<p className="cs-note">{t(locale, 'cs.anFull', { list: ANALOGIES[preset].full_vocab_top.map((x) => `${x.word} ${fmtCos(x.cos)}`).join(', ') })}</p>
			<p className="cs-verdict">{rich(t(locale, NOTE_KEY[preset]))}</p>
		</>}
	</section>;
}
export const CosAnalogy = localized(Analogy);

// ---------------------------------------------------------------------------------------------------------------
// Widget: semantic search and the retrieval step of RAG.

function Search() {
	const locale = useLocale();
	const uid = useId();
	const [q, setQ] = useState(0);
	const [k, setK] = useState(2);
	const ranked = rank(DOCS.map((d) => d.v), QUERIES[q].v);
	const top = ranked.slice(0, k);
	const ru = locale !== 'en';
	return <section className="not-content cs-widget" aria-labelledby={uid + 'h'}>
		<h3 id={uid + 'h'} className="cs-widget-title">{t(locale, 'cs.srTitle')}</h3>
		<div className="cs-row-inputs">
			<label className="cs-field cs-field-wide">
				<span>{t(locale, 'cs.srQuery')}</span>
				<select value={q} onChange={(e) => setQ(Number(e.target.value))}>
					{QUERIES.map((_, i) => <option key={i} value={i}>{t(locale, Q_KEY[i])}</option>)}
				</select>
			</label>
			<label className="cs-field">
				<span>{t(locale, 'cs.srK', { k })}</span>
				<input type="range" min={1} max={5} step={1} value={k} onChange={(e) => setK(Number(e.target.value))} />
			</label>
		</div>
		{ru && <p className="cs-orig">{t(locale, 'cs.srOrig')} <span lang="en">{QUERIES[q].text}</span></p>}
		<ol className="cs-list cs-docs">
			{ranked.map((h, r) => <li key={h.i} className={r < k ? 'is-top' : undefined}>
				<span className="cs-doc">{t(locale, DOC_KEY[h.i])}{ru && <span className="cs-orig" lang="en">{DOCS[h.i].text}</span>}</span>
				<CosBar cos={h.cos} /><span className="cs-num">{fmtCos(h.cos)}</span>
			</li>)}
		</ol>
		<figure className="cs-fig">
			<pre className="cs-prompt" aria-label={t(locale, 'cs.srPromptLabel')}>{t(locale, 'cs.srPrompt', {
				context: top.map((h) => '- ' + t(locale, DOC_KEY[h.i])).join('\n'), question: t(locale, Q_KEY[q]),
			})}</pre>
			<figcaption>{t(locale, 'cs.srCaption', { k })}</figcaption>
		</figure>
	</section>;
}
export const CosSearch = localized(Search);

// ---------------------------------------------------------------------------------------------------------------
// Widget: attention preview, the raw dot-product table of one sentence.

const ATT = dotMatrix(SENTENCE.map((w) => w.v));
const ATT_MAX = Math.max(...ATT.flat().map(Math.abs));

function Attention() {
	const locale = useLocale();
	const [hot, setHot] = useState<[number, number] | null>(null);
	const ramp = useRef<HTMLTableElement>(null);
	const colors = useRamp(ramp);
	const words = SENTENCE.map((w) => w.word);
	return <figure className="not-content cs-widget cs-fig">
		<div className="cs-att-wrap">
		<table ref={ramp} className="cs-att" aria-label={t(locale, 'cs.attLabel')}>
			<thead><tr><th scope="col" />{words.map((w, j) => <th key={j} scope="col" className={hot?.[1] === j ? 'is-hot' : undefined}>{w}</th>)}</tr></thead>
			<tbody>
				{ATT.map((row, i) => <tr key={i}>
					<th scope="row" className={hot?.[0] === i ? 'is-hot' : undefined}>{words[i]}</th>
					{row.map((x, j) => <td key={j} tabIndex={0}
						style={colors ? { background: rampColor(colors, x / ATT_MAX) } : undefined}
						className={Math.abs(x) / ATT_MAX > 0.55 ? (x < 0 ? 'is-strong-neg' : 'is-strong-pos') : undefined}
						onPointerEnter={() => setHot([i, j])} onPointerLeave={() => setHot(null)} onFocus={() => setHot([i, j])} onBlur={() => setHot(null)}>
						{fmt2(x)}
					</td>)}
				</tr>)}
			</tbody>
		</table>
		</div>
		<figcaption aria-live="polite">{hot
			? t(locale, 'cs.attRead', { a: words[hot[0]], b: words[hot[1]], dot: fmt2(ATT[hot[0]][hot[1]]), cos: fmtCos(cosine(SENTENCE[hot[0]].v, SENTENCE[hot[1]].v)) })
			: t(locale, 'cs.attCaption')}</figcaption>
	</figure>;
}
export const CosAttention = localized(Attention);
