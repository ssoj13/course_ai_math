import './dom';
import { afterEach, beforeEach, expect, test } from 'bun:test';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { renderToString } from 'react-dom/server';
import { CrossControls, readout } from '../../src/viz/lessons/cross-product';
import { INITIAL, LESSON } from '../../src/viz/lessons/cross-product.math';
import { lessonStore } from '../../src/viz/store';
import '../../src/i18n/ui';

const roots: Root[] = [];
beforeEach(() => {
	lessonStore(LESSON).getState().setMany({ ...INITIAL });
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

test('controls render on the server in both locales for every scene', () => {
	for (const locale of ['en', 'ru'] as const) {
		for (const scene of [1, 2, 3] as const) {
			const html = renderToString(<CrossControls scene={scene} locale={locale} />);
			expect(html).toContain('lab-challenge');
			expect(html).toContain('lab-formula');
		}
	}
});

test('CP-1: the formula and table follow the order; parallel inputs read as the zero vector', () => {
	const store = lessonStore(LESSON);
	const host = mount(<CrossControls scene={1} locale="en" />);
	const cells = () => [...host.querySelectorAll('td')].map((td) => td.textContent);
	// a = (0, −1, 2), b = (2, 0.5, 0): rows x, y, z = (0 − 1, 4 − 0, 0 − (−2)).
	expect(cells()).toEqual(['0', '1', '-1', '4', '0', '4', '0', '-2', '2']);
	expect(host.textContent).toContain('a × b = (-1.00, 4.00, 2.00)');
	act(() => store.getState().set('swap', 1));
	expect(cells()).toEqual(['1', '0', '1', '0', '4', '-4', '-2', '0', '-2']);
	expect(host.textContent).toContain('The order is b × a');
	act(() => store.getState().setMany({ swap: 0, b: [0, -2, 4] }));
	expect(host.textContent).toContain('a and b lie on one line');
	expect(host.querySelectorAll('.lab-challenge.is-done').length).toBeGreaterThan(0);
});

test('readouts: 2D orientation words and the triangle states, in both locales', () => {
	const v = { ...INITIAL } as Parameters<typeof readout>[2];
	expect(readout(2, 'en', v)).toContain('counter-clockwise');
	expect(readout(2, 'en', { ...v, b2: [1, -1] })).toContain('clockwise and the parallelogram is hatched');
	expect(readout(2, 'ru', { ...v, b2: [-3, -1] })).toContain('на одной прямой');
	expect(readout(3, 'en', v)).toContain('toward the light');
	expect(readout(3, 'en', { ...v, rev: 1, cull: 0 })).toContain('it is black');
	expect(readout(3, 'en', { ...v, tA: [0, 0, 0], tC: [4, 0, 0] })).toContain('no normal');
	expect(readout(1, 'ru', { ...v, a: [0, 0, 0] })).toContain('нулевой длины');
});
