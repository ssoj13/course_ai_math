// Server render of the term map card (TermMap.tsx): the accessible list view carries every drawn term with its
// definition, relations and links in the page locale, and the map box is labelled for screen readers. The drawn map
// itself (lazy React Flow + ELK worker) is checked in the browser evidence, not here.
import './dom';
import { expect, test } from 'bun:test';
import { renderToString } from 'react-dom/server';
import { DICT } from '../../src/i18n/ui';
import { EDGES, ORDER, mapData, termsOf } from '../../src/viz/terms/data';
import { neighborhoodIn } from '../../src/viz/terms/graph';
import { TermMap } from '../../src/viz/terms/TermMap';

process.env.BASE_URL = '/math-course/';

const render = (html: string) => {
	const box = document.createElement('div');
	box.innerHTML = html;
	return box;
};

for (const locale of ['en', 'ru'] as const) {
	test(`${locale}: a lesson map lists its slice with relations and links`, () => {
		const own = termsOf('law-of-cosines').map((t) => t.id);
		const s = neighborhoodIn(ORDER, EDGES, own, 1);
		const box = render(renderToString(<TermMap locale={locale} data={mapData(s.ids, s.edges, locale)} mode="local" focus={own} />));
		const items = [...box.querySelectorAll('.lab-tm-list li')];
		expect(items.length).toBe(s.ids.length);
		expect(box.querySelectorAll('.lab-tm-list li.is-focus').length).toBe(own.length);
		expect(box.querySelector('.lab-tm-box')?.getAttribute('aria-label')).toBe(DICT[locale]['terms.aria'].replace('{count}', String(s.ids.length)));
		expect(box.querySelector('.lab-tm-msg')?.textContent).toBe(DICT[locale]['terms.noscript']);
		const loc = items.find((li) => li.querySelector('a')?.getAttribute('href')?.endsWith('#term-law-of-cosines'));
		expect(loc?.textContent).toContain(DICT[locale]['terms.builtOn']);
		expect(loc?.querySelector(`a[href="/math-course/${locale === 'ru' ? 'ru/' : ''}ground/law-of-cosines/"]`)).not.toBeNull();
	});
}

test('genealogy shows the selected term with its roots and machine level only', () => {
	const box = render(renderToString(<TermMap locale="en" data={mapData(ORDER, EDGES, 'en')} mode="genealogy" focus={['dot-product']} />));
	const ids = [...box.querySelectorAll('.lab-tm-list li > a')].map((a) => a.getAttribute('href')?.split('#term-')[1]);
	for (const id of ['dot-product', 'law-of-cosines', 'pythagoras', 'square', 'number', 'fma', 'summation-order']) expect(ids).toContain(id);
	for (const id of ['projection', 'cross-product', 'matrix']) expect(ids).not.toContain(id); // built on it, not under it
	expect(box.querySelector('select')?.querySelectorAll('option').length).toBe(ORDER.length);
});

test('an unknown focus is an authoring error, not an empty map', () => {
	expect(() => renderToString(<TermMap locale="en" data={mapData(['square'], [], 'en')} mode="genealogy" focus={['nope']} />)).toThrow(/focus "nope"/);
});
