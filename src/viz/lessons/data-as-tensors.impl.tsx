/**
 * Lesson 3.2 scenes (CURRICULUM 3.2, completion map row 3.2) in the 6d Blender look: an image as H x W x C with its
 * colour planes peeling apart, token ids looked up in an embedding table into B x T x D, a sound cut into frames, and
 * a mesh's vertex and index buffers. Tensor cells are ONE instanced draw each (tensor-cells.impl.tsx over the DotUnit's
 * Boxes); the mesh uses the engine's points, segments and polygon. Loaded lazily by the entry (data-as-tensors.tsx).
 */
import { useMemo } from 'react';
import { useStore } from 'zustand';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import type { V } from '../geom';
import { lessonStore, useVals } from '../store';
import { ViewportCard } from '../scene/ViewportCard';
import { Segment } from '../scene/Segment';
import { Point } from '../scene/Point';
import { Poly } from '../scene/Poly';
import { Label } from '../scene/Label';
import { useOverlay } from '../scene/handles';
import type { Hint, StatusSpec } from '../scene/StatusBar';
import type { Frame } from '../dotunit/choreo';
import { Tag } from '../dotunit/scene';
import { flat, slice } from '../../core';
import { Cells } from './tensor-cells.impl';
import { GLOVE_ROW } from './tensor-road.data';
import { CELL, CHANNEL, maxAbsOf, paintCell } from './tensor-cells';
import { audioRows, imageRows, meshRows, textRows } from './data-as-tensors.rows';
import {
	audioModel, corners, edges, EMBEDDED, fmt3, IDS, IMG_C, IMG_H, IMG_HWC, IMG_W, imageModel, imgAddr, imgAt, LESSON, lookupAt, LOOKUPS,
	FACES, RATE, SAMPLES, TABLE, tableRow, TEXT_B, TEXT_D, TEXT_T, VERTS, VOCAB,
} from './data-as-tensors.math';

type Vals = Parameters<typeof imageModel>[0];
/** A status hint table keyed by a term-id pattern (cells are too many to list). */
const byId = (re: RegExp, hint: (m: RegExpExecArray) => Hint): Record<string, Hint> =>
	new Proxy({} as Record<string, Hint>, { get: (_, id) => (typeof id === 'string' && re.exec(id) ? hint(re.exec(id)!) : undefined) });

// ---------------------------------------------------------------------------------------------------------------
// 3.2-1 image: H x W x C, channels as planes.

/** Fully peeled, each colour plane stands one image width plus one cell to the right of the previous one, one cell behind. */
const PEEL_X = IMG_W + 1;
const IMG_HOME: readonly [number, number, number] = [0.45, 0.5, 1.5];
const CH_NAME = ['R', 'G', 'B'] as const;

/**
 * Centre of byte (h, w, c): rows down, columns right, channel c one plane behind the previous one. Peeling slides the
 * planes apart sideways (0 = one solid 8 x 8 x 3 block, 1 = three pictures side by side), so every plane shows.
 */
const imgPos = (h: number, w: number, c: number, peel: number): V => [w + c * PEEL_X * peel, -h, -c * (1 + peel)];

