/**
 * Einsum cards (docs/design/intro-and-dotunit.md §3, §4): the same procedural renderer for every preset.
 * - `PlaygroundCard` (INTRO-3, spec §3.5): pick a preset, edit its extents and seed; the layout re-forms, the `pass`
 *   timeline runs the shafts, a click on a cell dives into its dot product (the DotUnit face 2 of that row and column).
 * - `MatvecCard` (row view, §3.1) and `ColumnCard` (column view, §3.2) share one store: the same `explain` floats, so
 *   both views end on the same y.
 * - `LadderCard` (INTRO-2, §4): five rungs from one tile to attention cubes, dive down by clicking, climb back up.
 * Loaded lazily by the entry (einsum.tsx).
 */
import { useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import type { V } from '../geom';
import { lessonStore, useStep, useVals } from '../store';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Arrow } from '../scene/Arrow';
import { Point } from '../scene/Point';
import { Label } from '../scene/Label';
import { Show, useOverlay, useStage } from '../scene/handles';
import type { Hint, StatusSpec } from '../scene/StatusBar';
import { Boxes } from './Boxes';
import { cellTop, dotCap, dotFrame, dotGeom, dotIds, Frame, passCap, passFrame, passIds, passMs, type DotGeom } from './choreo';
import { diveVectors, ladderData, LADDER, MATVEC, mvData, PLAY, playData, presetOf } from './einsum.store';
import { mvRows, playRows } from './einsum.rows';
import { columnView, dotData, fmt, fmtShort, ids, type SceneData } from './model';
import { Tag, useClock } from './scene';

const FRONT = [0, 0, 1] as const;
/**
 * Default camera of a cube (spec §3.3 "three-quarter"): from the front-LEFT-top, so all three faces that carry numbers
 * face the viewer (A on the left face, B on top, C in front); the engine's front-right home sees A from behind.
 */
const CUBE = [-1.1, 1, 1.7] as const;

/**
 * Re-frames the home view one frame after the drawn figure changed (Boxes updates its bounds proxy in the frame after
 * the data change), from the front or the three-quarter home direction.
 */
function useRefit(dir: readonly [number, number, number]) {
	const reg = useStage();
	const due = useRef(true);
	const view = useRef(dir);
	view.current = dir;
	return {
		request: () => void (due.current = true),
		pending: () => due.current,
		/** Call once per update; true while a refit is pending (the scene asks for one more frame). */
		run: (): boolean => {
			if (!due.current) return false;
			due.current = false;
			reg.nav?.frame(view.current);
			return true;
		},
	};
}

// ---------------------------------------------------------------------------------------------------------------
// The generic renderer.

interface ViewProps {
	lesson: string;
	d: SceneData;
	/** Timeline driving the pass (operands, one shaft, all shafts, stacks, result); without it the result is shown. */
	timeline?: string;
	focus: number;
	onCell?(c: number): void;
	/** Extra text after each cell value (the layer's bias), by cell. */
	cellText?(c: number): string;
	offset?: V;
	front?: boolean;
	/** Changes when the scene should be re-framed (preset, extents, rung). */
	frameKey: string;
}

/** Labels of one scene: operand values on their faces and the cell values on the stacks (when few enough to read). */
function PassTags({ lesson, d, show, cellText, offset = [0, 0, 0] }: { lesson: string; d: SceneData; show: () => { step: number }; cellText?(c: number): string; offset?: V }) {
	const labels = useOverlay('labels');
	if (!labels) return null;
	const L = d.L, ops = L.operands[0].n + L.operands[1].n;
	const at = (p: ArrayLike<number>, i: number): number[] => [p[3 * i] + offset[0], p[3 * i + 1] + offset[1], p[3 * i + 2] + offset[2]];
	const P = [0, 0, 0];
	return <>
		{ops <= 72 && ([0, 1] as const).flatMap((o) => Array.from({ length: L.operands[o].n }, (_, e) => (
			<Tag key={o + ':' + e} lesson={lesson} hot={ids.elem(d, o, e)} small={ops > 24} dense role={o === 0 ? 'vecA' : 'vecB'} text={fmtShort(d.vals[o][e])} at={() => at(L.operands[o].pos, e)} />
		)))}
		{L.cells.n <= 64 && Array.from({ length: L.cells.n }, (_, c) => (
			<Tag key={'c' + c} lesson={lesson} hot={ids.cell(d, c)} role="ink" offset={[0, -10]} dense={cellText !== undefined} text={fmtShort(d.out[c]) + (cellText?.(c) ?? '')}
				at={() => (show().step >= 3 ? at(cellTop(d, c, P), 0) : null)} />
		))}
	</>;
}

