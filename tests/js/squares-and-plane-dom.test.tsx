import './dom';
import { afterEach, beforeEach, expect, test } from 'bun:test';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { renderToString } from 'react-dom/server';
import { SquaresControls } from '../../src/viz/lessons/squares-and-plane';
import { NPanel } from '../../src/viz/scene/NPanel';
import { rowsFor } from '../../src/viz/lessons/squares-and-plane.rows';
import { INITIAL, LESSON } from '../../src/viz/lessons/squares-and-plane.math';
import { lessonStore } from '../../src/viz/store';
import '../../src/i18n/ui';

const roots: Root[] = [];
let frame: FrameRequestCallback | undefined;
beforeEach(() => {
	HTMLElement.prototype.setPointerCapture = () => {};
	globalThis.requestAnimationFrame = (cb) => { frame = cb; return 1; };
	globalThis.cancelAnimationFrame = () => { frame = undefined; };
	lessonStore(LESSON).getState().setMany({ ...INITIAL });
});
afterEach(() => {
	for (const r of roots.splice(0)) act(() => r.unmount());
	document.body.innerHTML = '';
	frame = undefined;
});
function mount(el: React.ReactElement) {
	const host = document.createElement('div');
	document.body.append(host);
	const root = createRoot(host);
	roots.push(root);
	act(() => root.render(el));
	return host;
}
const ptr = (type: string, x: number) => new PointerEvent(type, { bubbles: true, clientX: x, pointerId: 1, pointerType: 'mouse' });
const flush = () => act(() => { const f = frame; frame = undefined; f?.(performance.now()); });

test('0.G-1 controls: formula, live readout and challenges in both locales; the second line shows only with a partial area', () => {
	for (const locale of ['en', 'ru'] as const) expect(renderToString(<SquaresControls scene={1} locale={locale} />)).toContain('lab-challenge');
	const host = mount(<SquaresControls scene={1} locale="en" />);
	const store = lessonStore(LESSON);
	expect(host.querySelector('[data-m="sq-2"]')!.classList.contains('is-hidden')).toBe(true);
	act(() => store.getState().set('x', 2.5));
	expect(host.querySelector('[data-m="sq-2"]')!.classList.contains('is-hidden')).toBe(false);
	expect(host.textContent).toContain('whole tiles: 4, plus partial area 2.25 in cut (yellow) pieces: 5.');
	act(() => store.getState().set('x', -3));
	expect(host.textContent).toContain('squares forget the sign');
	expect(host.querySelector('.lab-formula')!.textContent).toContain('(−3.0)');
	act(() => store.getState().set('x', 0));
	expect(host.textContent).toContain('x = 0: no tiles');
	act(() => store.getState().set('x', -4));
	expect([...host.querySelectorAll('.lab-challenge')].every((c) => c.classList.contains('is-done'))).toBe(true);
});

test('formula and N-panel scrubs of A cancel back to the full linked pair (x keeps its sign)', () => {
	const store = lessonStore(LESSON);
	act(() => store.getState().set('x', -3));
	const host = mount(<><SquaresControls scene={3} locale="en" /><NPanel lesson={LESSON} rows={rowsFor(3, 'en')} /></>);
	for (const selector of ['[data-s="A"]', '.lab-np-field']) {
		const handle = host.querySelector<HTMLElement>(selector)!;
		const capture = selector.startsWith('[data-s') ? host.querySelector<HTMLElement>('.lab-formula')! : handle;
		act(() => void handle.dispatchEvent(ptr('pointerdown', 100)));
		act(() => void capture.dispatchEvent(ptr('pointermove', 140)));
		flush();
		expect(store.getState().vals.x).toBeGreaterThan(0);
		const { x, A } = store.getState().vals as { x: number; A: number };
		expect(A).not.toBe(9);
		expect(Math.abs(x * x - A)).toBeLessThan(1e-9);
		act(() => void capture.dispatchEvent(ptr('pointercancel', 140)));
		expect(store.getState().vals).toMatchObject({ x: -3, A: 9 });
	}
});

test('0.G-2: the two-readings table reads P, the squares line dims until a SQUARES id is hot', () => {
	const store = lessonStore(LESSON);
	const host = mount(<SquaresControls scene={2} locale="ru" />);
	const cells = [...host.querySelectorAll('td')].map((td) => td.textContent);
	expect(cells).toEqual(['1', '2']);
	expect(host.querySelector('[data-m="sq-sum"]')!.classList.contains('is-dim')).toBe(true);
	act(() => store.getState().setHot(['sum']));
	expect(host.querySelector('[data-m="sq-sum"]')!.classList.contains('is-dim')).toBe(false);
	act(() => store.getState().setHot([]));
	act(() => store.getState().set('P', [3, 2]));
	expect([...host.querySelectorAll('td')].map((td) => td.textContent)).toEqual(['3', '2']);
	expect(host.textContent).toContain('x² + y² = 9.00 + 4.00 = 13.00');
});
