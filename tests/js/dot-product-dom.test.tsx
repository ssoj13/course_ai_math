import './dom';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { renderToString } from 'react-dom/server';
import { expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { DotControls, DotFormula, marksFor, readout } from '../../src/viz/lessons/dot-product';
import { NPanel } from '../../src/viz/scene/NPanel';
import { rowsFor } from '../../src/viz/lessons/dot-product.rows';
import { lessonStore } from '../../src/viz/store';
import { INITIAL, LESSON, STRIKE_GAP_MS } from '../../src/viz/lessons/dot-product.math';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Mounts the controls of all four scenes (as on the page) with fresh lesson values. */
async function mount(locale: 'en' | 'ru') {
	// Challenge success is sticky across visits (stored per reader): start every case from a fresh reader.
	localStorage.clear();
	const host = document.createElement('div');
	document.body.append(host);
	const root = createRoot(host), store = lessonStore(LESSON);
	store.getState().setMany(INITIAL);
	store.getState().setStep('derive', 0);
	store.getState().setHot([]);
	await act(async () => root.render(<>
		{([1, 2, 3, 4] as const).map((s) => <DotControls key={s} scene={s} locale={locale} />)}
		<NPanel lesson={LESSON} rows={rowsFor(1, locale)} />
	</>));
	const step = async (i: number) => act(async () => store.getState().setStep('derive', i));
	const hidden = (m: string) => host.querySelector(`[data-dp-controls="1"] [data-m="${m}"]`)!.classList.contains('is-hidden');
	const struck = (m: string) => host.querySelector(`[data-dp-controls="1"] [data-m="${m}"]`)!.classList.contains('is-struck');
	const cols = () => [...host.querySelectorAll('[data-dp-controls="1"] .lab-num-grid thead th[data-col]')].map((th) => (th as HTMLElement).dataset.col);
	const cell = (id: string) => host.querySelector<HTMLElement>(`[data-dp-controls="1"] .lab-num-grid td[data-t="${id}"]`);
	const done = async () => { await act(async () => root.unmount()); store.getState().setMany(INITIAL); host.remove(); };
	return { host, store, step, hidden, struck, cols, cell, done };
}

for (const locale of ['en', 'ru'] as const) {
	test('DP-1 all nine steps: line reveals, table columns, cancellation sequence, back cancels the pending strike ' + locale, async () => {
		const m = await mount(locale);
		try {
			expect(m.host.querySelector('[data-timeline="derive"] input')!.getAttribute('max')).toBe('8');
			// Step 0: every formula line hidden, only a and b columns; the guide lights a − b.
			for (const l of ['l1', 'l2', 'l3', 'l4', 'l5']) expect(m.hidden(l)).toBe(true);
			expect(m.cols()).toEqual(['a', 'b']);
			expect(m.store.getState().hot.has('amb')).toBe(true);
			await m.step(1);
			expect(m.cols()).toEqual(['a', 'b', 'd', 'dsq', 'asq', 'bsq']);
			expect(m.cell('namb2')!.textContent).toBe('5'); expect(m.cell('na2')!.textContent).toBe('10');
			await m.step(2);
			expect(m.hidden('l1')).toBe(false); expect(m.hidden('l2')).toBe(true);
			expect(m.store.getState().hot.has('th')).toBe(true); // corr links the arc
			await m.step(3);
			expect(m.cols()).toContain('p'); expect(m.cell('dot')).toBeNull();
			for (const id of ['amb', 'expand', 'd1sq', 'd2sq']) expect(m.store.getState().hot.has(id)).toBe(true);
			await m.step(4);
			expect(m.cell('dot')!.textContent).toBe('5');
			for (const id of ['cross', 'p1', 'p2', 'dot']) expect(m.store.getState().hot.has(id)).toBe(true);
			// The N-panel row `dot` appears at step 4, `rhs` at 7, `rect` at 8.
			const rowIds = () => [...m.host.querySelectorAll('.lab-vp-panel .lab-np-row:not(.is-pending)')].map((r) => (r as HTMLElement).dataset.id);
			expect(rowIds()).toContain('dot'); expect(rowIds()).not.toContain('rhs');
			await m.step(5);
			expect(m.hidden('l3')).toBe(false); expect(m.host.querySelector('[data-m="l1"]')!.classList.contains('is-dim')).toBe(true);
			// Step 6: the |a|² pair is struck at once, the |b|² pair STRIKE_GAP_MS later; the table sums follow.
			await m.step(6);
			expect(m.struck('x-na2l')).toBe(true); expect(m.struck('x-na2r')).toBe(true); expect(m.struck('x-nb2l')).toBe(false);
			expect(m.cell('na2')!.classList.contains('is-struck')).toBe(true); expect(m.cell('nb2')!.classList.contains('is-struck')).toBe(false);
			await act(async () => sleep(STRIKE_GAP_MS + 80));
			expect(m.struck('x-nb2l')).toBe(true); expect(m.struck('x-nb2r')).toBe(true); expect(m.cell('nb2')!.classList.contains('is-struck')).toBe(true);
			// Back before the gap: the pending second strike is cancelled, nothing stays struck.
			await m.step(5); await m.step(6); await m.step(5);
			await act(async () => sleep(STRIKE_GAP_MS + 80));
			expect(m.store.getState().vals.strike).toBe(0); expect(m.struck('x-nb2l')).toBe(false);
			await m.step(7);
			expect(m.hidden('x-na2l')).toBe(true); expect(m.hidden('l4')).toBe(false); expect(m.hidden('l5')).toBe(false); expect(m.hidden('l5-rect')).toBe(true);
			expect(m.store.getState().vals.strike).toBe(2);
			expect(rowIds()).toContain('rhs');
			await m.step(8);
			expect(m.hidden('l5-rect')).toBe(false); expect(rowIds()).toContain('rect');
			// The three faces light together (alias group), with both arrows and the shadow.
			for (const id of ['rect', 'dot', 'rhs', 'sum', 'a', 'b', 'shadow']) expect(m.store.getState().hot.has(id)).toBe(true);
			// Drag at a step: a write keeps the step and changes every number (step 8, b antiparallel).
			await act(async () => m.store.getState().set('b', [-1, -2]));
			expect(m.store.getState().step.derive).toBe(8);
			expect(m.cell('dot')!.textContent).toBe('-5');
			const text = m.host.querySelector('[data-dp-controls="1"] [data-dp-readout] p[aria-live]')!.textContent!;
			expect(text).toContain('-5.00');
			expect(text).not.toContain('NaN');
		} finally { await m.done(); }
	});

	test('hover links: one hop, faces alias, sign words and half-planes, leg cells ' + locale, async () => {
		const m = await mount(locale);
		try {
			const hover = async (sel: string) => { const el = m.host.querySelector<HTMLElement>(sel)!; await act(async () => el.dispatchEvent(new Event('pointerover', { bubbles: true }))); await act(async () => el.dispatchEvent(new Event('pointerenter', { bubbles: false }))); return el; };
			const leave = async (el: HTMLElement) => { await act(async () => el.dispatchEvent(new Event('pointerout', { bubbles: true }))); await act(async () => el.dispatchEvent(new Event('pointerleave', { bubbles: false }))); };
			const sum = await hover('[data-dp-faces] .lab-np-row[data-id="sum"]');
			for (const id of ['sum', 'dot', 'rect', 'bright', 'rhs', 'a', 'b', 'shadow']) expect(m.store.getState().hot.has(id)).toBe(true);
			// One hop only: `cross` is reached from nothing here, and `th` (only rhs links it) stays dark.
			expect(m.store.getState().hot.has('th')).toBe(false);
			await leave(sum);
			const pos = await hover('[data-t="sgn_pos"]');
			expect(m.store.getState().hot.has('hp_pos')).toBe(true);
			await leave(pos);
			const a1 = await hover('[data-dp-controls="3"] td[data-t="a31"]');
			expect(m.store.getState().hot.has('a3:x')).toBe(true);
			await leave(a1);
		} finally { await m.done(); }
	});

	test('zero arrows and the lamp: readouts never print NaN, dashes with a reason ' + locale, async () => {
		const m = await mount(locale);
		try {
			for (const [a, b] of [[[0, 0], [1, 2]], [[3, 1], [0, 0]], [[0, 0], [0, 0]]]) {
				await act(async () => m.store.getState().setMany({ a, b, a3: [0, 0, 0] }));
				const text = m.host.textContent!;
				expect(text).not.toContain('NaN');
				expect(text).not.toContain('Infinity');
				expect(readout(1, locale, m.store.getState().vals)).toContain('0.00');
			}
			// a ≠ 0, b = 0: shadow and area are 0, the angle route is undefined (ruling 2).
			expect(readout(1, 'en', { ...INITIAL, a: [3, 1], b: [0, 0] })).toContain('shadow on a is 0');
			expect(renderToString(<DotFormula scene={2} />)).not.toContain('NaN');
			// The perpendicular word needs two nonzero arrows (ruling 3).
			expect(m.host.querySelector('[data-t="sgn_zero"]')!.hasAttribute('data-active')).toBe(true);
			await act(async () => m.store.getState().setMany({ a: [2, 1], b: [-1, 2] }));
			expect(m.host.querySelector('[data-t="sgn_zero"]')!.textContent).toBe('⊥ (= 0)');
			await act(async () => m.store.getState().setMany({ nl_raw: 1 }));
			expect(m.host.querySelector('[data-dp-controls="4"] [data-dp-readout] p[aria-live]')!.textContent).toContain('0.834');
		} finally { await m.done(); }
	});

	test('challenges complete from store values only ' + locale, async () => {
		const m = await mount(locale);
		try {
			const isDone = (scene: number, k: number) => m.host.querySelectorAll(`[data-dp-controls="${scene}"] .lab-challenge`)[k].classList.contains('is-done');
			expect(isDone(1, 0)).toBe(false);
			await act(async () => m.store.getState().setMany({ a: [2, 1], b: [-1, 2] }));
			expect(isDone(1, 0)).toBe(true); expect(isDone(2, 0)).toBe(true);
			await act(async () => m.store.getState().setMany({ a: [3, 1], b: [-1, -2] }));
			expect(isDone(1, 1)).toBe(true);
			await act(async () => m.store.getState().setMany({ a: [3, 1], b: [3, 1] }));
			expect(isDone(1, 2)).toBe(true);
			await act(async () => m.store.getState().setMany({ a: [3, 0], b: [2, 0] }));
			expect(isDone(2, 1)).toBe(true);
			await act(async () => m.store.getState().setMany({ b: [-2, 0] }));
			expect(isDone(2, 2)).toBe(true);
			await act(async () => m.store.getState().setMany({ a: [1, 0], b: [1, Math.sqrt(3)] }));
			expect(isDone(2, 3)).toBe(true);
			await act(async () => m.store.getState().setMany({ a3: [1, 0, 0], b3: [0, 1, 1] }));
			expect(isDone(3, 0)).toBe(true);
			await act(async () => m.store.getState().setMany({ a3: [1, 0, 0], b3: [-1, 0, 1] }));
			expect(isDone(3, 1)).toBe(true);
		} finally { await m.done(); }
	});
}

test('marks: struck terms only on line 3, hidden from step 7', () => {
	expect(marksFor(6, 1)['x-na2l']).toEqual({ struck: true, hidden: false });
	expect(marksFor(6, 1)['x-nb2l']).toEqual({ struck: false, hidden: false });
	expect(marksFor(7, 2)['x-nb2r']).toEqual({ struck: true, hidden: true });
	expect(marksFor(8, 2).l4).toEqual({ hidden: false, dim: true });
});
