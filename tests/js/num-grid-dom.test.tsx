import './dom';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, test } from 'bun:test';
import { NumGrid } from '../../src/viz/NumTable';
import { lessonStore } from '../../src/viz/store';

// NumGrid (the multi-column linked table): cells carry term ids both ways, marks apply per cell id, missing cells are
// blank, non-finite values are refused, and `reveal` scrolls only the table's own box.
test('NumGrid links cells, applies marks and reveals a column inside its own scroll box', async () => {
	const store = lessonStore('num-grid-test', {});
	const host = document.createElement('div');
	document.body.append(host);
	const root = createRoot(host);
	const view = (reveal: string | null, value = 3) => <NumGrid lesson="num-grid-test" caption="grid" corner="i" reveal={reveal}
		cols={[{ id: 'x', label: 'x' }, { id: 'y', label: 'y' }]}
		rows={[{ head: '1', cells: { x: { id: 'x1', value }, y: { id: 'y1', value: -2 } } }, { head: 'Σ', cells: { y: { id: 'sy', value: 1 } } }]}
		marks={{ sy: { struck: true } }} fmt={(v) => String(v)} />;
	try {
		await act(async () => root.render(view(null)));
		const cells = [...host.querySelectorAll('td')];
		expect(cells.map((c) => c.dataset.t ?? '')).toEqual(['x1', 'y1', '', 'sy']);
		expect(host.querySelector('td[data-t="sy"]')!.classList.contains('is-struck')).toBe(true);
		await act(async () => host.querySelector<HTMLElement>('td[data-t="x1"]')!.dispatchEvent(new Event('pointerover', { bubbles: true })));
		expect(store.getState().hot.has('x1')).toBe(true);
		await act(async () => store.getState().setHot(['y1']));
		expect(host.querySelector('td[data-t="y1"]')!.classList.contains('is-hot')).toBe(true);
		const box = host.querySelector<HTMLElement>('.lab-num-grid')!;
		expect(box.style.overflowX).toBe('auto');
		// Layout is not computed in happy-dom: give the column a position past the box and check the box (not the page) scrolls.
		const th = host.querySelector<HTMLElement>('th[data-col="y"]')!;
		Object.defineProperty(th, 'offsetLeft', { value: 500 });
		Object.defineProperty(th, 'offsetWidth', { value: 60 });
		Object.defineProperty(box, 'clientWidth', { value: 300 });
		await act(async () => root.render(view('y')));
		expect(box.scrollLeft).toBe(260);
		let error: unknown = null;
		const quiet = console.error;
		console.error = () => {};
		try { await act(async () => root.render(view(null, NaN))); } catch (e) { error = e; } finally { console.error = quiet; }
		expect(String(error)).toContain('must be finite');
	} finally {
		await act(async () => root.unmount());
		host.remove();
	}
});