function ImageCells({ locale }: { locale: Locale }) {
	const m = imageModel(useVals(LESSON));
	// Re-frame once per edit of the peel (its target value), not on every frame of the eased glide.
	const peelTruth = useStore(lessonStore(LESSON), (s) => s.vals.peel);
	const labels = useOverlay('labels');
	const paint = (f: Frame) => {
		let i = 0;
		for (let h = 0; h < IMG_H; h++) for (let w = 0; w < IMG_W; w++) for (let c = 0; c < IMG_C; c++, i++) {
			paintCell(f, i, imgPos(h, w, c, m.peel), 'rgb', IMG_HWC.get([h, w, c]), 1, { role: CHANNEL[c] });
			f.id[i] = 'a' + imgAddr(m.chw, h, w, c);
			f.flash[i] = h === m.h && w === m.w && c === m.c ? 1 : 0;
		}
		f.n = i;
	};
	const pick = (id: string) => {
		const [h, w, c] = imgAt(imageModel(lessonStore(LESSON).getState().vals).chw, Number(id.slice(1)));
		lessonStore(LESSON).getState().setMany({ h, w, c });
	};
	const sel = imgPos(m.h, m.w, m.c, m.peel);
	const z = (c: number) => imgPos(0, 0, c, m.peel)[2];
	// Plane names sit above the middle of each plane's top edge.
	const top = (c: number) => imgPos(0, (IMG_W - 1) / 2, c, m.peel)[0];
	return <>
		<Cells lesson={LESSON} cap={IMG_H * IMG_W * IMG_C} paint={paint} paintKey={m} frameKey={peelTruth} home={IMG_HOME} pad={1.2} onPick={pick} />
		{labels && <>
			{CH_NAME.map((n, c) => (
				<Label key={c} lesson={LESSON} id={'plane' + c} at={[top(c), 0.6, z(c)]} role={CHANNEL[c]} away={[top(c), 0.55, z(c)]} gap={4}>
					{t(locale, 'dat.plane', { c, name: n })}
				</Label>
			))}
			<Label lesson={LESSON} id="axH" at={[-0.9, -(IMG_H - 1) / 2, 0]} role="axisY" away={[-0.85, -(IMG_H - 1) / 2, 0]} gap={4}>{'H = ' + IMG_H}</Label>
			<Label lesson={LESSON} id="axW" at={[(IMG_W - 1) / 2, -IMG_H + 0.3, 0]} role="axisX" away={[(IMG_W - 1) / 2, -IMG_H + 0.35, 0]} gap={4}>{'W = ' + IMG_W}</Label>
			<Tag lesson={LESSON} role="hot" hot={'a' + m.addr} offset={[0, -18]} text={`x[${m.idx.join(', ')}] = ${m.value}`}
				at={() => [sel[0], sel[1] + CELL / 2, sel[2]]} />
		</>}
	</>;
}

function imageStatus(locale: Locale): StatusSpec {
	return {
		hints: byId(/^a(\d+)$/, (r) => (vals: Vals) => {
			const m = imageModel(vals), p = Number(r[1]);
			const [h, w, c] = imgAt(m.chw, p);
			const idx = m.chw ? [c, h, w] : [h, w, c];
			return t(locale, 'dat.hintByte', { idx: idx.join(', '), v: m.t.data[p], h, w, ch: CH_NAME[c], p });
		}),
		idle: { mouse: t(locale, 'dat.idleImage'), touch: t(locale, 'dat.idleImageTouch') },
	};
}

export function ImageScene() {
	const locale = useLocale();
	const rows = useMemo(() => imageRows(locale), [locale]);
	const status = useMemo(() => imageStatus(locale), [locale]);
	return <ViewportCard lesson={LESSON} id="3.2-1" name={'3.2-1 · ' + t(locale, 'dat.nameImage')} mode="3d" extent={8} rows={rows} status={status}
		overlays={[{ id: 'labels', label: t(locale, 'dat.ovLabels'), on: true }]}>
		<ImageCells locale={locale} />
	</ViewportCard>;
}

// ---------------------------------------------------------------------------------------------------------------
// 3.2-2 text: token ids -> embedding rows -> B x T x D.

/** Columns of the three tensors: the table E, the ids column (wide cells: ids have up to 5 digits), the output X. */
const X_IDS = 10.6, ID_W = 2.4, X_OUT = 13;
/** Row of output token (b, t): the sentences one under another with one empty row between them. */
const outRow = (b: number, t: number) => b * (TEXT_T + 1) + t;
const TEXT_HOME: readonly [number, number, number] = [0.04, 0.08, 1.6];
const TABLE_MAX = maxAbsOf(TABLE.data);

