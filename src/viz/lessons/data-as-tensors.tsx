import { useStore } from 'zustand';
import { t } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { hotOff, hotOn, lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { roleVar, type SceneRole } from '../palette';
import { Challenge } from '../Challenge';
import { flat, slice } from '../../core';
import { MemStrip, Tuple } from './MemStrip';
import { CHANNEL, fmtShape, unravel } from './tensor-cells';
import {
	audioModel, CHECKS, corners, EMBEDDED, FACES, fmt3, fmtBig, fmtBytes, IDS, imageModel, imgAt, INITIAL, ITEMSIZE, LESSON, LINKS, lookupAt,
	LOOKUPS, N_FACES, PRESETS, presetFacts, RATE, SAMPLES, SIGNAL, TEXT_D, VERTS, VOCAB, tableRow, type AxisId, type PresetId,
} from './data-as-tensors.math';
import './tensor-cells.css';
import './data-as-tensors.css';

/**
 * Lesson 3.2 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store, the DOM-only controls under
 * each card (memory strips, tables, readouts, challenges) and the shape reader, plus the lazy scene islands (three
 * lives in data-as-tensors.impl.tsx). Words come from the `dat` namespace; numbers from data-as-tensors.math.ts.
 */
lessonStore(LESSON, INITIAL, LINKS);

export const ImageScene = localized(lazyIsland(() => import('./data-as-tensors.impl').then((m) => m.ImageScene)));
export const TextScene = localized(lazyIsland(() => import('./data-as-tensors.impl').then((m) => m.TextScene)));
export const AudioScene = localized(lazyIsland(() => import('./data-as-tensors.impl').then((m) => m.AudioScene)));
export const MeshScene = localized(lazyIsland(() => import('./data-as-tensors.impl').then((m) => m.MeshScene)));

const set = (patch: Record<string, number>) => lessonStore(LESSON).getState().setMany(patch);
const CH = ['R', 'G', 'B'] as const;

/** Axis colours of the image (H rows = y, W columns = x, C channel = z), as in the scene. */
const AXIS_ROLE: Readonly<Record<string, SceneRole>> = { H: 'axisY', W: 'axisX', C: 'axisZ' };

// ---------------------------------------------------------------------------------------------------------------

/** Under 3.2-1: the stored layout, the address sum of the selected byte, the buffer strip, two tasks. */
function Image() {
	const locale = useLocale();
	const m = imageModel(useVals(LESSON));
	const st = m.t.strides;
	const terms = m.idx.map((i, a) => <span key={a}>{a > 0 && ' + '}<span style={{ color: roleVar(AXIS_ROLE[m.names[a]]) }}>{i}</span>·{st[a]}</span>);
	return <section className="not-content dat-controls">
		<div className="ts-box">
			<code>{m.chw ? 'x = img.transpose(2, 0, 1).copy()' : 'x = img'}</code>
			{'  '}{t(locale, 'dat.shape')} <Tuple nums={m.t.shape} roles={m.names.map((n) => AXIS_ROLE[n])} />
			{'  '}{t(locale, 'dat.strides')} <Tuple nums={st} roles={m.names.map((n) => AXIS_ROLE[n])} />
		</div>
		<p className="ts-box">
			x[{m.idx.join(', ')}] → {t(locale, 'dat.address')} = {terms} = <b>{m.addr}</b> · {t(locale, 'dat.pixelIs', { h: m.h, w: m.w, rgb: m.rgb.join(', ') })}
		</p>
		<MemStrip lesson={LESSON} label={t(locale, 'dat.stripImage')} n={m.t.size} center={m.addr} sel={m.addr}
			more={(k) => t(locale, 'dat.more', { k })}
			onSelect={(p) => {
				const [h, w, c] = imgAt(m.chw, p);
				set({ h, w, c });
			}}
			cell={(p) => {
				const [h, w, c] = imgAt(m.chw, p);
				return { text: String(m.t.data[p]), role: CHANNEL[c], hot: 'a' + p, label: t(locale, 'dat.hintByte', { idx: (m.chw ? [c, h, w] : [h, w, c]).join(', '), v: m.t.data[p], h, w, ch: CH[c], p }) };
			}} />
		<p className="dat-note">{t(locale, m.chw ? 'dat.noteChw' : 'dat.noteHwc')}</p>
		<Challenge lesson={LESSON} n={1} text={t(locale, 'dat.ch1')} check={CHECKS.blueSun} hint={[t(locale, 'dat.ch1Hint')]} success={t(locale, 'dat.ch1Success')} />
		<Challenge lesson={LESSON} n={2} text={t(locale, 'dat.ch2')} check={CHECKS.chw64} hint={[t(locale, 'dat.ch2Hint')]} success={t(locale, 'dat.ch2Success')} />
	</section>;
}

/** Under 3.2-2: the latest lookup spelled out, and a task. */
function Text() {
	const locale = useLocale();
	const look = Math.round(useVals(LESSON).look as number);
	const line = (() => {
		if (look < 1) return t(locale, 'dat.lookNone');
		const [b, k] = lookupAt(look - 1);
		const id = IDS.get([b, k]);
		const row = flat(slice(EMBEDDED, [b, k])).map(fmt3).join(', ');
		return t(locale, 'dat.lookLine', { b, t: k, id, word: VOCAB[tableRow(id)], row });
	})();
	return <section className="not-content dat-controls">
		<p className="ts-box" aria-live="polite">{line}</p>
		<p className="dat-note">{t(locale, 'dat.lookCount', { k: look, n: LOOKUPS, d: TEXT_D })}</p>
		<Challenge lesson={LESSON} n={3} text={t(locale, 'dat.ch3')} check={CHECKS.fourthThe} hint={[t(locale, 'dat.ch3Hint')]} success={t(locale, 'dat.ch3Success')} />
	</section>;
}

/** Under 3.2-3: the same 48 samples as one strip (reshape changes the shape, not the buffer), and a task. */
function Audio() {
	const locale = useLocale();
	const m = audioModel(useVals(LESSON));
	return <section className="not-content dat-controls">
		<p className="ts-box">
			<code>{`frames = x.reshape(-1, ${m.fl})`}</code>{'  '}{t(locale, 'dat.shape')} {fmtShape(m.t.shape)}{'  '}{t(locale, 'dat.strides')} {fmtShape(m.t.strides)}
			{'  '}x[{m.idx.join(', ')}] → {t(locale, 'dat.address')} {m.idx[0]}·{m.t.strides[0]} + {m.idx[1]}·1 = <b>{m.s}</b>
		</p>
		<MemStrip lesson={LESSON} label={t(locale, 'dat.stripAudio')} n={SAMPLES} center={m.s} sel={m.s}
			more={(k) => t(locale, 'dat.more', { k })} onSelect={(p) => set({ s: p })}
			cell={(p) => ({
				text: fmt3(SIGNAL.data[p]), step: unravel(p, m.t.shape)[0], hot: 'n' + p,
				label: t(locale, 'dat.hintSample', { s: p, r: unravel(p, m.t.shape)[0], j: unravel(p, m.t.shape)[1], v: fmt3(SIGNAL.data[p]), ms: ((1000 * p) / RATE).toFixed(3) }),
			})} />
		<p className="dat-note">{t(locale, 'dat.noteAudio')}</p>
		<Challenge lesson={LESSON} n={4} text={t(locale, 'dat.ch4')} check={CHECKS.fourFrames} hint={[t(locale, 'dat.ch4Hint')]} />
	</section>;
}

/** Under 3.2-4: the vertex buffer, the index buffer and the gather V[F[f]], linked to the scene by hover. */
function Mesh() {
	const locale = useLocale();
	const face = Math.round(useVals(LESSON).face as number);
	const hot = useStore(lessonStore(LESSON), (s) => s.hot);
	const tri = flat(slice(FACES, [face]));
	const hover = (id: string) => ({
		onPointerEnter: () => hotOn(LESSON, id), onPointerLeave: () => hotOff(LESSON, id),
	});
	const cls = (id: string, on: boolean) => (hot.has(id) ? 'is-hot' : '') + (on ? ' is-sel' : '');
	return <section className="not-content dat-controls">
		<div className="dat-tables">
			<table className="dat-table">
				<caption>V (5, 3) float32</caption>
				<thead><tr><th>i</th><th>x</th><th>y</th><th>z</th></tr></thead>
				<tbody>{Array.from({ length: VERTS.shape[0] }, (_, i) => (
					<tr key={i} className={cls('v' + i, tri.includes(i))} {...hover('v' + i)}>
						<th>{i}</th>{flat(slice(VERTS, [i])).map((x, k) => <td key={k}>{x}</td>)}
					</tr>
				))}</tbody>
			</table>
			<table className="dat-table">
				<caption>F (6, 3) int32</caption>
				<thead><tr><th>f</th><th colSpan={3}>{t(locale, 'dat.corners')}</th></tr></thead>
				<tbody>{Array.from({ length: N_FACES }, (_, f) => (
					<tr key={f} className={cls('f' + f, f === face)} {...hover('f' + f)} onClick={() => set({ face: f })}>
						<th>{f}</th>{flat(slice(FACES, [f])).map((x, k) => <td key={k}>{x}</td>)}
					</tr>
				))}</tbody>
			</table>
			<table className="dat-table">
				<caption>V[F[{face}]] (3, 3)</caption>
				<thead><tr><th>i</th><th>x</th><th>y</th><th>z</th></tr></thead>
				<tbody>{corners(face).map((row, k) => (
					<tr key={k} className={cls('v' + tri[k], false)} {...hover('v' + tri[k])}><th>{tri[k]}</th>{row.map((x, j) => <td key={j}>{x}</td>)}</tr>
				))}</tbody>
			</table>
		</div>
		<p className="dat-note">{t(locale, 'dat.noteMesh')}</p>
		<Challenge lesson={LESSON} n={5} text={t(locale, 'dat.ch5')} check={CHECKS.baseFace} hint={[t(locale, 'dat.ch5Hint')]} />
	</section>;
}

// ---------------------------------------------------------------------------------------------------------------
// Shape reader.

const PRESET_NAME: Readonly<Record<PresetId, Parameters<typeof t>[1]>> = {
	imgLast: 'dat.pImgLast', imgFirst: 'dat.pImgFirst', ids: 'dat.pIds', hidden: 'dat.pHidden', audio: 'dat.pAudio', verts: 'dat.pVerts',
};
const AXIS_NAME: Readonly<Record<AxisId, Parameters<typeof t>[1]>> = {
	B: 'dat.axB', H: 'dat.axH', W: 'dat.axW', C: 'dat.axC', T: 'dat.axT', D: 'dat.axD', S: 'dat.axS', N: 'dat.axN', xyz: 'dat.axXyz',
};

/** Real shapes from pipelines: pick one, read its axes, count its numbers and bytes. */
function Reader() {
	const locale = useLocale();
	const i = Math.round(useVals(LESSON).preset as number);
	const p = PRESETS[i];
	const f = presetFacts(p);
	const b = fmtBytes(f.bytes);
	return <section className="not-content dat-controls">
		<div role="radiogroup" aria-label={t(locale, 'dat.presets')} className="dat-picks">
			{PRESETS.map((q, k) => (
				<button key={q.id} type="button" role="radio" aria-checked={k === i} onClick={() => set({ preset: k })}>{t(locale, PRESET_NAME[q.id])}</button>
			))}
		</div>
		<table className="dat-table dat-axes">
			<thead><tr><th>{t(locale, 'dat.colAxis')}</th><th>{t(locale, 'dat.colMeaning')}</th><th>{t(locale, 'dat.colSize')}</th><th>{t(locale, 'dat.colStride')}</th></tr></thead>
			<tbody>{p.axes.map((a, k) => (
				<tr key={k}><th>{k} · {a}</th><td>{t(locale, AXIS_NAME[a])}</td><td>{fmtBig(p.shape[k])}</td><td>{fmtBig(f.strides[k])}</td></tr>
			))}</tbody>
		</table>
		<p className="ts-box" aria-live="polite">
			{t(locale, 'dat.readerLine', {
				shape: fmtShape(p.shape), prod: p.shape.map(fmtBig).join(' · '), n: fmtBig(f.n), dtype: p.dtype, size: ITEMSIZE[p.dtype],
				bytes: fmtBig(f.bytes), human: b.v + ' ' + b.unit,
			})}
		</p>
	</section>;
}

function Controls({ part }: { part: 'image' | 'text' | 'audio' | 'mesh' | 'reader' }) {
	switch (part) {
		case 'image': return <Image />;
		case 'text': return <Text />;
		case 'audio': return <Audio />;
		case 'mesh': return <Mesh />;
		case 'reader': return <Reader />;
	}
}
export const DataControls = localized(Controls);

