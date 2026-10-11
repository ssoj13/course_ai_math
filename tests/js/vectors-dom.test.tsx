import './dom';
import { afterEach, beforeEach, expect, test } from 'bun:test';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { renderToString } from 'react-dom/server';
import '../../src/i18n/ui';
import { lessonStore } from '../../src/viz/store';
import { NPanel } from '../../src/viz/scene/NPanel';
import { VectorControls } from '../../src/viz/lessons/vector';
import { rowsFor as rowsVe } from '../../src/viz/lessons/vector.rows';
import { INITIAL as INIT_VE, LESSON as VE } from '../../src/viz/lessons/vector.math';
import { ArithmeticControls } from '../../src/viz/lessons/vector-arithmetic';
import { rowsFor as rowsVa } from '../../src/viz/lessons/vector-arithmetic.rows';
import { INITIAL as INIT_VA, LESSON as VA } from '../../src/viz/lessons/vector-arithmetic.math';

/**
 * Lessons 1.1 and 1.2, DOM side: every scene's controls render on the server in both locales (TeX, tables,
 * challenges), the formula, table and readout follow the store, hover links reach the scene ids, and N-panel fields
 * write the same keys the scenes draw.
 */
const roots: Root[] = [];
beforeEach(() => {
	HTMLElement.prototype.setPointerCapture = () => {};
	globalThis.requestAnimationFrame = () => 1;
	globalThis.cancelAnimationFrame = () => {};
	lessonStore(VE).getState().setMany({ ...INIT_VE });
	lessonStore(VA).getState().setMany({ ...INIT_VA });
	lessonStore(VE).getState().setHot([]);
	lessonStore(VA).getState().setHot([]);
});
afterEach(() => {
	for (const r of roots.splice(0)) act(() => r.unmount());
	document.body.innerHTML = '';
});
function mount(el: React.ReactElement) {
	const host = document.createElement('div');
	document.body.append(host);
	const root = createRoot(host);
	roots.push(root);
	act(() => root.render(el));
	return host;
}

test('every 1.1 and 1.2 control block renders on the server in EN and RU', () => {
	for (const locale of ['en', 'ru'] as const) {
		for (const scene of [1, 2, 3, 4, 5, 6] as const) {
			const html = renderToString(<VectorControls scene={scene} locale={locale} />);
			expect(html).toContain('lab-challenge');
			expect(html).toContain('katex');
		}
		for (const scene of [1, 2, 3, 4, 5] as const) {
			const html = renderToString(<ArithmeticControls scene={scene} locale={locale} />);
			expect(html).toContain('lab-challenge');
			expect(html).toContain('katex');
		}
	}
});

test('VE-1: table, formula and readout follow v; a component cell lights its dashed leg and formula term', () => {
	const host = mount(<VectorControls scene={1} locale="en" />);
	const store = lessonStore(VE);
	act(() => store.getState().set('v', [-2, 3]));
	expect(host.querySelector('td[data-t="vx"]')!.textContent).toBe('-2.0');
	expect(host.querySelector('.lab-formula')!.textContent).toContain('3.0');
	expect(host.textContent).toContain('-2.0 along x');
	expect(host.querySelector('.lab-challenge')!.classList.contains('is-done')).toBe(true);
	act(() => host.querySelector('td[data-t="vy"]')!.dispatchEvent(new PointerEvent('pointerover', { bubbles: true })));
	expect([...store.getState().hot].sort()).toEqual(['v:y', 'vy']);
});

test('VE-2: typing the start in the N-panel slides the arrow and keeps its components', () => {
	const host = mount(<><VectorControls scene={2} locale="en" /><NPanel lesson={VE} rows={rowsVe(2, 'en')} /></>);
	const store = lessonStore(VE);
	act(() => store.getState().set('tail', [5, 5]));
	expect(store.getState().vals.w).toEqual([3, 2]);
	expect(host.textContent).toContain('starts at (5.0, 5.0) and ends at (8.0, 7.0)');
	expect(host.querySelector('.lab-challenge.is-done')).not.toBeNull();
	act(() => store.getState().set('anchor', 1));
	expect(host.textContent).toContain('position of the point P = (3.0, 2.0)');
});

test('VE-5: the trip back swaps the table columns and the formula order', () => {
	const host = mount(<VectorControls scene={5} locale="en" />);
	act(() => lessonStore(VE).getState().set('rev', 1));
	const heads = [...host.querySelectorAll('th[data-col]')].map((th) => th.textContent);
	expect(heads).toEqual(['A', 'B', 'A − B']);
	expect(host.querySelector('td[data-t="dx"]')!.textContent).toBe('-3.0');
	expect(host.textContent).toContain('trip back');
});

test('VA-1: the sum cell lights the pair it adds; VA-2: scrubbing s is a slider of the formula', () => {
	const host = mount(<><ArithmeticControls scene={1} locale="en" /><ArithmeticControls scene={2} locale="en" /></>);
	const store = lessonStore(VA);
	act(() => host.querySelector('td[data-t="sx"]')!.dispatchEvent(new PointerEvent('pointerover', { bubbles: true })));
	expect([...store.getState().hot].sort()).toEqual(['ax', 'bx', 'sx']);
	const s = host.querySelector<HTMLElement>('[data-s="s"]')!;
	expect(s.getAttribute('role')).toBe('slider');
	expect(s.getAttribute('aria-valuemin')).toBe('-3');
	act(() => store.getState().set('s', -1));
	expect(host.textContent).toContain('turned around');
	expect(host.querySelector('td[data-t="kx"]')!.textContent).toBe('-2.0');
});

test('VA-5: weights from the N-panel move the corner; the prose example reads (1.10, 0.55)', () => {
	const host = mount(<><ArithmeticControls scene={5} locale="ru" /><NPanel lesson={VA} rows={rowsVa(5, 'ru')} /></>);
	expect(host.textContent).toContain("P' = (1.10, 0.55)");
	act(() => lessonStore(VA).getState().setMany({ w1: 1, w2: 0.75 }));
	expect(host.textContent).toContain("P' = (1.20, 0.50)");
	expect(host.querySelector('.lab-challenge.is-done')).not.toBeNull();
});
