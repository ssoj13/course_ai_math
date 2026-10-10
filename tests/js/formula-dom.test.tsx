import './dom';
import { afterEach, beforeEach, expect, test } from 'bun:test';
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
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