const FIXED = { step: 4, t: 1 };

function EinsumView({ lesson, d, timeline, focus, onCell, cellText, offset, front = false, frameKey }: ViewProps) {
	const frame = useMemo(() => {
		const f = new Frame(passCap(d));
		passIds(d, { elem: (o, e) => ids.elem(d, o, e), cell: (c) => ids.cell(d, c), tile: (c, k) => ids.tile(d, c, k) }, f);
		return f;
	}, [d]);
	const { clock, tick } = useClock(lesson, timeline ?? '', (s) => (timeline ? passMs(s, d.L.plan.sumLen) : 0));
	const planar = d.L.axes.onAxis[2] === null && d.L.axes.shaft !== 2;
	const refit = useRefit(front || planar ? FRONT : CUBE);
	const dirty = useRef(true);
	const seen = useRef({ d, focus, frameKey });
	if (seen.current.d !== d || seen.current.focus !== focus) dirty.current = true;
	if (seen.current.frameKey !== frameKey) refit.request();
	seen.current = { d, focus, frameKey };
	const invalidate = useThree((st) => st.invalidate);
	useEffect(() => invalidate(), [d, focus, frameKey, invalidate]);
	const at = () => (timeline ? clock : FIXED);
	// The refit runs one update AFTER the frame was written: Boxes moves its bounds proxy right after an update.
	const update = () => {
		const changed = timeline ? tick() : false;
		if (changed || dirty.current) {
			dirty.current = false;
			const c = at();
			passFrame(d, c.step, c.t, focus, frame);
			return c.t < 1 || refit.pending() ? ('moving' as const) : ('moved' as const);
		}
		return refit.run() ? ('moved' as const) : ('still' as const);
	};
	const ops = d.L.operands[0].n + d.L.operands[1].n;
	const pick = (_: string, i: number) => {
		if (!onCell || i < ops) return;
		const c = d.L.lod === 1 ? Math.floor((i - ops) / d.L.plan.sumLen) : i - ops;
		if (c < d.L.cells.n) onCell(c);
	};
	return <group position={offset ? [offset[0], offset[1], offset[2] ?? 0] : undefined}>
		<Boxes lesson={lesson} frame={frame} update={update} onPick={pick} />
		<PassTags lesson={lesson} d={d} show={at} cellText={cellText} offset={offset} />
	</group>;
}

