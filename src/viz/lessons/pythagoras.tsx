import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useStep, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { NumGrid, type GridRow } from '../NumTable';
import { Num } from '../Num';
import { Timeline } from '../Timeline';
import { CodeView } from '../CodeView';
import { Challenge, type ChallengeProps } from '../Challenge';
import { HotLinks, type HotLinkMap } from '../HotLinks';
import type { Marks } from '../marks';
import { alignedRows, type AlignedRow } from '../tex';
import {
	CHECKS, derive2, derive3, deriveDis, deriveN, deriveR, DIS_STEPS, fmt1, fmt2, fmtCell, INITIAL, LESSON, MAX_N, pKey, qKey, ROOM_STEPS, SPEC,
} from './pythagoras.math';
import type { Scene } from './pythagoras.rows';

/**
 * Lesson 0.3 entry (SSR-safe, see AGENTS.md "Lesson authoring pattern"): the module-level store, the DOM-only
 * controls (formulas, timelines, the n-D table, challenges, readouts) and the lazy scene islands. Everything three
 * lives in pythagoras.impl.tsx. Reader-facing words come from the `pt` dictionary namespace.
 */
lessonStore(LESSON, INITIAL);

export const PythagorasScene = localized(lazyIsland(() => import('./pythagoras.impl').then((m) => m.PythagorasScene)));

const tex = String.raw;
type Vals = Parameters<typeof derive2>[0];

/** `\s{k}` of a value that may be negative, parenthesised after a minus or as a base of a square: "(−3.0)²". */
const par = (k: string, v: number) => (v < 0 ? tex`(\s{${k}})` : tex`\s{${k}}`);
const sq = (k: string) => tex`(\s{${k}})^2`;
const row = (key: string, body: string): AlignedRow => [key, body];

/**
 * Hover maps: a square lights its side, the sum lights its parts. `slide` and `ab2` are virtual focus ids of the
 * dissection timeline (the pieces that move in step 3, the two squares of step 4).
 */
const HOT: HotLinkMap = {
	dx2: ['dx'], dy2: ['dy'], d2: ['d'], walk: ['dx', 'dy'],
	tris: ['tri1', 'tri2', 'tri3', 'tri4'], tri1: ['tris'], tri2: ['tris'], tri3: ['tris'], tri4: ['tris'],
	slide: ['tri2', 'tri4'], ab2: ['a2', 'b2'],
	lx2: ['lx'], ly2: ['ly'], lz2: ['lz'], f2: ['f'], s2: ['s'],
	xd2: ['xd'], r: ['r2'],
	...Object.fromEntries(Array.from({ length: MAX_N }, (_, i) => ['ns' + (i + 1), ['nd' + (i + 1)]])),
};

// ---------------------------------------------------------------------------------------------------------------
// Formulas.

const D1 = {
	px: (v: Vals) => derive2(v).P[0], py: (v: Vals) => derive2(v).P[1], qx: (v: Vals) => derive2(v).Q[0], qy: (v: Vals) => derive2(v).Q[1],
	dx: (v: Vals) => derive2(v).dx, dy: (v: Vals) => derive2(v).dy, dx2: (v: Vals) => derive2(v).dx2, dy2: (v: Vals) => derive2(v).dy2,
	d2: (v: Vals) => derive2(v).d2, d: (v: Vals) => derive2(v).d, walk: (v: Vals) => derive2(v).walk,
};
const F1 = { px: fmt1, py: fmt1, qx: fmt1, qy: fmt1, dx: fmt1, dy: fmt1, dx2: fmt2, dy2: fmt2, d2: fmt2, d: fmt2, walk: fmt1 };

function Formula1() {
	const locale = useLocale();
	const z = derive2(useVals(LESSON));
	const legX = tex`\t{dx}{dx}=\s{qx}-${par('px', z.P[0])}=\s{dx}`;
	const legY = tex`\t{dy}{dy}=\s{qy}-${par('py', z.P[1])}=\s{dy}`;
	const sum = tex`\t{d2}{d^2}=\t{dx2}{dx^2}+\t{dy2}{dy^2}`;
	const nums = tex`${sq('dx')}+${sq('dy')}=\s{dx2}+\s{dy2}=\t{d2}{\s{d2}}`;
	const root = tex`\t{d}{d}=\sqrt{\s{d2}}=\s{d}`;
	const walk = tex`\t{walk}{\lvert dx\rvert+\lvert dy\rvert}=\s{walk}`;
	return <Formula lesson={LESSON} block label={t(locale, 'pt.formula1')}
		tex={alignedRows([tex`${legX},\qquad ${legY}`, tex`${sum}=${nums}`, tex`${root},\qquad ${walk}`])}
		texNarrow={alignedRows([legX, legY, sum, tex`=${sq('dx')}+${sq('dy')}`, tex`=\s{dx2}+\s{dy2}=\t{d2}{\s{d2}}`, root, walk])}
		derived={D1} fmt={F1}
		tips={{ walk: { text: t(locale, 'pt.tipWalk') }, d2: { text: t(locale, 'pt.tipD2') } }} />;
}

