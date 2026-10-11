import './dom';
import { afterEach, beforeEach, expect, test } from 'bun:test';
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { createStage } from '../../src/viz/scene/handles';
import { NPanel, type Row } from '../../src/viz/scene/NPanel';
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

const ptr = (type: string, clientX: number, o: PointerEventInit = {}) => new PointerEvent(type, { bubbles: true, clientX, pointerId: 1, ...o });
const fire = (el: Element, ev: Event) => act(() => void el.dispatchEvent(ev));
const key = (el: Element, k: string, o: KeyboardEventInit = {}) => {
	const ev = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true, ...o });
	fire(el, ev);
	return ev.defaultPrevented;
};
const vals = (l: string) => lessonStore(l).getState().vals;
const LEN = { min: 0.5, max: 4, step: 0.1 };
const ANG = { min: 0, max: 180, step: 1 };

function panel(lesson: string, rows: readonly Row[]) {
	const reg = createStage('2d', { lesson, id: 'c', overlays: [] });
	const host = mount(<NPanel lesson={lesson} rows={rows} reg={reg} />);
	return { host, reg };
}
const field = (host: HTMLElement, i = 0) => host.querySelectorAll<HTMLElement>('[role="slider"]')[i];

test('a num row is a labelled slider field showing the value in field format', () => {
	lessonStore('np1', { a: 3 });
	const { host } = panel('np1', [{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: LEN }]);
	const f = field(host);
	expect(host.querySelector('.lab-np-label')!.textContent).toBe('a');
	expect(f.getAttribute('aria-label')).toBe('a');
	expect(f.getAttribute('aria-valuenow')).toBe('3');
	expect(f.getAttribute('aria-valuemin')).toBe('0.5');
	expect(f.textContent).toContain('3.00');
});

test('horizontal drag scrubs through the shared scrub engine (4 px per step), Shift = fine from the current value, Ctrl snaps', () => {
	lessonStore('np2', { a: 3, th: 50 });
	const { host } = panel('np2', [
		{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: LEN },
		{ id: 'th', label: 'θ', kind: 'num', key: 'th', spec: ANG, snap: [0, 30, 45, 60, 90] },
	]);
	const f = field(host);
	fire(f, ptr('pointerdown', 100));
	fire(f, ptr('pointermove', 116));
	expect(vals('np2').a).toBe(3.4);
	// Shift mid-drag re-bases: no jump, then 1/10 steps.
	fire(f, ptr('pointermove', 116, { shiftKey: true }));
	expect(vals('np2').a).toBe(3.4);
	fire(f, ptr('pointermove', 136, { shiftKey: true }));
	expect(vals('np2').a).toBe(3.45);
	fire(f, ptr('pointerup', 136));
	expect(field(host).textContent).toContain('3.45');

	const t = field(host, 1);
	fire(t, ptr('pointerdown', 0));
	fire(t, ptr('pointermove', 8, { ctrlKey: true })); // 52 -> nearest target 45
	expect(vals('np2').th).toBe(45);
	fire(t, ptr('pointermove', 40, { ctrlKey: true })); // 60
	expect(vals('np2').th).toBe(60);
	fire(t, ptr('pointerup', 40));
});

test('arrow keys step, PageUp steps x10, both clamp', () => {
	lessonStore('np3', { a: 3.95 });
	const { host } = panel('np3', [{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: LEN }]);
	expect(key(field(host), 'ArrowLeft')).toBe(true);
	expect(vals('np3').a).toBe(3.9);
	key(field(host), 'PageUp');
	expect(vals('np3').a).toBe(4);
	expect(key(field(host), 'a')).toBe(false);
});

test('click without moving opens the text input; Enter commits a valid value (snapped, clamped)', () => {
	lessonStore('np4', { a: 3 });
	const { host } = panel('np4', [{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: LEN }]);
	fire(field(host), ptr('pointerdown', 50));
	fire(field(host), ptr('pointerup', 51));
	const input = host.querySelector<HTMLInputElement>('input.lab-np-input')!;
	expect(input).not.toBeNull();
	expect(input.value).toBe('3.00');
	act(() => {
		input.value = '2,26';
	});
	key(input, 'Enter');
	expect(vals('np4').a).toBe(2.3);
	expect(host.querySelector('input')).toBeNull();
	expect(field(host).textContent).toContain('2.30');
	// Keyboard flow: focus is back on the field.
	expect(document.activeElement).toBe(field(host));
});

