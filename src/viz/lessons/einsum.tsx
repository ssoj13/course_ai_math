import { useMemo, useState, type KeyboardEvent } from 'react';
import { useStore } from 'zustand';
import { t, type Locale } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
import { CoreError, parseEinsum, SpecError, type Parsed, type SpecReason } from '../../core';
import { hotOff, hotOn, lessonStore, useVals } from '../store';
import { lazyIsland } from '../island';
import { roleVar } from '../palette';
import { rich } from '../rich';
import { Challenge } from '../Challenge';
import { CodeView } from '../CodeView';
import { Timeline } from '../Timeline';
import { FieldCell, type Row } from '../scene/NPanel';
import { diveVectors } from '../dotunit/einsum.store';
import { fmtShort } from '../dotunit/model';
import {
	BROKEN, CHECKS, errInfo, EXT, extKey, extOf, flops, INITIAL, lab, LAB_PRESETS, LESSON, lettersOf, loops, opOf, opVal,
	plug, presetPatch, roleOf, S4, s4Layout, sumText, toggleKeep, type Lab, type Role,
} from './einsum.math';
import { fmtShape } from './tensor-ops';
import './tensor-road.css';
import './tensor-ops.css';
import './einsum.css';

/**
 * Lesson 3.6 entry (SSR-safe, see AGENTS.md "Lesson pattern"): the module-level store, the DOM-only controls (the op
 * editor with presets and index lengths, the reading of the op, its loops, the pass timeline, the dive bar, the S4
 * socket diagram, challenges) and the lazy light-box island (einsum.impl.tsx). Every number and every refusal comes
 * from the tensor core through einsum.math.ts.
 */
lessonStore(LESSON, INITIAL);

export const LabScene = localized(lazyIsland(() => import('./einsum.impl').then((m) => m.LabScene)));

type Key = Parameters<typeof t>[1];
const PRESET_NAME: Record<string, Key> = {
	dot: 'es.pDot', matvec: 'es.pMatvec', matmul: 'es.pMatmul', frob: 'es.pFrob', abt: 'es.pAbt', outer: 'es.pOuter', batched: 'es.pBatched', attention: 'es.pAttention',
};
const ROLE_NAME: Record<Role, Key> = { batch: 'es.rBatch', free: 'es.rFree', contracted: 'es.rContracted', reduced: 'es.rReduced' };
/** Colour of a letter by what happens to it: kept = the output colour, summed = the hot (stack) colour. */
const ROLE_COLOR: Record<Role, string> = { batch: roleVar('vecC'), free: roleVar('vecC'), contracted: roleVar('hot'), reduced: roleVar('hot') };
const PASS_KEYS = ['du.pass0', 'du.pass1', 'du.pass2', 'du.pass3', 'du.pass4'] as const;

const setOp = (op: string, extra: Record<string, number> = {}) => lessonStore(LESSON).getState().setMany({ op: opVal(op), sel: 0, dive: 0, ...extra });
const letterId = (l: string) => 'l_' + l;
const hoverProps = (id: string) => ({
	onPointerEnter: () => hotOn(LESSON, id), onPointerLeave: () => hotOff(LESSON, id), onFocus: () => hotOn(LESSON, id), onBlur: () => hotOff(LESSON, id),
});

const SPEC_ERR: Record<SpecReason, Key> = {
	noArrow: 'es.errNoArrow', arrows: 'es.errArrows', ellipsis: 'es.errEllipsis', outComma: 'es.errOutComma', space: 'es.errSpace',
	char: 'es.errChar', arity: 'es.errArity', repeatIn: 'es.errRepeatIn', repeatOut: 'es.errRepeatOut', unknownOut: 'es.errUnknownOut',
};

/** The reason the core refused an op or a cable, in the reader's language. */
export function errText(e: CoreError, locale: Locale): string {
	const i = errInfo(e);
	if (i.kind === 'spec') return t(locale, SPEC_ERR[i.reason], { token: i.token, pos: i.pos });
	if (i.kind === 'extent') return t(locale, 'es.errExtent', { l: i.letter, a: i.a, b: i.b });
	return t(locale, 'es.errOther');
}

function ErrBox({ e, op }: { e: CoreError; op?: string }) {
	const locale = useLocale();
	const at = e instanceof SpecError && op !== undefined ? e.pos : -1;
	return <div className="to-err" role="status">
		<p>{rich(errText(e, locale))}</p>
		{at >= 0 && <p className="es-caret" aria-hidden="true"><code>{op}<br />{' '.repeat(at) + '^'}</code></p>}
	</div>;
}