/** A dot product drawn as the DotUnit's face 2 at a fixed step (the dive target, ladder rungs 1 and 2). */
function DotStatic({ lesson, a, b, step, onTile, frameKey }: { lesson: string; a: readonly number[]; b: readonly number[]; step: number; onTile?(i: number): void; frameKey: string }) {
	const g: DotGeom = useMemo(() => dotGeom(dotData(a, b)), [a, b]);
	const n = a.length;
	const frame = useMemo(() => {
		const f = new Frame(dotCap(n));
		dotIds(n, f);
		return f;
	}, [n]);
	const refit = useRefit(FRONT);
	const dirty = useRef(true);
	const seen = useRef({ g, step, frameKey });
	if (seen.current.g !== g || seen.current.step !== step) dirty.current = true;
	if (seen.current.frameKey !== frameKey) refit.request();
	seen.current = { g, step, frameKey };
	const invalidate = useThree((st) => st.invalidate);
	useEffect(() => invalidate(), [g, step, frameKey, invalidate]);
	const update = () => {
		if (dirty.current) {
			dirty.current = false;
			dotFrame(g, step, 1, frame);
			return refit.pending() ? ('moving' as const) : ('moved' as const);
		}
		return refit.run() ? ('moved' as const) : ('still' as const);
	};
	const labels = useOverlay('labels');
	const centre = (k: number): number[] | null => (frame.alpha[k] > 0.5 ? [frame.c[3 * k], frame.c[3 * k + 1], frame.c[3 * k + 2]] : null);
	return <>
		<Boxes lesson={lesson} frame={frame} update={update} onPick={(_, i) => onTile && i >= 2 * n && i < 3 * n && onTile(i - 2 * n)} />
		{labels && <>
			{g.d.a.map((x, i) => <Tag key={'a' + i} lesson={lesson} hot={'a' + i} small role="ink" text={fmtShort(x)} offset={[0, 14]} at={() => centre(i)} />)}
			{g.d.b.map((x, i) => <Tag key={'b' + i} lesson={lesson} hot={'b' + i} small role="ink" text={fmtShort(x)} offset={[-16, 0]} at={() => centre(n + i)} />)}
			{g.d.prods.map((p, i) => <Tag key={'p' + i} lesson={lesson} hot={'p' + i} role="ink" text={fmtShort(p)} at={() => centre(2 * n + i)} />)}
			{step >= 7 && <Tag lesson={lesson} hot="sum" role="ink" text={'Σ = ' + fmt(g.d.dot)} offset={[-44, -10]} at={() => [g.stackX + 0.5, g.run[n - 1], 0]} />}
			{step >= 8 && <Tag lesson={lesson} hot="proj" role="vecC" text={'|a|·s = ' + fmt(g.proj)} offset={[0, -24]} at={() => [g.barX + 0.3, g.proj, 0]} />}
		</>}
	</>;
}

function Floor({ lesson, extent }: { lesson: string; extent: number }) {
	const axes = useOverlay('axes');
	return <Show ov="grid"><Grid lesson={lesson} extent={extent} step={1} axes={axes} /></Show>;
}

const overlays = (locale: Locale) => [
	{ id: 'grid', label: t(locale, 'du.ovGrid'), on: true },
	{ id: 'axes', label: t(locale, 'du.ovAxes'), on: false },
	{ id: 'labels', label: t(locale, 'du.ovLabels'), on: true },
];

/** Hints for any tile / cell id (they are generated from the spec, so the lookup is by prefix). */
function pickHints(locale: Locale): Readonly<Record<string, Hint>> {
	const dive: Hint = { mouse: t(locale, 'du.hintDive'), touch: t(locale, 'du.hintDiveTouch') };
	return new Proxy({} as Record<string, Hint>, { get: (_, id) => (typeof id === 'string' && /^[pc]\d/.test(id) ? dive : undefined) });
}

// ---------------------------------------------------------------------------------------------------------------
// Playground.

function Playground() {
	const locale = useLocale(), v = useVals(PLAY), b = playData(v);
	const dive = (v.dive as number) === 1, sel = Math.round(v.sel as number);
	const id = presetOf(v);
	if (!b.ok) return <Label lesson={PLAY} id="err" at={[0, 0, 0]} role="hot">{t(locale, 'du.tooLarge')}</Label>;
	const c = Math.min(b.d.L.cells.n - 1, Math.max(0, sel));
	if (dive) {
		const { a, b: bb } = diveVectors(b.d, c);
		return <DotStatic lesson={PLAY} a={a} b={bb} step={9} frameKey={'dive:' + JSON.stringify(b.d.L.plan.dims) + c} />;
	}
	return <EinsumView lesson={PLAY} d={b.d} timeline="pass" focus={c} frameKey={id + JSON.stringify(b.d.L.plan.dims)}
		onCell={(cell) => lessonStore(PLAY).getState().setMany({ sel: cell, dive: 1 })} />;
}

function playStatus(locale: Locale): StatusSpec {
	return {
		hints: pickHints(locale),
		state: (v) => (playData(v).ok ? null : t(locale, 'du.tooLarge')),
		idle: (v) => {
			const b = playData(v);
			if (!b.ok) return null;
			const s = b.d.L.plan.summed;
			return (v.dive as number) === 1 ? t(locale, 'du.diveIdle') : s.length ? t(locale, 'du.summed', { letters: s.join(', ') }) : t(locale, 'du.outer');
		},
	};
}

