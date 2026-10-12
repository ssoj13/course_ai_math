import { useFrameStub } from './dom';
import { afterEach, beforeEach, expect, test } from 'bun:test';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { renderToString } from 'react-dom/server';
import { PythagorasControls, PythagorasND } from '../../src/viz/lessons/pythagoras';
import { rowsFor } from '../../src/viz/lessons/pythagoras.rows';
import { INITIAL, LESSON } from '../../src/viz/lessons/pythagoras.math';
import { lessonStore } from '../../src/viz/store';
import '../../src/i18n/ui';

const roots: Root[] = [];
useFrameStub(() => ({ raf: () => 1 }));
beforeEach(() => {
	const st = lessonStore(LESSON).getState();
	st.setMany({ ...INITIAL });
	st.setStep('dis', 0);
	st.setStep('room', 0);
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

test('every control block renders on the server in both locales', () => {
	for (const locale of ['en', 'ru'] as const) {
		for (const scene of [1, 2, 3, 5] as const) expect(renderToString(<PythagorasControls scene={scene} locale={locale} />)).toContain('lab-challenge');
		expect(renderToString(<PythagorasND locale={locale} />)).toContain('lab-num-table');
	}
});

test('0.3-1 readout follows the points; a negative leg is squared in parentheses', () => {
	const host = mount(<PythagorasControls scene={1} locale="en" />);
	expect(host.textContent).toContain('straight line d = 5.00, grid walk 7.0');
	act(() => lessonStore(LESSON).getState().set('Q', [-2, 6]));
	expect(host.querySelector('.lab-formula')!.textContent).toContain('(−3.0)');
	expect(host.textContent).toContain('A leg is negative');
});

test('0.3-2: the second count and the theorem appear only at the last step', () => {
	const host = mount(<PythagorasControls scene={2} locale="en" />);
	const lines = () => [...host.querySelectorAll('.lab-formula [data-variant="wide"] [data-m]')].map((m) => (m as HTMLElement).dataset.m);
	expect(lines()).not.toContain('dis-3');
	act(() => lessonStore(LESSON).getState().setStep('dis', 4));
	expect(lines()).toContain('dis-3');
	expect(host.textContent).toContain('a² + b² = c²');
});

test('0.3-4: the + button adds a dimension and a table row; the sum follows', () => {
	const host = mount(<PythagorasND locale="en" />);
	const bodyRows = () => host.querySelectorAll('.lab-num-table tbody tr').length;
	expect(bodyRows()).toBe(4 + 2);
	expect(host.textContent).toContain('sum 9; distance d = 3.00');
	act(() => void host.querySelectorAll<HTMLButtonElement>('[data-pt-nd] button')[1].click());
	expect(lessonStore(LESSON).getState().vals.n).toBe(5);
	expect(bodyRows()).toBe(5 + 2);
	expect(host.textContent).toContain('sum 25; distance d = 5.00');
});

test('row ids are unique per card and identical across locales', () => {
	for (const scene of [1, 2, 3, 5] as const) {
		const en = rowsFor(scene, 'en').map((r) => r.id), ru = rowsFor(scene, 'ru').map((r) => r.id);
		expect(ru).toEqual(en);
		expect(new Set(en).size).toBe(en.length);
	}
});