function TextCells({ locale }: { locale: Locale }) {
	const look = useVals(LESSON).look as number;
	const labels = useOverlay('labels');
	// Lookups completed (the eased value glides between integers: the row being copied fades in).
	const done = Math.floor(look + 1e-6), part = look - done;
	const cur = Math.min(LOOKUPS, Math.round(look)) - 1;
	const [cb, ct] = cur >= 0 ? lookupAt(cur) : [-1, -1];
	const curRow = cur >= 0 ? tableRow(IDS.get([cb, ct])) : -1;
	const paint = (f: Frame) => {
		let i = 0;
		const c = [0, 0, 0];
		for (let r = 0; r < VOCAB.length; r++) for (let d = 0; d < TEXT_D; d++, i++) {
			c[0] = d; c[1] = -r; c[2] = 0;
			paintCell(f, i, c, 'signed', TABLE.get([r, d]), 1, { maxAbs: TABLE_MAX });
			// One id per table row: the row is what a lookup reads, so hovering any of its numbers lights all of them.
			f.id[i] = 'e' + r;
			f.flash[i] = 0;
		}
		for (let b = 0; b < TEXT_B; b++) for (let k = 0; k < TEXT_T; k++, i++) {
			const y = -outRow(b, k);
			f.box(i, X_IDS - ID_W / 2, X_IDS + ID_W / 2, y - CELL / 2, y + CELL / 2, -CELL / 2, CELL / 2);
			f.look(i, 'ink', false, 1, 0, b === cb && k === ct);
			f.id[i] = `i${b}_${k}`;
		}
		for (let b = 0; b < TEXT_B; b++) for (let k = 0; k < TEXT_T; k++) {
			const n = b * TEXT_T + k;
			// Filled rows are copies of their table row; the row being copied fades in; the rest are empty slots.
			const alpha = n < done ? 1 : n === done ? part : 0;
			for (let d = 0; d < TEXT_D; d++, i++) {
				c[0] = X_OUT + d; c[1] = -outRow(b, k); c[2] = 0;
				if (alpha > 0.01) paintCell(f, i, c, 'signed', EMBEDDED.get([b, k, d]), alpha, { maxAbs: TABLE_MAX });
				else paintCell(f, i, c, 'cells', 0, 0.25, { role: 'muted' });
				// No outline on the copied numbers: their colour and hatch (the sign) must stay readable.
				f.flash[i] = 0;
				f.id[i] = `x${b}_${k}_${d}`;
			}
		}
		f.n = i;
	};
	const top = 1;
	return <>
		<Cells lesson={LESSON} cap={VOCAB.length * TEXT_D + LOOKUPS * (TEXT_D + 1)} paint={paint} paintKey={look} frameKey={0} home={TEXT_HOME} pad={3} />
		{cur >= 0 && <Segment lesson={LESSON} id="arrow" a={[X_IDS - ID_W / 2 - 0.1, -outRow(cb, ct), 0]} b={[TEXT_D - 0.5, -curRow, 0]} role="hot" dashed />}
		{labels && <>
			{Array.from({ length: TEXT_B * TEXT_T }, (_, n) => {
				const [b, k] = lookupAt(n);
				return <Tag key={'id' + n} lesson={LESSON} hot={`i${b}_${k}`} small dense role="ink" text={String(IDS.get([b, k]))} at={() => [X_IDS, -outRow(b, k), 0]} />;
			})}
			{/* Each table row is named by its token id: the same number the ids column holds. */}
			{VOCAB.map((w, r) => (
				<Tag key={w} lesson={LESSON} hot={'e' + r} small dense role={r === curRow ? 'hot' : 'muted'} text={String(IDS_OF[r])}
					at={() => [-0.7, -r, 0]} away={() => [-0.65, -r, 0]} gap={2} />
			))}
			<Label lesson={LESSON} id="capE" at={[(TEXT_D - 1) / 2, top, 0]} role="muted">{t(locale, 'dat.capTable')}</Label>
			<Tag lesson={LESSON} dense role="muted" text="ids (2, 6)" at={() => [X_IDS, top, 0]} />
			<Label lesson={LESSON} id="capX" at={[X_OUT + (TEXT_D - 1) / 2, top, 0]} role="muted">{'X (2, 6, 8)'}</Label>
		</>}
	</>;
}
/** Token id of each drawn table row (the real GloVe row number). */
const IDS_OF: readonly number[] = VOCAB.map((w) => GLOVE_ROW[w]);

