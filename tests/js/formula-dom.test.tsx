import './dom';
import { afterEach, beforeEach, expect, spyOn, test } from 'bun:test';
import { act, useState, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import * as texMod from '../../src/viz/tex';
import { Formula } from '../../src/viz/Formula';
import { Num } from '../../src/viz/Num';
import { lessonStore } from '../../src/viz/store';

const roots: Root[] = [];
const spec = { min: 0, max: 180, step: 1 };
const TEX = String.raw`\t{ang}{\theta} = \s{theta}`;

/** Mounts an element into a fresh container under document.body and returns the container. */
function mount(el: ReactElement): HTMLElement {
	const host = document.createElement('div');
	document.body.appendChild(host);
	const root = createRoot(host);
	roots.push(root);
	act(() => root.render(el));
	return host;
}

// happy-dom lacks pointer capture; frames run synchronously so each pointermove is applied before the next one.
beforeEach(() => {
	HTMLElement.prototype.setPointerCapture = () => {};
	globalThis.requestAnimationFrame = (cb) => {
		cb(0);
		return 1;
	};
	globalThis.cancelAnimationFrame = () => {};
});

afterEach(() => {
	for (const r of roots.splice(0)) act(() => r.unmount());
	document.body.innerHTML = '';
});

const ptr = (type: string, clientX: number) => new PointerEvent(type, { bubbles: true, clientX, pointerId: 1 });
const val = (lesson: string) => lessonStore(lesson).getState().vals.theta;

test('drag keeps updating after the pressed handle was replaced by a re-render (C2 regression)', () => {
	const lesson = 'fdom-drag';
	lessonStore(lesson, { theta: 60 });
	const host = mount(<Formula lesson={lesson} tex={TEX} scrub={{ theta: spec }} label="angle" />);
	const root = host.querySelector<HTMLElement>('.lab-formula')!;
	const handle = host.querySelector<HTMLElement>('[data-s]')!;

	act(() => void handle.dispatchEvent(ptr('pointerdown', 100)));
	act(() => void root.dispatchEvent(ptr('pointermove', 108))); // +2 steps; this re-renders innerHTML
	expect(val(lesson)).toBe(62);
	expect(host.querySelector('[data-s]')).not.toBe(handle); // the pressed span really was replaced
	act(() => void root.dispatchEvent(ptr('pointermove', 120)));
	expect(val(lesson)).toBe(65);
	act(() => void root.dispatchEvent(ptr('pointermove', 80)));
	expect(val(lesson)).toBe(55);
	act(() => void root.dispatchEvent(ptr('pointerup', 80)));
	act(() => void root.dispatchEvent(ptr('pointermove', 200))); // listeners removed after release
	expect(val(lesson)).toBe(55);
});

test('arrow keys change the value by one step each and focus stays on a handle', () => {
	const lesson = 'fdom-keys';
	lessonStore(lesson, { theta: 60 });
	const host = mount(<Formula lesson={lesson} tex={TEX} scrub={{ theta: spec }} label="angle" />);
	const first = host.querySelector<HTMLElement>('[data-s]')!;
	expect(first.getAttribute('role')).toBe('slider');
	act(() => first.focus());
	expect(document.activeElement).toBe(first);

	for (let i = 0; i < 2; i++) {
		const el = host.querySelector<HTMLElement>('[data-s]')!;
		act(() => void el.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true })));
	}
	expect(val(lesson)).toBe(62);
	const active = document.activeElement as HTMLElement | null;
	expect(active?.dataset.s).toBe('theta');
	expect(host.contains(active)).toBe(true);
	expect(active?.getAttribute('aria-valuenow')).toBe('62');
});

test('hovering a term sets it hot and marks the span', () => {
	const lesson = 'fdom-hot';
	lessonStore(lesson, { theta: 60 });
	const host = mount(<Formula lesson={lesson} tex={TEX} scrub={{ theta: spec }} label="angle" />);
	const term = host.querySelector<HTMLElement>('[data-t="ang"]')!;
	act(() => void term.dispatchEvent(new PointerEvent('pointerover', { bubbles: true })));
	expect(lessonStore(lesson).getState().hot.has('ang')).toBe(true);
	expect(host.querySelector('[data-t="ang"]')!.classList.contains('is-hot')).toBe(true);
});

test('Num drags and key-scrubs through the same store', () => {
	const lesson = 'fdom-num';
	lessonStore(lesson, { theta: 60 });
	const host = mount(<Num lesson={lesson} k="theta" spec={spec} label="angle" />);
	const num = host.querySelector<HTMLElement>('.lab-num')!;
	act(() => void num.dispatchEvent(ptr('pointerdown', 0)));
	act(() => void num.dispatchEvent(ptr('pointermove', 40)));
	expect(val(lesson)).toBe(70);
	act(() => void num.dispatchEvent(ptr('pointerup', 40)));
	act(() => void num.dispatchEvent(new KeyboardEvent('keydown', { key: 'PageDown', bubbles: true, cancelable: true })));
	expect(val(lesson)).toBe(60);
	expect(num.textContent).toBe('60');
});