const D2 = {
	big: (v: Vals) => deriveDis(v).big, tris: (v: Vals) => deriveDis(v).tris, c2: (v: Vals) => deriveDis(v).c2,
	a2: (v: Vals) => deriveDis(v).a2, b2: (v: Vals) => deriveDis(v).b2,
};
const F2 = { big: fmt2, tris: fmt2, c2: fmt2, a2: fmt2, b2: fmt2 };

/** Dissection lines revealed by the `dis` step: the count (step 1), the second count and the theorem (step 4). */
export function disMarks(step: number): Marks {
	return { 'dis-1': { hidden: step < 1 }, 'dis-2': { hidden: step < 4 }, 'dis-3': { hidden: step < 4 } };
}

function Formula2() {
	const locale = useLocale();
	const step = useStep(LESSON, 'dis');
	const marks = disMarks(step);
	const first = row('dis-0', tex`\t{big}{(a+b)^2}=\t{tris}{4\cdot\tfrac12 ab}+\t{c2}{c^2}`);
	const count = row('dis-1', tex`\s{big}=\s{tris}+\t{c2}{\s{c2}}`);
	const second = row('dis-2', tex`\t{big}{(a+b)^2}=\t{tris}{4\cdot\tfrac12 ab}+\t{a2}{a^2}+\t{b2}{b^2}`);
	const theorem = row('dis-3', tex`\t{a2}{a^2}+\t{b2}{b^2}=\t{c2}{c^2}:\quad\s{a2}+\s{b2}=\s{c2}`);
	return <Formula lesson={LESSON} block label={t(locale, 'pt.formula2')}
		tex={alignedRows([first, count, second, theorem], marks)}
		texNarrow={alignedRows([first, count, second, row('dis-3', tex`\t{a2}{a^2}+\t{b2}{b^2}=\t{c2}{c^2}`), row('dis-3', tex`\s{a2}+\s{b2}=\s{c2}`)], marks)}
		marks={marks} derived={D2} fmt={F2}
		tips={{ tris: { text: t(locale, 'pt.tipTris') }, big: { text: t(locale, 'pt.tipBig') } }} />;
}

const D3 = {
	dx: (v: Vals) => derive3(v).dx, dy: (v: Vals) => derive3(v).dy, dz: (v: Vals) => derive3(v).dz,
	f2: (v: Vals) => derive3(v).f2, s2: (v: Vals) => derive3(v).d2, s: (v: Vals) => derive3(v).d,
};
const F3 = { dx: fmt1, dy: fmt1, dz: fmt1, f2: fmt2, s2: fmt2, s: fmt2 };

export function roomMarks(step: number): Marks {
	return { 'room-1': { hidden: step < 1 }, 'room-2': { hidden: step < 2 } };
}

function Formula3() {
	const locale = useLocale();
	const step = useStep(LESSON, 'room');
	const marks = roomMarks(step);
	const floor = tex`\t{f2}{f^2}=\t{lx2}{dx^2}+\t{lz2}{dz^2}`, floorN = tex`${sq('dx')}+${sq('dz')}=\s{f2}`;
	const up = tex`\t{s2}{d^2}=\t{f2}{f^2}+\t{ly2}{dy^2}`, upN = tex`\s{f2}+${sq('dy')}=\s{s2}`;
	const all = tex`\t{s}{d}=\sqrt{dx^2+dy^2+dz^2}`, allN = tex`\sqrt{\s{s2}}=\s{s}`;
	return <Formula lesson={LESSON} block label={t(locale, 'pt.formula3')}
		tex={alignedRows([row('room-0', tex`${floor}=${floorN}`), row('room-1', tex`${up}=${upN}`), row('room-2', tex`${all}=${allN}`)], marks)}
		texNarrow={alignedRows([
			row('room-0', floor), row('room-0', tex`=${floorN}`), row('room-1', up), row('room-1', tex`=${upN}`), row('room-2', all), row('room-2', tex`=${allN}`),
		], marks)}
		marks={marks} derived={D3} fmt={F3} tips={{ f2: { text: t(locale, 'pt.tipF2') } }} />;
}