function textStatus(locale: Locale): StatusSpec {
	const hints: Record<string, Hint> = new Proxy({} as Record<string, Hint>, {
		get: (_, id) => {
			if (typeof id !== 'string') return undefined;
			let r = /^i(\d)_(\d)$/.exec(id);
			if (r) {
				const b = +r[1], k = +r[2], tok = IDS.get([b, k]);
				return t(locale, 'dat.hintId', { b, t: k, id: tok, word: VOCAB[tableRow(tok)] });
			}
			r = /^e(\d)$/.exec(id);
			if (r) return t(locale, 'dat.hintTable', { id: IDS_OF[+r[1]], word: VOCAB[+r[1]], row: flat(slice(TABLE, [+r[1]])).map(fmt3).join(', ') });
			r = /^x(\d)_(\d)_(\d)$/.exec(id);
			if (r) {
				const b = +r[1], k = +r[2], d = +r[3];
				return t(locale, 'dat.hintOut', { b, t: k, d, v: fmt3(EMBEDDED.get([b, k, d])), id: IDS.get([b, k]) });
			}
			return undefined;
		},
	});
	return { hints, idle: t(locale, 'dat.idleText') };
}

export function TextScene() {
	const locale = useLocale();
	const rows = useMemo(() => textRows(locale), [locale]);
	const status = useMemo(() => textStatus(locale), [locale]);
	return <ViewportCard lesson={LESSON} id="3.2-2" name={'3.2-2 · ' + t(locale, 'dat.nameText')} mode="2d" extent={12} panelOpen={false} rows={rows} status={status}
		overlays={[{ id: 'labels', label: t(locale, 'dat.ovLabels'), on: true }]}>
		<TextCells locale={locale} />
	</ViewportCard>;
}

// ---------------------------------------------------------------------------------------------------------------
// 3.2-3 audio: samples -> frames.

/** Vertical pitch of the frames (each row holds bars up to +-0.72) and the bar height per unit of amplitude. */
const ROW_PITCH = 2.2, BAR = 0.9, BAR_W = 0.6;
const AUDIO_HOME: readonly [number, number, number] = [0.04, 0.06, 1.6];

function AudioCells({ locale }: { locale: Locale }) {
	const m = audioModel(useVals(LESSON));
	const labels = useOverlay('labels');
	const [nf, fl] = m.t.shape;
	const paint = (f: Frame) => {
		for (let s = 0; s < SAMPLES; s++) {
			const [r, j] = [Math.floor(s / fl), s % fl];
			const v = m.t.get([r, j]), y0 = -r * ROW_PITCH;
			// A bar from the frame's zero line to the sample: up = positive (green), down = negative (magenta, hatched).
			f.box(s, j - BAR_W / 2, j + BAR_W / 2, y0, y0 + Math.max(Math.abs(v), 0.02) * BAR * Math.sign(v || 1), -0.05, 0.05);
			f.look(s, v < 0 ? 'vecB' : 'vecC', false, 1, v < 0 ? 1 : 0, s === m.s);
			f.id[s] = 'n' + s;
		}
		f.n = SAMPLES;
	};
	const pick = (id: string) => lessonStore(LESSON).getState().set('s', Number(id.slice(1)));
	return <>
		<Cells lesson={LESSON} cap={SAMPLES} paint={paint} paintKey={m} frameKey={fl} home={AUDIO_HOME} pad={1} onPick={pick} />
		{Array.from({ length: nf }, (_, r) => (
			<Segment key={'z' + r + '_' + fl} lesson={LESSON} id={'zero' + r} a={[-0.5, -r * ROW_PITCH, 0]} b={[fl - 0.5, -r * ROW_PITCH, 0]} role="muted" />
		))}
		{labels && nf <= 12 && Array.from({ length: nf }, (_, r) => (
			<Label key={'f' + r + '_' + fl} lesson={LESSON} id={'frame' + r} at={[-0.8, -r * ROW_PITCH, 0]} role="muted" away={[-0.75, -r * ROW_PITCH, 0]} gap={2}>
				{t(locale, 'dat.frame', { r })}
			</Label>
		))}
		{labels && <Tag lesson={LESSON} role="hot" hot={'n' + m.s} offset={[0, -16]} text={`x[${m.idx.join(', ')}] = ${fmt3(m.value)}`}
			at={() => [m.idx[1], -m.idx[0] * ROW_PITCH + Math.max(0, m.value) * BAR, 0]} />}
	</>;
}

