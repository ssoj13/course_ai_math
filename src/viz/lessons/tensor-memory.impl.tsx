/**
 * Lesson 3.3 scenes in the 6d Blender look. 3.3-1: the view y on top (its cells in its own row-major order, each
 * printing the number it reads), the buffer it reads below (and the new buffer when a copy was made), with the selected
 * cell's address drawn as a line down into memory. 3.3-2: a 16 x 16 float32 matrix walked by rows or by columns
 * through a small cache, the cache's lines drawn beside it. Cells are ONE instanced draw per card
 * (tensor-cells.impl.tsx). Loaded lazily by the entry (tensor-memory.tsx).
 */
import { useMemo } from 'react';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { lessonStore, useVals } from '../store';
import { ViewportCard } from '../scene/ViewportCard';
import { Segment } from '../scene/Segment';
import { Label } from '../scene/Label';
import { useOverlay } from '../scene/handles';
import type { Hint, StatusSpec } from '../scene/StatusBar';
import type { Frame } from '../dotunit/choreo';
import { Tag } from '../dotunit/scene';
import { Cells } from './tensor-cells.impl';
import { CELL, flatDirs, layout, paintCell } from './tensor-cells';
import { cacheRows, viewRows } from './tensor-memory.rows';
import { BUF, cacheModel, chain, LESSON, N, selected, walkOf, type Chain } from './tensor-memory.math';

type Vals = Parameters<typeof chain>[0];
const FRONT: readonly [number, number, number] = [0.03, 0.05, 1.6];

// ---------------------------------------------------------------------------------------------------------------
// 3.3-1: the view and its buffer.

/** Rows below the view where x's buffer and the copy's buffer are drawn. */
function rowsOf(c: Chain): { yView: number; yX: number; yCopy: number } {
	const L = layout(c.res.shape, flatDirs(c.res.ndim));
	const yX = -L.max[1] - 2.5;
	return { yView: 0, yX, yCopy: yX - 2 };
}

function ViewCells({ locale }: { locale: Locale }) {
	const v = useVals(LESSON);
	const c = chain(v);
	const labels = useOverlay('labels');
	const L = layout(c.res.shape, flatDirs(c.res.ndim));
	const walk = walkOf(c.res);
	const onX = walkOf(c.onX);
	const sel = selected(c.res, v.sel as number);
	const { yX, yCopy } = rowsOf(c);
	const resOnX = !c.copied;
	const paint = (f: Frame) => {
		let i = 0;
		for (let k = 0; k < c.res.size; k++, i++) {
			const val = c.res.data[walk.pos[k]];
			paintCell(f, i, [L.pos[3 * k], L.pos[3 * k + 1], 0], 'cells', val, 1, { role: 'vecA' });
			f.id[i] = 'a' + val;
			f.flash[i] = k === sel.k ? 1 : 0;
		}
		for (let p = 0; p < BUF; p++, i++) {
			const reached = onX.step[p] >= 0;
			paintCell(f, i, [p, yX, 0], 'cells', p, reached ? 1 : 0.35, { role: reached ? 'ink' : 'muted' });
			f.id[i] = 'a' + p;
			f.flash[i] = resOnX && p === sel.addr ? 1 : 0;
		}
		if (c.copied) {
			for (let q = 0; q < c.res.size; q++, i++) {
				const val = c.res.data[q];
				paintCell(f, i, [q, yCopy, 0], 'cells', val, 1, { role: 'vecC' });
				f.id[i] = 'a' + val;
				f.flash[i] = q === sel.addr ? 1 : 0;
			}
		}
		f.n = i;
	};
	// Only cells of the view select (the first boxes of the frame); buffer cells are read by hover.
	const pick = (_: string, index: number) => {
		if (index < c.res.size) lessonStore(LESSON).getState().set('sel', index);
	};
	const key = JSON.stringify([c.res.shape, c.res.strides, c.res.offset, c.copied, c.onX.strides, c.onX.offset, sel.k]);
	const top = [L.pos[3 * sel.k], L.pos[3 * sel.k + 1], 0];
	const mem: [number, number, number] = [sel.addr, resOnX ? yX : yCopy, 0];
	return <>
		<Cells lesson={LESSON} cap={3 * BUF} paint={paint} paintKey={key} frameKey={JSON.stringify([c.res.shape, c.copied])} home={FRONT} pad={1.4}
			onPick={pick} />
		<Segment lesson={LESSON} id="ptr" a={[top[0], top[1] - CELL / 2, 0]} b={[mem[0], mem[1] + CELL / 2, 0]} role="hot" dashed />
		{labels && <>
			{Array.from({ length: c.res.size }, (_, k) => (
				<Tag key={'y' + k} lesson={LESSON} small dense role="ink" hot={'a' + c.res.data[walk.pos[k]]} text={String(c.res.data[walk.pos[k]])}
					at={() => [L.pos[3 * k], L.pos[3 * k + 1], 0]} />
			))}
			{Array.from({ length: BUF }, (_, p) => (
				<Tag key={'x' + p} lesson={LESSON} small dense role={onX.step[p] >= 0 ? 'ink' : 'muted'} hot={'a' + p} text={String(p)} at={() => [p, yX, 0]} />
			))}
			{c.copied && Array.from({ length: c.res.size }, (_, q) => (
				<Tag key={'c' + q} lesson={LESSON} small dense role="ink" hot={'a' + c.res.data[q]} text={String(c.res.data[q])} at={() => [q, yCopy, 0]} />
			))}
			<Label lesson={LESSON} id="capY" at={[0, 0.6, 0]} role="vecA" away={[0, 0.55, 0]} gap={4}>y</Label>
			<Label lesson={LESSON} id="capX" at={[1.5, yX + 0.6, 0]} role="ink" away={[1.5, yX + 0.55, 0]} gap={4}>{t(locale, 'mem.capX')}</Label>
			{c.copied && <Label lesson={LESSON} id="capCopy" at={[1.5, yCopy + 0.6, 0]} role="vecC" away={[1.5, yCopy + 0.55, 0]} gap={4}>{t(locale, 'mem.capCopy')}</Label>}
		</>}
	</>;
}