const D5 = {
	x: (v: Vals) => deriveR(v).x, y: (v: Vals) => deriveR(v).y, xd2: (v: Vals) => deriveR(v).d2, xd: (v: Vals) => deriveR(v).d,
	r: (v: Vals) => deriveR(v).r, r2: (v: Vals) => deriveR(v).r2,
};
const F5 = { x: fmt1, y: fmt1, xd2: fmt2, xd: fmt2, r: fmt2, r2: fmt2 };

function Formula5() {
	const locale = useLocale();
	const z = deriveR(useVals(LESSON));
	const cmp = z.side === 'in' ? '<' : z.side === 'on' ? '=' : '>';
	const d2 = tex`\t{xd2}{d^2}=x^2+y^2=${sq('x')}+${sq('y')}=\s{xd2}`, r2 = tex`\t{r2}{r^2}=\s{r2}`;
	const squares = tex`\s{xd2}\;${cmp}\;\s{r2}`, roots = tex`\t{xd}{d=\s{xd}}\;${cmp}\;\t{r}{r=\s{r}}`;
	return <Formula lesson={LESSON} block label={t(locale, 'pt.formula5')}
		tex={alignedRows([tex`${d2},\qquad ${r2}`, tex`${squares}\quad\Longleftrightarrow\quad ${roots}`])}
		texNarrow={alignedRows([d2, r2, squares, tex`\Longleftrightarrow\;${roots}`])}
		derived={D5} fmt={F5} tips={{ xd2: { text: t(locale, 'pt.tipD2_5') } }} />;
}

// ---------------------------------------------------------------------------------------------------------------
// Timelines (texts carry the live numbers).

const DIS_KEYS = ['pt.dis0', 'pt.dis1', 'pt.dis2', 'pt.dis3', 'pt.dis4'] as const;
const DIS_FOCUS = ['c2', 'tris', 'tri3', 'slide', 'ab2'];
const ROOM_KEYS = ['pt.room0', 'pt.room1', 'pt.room2'] as const;
const ROOM_FOCUS = ['f2', 's2', 's'];

function disSteps(locale: Locale, v: Vals) {
	const z = deriveDis(v);
	const nums = { big: fmt2(z.big), tris: fmt2(z.tris), c2: fmt2(z.c2), a2: fmt2(z.a2), b2: fmt2(z.b2) };
	if (DIS_KEYS.length !== DIS_STEPS) throw new Error('pythagoras: dissection step texts and DIS_STEPS disagree');
	return DIS_KEYS.map((key, i) => ({ id: String(i), text: t(locale, key, nums), focus: DIS_FOCUS[i] }));
}

function roomSteps(locale: Locale, v: Vals) {
	const z = derive3(v);
	const nums = { f2: fmt2(z.f2), s2: fmt2(z.d2), s: fmt2(z.d) };
	if (ROOM_KEYS.length !== ROOM_STEPS) throw new Error('pythagoras: room step texts and ROOM_STEPS disagree');
	return ROOM_KEYS.map((key, i) => ({ id: String(i), text: t(locale, key, nums), focus: ROOM_FOCUS[i] }));
}

// ---------------------------------------------------------------------------------------------------------------
// Readouts and challenges.