export function PlaygroundCard({ card = 'INTRO-3' }: { card?: string }) {
	const locale = useLocale();
	const status = useMemo(() => playStatus(locale), [locale]);
	// The camera's distance range scales with `extent`: a 64-wide heatmap needs a far camera, a dot product a near one.
	const b = playData(useVals(PLAY));
	const extent = b.ok ? Math.max(5, 0.5 * Math.hypot(b.d.L.hi[0] - b.d.L.lo[0], b.d.L.hi[1] - b.d.L.lo[1], b.d.L.hi[2] - b.d.L.lo[2])) : 5;
	return <ViewportCard lesson={PLAY} id={card} name={card + ' · ' + t(locale, 'du.namePlay')} mode="3d" extent={extent}
		rows={playRows(locale)} status={status} overlays={overlays(locale)}>
		<Floor lesson={PLAY} extent={8} />
		<Playground />
	</ViewportCard>;
}

// ---------------------------------------------------------------------------------------------------------------
// Matrix x vector: row view and column view.

function MatvecRows() {
	const b = mvData(useVals(MATVEC));
	if (!b.ok) return null;
	return <EinsumView lesson={MATVEC} d={b.d} timeline="pass" focus={0} front frameKey={JSON.stringify(b.d.L.plan.dims)} />;
}

export function MatvecCard({ card = 'MV-1' }: { card?: string }) {
	const locale = useLocale();
	const status = useMemo<StatusSpec>(() => ({ idle: t(locale, 'du.mvIdle') }), [locale]);
	return <ViewportCard lesson={MATVEC} id={card} name={card + ' · ' + t(locale, 'du.nameRows')} mode="3d" extent={4}
		rows={mvRows(locale)} status={status} overlays={overlays(locale)}>
		<Floor lesson={MATVEC} extent={6} />
		<MatvecRows />
	</ViewportCard>;
}

/** Arrow ends of column j at a `cv` step (spec §3.2): the column, scaled by xⱼ, then head to tail. */
export function columnArrows(d: SceneData, step: number): { from: number[]; to: number[] }[] {
	const cv = columnView(d.L.plan, d.ts);
	const J = d.L.plan.dims.j, W = d.ts[0];
	return Array.from({ length: J }, (_, j) => {
		if (step <= 0) return { from: [0, 0], to: [W.get([0, j]), W.get([1, j])] };
		if (step === 1) return { from: [0, 0], to: cv.cols[j] };
		return { from: j ? cv.heads[j - 1] : [0, 0], to: cv.heads[j] };
	});
}

function Columns() {
	const v = useVals(MATVEC), b = mvData(v), step = useStep(MATVEC, 'cv'), locale = useLocale();
	// Steps move the arrows through the store with an eased write (they glide; reduced motion cuts, engine rule).
	const d = b.ok ? b.d : null;
	const last = useRef<{ d: SceneData | null; step: number } | null>(null);
	useEffect(() => {
		if (!d) return;
		const how = last.current && last.current.d === d ? 'ease' : 'cut';
		last.current = { d, step };
		const patch: Record<string, number[]> = {};
		const keys: string[] = [];
		columnArrows(d, step).forEach((e, j) => {
			patch['cv_f' + j] = e.from;
			patch['cv_t' + j] = e.to;
			keys.push('cv_f' + j, 'cv_t' + j);
		});
		lessonStore(MATVEC).getState().setMany(patch, 'cut', how === 'ease' ? keys : []);
	}, [d, step]);
	if (!d) return null;
	const x = d.ts[1], J = d.L.plan.dims.j, y: V = [d.out[0], d.out[1]];
	return <>
		{Array.from({ length: J }, (_, j) => <Arrow key={j} lesson={MATVEC} id={'col' + j} from={'cv_f' + j} to={'cv_t' + j} role="vecC" drag="none" />)}
		<Show ov="labels">{Array.from({ length: J }, (_, j) => (
			<Label key={j} lesson={MATVEC} id={'col' + j} at={'cv_t' + j} role="vecC" offset={[10, -10]}>{(step >= 1 ? fmtShort(x.get([j])) + '·' : '') + 'w' + (j + 1)}</Label>
		))}</Show>
		{step >= 3 && <>
			<Arrow lesson={MATVEC} id="ysum" to={y} role="vecA" drag="none" />
			<Point lesson={MATVEC} id="c0" at={y} drag={false} role="vecA" size={9} />
			<Label lesson={MATVEC} id="ysum" at={y} role="vecA" offset={[12, 16]}>{t(locale, 'du.ySame', { y: '(' + fmtShort(y[0]) + ', ' + fmtShort(y[1]) + ')' })}</Label>
		</>}
	</>;
}