test('stale keyboard focus is never restored after the user moved away', () => {
	const lesson = 'fdom-stale';
	const s = lessonStore(lesson, { theta: 60 });
	const host = mount(<Formula lesson={lesson} tex={TEX} scrub={{ theta: spec }} label="angle" />);
	act(() => host.querySelector<HTMLElement>('[data-s]')!.focus());
	act(() => void host.querySelector('[data-s]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true })));
	expect((document.activeElement as HTMLElement).dataset.s).toBe('theta'); // restored while focus was inside
	act(() => (document.activeElement as HTMLElement).blur()); // click on blank page
	expect(document.activeElement).toBe(document.body);
	act(() => s.getState().setHot(['x']));
	act(() => s.getState().set('theta', 100));
	expect(host.querySelector('[data-s]')!.getAttribute('aria-valuenow')).toBe('100');
	expect(document.activeElement).toBe(document.body);
});

test('pointerout over non-term spans does not write the store when nothing is hot', () => {
	const lesson = 'fdom-out';
	const s = lessonStore(lesson, { theta: 60 });
	const host = mount(<Formula lesson={lesson} tex={TEX} scrub={{ theta: spec }} label="angle" />);
	const before = s.getState().hot;
	let writes = 0;
	const unsub = s.subscribe(() => writes++);
	act(() => void host.querySelector('.katex-html')!.dispatchEvent(new PointerEvent('pointerout', { bubbles: true })));
	unsub();
	expect(writes).toBe(0);
	expect(s.getState().hot).toBe(before);
});

test('inline fmt/derived/scrub objects do not re-render KaTeX when the parent re-renders', () => {
	const lesson = 'fdom-memo';
	lessonStore(lesson, { theta: 60 });
	const spy = spyOn(texMod, 'renderLinked');
	const calls = () => spy.mock.calls.length;
	let bump = (): void => {};
	function Parent() {
		const [n, setN] = useState(0);
		bump = () => setN(n + 1);
		return (
			<Formula
				lesson={lesson}
				tex={TEX}
				scrub={{ theta: { ...spec } }}
				fmt={{ theta: (v) => `${v}^\\circ` }}
				derived={{ twice: (v) => (v.theta as number) * 2 }}
				label="angle"
			/>
		);
	}
	const host = mount(<Parent />);
	expect(calls()).toBe(1);
	for (let i = 0; i < 3; i++) act(() => bump());
	expect(calls()).toBe(1);
	act(() => lessonStore(lesson).getState().set('theta', 61));
	expect(calls()).toBe(2);
	expect(host.querySelector('[data-s]')!.getAttribute('aria-valuemax')).toBe('180');
	spy.mockRestore();
});

test('a11y: root is a labelled group and handles carry readable aria-valuetext', () => {
	const lesson = 'fdom-a11y';
	lessonStore(lesson, { theta: 60 });
	const fmt = { theta: (v: number) => `${v}^\\circ` };
	const host = mount(<Formula lesson={lesson} tex={TEX} scrub={{ theta: spec }} fmt={fmt} label="angle" />);
	const root = host.querySelector('.lab-formula')!;
	expect(root.getAttribute('role')).toBe('group');
	expect(root.getAttribute('aria-label')).toBe('angle');
	expect(host.querySelector('[data-s]')!.getAttribute('aria-valuetext')).toBe('60°');
});

test('a cancelled scrub (pointercancel, e.g. the page took the swipe) restores the start value: Formula and Num share the loop', () => {
	const lesson = 'fdom-cancel';
	lessonStore(lesson, { theta: 60 });
	const host = mount(
		<>
			<Formula lesson={lesson} tex={TEX} scrub={{ theta: spec }} label="angle" />
			<Num lesson={lesson} k="theta" spec={spec} label="theta" />
		</>,
	);
	const root = host.querySelector<HTMLElement>('.lab-formula')!;
	act(() => void host.querySelector('[data-s]')!.dispatchEvent(ptr('pointerdown', 100)));
	act(() => void root.dispatchEvent(ptr('pointermove', 120)));
	expect(val(lesson)).toBe(65);
	act(() => void root.dispatchEvent(ptr('pointercancel', 120)));
	expect(val(lesson)).toBe(60);
	const num = host.querySelector<HTMLElement>('.lab-num')!;
	act(() => void num.dispatchEvent(ptr('pointerdown', 0)));
	act(() => void num.dispatchEvent(ptr('pointermove', -40)));
	expect(val(lesson)).toBe(50);
	act(() => void num.dispatchEvent(ptr('pointercancel', -40)));
	expect(val(lesson)).toBe(60);
	act(() => void num.dispatchEvent(ptr('pointerup', -40)));
	expect(val(lesson)).toBe(60);
});