function audioStatus(locale: Locale): StatusSpec {
	return {
		hints: byId(/^n(\d+)$/, (r) => (vals: Vals) => {
			const m = audioModel(vals), s = Number(r[1]);
			return t(locale, 'dat.hintSample', { s, r: Math.floor(s / m.fl), j: s % m.fl, v: fmt3(m.t.data[s]), ms: ((1000 * s) / RATE).toFixed(3) });
		}),
		idle: t(locale, 'dat.idleAudio'),
	};
}

export function AudioScene() {
	const locale = useLocale();
	const rows = useMemo(() => audioRows(locale), [locale]);
	const status = useMemo(() => audioStatus(locale), [locale]);
	return <ViewportCard lesson={LESSON} id="3.2-3" name={'3.2-3 · ' + t(locale, 'dat.nameAudio')} mode="2d" extent={10} rows={rows} status={status}
		overlays={[{ id: 'labels', label: t(locale, 'dat.ovLabels'), on: true }]}>
		<AudioCells locale={locale} />
	</ViewportCard>;
}

// ---------------------------------------------------------------------------------------------------------------
// 3.2-4 mesh: vertex buffer + index buffer.

const EDGES = edges();
const vert = (i: number): V => flat(slice(VERTS, [i])) as V;

function Mesh() {
	const face = Math.round(useVals(LESSON).face as number);
	const labels = useOverlay('labels');
	return <>
		{EDGES.map(([a, b]) => <Segment key={a + '-' + b} lesson={LESSON} id={`e${a}_${b}`} a={vert(a)} b={vert(b)} role="muted" />)}
		<Poly lesson={LESSON} id={'f' + face} pts={(v) => corners(Math.round(v.face as number)) as V[]} role="vecA" />
		{Array.from({ length: VERTS.shape[0] }, (_, i) => <Point key={i} lesson={LESSON} id={'v' + i} at={vert(i)} role={flat(slice(FACES, [face])).includes(i) ? 'vecA' : 'ink'} />)}
		{labels && Array.from({ length: VERTS.shape[0] }, (_, i) => (
			<Label key={'l' + i} lesson={LESSON} id={'v' + i} at={vert(i)} role="ink" offset={[12, -12]}>{String(i)}</Label>
		))}
	</>;
}

function meshStatus(locale: Locale): StatusSpec {
	return {
		hints: byId(/^v(\d)$/, (r) => t(locale, 'dat.hintVert', { i: +r[1], xyz: vert(+r[1]).join(', ') })),
		idle: t(locale, 'dat.idleMesh'),
	};
}

export function MeshScene() {
	const locale = useLocale();
	const rows = useMemo(() => meshRows(locale), [locale]);
	const status = useMemo(() => meshStatus(locale), [locale]);
	return <ViewportCard lesson={LESSON} id="3.2-4" name={'3.2-4 · ' + t(locale, 'dat.nameMesh')} mode="3d" extent={2} rows={rows} status={status}
		overlays={[{ id: 'labels', label: t(locale, 'dat.ovLabels'), on: true }]}>
		<Mesh />
	</ViewportCard>;
}