test('invalid text is flagged and not written; Escape reverts; Enter on a focused field also opens the input', () => {
	lessonStore('np5', { a: 3 });
	const { host } = panel('np5', [{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: LEN }]);
	key(field(host), 'Enter');
	const input = host.querySelector<HTMLInputElement>('input.lab-np-input')!;
	act(() => {
		input.value = 'abc';
	});
	key(input, 'Enter');
	expect(input.getAttribute('aria-invalid')).toBe('true');
	expect(host.querySelector('.lab-np-err')!.textContent).toContain('not a number');
	expect(vals('np5').a).toBe(3);
	key(input, 'Escape');
	expect(host.querySelector('input')).toBeNull();
	expect(vals('np5').a).toBe(3);
});

test('a vec row has one field per component and writes the whole vector', () => {
	lessonStore('np6', { v: [3, 1] });
	const { host } = panel('np6', [{ id: 'v', label: 'a', kind: 'vec', key: 'v', spec: { min: -5, max: 5, step: 0.1 } }]);
	const fs = host.querySelectorAll<HTMLElement>('[role="slider"]');
	expect(fs.length).toBe(2);
	expect(fs[1].getAttribute('aria-label')).toBe('a y');
	key(fs[1], 'ArrowUp');
	expect(vals('np6').v).toEqual([3, 1.1]);
	expect(host.querySelectorAll('.lab-np-ax').length).toBe(2);
});

test('read-only rows are flat text from a key or a derived value; no slider', () => {
	lessonStore('np7', { a: 3, b: 4 });
	const { host } = panel('np7', [
		{ id: 'c', label: 'c', kind: 'ro', value: (v) => Math.hypot(v.a as number, v.b as number), group: 'Derived' },
		{ id: 'b', label: 'b', kind: 'ro', key: 'b', fmt: (x) => `${x} u` },
	]);
	expect(host.querySelectorAll('[role="slider"]').length).toBe(0);
	const ro = [...host.querySelectorAll('.lab-np-ro')].map((e) => e.textContent);
	expect(ro).toEqual(['5.00', '4 u']);
	expect(host.querySelector('.lab-np-sep')!.textContent).toBe('Derived');
	act(() => lessonStore('np7').getState().set('a', 0));
	expect(host.querySelector('.lab-np-ro')!.textContent).toBe('4.00');
});

test('hover lights the row id (hot) and tells the status bar; an external hot outlines the row', () => {
	lessonStore('np8', { a: 3 });
	const { host, reg } = panel('np8', [{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: LEN }]);
	const row = host.querySelector<HTMLElement>('.lab-np-row')!;
	fire(row, new PointerEvent('pointerover', { bubbles: true }));
	expect(lessonStore('np8').getState().hot.has('a')).toBe(true);
	expect(reg.hover.get().field).toBe('a');
	fire(row, new PointerEvent('pointerout', { bubbles: true }));
	expect(lessonStore('np8').getState().hot.size).toBe(0);
	expect(reg.hover.get().field).toBeNull();
	act(() => lessonStore('np8').getState().setHot(['a']));
	expect(row.classList.contains('is-hot')).toBe(true);
});

test('visible() hides rows; authoring errors throw', () => {
	lessonStore('np9', { a: 3 });
	const { host } = panel('np9', [
		{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: LEN },
		{ id: 'x', label: 'x', kind: 'ro', value: () => 1, visible: (v) => (v.a as number) > 3 },
	]);
	expect(host.querySelectorAll('.lab-np-row').length).toBe(1);
	act(() => lessonStore('np9').getState().set('a', 3.5));
	expect(host.querySelectorAll('.lab-np-row').length).toBe(2);
	expect(() => panel('np9', [{ id: 'a', label: 'a', kind: 'num', key: 'a' }])).toThrow(/spec/);
	expect(() => panel('np9', [{ id: 'r', label: 'r', kind: 'ro' }])).toThrow(/key or value/);
});
