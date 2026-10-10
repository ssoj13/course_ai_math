import './dom';
import { afterEach, expect, spyOn, test } from 'bun:test';
import { act, useState, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { lessonStore, useHot, useStep, useVal } from '../../src/viz/store';

const roots: Root[] = [];

/** Mounts an element into a fresh container under document.body and returns the container. */
function mount(el: ReactElement): HTMLElement {
	const host = document.createElement('div');
	document.body.appendChild(host);
	const root = createRoot(host);
	roots.push(root);
	act(() => root.render(el));
	return host;
}

afterEach(() => {
	for (const r of roots.splice(0)) act(() => r.unmount());
	document.body.innerHTML = '';
});

function Probe({ lesson }: { lesson: string }) {
	const v = useVal(lesson, 'v', [0, 0]);
	const hot = useHot(lesson, 'dot');
	const step = useStep(lesson, 't');
	return <p>{JSON.stringify([v, hot, step])}</p>;
}

test('hooks render the initial state and re-render on every store update', () => {
	const lesson = 'dom-reactive';
	const s = lessonStore(lesson);
	const host = mount(<Probe lesson={lesson} />);
	expect(host.textContent).toBe('[[0,0],false,0]');

	act(() => s.getState().set('v', [3, 4]));
	expect(host.textContent).toBe('[[3,4],false,0]');

	act(() => s.getState().setHot(['dot']));
	expect(host.textContent).toBe('[[3,4],true,0]');

	act(() => s.getState().setStep('t', 2));
	expect(host.textContent).toBe('[[3,4],true,2]');
});

// Guard: an inline fallback literal must not leak into the useSyncExternalStore snapshot. Note: with the previous
// selector (fallback returned from inside it) React 19.3 did NOT warn or loop in this harness, so this test guards
// the design rather than reproducing a failure.
let setTick: (n: number) => void = () => {};
let inlineRenders = 0;

function Inline({ lesson }: { lesson: string; tick: number }) {
	inlineRenders++;
	const v = useVal(lesson, 'missing', [0, 0]);
	return <p>{String(v)}</p>;
}

function Driver({ lesson }: { lesson: string }) {
	const [tick, set] = useState(0);
	setTick = set;
	return <Inline lesson={lesson} tick={tick} />;
}

test('inline fallback: no React warnings and bounded renders across re-renders and unrelated store writes', () => {
	const lesson = 'dom-inline-fallback';
	const s = lessonStore(lesson);
	const errors = spyOn(console, 'error');
	try {
		const host = mount(<Driver lesson={lesson} />);
		expect(host.textContent).toBe('0,0');
		for (let n = 1; n <= 3; n++) act(() => setTick(n));
		act(() => s.getState().set('other', 1));
		expect(host.textContent).toBe('0,0');
		expect(errors).not.toHaveBeenCalled();
		// 1 mount + 3 parent re-renders; the unrelated write must not re-render the hook.
		expect(inlineRenders).toBeLessThanOrEqual(4);
	} finally {
		errors.mockRestore();
	}
});