function Presets({ op }: { op: string }) {
	const locale = useLocale();
	return <>
		<div role="radiogroup" aria-label={t(locale, 'es.presets')} className="tr-picks">
			{LAB_PRESETS.map((p) => (
				<button key={p.id} type="button" role="radio" aria-checked={op === p.op} onClick={() => lessonStore(LESSON).getState().setMany(presetPatch(p))}>
					<code>{p.op}</code> {t(locale, PRESET_NAME[p.id])}
				</button>
			))}
		</div>
		<div role="group" aria-label={t(locale, 'es.broken')} className="tr-picks">
			<span className="tr-note">{t(locale, 'es.broken')}:</span>
			{BROKEN.map((op2) => <button key={op2} type="button" onClick={() => setOp(op2)}><code>{op2}</code></button>)}
		</div>
	</>;
}

function OpField({ op, b }: { op: string; b: Lab }) {
	const locale = useLocale();
	return <label className="es-op">
		<span>{t(locale, 'es.opLabel')}</span>
		<input type="text" value={op} spellCheck={false} autoCapitalize="off" autoComplete="off" aria-invalid={!b.ok}
			onChange={(e) => setOp(e.currentTarget.value)} />
	</label>;
}

function Extents({ b }: { b: Lab }) {
	const locale = useLocale();
	let letters: string[] = [];
	try {
		letters = b.ok ? lettersOf(b.plan) : lettersOf(parseEinsum(b.op));
	} catch (e) {
		if (!(e instanceof CoreError)) throw e;
		letters = [...new Set(b.op)].filter((c) => c >= 'a' && c <= 'z');
	}
	const rows = useMemo<Row[]>(() => letters.map((l) => ({
		id: 'ext_' + l, label: l, kind: 'num', key: extKey(l), spec: { ...EXT }, fmt: (x) => String(Math.round(x)), tip: { text: t(locale, 'es.extTip', { l }) },
	})), [letters.join(''), locale]);
	return <div role="group" aria-label={t(locale, 'es.extents')} className="es-ext">
		{rows.map((r) => <FieldCell key={r.key} lesson={LESSON} row={r} size="sm" />)}
	</div>;
}

/** The op with every letter coloured by its role and linked to its socket, axis tag and loop. */
function OpLine({ p }: { p: Parsed }) {
	const hot = useStore(lessonStore(LESSON), (s) => s.hot);
	return <code className="es-line">
		{[...p.op].map((c, i) => {
			if (c < 'a' || c > 'z') return <span key={i}>{c}</span>;
			const id = letterId(c);
			return <span key={i} tabIndex={0} data-t={id} className={hot.has(id) ? 'is-hot' : undefined} style={{ color: ROLE_COLOR[roleOf(p, c)] }} {...hoverProps(id)}>{c}</span>;
		})}
	</code>;
}

/** The reading of a valid op: what stays (the output shape), what disappears (the shaft), the batch axes, the cost. */
function Reading({ b }: { b: Extract<Lab, { ok: true }> }) {
	const locale = useLocale();
	const p = b.plan;
	const batch = p.kept.filter((l) => roleOf(p, l) === 'batch');
	return <div className="tr-formula es-read">
		<p><OpLine p={p} /> &nbsp; <span className="es-sum">{sumText(p)}</span></p>
		<p>{p.kept.length ? t(locale, 'es.readKeep', { letters: p.kept.join(', '), shape: fmtShape(p.outShape) }) : t(locale, 'es.readNoKeep')}</p>
		<p>{p.summed.length ? t(locale, 'es.readSum', { letters: p.summed.join(', '), n: p.sumLen }) : t(locale, 'es.readNoSum')}</p>
		{batch.length > 0 && <p>{t(locale, 'es.readBatch', { letters: batch.join(', ') })}</p>}
		<p>{t(locale, 'es.readFlops', { cells: p.cells, n: p.sumLen, mul: p.cells * p.sumLen, flops: flops(p) })}</p>
	</div>;
}