export function ColumnCard({ card = 'MV-2' }: { card?: string }) {
	const locale = useLocale();
	const status = useMemo<StatusSpec>(() => ({ idle: t(locale, 'du.cvIdle') }), [locale]);
	return <ViewportCard lesson={MATVEC} id={card} name={card + ' · ' + t(locale, 'du.nameCols')} mode="2d" extent={5}
		rows={mvRows(locale)} status={status} overlays={overlays(locale)}>
		<Show ov="grid"><Grid lesson={MATVEC} extent={8} step={1} axes ticks /></Show>
		<Columns />
	</ViewportCard>;
}

// ---------------------------------------------------------------------------------------------------------------
// The zoom ladder.

function Rungs() {
	const step = useStep(LADDER, 'ladder'), v = useVals(LADDER), locale = useLocale();
	const L = ladderData();
	const a = v.lad_a as readonly number[], b = v.lad_b as readonly number[], k = Math.round(v.lad_tile as number);
	const st = lessonStore(LADDER).getState();
	const diveTo = (d: SceneData) => (c: number) => {
		const p = diveVectors(d, c);
		st.setMany({ lad_a: p.a, lad_b: p.b, lad_tile: 0 });
		st.setStep('ladder', 1);
	};
	const key = 'rung' + step;
	if (step === 0) return <DotStatic lesson={LADDER} a={[a[k] ?? a[0]]} b={[b[k] ?? b[0]]} step={6} frameKey={key} />;
	if (step === 1) return <DotStatic lesson={LADDER} a={a} b={b} step={9} frameKey={key} onTile={(i) => { st.set('lad_tile', i); st.setStep('ladder', 0); }} />;
	const bias = (c: number) => {
		const o = L.layer.L.cells.idx[2 * c + 1];
		const bv = L.bias.get([o]);
		return (bv < 0 ? ' - ' : ' + ') + fmtShort(Math.abs(bv)) + ' = ' + fmtShort(L.biased.data[L.layer.L.cells.flat[c]]);
	};
	if (step === 2) return <EinsumView lesson={LADDER} d={L.matmul} focus={0} frameKey={key} onCell={diveTo(L.matmul)} />;
	if (step === 3) return <EinsumView lesson={LADDER} d={L.layer} focus={0} frameKey={key} onCell={diveTo(L.layer)} cellText={bias} />;
	const gap = L.layer.L.hi[0] - L.attention.L.lo[0] + 3;
	return <>
		<EinsumView lesson={LADDER} d={L.layer} focus={0} frameKey={key} onCell={diveTo(L.layer)} cellText={bias} />
		<EinsumView lesson={LADDER} d={L.attention} focus={0} frameKey={key + 'b'} offset={[gap, 0, 0]} onCell={diveTo(L.attention)} />
		<Show ov="labels"><Label lesson={LADDER} id="att" at={[gap + 2, L.attention.L.hi[1] + 1, 0]} role="muted">{t(locale, 'du.attLabel')}</Label></Show>
	</>;
}

export function LadderCard({ card = 'INTRO-2' }: { card?: string }) {
	const locale = useLocale();
	const status = useMemo<StatusSpec>(() => ({ hints: pickHints(locale), idle: t(locale, 'du.ladderIdle') }), [locale]);
	return <ViewportCard lesson={LADDER} id={card} name={card + ' · ' + t(locale, 'du.nameLadder')} mode="3d" extent={5}
		status={status} overlays={overlays(locale)}>
		<Floor lesson={LADDER} extent={10} />
		<Rungs />
	</ViewportCard>;
}