/** The live sentence under each scene (the canvas-free account of what it shows). */
export function readout(scene: Scene, locale: Locale, v: Vals): string {
	if (scene === 1) {
		const z = derive2(v);
		if (z.d === 0) return t(locale, 'pt.read1Zero');
		const base = t(locale, 'pt.read1', {
			P: fmt1(z.P[0]) + ', ' + fmt1(z.P[1]), Q: fmt1(z.Q[0]) + ', ' + fmt1(z.Q[1]),
			dx: fmt1(z.dx), dy: fmt1(z.dy), dx2: fmt2(z.dx2), dy2: fmt2(z.dy2), d2: fmt2(z.d2), d: fmt2(z.d), walk: fmt1(z.walk),
		});
		return base + (z.dx === 0 || z.dy === 0 ? t(locale, 'pt.read1Line') : z.dx < 0 || z.dy < 0 ? t(locale, 'pt.read1Negative') : '');
	}
	if (scene === 2) {
		const z = deriveDis(v);
		return t(locale, 'pt.read2', { a: fmt1(z.a), b: fmt1(z.b), big: fmt2(z.big), tris: fmt2(z.tris), c2: fmt2(z.c2), a2: fmt2(z.a2), b2: fmt2(z.b2), c: fmt2(z.c) });
	}
	if (scene === 3) {
		const z = derive3(v);
		if (z.d === 0) return t(locale, 'pt.read3Zero');
		return t(locale, 'pt.read3', { dx2: fmt2(z.dx2), dz2: fmt2(z.dz2), f2: fmt2(z.f2), dy2: fmt2(z.dy2), s2: fmt2(z.d2), s: fmt2(z.d) });
	}
	const z = deriveR(v);
	return t(locale, 'pt.read5', { x: fmt1(z.x), y: fmt1(z.y), d2: fmt2(z.d2), r2: fmt2(z.r2), d: fmt2(z.d), r: fmt2(z.r) })
		+ t(locale, z.side === 'in' ? 'pt.read5In' : z.side === 'on' ? 'pt.read5On' : 'pt.read5Out');
}

function challengesFor(scene: Scene | 4, locale: Locale): Omit<ChallengeProps, 'lesson'>[] {
	if (scene === 1) {
		return [
			{ n: 1, text: t(locale, 'pt.ch1'), check: CHECKS.fiveElsewhere, hint: [t(locale, 'pt.ch1Hint')] },
			{ n: 2, text: t(locale, 'pt.ch2'), check: CHECKS.walkEqual, hint: [t(locale, 'pt.ch2Hint')], success: t(locale, 'pt.ch2Success') },
		];
	}
	if (scene === 2) {
		return [
			{ n: 3, text: t(locale, 'pt.ch3'), check: CHECKS.slid },
			{ n: 4, text: t(locale, 'pt.ch4'), check: CHECKS.wholeC, hint: [t(locale, 'pt.ch4Hint')] },
		];
	}
	if (scene === 3) {
		return [
			{ n: 5, text: t(locale, 'pt.ch5'), check: CHECKS.above, hint: [t(locale, 'pt.ch5Hint')], success: t(locale, 'pt.ch5Success') },
			{ n: 6, text: t(locale, 'pt.ch6'), check: CHECKS.threeElsewhere, hint: [t(locale, 'pt.ch6Hint')] },
		];
	}
	if (scene === 4) return [{ n: 7, text: t(locale, 'pt.ch7'), check: CHECKS.eightFive, hint: [t(locale, 'pt.ch7Hint')] }];
	return [{ n: 8, text: t(locale, 'pt.ch8'), check: CHECKS.onCircle, hint: [t(locale, 'pt.ch8Hint')] }];
}

const CODE = [
	'const dx = x - cx, dy = y - cy;',
	'const d2 = dx * dx + dy * dy;   // no square root',
	'const inside = d2 < r * r;      // same answer as Math.hypot(dx, dy) < r',
].join('\n');