function viewStatus(locale: Locale): StatusSpec {
	const hints = new Proxy({} as Record<string, Hint>, {
		get: (_, id) => {
			const r = typeof id === 'string' ? /^a(\d+)$/.exec(id) : null;
			if (!r) return undefined;
			const val = +r[1];
			return (vals: Vals) => {
				const c = chain(vals);
				const w = walkOf(c.onX);
				const k = w.step[val];
				return k >= 0 ? t(locale, 'mem.hintReached', { p: val, k }) : t(locale, 'mem.hintSkipped', { p: val });
			};
		},
	});
	return { hints, idle: { mouse: t(locale, 'mem.idleView'), touch: t(locale, 'mem.idleViewTouch') } };
}

export function ViewScene() {
	const locale = useLocale();
	const rows = useMemo(() => viewRows(locale), [locale]);
	const status = useMemo(() => viewStatus(locale), [locale]);
	return <ViewportCard lesson={LESSON} id="3.3-1" name={'3.3-1 · ' + t(locale, 'mem.nameView')} mode="2d" extent={14} panelOpen={false} rows={rows} status={status}
		overlays={[{ id: 'labels', label: t(locale, 'mem.ovLabels'), on: true }]}>
		<ViewCells locale={locale} />
	</ViewportCard>;
}

// ---------------------------------------------------------------------------------------------------------------
// 3.3-2: cache lines.

/** Column where the cache's slots stand, right of the matrix. */
const X_CACHE = N + 2.5;

function CacheCells({ locale }: { locale: Locale }) {
	const m = cacheModel(useVals(LESSON));
	const labels = useOverlay('labels');
	const held = new Set(m.held);
	const paint = (f: Frame) => {
		let i = 0;
		for (let r = 0; r < N; r++) for (let c = 0; c < N; c++, i++) {
			// Walk step that reads (r, c): the cell is visited once that step has run.
			const k = m.col ? c * N + r : r * N + c;
			const seen = k <= m.step;
			const role = held.has(r) ? (seen ? 'vecC' : 'vecA') : 'ink';
			paintCell(f, i, [c, -r, 0], 'cells', 0, held.has(r) ? 1 : seen ? 0.55 : 0.25, { role });
			f.id[i] = 'line' + r;
			f.flash[i] = k === m.step ? 1 : 0;
		}
		for (let s = 0; s < m.lines; s++, i++) {
			const line = m.held[s];
			const h = CELL / 2;
			f.box(i, X_CACHE - 1.6, X_CACHE + 1.6, -s - h, -s + h, -h, h);
			f.look(i, line === undefined ? 'muted' : 'vecC', false, line === undefined ? 0.4 : 1, 0, line === m.cell[0]);
			f.id[i] = line === undefined ? 'slot' + s : 'line' + line;
		}
		f.n = i;
	};
	const pick = (_: string, index: number) => {
		if (index >= N * N) return;
		const r = Math.floor(index / N), c = index % N;
		lessonStore(LESSON).getState().set('step', m.col ? c * N + r : r * N + c);
	};
	return <>
		<Cells lesson={LESSON} cap={N * N + N} paint={paint} paintKey={JSON.stringify([m.col, m.lines, m.step])} frameKey={0} home={FRONT} pad={1.2} onPick={pick} />
		{labels && <>
			{m.held.map((line, s) => (
				<Tag key={'s' + s} lesson={LESSON} small role="ink" hot={'line' + line} text={t(locale, 'mem.slot', { r: line })} at={() => [X_CACHE, -s, 0]} />
			))}
			<Label lesson={LESSON} id="capCache" at={[X_CACHE, 1, 0]} role="muted">{t(locale, 'mem.capCache', { n: m.lines })}</Label>
			<Label lesson={LESSON} id="capM" at={[(N - 1) / 2, 1, 0]} role="muted">{t(locale, 'mem.capMatrix')}</Label>
			<Tag lesson={LESSON} role="hot" offset={[0, -16]} text={`a[${m.cell.join(', ')}]`} at={() => [m.cell[1], -m.cell[0] + CELL / 2, 0]} />
		</>}
	</>;
}

function cacheStatus(locale: Locale): StatusSpec {
	const hints = new Proxy({} as Record<string, Hint>, {
		get: (_, id) => {
			const r = typeof id === 'string' ? /^line(\d+)$/.exec(id) : null;
			return r ? t(locale, 'mem.hintLine', { r: +r[1], a: +r[1] * N, b: +r[1] * N + N - 1 }) : undefined;
		},
	});
	return { hints, idle: (vals: Vals) => {
		const m = cacheModel(vals);
		return t(locale, m.run.hit[m.step] ? 'mem.idleHit' : 'mem.idleMiss', { k: m.step, r: m.cell[0], c: m.cell[1] });
	} };
}

export function CacheScene() {
	const locale = useLocale();
	const rows = useMemo(() => cacheRows(locale), [locale]);
	const status = useMemo(() => cacheStatus(locale), [locale]);
	return <ViewportCard lesson={LESSON} id="3.3-2" name={'3.3-2 · ' + t(locale, 'mem.nameCache')} mode="2d" extent={12} rows={rows} status={status}
		overlays={[{ id: 'labels', label: t(locale, 'mem.ovLabels'), on: true }]}>
		<CacheCells locale={locale} />
	</ViewportCard>;
}