function DiveBar({ b }: { b: Extract<Lab, { ok: true }> }) {
	const locale = useLocale(), v = useVals(LESSON), st = lessonStore(LESSON).getState();
	const dive = v.dive === 1, n = b.d.L.cells.n, rank = b.plan.kept.length;
	const c = Math.min(n - 1, Math.max(0, Math.round(v.sel as number)));
	const cell = '(' + Array.from(b.d.L.cells.idx.subarray(c * rank, c * rank + rank)).join(', ') + ')';
	const sel: Row = { id: 'sel', label: t(locale, 'du.selCell'), kind: 'num', key: 'sel', spec: { min: 0, max: Math.max(1, n - 1), step: 1 }, fmt: (x) => String(Math.round(x)), tip: { text: t(locale, 'du.selTip') } };
	const pair = dive ? diveVectors(b.d, c) : null;
	return <div className="es-dive">
		{!dive && n > 1 && <FieldCell lesson={LESSON} row={sel} size="sm" />}
		{dive
			? <button type="button" onClick={() => st.set('dive', 0)}>{t(locale, 'du.backUp')}</button>
			: <button type="button" onClick={() => st.set('dive', 1)}>{t(locale, 'du.diveBtn', { cell })}</button>}
		<span aria-live="polite">{pair ? t(locale, 'es.diveText', { cell, a: pair.a.map(fmtShort).join(', '), b: pair.b.map(fmtShort).join(', '), v: fmtShort(b.d.out[c]) }) : ''}</span>
	</div>;
}

function LabPart() {
	const locale = useLocale();
	const v = useVals(LESSON), op = opOf(v), b = lab(v);
	return <section className="not-content tr-controls" data-to-controls="lab">
		<Presets op={op} />
		<OpField op={op} b={b} />
		<Extents b={b} />
		{b.ok ? <Reading b={b} /> : <ErrBox e={b.error} op={op} />}
		<Timeline lesson={LESSON} id="pass" steps={PASS_KEYS.map((k, i) => ({ id: String(i), text: t(locale, k) }))} />
		{b.ok && <DiveBar b={b} />}
		{b.ok && <CodeView lang="python" code={loops(b.plan)} />}
		<Challenge lesson={LESSON} n={1} text={t(locale, 'es.ch1')} check={CHECKS.frob} hint={[t(locale, 'es.ch1Hint')]} success={t(locale, 'es.ch1Success')} />
		<Challenge lesson={LESSON} n={2} text={t(locale, 'es.ch2')} check={CHECKS.abt} hint={[t(locale, 'es.ch2Hint')]} success={t(locale, 'es.ch2Success')} />
	</section>;
}

// ---------------------------------------------------------------------------------------------------------------
// S4: plugs and cables.

const CABLE_COLOR: Record<Role, string> = { batch: roleVar('vecC'), free: roleVar('muted'), contracted: roleVar('light'), reduced: roleVar('light') };
const BLOCK_COLOR = { A: roleVar('vecA'), B: roleVar('vecB'), C: roleVar('vecC') } as const;

function press(f: () => void) {
	return (e: KeyboardEvent) => {
		if (e.key !== 'Enter' && e.key !== ' ') return;
		e.preventDefault();
		f();
	};
}

