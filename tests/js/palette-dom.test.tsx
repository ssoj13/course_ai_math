import './dom';
import { expect, test } from 'bun:test';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { usePalette } from '../../src/viz/palette';
import { VARS_FOR_TEST } from './palette-vars';

function Probe() {
	return <p>{usePalette().vecA}</p>;
}

test('usePalette re-renders when data-theme flips and unsubscribes cleanly', async () => {
	const root = document.documentElement;
	const set = (v: string) => {
		for (const n of VARS_FOR_TEST) root.style.setProperty(n, v);
	};
	set('#111111');
	const host = document.createElement('div');
	document.body.appendChild(host);
	const r = createRoot(host);
	act(() => r.render(<Probe />));
	expect(host.textContent).toBe('#111111');

	set('#222222');
	// MutationObserver callbacks are microtasks; act awaits them.
	await act(async () => root.setAttribute('data-theme', 'light'));
	expect(host.textContent).toBe('#222222');

	act(() => r.unmount());
	host.remove();
});
