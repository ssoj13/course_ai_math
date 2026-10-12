import { timerFrames, useFrameStub } from './dom';
import { afterEach, expect, test } from 'bun:test';
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { createStage, DragKeys, type StageReg } from '../../src/viz/scene/handles';
import { lessonStore } from '../../src/viz/store';

const roots: Root[] = [];

function mount(el: ReactElement): HTMLElement {
	const host = document.createElement('div');
	document.body.appendChild(host);
	const root = createRoot(host);
	roots.push(root);
	act(() => root.render(el));
	return host;
}

// The eased ring needs real frames: a timer-driven frame clock of its own, whatever clock the file before left.
useFrameStub(timerFrames);

afterEach(() => {
	for (const r of roots.splice(0)) act(() => r.unmount());
	document.body.innerHTML = '';
});

const key = (el: Element, k: string, shiftKey = false) => {
	const ev = new KeyboardEvent('keydown', { key: k, shiftKey, bubbles: true, cancelable: true });
	act(() => void el.dispatchEvent(ev));
	return ev.defaultPrevented;
};
const vals = (lesson: string) => lessonStore(lesson).getState().vals;

/** A stage with a tip handle on `a` and a whole-vector handle on `b` (from `o`). Projection: 1 unit = 10 px. */
function stage(lesson: string, mode: '2d' | '3d' = '2d'): StageReg {
	const reg = createStage(mode);
	reg.project = (p) => [p[0] * 10, -p[1] * 10];
	reg.add({ id: 'a:tip', owner: 'a', label: 'Move tip of a', lesson, keys: ['a'], at: (v) => v.a as number[] });
	reg.add({ id: 'b:whole', owner: 'b', label: 'Move b', lesson, keys: ['o', 'b'], at: (v) => v.b as number[] });
	return reg;
}

test('one labelled button per draggable; arrows move by 0.1, Shift by 1', () => {
	const lesson = 'hdom-keys';
	lessonStore(lesson, { a: [3, 1], o: [0, 0], b: [1, 2] });
	const host = mount(<DragKeys reg={stage(lesson)} />);
	const buttons = [...host.querySelectorAll('button')].map((b) => b.getAttribute('aria-label'));
	expect(buttons).toEqual(['Move tip of a', 'Move b']);

	const a = host.querySelector('[aria-label="Move tip of a"]')!;
	expect(key(a, 'ArrowRight')).toBe(true);
	expect(vals(lesson).a).toEqual([3.1, 1]);
	key(a, 'ArrowUp', true);
	expect(vals(lesson).a).toEqual([3.1, 2]);
	// PageUp is a z move: no z in 2D, so the page keeps its default (scroll) behaviour.
	expect(key(a, 'PageUp')).toBe(false);
	expect(key(a, 'Tab')).toBe(false);
	expect(vals(lesson).a).toEqual([3.1, 2]);

	key(host.querySelector('[aria-label="Move b"]')!, 'ArrowLeft');
	expect(vals(lesson).o).toEqual([-0.1, 0]);
	expect(vals(lesson).b).toEqual([0.9, 2]);
});

test('focus lights the owner and shows a ring at the projected handle; blur clears both', async () => {
	const lesson = 'hdom-focus';
	lessonStore(lesson, { a: [3, 1], o: [0, 0], b: [1, 2] });
	const host = mount(<DragKeys reg={stage(lesson)} />);
	const a = host.querySelector<HTMLButtonElement>('[aria-label="Move tip of a"]')!;
	act(() => a.focus());
	expect([...lessonStore(lesson).getState().hot]).toEqual(['a']);
	const ring = host.querySelector<HTMLElement>('.lab-handle-ring')!;
	expect([ring.style.left, ring.style.top]).toEqual(['30px', '-10px']);
	key(a, 'ArrowRight');
	// A key press is an indirect change (Ruling U): the value jumps, the drawn handle (and its ring) eases after it.
	expect(lessonStore(lesson).getState().vals.a).toEqual([3.1, 1]);
	expect(host.querySelector<HTMLElement>('.lab-handle-ring')!.style.left).toBe('30px');
	// Wait for the spring to land instead of a fixed sleep: CI machines run frames slower than a dev box.
	const ringLeft = () => host.querySelector<HTMLElement>('.lab-handle-ring')!.style.left;
	for (let t = 0; t < 3000 && ringLeft() !== '31px'; t += 50) await act(() => new Promise((r) => setTimeout(r, 50)));
	expect(ringLeft()).toBe('31px');
	act(() => a.blur());
	expect(lessonStore(lesson).getState().hot.size).toBe(0);
	expect(host.querySelector('.lab-handle-ring')).toBeNull();
});

test('handles added or removed later update the buttons', () => {
	const lesson = 'hdom-live';
	lessonStore(lesson, { a: [3, 1], o: [0, 0], b: [1, 2], p: [0, 0, 0] });
	const reg = stage(lesson, '3d');
	const host = mount(<DragKeys reg={reg} />);
	let drop = () => {};
	act(() => {
		drop = reg.add({ id: 'p', owner: 'p', label: 'Move point p', lesson, keys: ['p'], at: (v) => v.p as number[] });
	});
	expect(host.querySelectorAll('button').length).toBe(3);
	key(host.querySelector('[aria-label="Move point p"]')!, 'PageDown');
	expect(vals(lesson).p).toEqual([0, 0, -0.1]);
	act(() => drop());
	expect(host.querySelectorAll('button').length).toBe(2);
});

test('pick hits the nearest projected handle within 14 px and misses elsewhere', () => {
	const lesson = 'hdom-pick';
	lessonStore(lesson, { a: [3, 1], o: [0, 0], b: [1, 2] });
	const reg = stage(lesson);
	expect(reg.pick(31, -9)?.id).toBe('a:tip');
	expect(reg.pick(10, -20)?.id).toBe('b:whole');
	expect(reg.pick(100, 100)).toBeNull();
	reg.project = null; // camera not bound yet: nothing is grabbable, the page scrolls
	expect(reg.pick(30, -10)).toBeNull();
});