function Diagram({ p }: { p: Parsed }) {
	const locale = useLocale(), v = useVals(LESSON), hot = useStore(lessonStore(LESSON), (s) => s.hot);
	const [pick, setPick] = useState<number | null>(null);
	const [err, setErr] = useState<CoreError | null>(null);
	const ext = (l: string) => extOf(v, l);
	const L = s4Layout(p, ext);
	const st = lessonStore(LESSON).getState();
	const clickA = (i: number) => {
		setErr(null);
		setPick(pick === i ? null : i);
	};
	const clickB = (i: number) => {
		if (pick === null) return;
		const e = plug(p.op, pick, i, ext);
		setPick(null);
		if (e.ok) {
			setErr(null);
			setOp(e.op, Object.fromEntries(Object.entries(e.ext ?? {}).map(([l, n]) => [extKey(l), n])));
		} else {
			setErr(e.error);
			st.set('bad', 1);
		}
	};
	const sock = (name: 'A' | 'B' | 'C', s: (typeof L.sockets.A)[number], i: number) => {
		const id = letterId(s.letter), on = hot.has(id), picked = name === 'A' && pick === i;
		const act = name === 'A' ? () => clickA(i) : name === 'B' ? () => clickB(i) : undefined;
		const label = t(locale, name === 'C' ? 'es.sockOut' : 'es.sockIn', { l: s.letter, n: s.size, name });
		const body = <>
			<circle cx={s.x} cy={s.y} r={10} className="es-sock" style={{ stroke: on || picked ? roleVar('hot') : BLOCK_COLOR[name], strokeWidth: on || picked ? 3 : 1.5 }} />
			<text x={s.x} y={s.y + 4} textAnchor="middle" className="es-sock-l">{s.letter}</text>
			<text x={s.x} y={name === 'C' ? s.y + 28 : s.y - 16} textAnchor="middle" className="es-sock-n">{s.size}</text>
		</>;
		return act
			? <g key={name + i} role="button" tabIndex={0} aria-label={label} aria-pressed={name === 'A' ? picked : undefined} data-t={id} className="es-hit"
				onClick={act} onKeyDown={press(act)} {...hoverProps(id)}>{body}</g>
			: <g key={name + i} aria-label={label} role="img" data-t={id} {...hoverProps(id)}>{body}</g>;
	};
	return <div className="es-s4">
		<svg viewBox={`0 0 ${S4.W} ${S4.H}`} role="group" aria-label={t(locale, 'es.s4Label')}>
			{L.cables.map((c) => {
				const on = hot.has(letterId(c.letter));
				return <g key={'c' + c.letter} data-t={letterId(c.letter)}>
					<path d={c.d} fill="none" stroke={on ? roleVar('hot') : CABLE_COLOR[c.role]} strokeWidth={on ? 4 : 2.5} strokeLinecap="round" />
					{c.mark && <>
						<circle cx={c.mark.x} cy={c.mark.y} r={10} className="es-mark" style={{ stroke: on ? roleVar('hot') : CABLE_COLOR[c.role] }} />
						<text x={c.mark.x} y={c.mark.y + 4} textAnchor="middle" className="es-sock-l">Σ</text>
					</>}
				</g>;
			})}
			{L.blocks.map((bl) => (
				<g key={bl.name}>
					<rect x={bl.x} y={bl.y} width={bl.w} height={bl.h} rx={5} className="es-block" style={{ stroke: BLOCK_COLOR[bl.name] }} />
					<text x={bl.x + 8} y={bl.y + (bl.name === 'C' ? bl.h - 8 : 16)} className="es-block-l" style={{ fill: BLOCK_COLOR[bl.name] }}>{bl.name + ' ' + fmtShape(bl.shape)}</text>
				</g>
			))}
			{L.sockets.A.map((s, i) => sock('A', s, i))}
			{L.sockets.B.map((s, i) => sock('B', s, i))}
			{L.sockets.C.map((s, i) => sock('C', s, i))}
		</svg>
		<p className="tr-note" aria-live="polite">{pick === null ? t(locale, 'es.s4Hint') : t(locale, 'es.pickA', { l: p.inputs[0][pick] })}</p>
		<div role="group" aria-label={t(locale, 'es.toggles')} className="tr-picks">
			{lettersOf(p).map((l) => {
				const r = roleOf(p, l), kept = p.output.includes(l);
				return <button key={l} type="button" aria-pressed={kept} data-t={letterId(l)} {...hoverProps(letterId(l))}
					onClick={() => { const e = toggleKeep(p.op, l); if (e.ok) setOp(e.op); }}>
					<code style={{ color: ROLE_COLOR[r] }}>{l}</code> {t(locale, ROLE_NAME[r])}
				</button>;
			})}
		</div>
		{err && <ErrBox e={err} />}
	</div>;
}

function S4Part() {
	const locale = useLocale();
	const v = useVals(LESSON), op = opOf(v);
	let p: Parsed | null = null, e: CoreError | null = null;
	try {
		p = parseEinsum(op, 2);
	} catch (x) {
		if (!(x instanceof CoreError)) throw x;
		e = x;
	}
	return <section className="not-content tr-controls" data-to-controls="s4">
		<OpField op={op} b={lab(v)} />
		{p ? <Diagram p={p} /> : <><p className="tr-note">{t(locale, 'es.s4Broken')}</p>{e && <ErrBox e={e} op={op} />}</>}
		<Challenge lesson={LESSON} n={3} text={t(locale, 'es.ch3')} check={CHECKS.batched} hint={[t(locale, 'es.ch3Hint')]} success={t(locale, 'es.ch3Success')} />
		<Challenge lesson={LESSON} n={4} text={t(locale, 'es.ch4')} check={CHECKS.mismatch} hint={[t(locale, 'es.ch4Hint')]} success={t(locale, 'es.ch4Success')} />
	</section>;
}

function Controls({ part }: { part: 'lab' | 's4' }) {
	return part === 'lab' ? <LabPart /> : <S4Part />;
}
export const EinsumControls = localized(Controls);