function Controls({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const v = useVals(LESSON);
	return <section className="not-content" data-pt-controls={scene}>
		{/* Hover links are lesson-wide; they mount once, with the lead scene's controls. */}
		{scene === 1 && <HotLinks lesson={LESSON} links={HOT} />}
		{scene === 2 && <Timeline lesson={LESSON} id="dis" steps={disSteps(locale, v)} />}
		{scene === 3 && <Timeline lesson={LESSON} id="room" steps={roomSteps(locale, v)} />}
		{scene === 1 ? <Formula1 /> : scene === 2 ? <Formula2 /> : scene === 3 ? <Formula3 /> : <Formula5 />}
		<p aria-live="polite">{readout(scene, locale, v)}</p>
		{scene === 5 && <CodeView lesson={LESSON} lang="ts" code={CODE} lines={{ 2: 'xd2', 3: 'r2' }} />}
		{challengesFor(scene, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const PythagorasControls = localized(Controls);

// ---------------------------------------------------------------------------------------------------------------
// 0.3-4: n dimensions, page-side (no picture: the point of the scene is that none is needed).

/** The editable coordinates of one point: a vector literal of scrubbable numbers, `n` of them. */
function PointRow({ name, keyOf, n }: { name: string; keyOf: (i: number) => string; n: number }) {
	const locale = useLocale();
	const idx = Array.from({ length: n }, (_, i) => i + 1);
	return <p className="lab-pt-point">
		<span>{name + ' = ('}</span>
		{idx.map((i) => <span key={i}>
			<Num lesson={LESSON} k={keyOf(i)} spec={SPEC.comp} fmt={fmtCell} label={t(locale, 'pt.ndComp', { i: String(i), name })} />
			{i < n ? ', ' : ''}
		</span>)}
		<span>)</span>
	</p>;
}

function FormulaN() {
	const locale = useLocale();
	const z = deriveN(useVals(LESSON));
	const terms = z.squares.map((_, i) => tex`\t{ns${i + 1}}{\s{ns${i + 1}}}`);
	const derived: Record<string, (v: Vals) => number> = {
		nsum: (v) => deriveN(v).d2, nroot: (v) => deriveN(v).d,
		...Object.fromEntries(z.squares.map((_, i) => ['ns' + (i + 1), (v: Vals) => deriveN(v).squares[i] ?? 0])),
	};
	const fmt = Object.fromEntries(Object.keys(derived).map((k) => [k, fmtCell]));
	// Written out, not with Σ (the notation comes in lesson 1.3): the first and the last difference.
	const last = String(z.n);
	const head = z.n === 1 ? tex`d^2=(q_1-p_1)^2` : tex`d^2=(q_1-p_1)^2+${z.n > 2 ? tex`\cdots+` : ''}(q_${last}-p_${last})^2`;
	const sum = (part: string[]) => part.join('+');
	const total = tex`=\t{nsum}{\s{nsum}}`, root = tex`d=\sqrt{\s{nsum}}=\t{nroot}{\s{nroot}}`;
	// Narrow: at most four terms per line.
	const lines = [terms.slice(0, 4), terms.slice(4)].filter((p) => p.length);
	return <Formula lesson={LESSON} block label={t(locale, 'pt.formula4')}
		tex={alignedRows([head, tex`=${sum(terms)}${total}`, root])}
		texNarrow={alignedRows([head, ...lines.map((p, i) => (i === 0 ? '=' : '+') + sum(p)), total, root])}
		derived={derived} fmt={fmt} />;
}

function NdScene() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const z = deriveN(v);
	const store = lessonStore(LESSON);
	const setN = (n: number) => store.getState().set('n', Math.min(MAX_N, Math.max(1, n)));
	const cell = (id: string, value: number) => ({ id, value });
	const rows: GridRow[] = z.legs.map((d, k) => {
		const i = k + 1;
		return { head: String(i), cells: { p: cell('np' + i, z.p[k]), q: cell('nq' + i, z.q[k]), d: cell('nd' + i, d), sq: cell('ns' + i, z.squares[k]) } };
	});
	rows.push({ head: 'd²', cells: { sq: cell('nsum', z.d2) } });
	rows.push({ head: 'd', cells: { sq: cell('nroot', z.d) } });
	return <section className="not-content lab-pt-nd" data-pt-nd="">
		<p className="lab-pt-dims">
			<span>{t(locale, 'pt.ndDims')}</span>{' '}
			<button type="button" aria-label={t(locale, 'pt.ndLess')} aria-disabled={z.n <= 1} onClick={() => setN(z.n - 1)}>−</button>{' '}
			<Num lesson={LESSON} k="n" spec={SPEC.n} fmt={(x) => String(x)} label={t(locale, 'pt.ndDims')} />{' '}
			<button type="button" aria-label={t(locale, 'pt.ndMore')} aria-disabled={z.n >= MAX_N} onClick={() => setN(z.n + 1)}>+</button>
		</p>
		<PointRow name="P" keyOf={pKey} n={z.n} />
		<PointRow name="Q" keyOf={qKey} n={z.n} />
		<NumGrid lesson={LESSON} caption={t(locale, 'pt.ndCaption')} corner="i" rows={rows} fmt={fmtCell}
			cols={[{ id: 'p', label: 'pᵢ' }, { id: 'q', label: 'qᵢ' }, { id: 'd', label: 'qᵢ − pᵢ' }, { id: 'sq', label: '(qᵢ − pᵢ)²' }]} />
		<FormulaN />
		<p aria-live="polite">{t(locale, 'pt.readN', {
			n: String(z.n), legs: z.legs.map(fmtCell).join(', '), squares: z.squares.map(fmtCell).join(', '), d2: fmtCell(z.d2), d: fmt2(z.d),
		})}</p>
		{challengesFor(4, locale).map((c) => <Challenge key={c.n} lesson={LESSON} {...c} />)}
	</section>;
}
export const PythagorasND = localized(NdScene);
