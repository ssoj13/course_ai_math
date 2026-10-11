// DOM parts of Task 6e: eased writes (Ruling U), custom handle mappings through the keyboard, the N-panel checkbox
// row and eased typed values, chip widths, the Ledger and the Challenge box.
import './dom';
import { afterEach, expect, test } from 'bun:test';
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { challengeKey, normalizeSolved, readSolved } from '../../src/lib/challenges';
import { Challenge } from '../../src/viz/Challenge';
import { Ledger } from '../../src/viz/Ledger';
import { createStage, DragKeys, writeMove } from '../../src/viz/scene/handles';
import { NPanel, type Row } from '../../src/viz/scene/NPanel';
import { chipBox } from '../../src/viz/scene/chips';
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
afterEach(() => {
	for (const r of roots.splice(0)) act(() => r.unmount());
	document.body.innerHTML = '';
});
const fire = (el: Element, ev: Event) => act(() => void el.dispatchEvent(ev));
const key = (el: Element, k: string) => {
	const ev = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true });
	fire(el, ev);
	return ev.defaultPrevented;
};
const wait = (ms: number) => act(() => new Promise<void>((r) => setTimeout(r, ms)));
const st = (l: string) => lessonStore(l).getState();

test('an eased write: the value jumps, the drawn value springs to it without overshoot; a cut moves both', async () => {
	const s = lessonStore('ease1', { x: 0, v: [0, 0] });
	s.getState().set('x', 1, 'ease');
	s.getState().set('v', [2, -2], 'ease');
	expect(st('ease1').vals.x).toBe(1);
	expect(st('ease1').shown.x).toBe(0);
	const seen: number[] = [];
	const off = s.subscribe((n) => seen.push(n.shown.x as number));
	await wait(90);
	const mid = st('ease1').shown.x as number;
	expect(mid).toBeGreaterThan(0.3);
	expect(mid).toBeLessThan(1);
	await wait(400);
	off();
	expect(st('ease1').shown.x).toBe(1); // lands exactly
	expect(st('ease1').shown.v).toEqual([2, -2]);
	expect(Math.max(...seen)).toBeLessThanOrEqual(1); // critically damped: never past the target
	s.getState().set('x', 5);
	expect(st('ease1').shown.x).toBe(5);
});

test('writeMove writes numbers and vectors, skips unchanged keys, and passes the motion through', () => {
	lessonStore('wm', { a: 3, v: [1, 2] });
	let writes = 0;
	const off = lessonStore('wm').subscribe(() => writes++);
	writeMove('wm', { a: 3, v: [1, 2] });
	expect(writes).toBe(0);
	writeMove('wm', { a: 4, v: [1, 3] }, 'ease');
	off();
	expect(st('wm').vals).toEqual({ a: 4, v: [1, 3] });
	expect(st('wm').shown.a).toBe(3); // still easing
});

test('a handle with `move` maps the key step through it (src "key") and eases the scene', () => {
	const lesson = 'mv-key';
	lessonStore(lesson, { r: 2, deg: 0 });
	const reg = createStage('2d');
	reg.project = (p) => [p[0] * 10, -p[1] * 10];
	const calls: string[] = [];
	reg.add({
		id: 'A:pt',
		owner: 'A',
		label: 'Move point A',
		lesson,
		keys: ['r', 'deg'],
		at: (v) => [(v.r as number) * Math.cos(((v.deg as number) * Math.PI) / 180), (v.r as number) * Math.sin(((v.deg as number) * Math.PI) / 180)],
		move: (v, raw, src) => {
			calls.push(`${src}:${raw.join()}`);
			return { set: { r: (v.r as number) + raw[0], deg: (v.deg as number) + 10 * raw[1] } };
		},
	});
	const host = mount(<DragKeys reg={reg} />);
	const b = host.querySelector('[aria-label="Move point A"]')!;
	expect(key(b, 'ArrowRight')).toBe(true);
	expect(key(b, 'ArrowUp')).toBe(true);
	expect(calls).toEqual(['key:0.1,0', 'key:0,0.1']);
	expect(st(lesson).vals.r).toBeCloseTo(2.1, 12);
	expect(st(lesson).vals.deg).toBeCloseTo(1, 12);
	expect(st(lesson).shown.r).toBe(2); // the glyph eases; the value is already there
	expect(key(b, 'PageUp')).toBe(false); // a 2D handle has no z
});

function panel(lesson: string, rows: readonly Row[]) {
	return mount(<NPanel lesson={lesson} rows={rows} reg={createStage('2d', { lesson, id: 'c', overlays: [] })} />);
}

test('drag path: only the keys a mapping marks as caught ease, the rest is 1:1; writes are clamped to the card ranges', () => {
	const lesson = 'mv-drag';
	lessonStore(lesson, { lb: 4, lth: 58, v: [0, 0] });
	const reg = createStage('2d', { lesson, id: 'c', overlays: [], ranges: { v: { min: -4, max: 4 }, lb: { min: 0.5, max: 4 } } });
	const d = {
		id: 'A:pt',
		owner: 'A',
		label: 'Move point A',
		lesson,
		keys: ['lb', 'lth'],
		at: () => [0, 0],
		move: () => ({ set: { lb: 3.5, lth: 60 }, ease: ['lth'] }),
	};
	reg.apply(d, st(lesson).vals, [1, 0], 'drag');
	expect(st(lesson).vals.lth).toBe(60);
	expect(st(lesson).shown.lth).toBe(58); // the caught angle glides
	expect(st(lesson).shown.lb).toBe(3.5); // the length follows the pointer
	// Grid moves are clamped to the range a field of the same key allows (a zoomed-out drag cannot reach -10).
	const v = { id: 'v:tip', owner: 'v', label: 'Move tip of v', lesson, keys: ['v'], at: (x: Record<string, unknown>) => x.v as number[] };
	reg.apply(v, st(lesson).vals, [-10, 2], 'drag');
	expect(st(lesson).vals.v).toEqual([-4, 2]);
	expect(st(lesson).shown.v).toEqual([-4, 2]);
});

test('reduced motion: an eased write is a cut', () => {
	const mm = globalThis.matchMedia;
	globalThis.matchMedia = ((q: string) => ({ matches: q.includes('prefers-reduced-motion'), media: q })) as unknown as typeof matchMedia;
	try {
		const s = lessonStore('ease-rm', { x: 0 });
		s.getState().set('x', 1, 'ease');
		expect(st('ease-rm').shown.x).toBe(1);
	} finally {
		globalThis.matchMedia = mm;
	}
});

test('a bool row is a checkbox writing 0/1; it throws on a non 0/1 value', () => {
	lessonStore('np-bool', { flip: 0, bad: 2 });
	const host = panel('np-bool', [{ id: 'flip', label: 'Elbow down', kind: 'bool', key: 'flip' }]);
	const box = host.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
	expect(box.getAttribute('aria-label')).toBe('Elbow down');
	expect(box.checked).toBe(false);
	act(() => box.click());
	expect(st('np-bool').vals.flip).toBe(1);
	expect(host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.checked).toBe(true);
	act(() => host.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
	expect(st('np-bool').vals.flip).toBe(0);
	const err = console.error;
	console.error = () => {};
	try {
		expect(() => panel('np-bool', [{ id: 'bad', label: 'bad', kind: 'bool', key: 'bad' }])).toThrow(/0 or 1/);
	} finally {
		console.error = err;
	}
});

test('N-panel: arrow keys and typed values ease the scene, the field shows the exact value at once', () => {
	lessonStore('np-ease', { a: 3 });
	const host = panel('np-ease', [{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: { min: 0.5, max: 4, step: 0.1 } }]);
	const f = host.querySelector('[role="slider"]')!;
	key(f, 'ArrowRight');
	expect(st('np-ease').vals.a).toBe(3.1);
	expect(st('np-ease').shown.a).toBe(3);
	expect(f.getAttribute('aria-valuetext')).toBe('3.10');
	key(f, 'Enter');
	const input = host.querySelector('input')!;
	input.value = '2';
	key(input, 'Enter');
	expect(st('np-ease').vals.a).toBe(2);
	expect(st('np-ease').shown.a).not.toBe(2);
});

test('rows carry the chip widths of the model as CSS variables', () => {
	lessonStore('np-chip', { v: [1, 2, 3] });
	const row: Row = { id: 'v', label: 'v', kind: 'vec', key: 'v', dims: 3, spec: { min: -3, max: 3, step: 0.1 } };
	const host = panel('np-chip', [row]);
	const el = host.querySelector<HTMLElement>('.lab-np-row')!;
	const box = chipBox(row);
	expect(el.style.getPropertyValue('--np-lw')).toBe(`${box.label}px`);
	expect(el.style.getPropertyValue('--np-fw')).toBe(`${box.field}px`);
	const err = console.error;
	console.error = () => {};
	try {
		expect(() => panel('np-chip', [{ ...row, dims: 2 }])).toThrow(/dims 2/);
	} finally {
		console.error = err;
	}
});

test('Ledger: bars, a hatched negative band, the zero line, the rule; hover lights the id', () => {
	lessonStore('ledger', { a: 3, b: 4, th: 60 });
	const corr = (v: Record<string, unknown>) => -2 * (v.a as number) * (v.b as number) * (v.th === 90 ? 0 : Math.cos(((v.th as number) * Math.PI) / 180));
	const host = mount(
		<Ledger
			lesson="ledger"
			label="Law of cosines"
			scale={3.4}
			max={64}
			zeroLabel="= Pythagoras"
			items={[
				{ id: 'a2', label: 'a²', role: 'vecA', value: (v) => (v.a as number) ** 2 },
				{ id: 'b2', label: 'b²', role: 'vecB', value: (v) => (v.b as number) ** 2 },
				{ id: 'corr', label: 'corr', role: 'vecC', value: corr },
			]}
			total={{ id: 'c2', label: 'c²', value: (v) => (v.a as number) ** 2 + (v.b as number) ** 2 + corr(v) }}
		/>,
	);
	const svg = host.querySelector('svg')!;
	expect(svg.getAttribute('height')).toBe(String(64 * 3.4 + 16));
	const rect = (id: string) => host.querySelector(`g[data-id="${id}"] rect`);
	expect(rect('corr')!.getAttribute('fill')).toMatch(/^url\(#/); // negative: hatched
	expect(host.textContent).toContain('↓ corr −12.00');
	expect(host.textContent).toContain('c² = 13.00');
	act(() => lessonStore('ledger').getState().set('th', 90));
	expect(rect('corr')).toBeNull();
	expect(host.textContent).toContain('corr 0.00 = Pythagoras');
	act(() => lessonStore('ledger').getState().set('th', 120));
	expect(rect('corr')!.getAttribute('fill')).not.toMatch(/^url/);
	expect(host.textContent).toContain('↑ corr 12.00');
	fire(host.querySelector('g[data-id="a2"]')!, new PointerEvent('pointerover', { bubbles: true }));
	fire(host.querySelector('g[data-id="a2"]')!, new PointerEvent('pointerenter', { bubbles: false }));
	expect([...st('ledger').hot]).toEqual(['a2']);
	expect(rect('a2')!.getAttribute('stroke')).toBe('var(--viz-hot)');
});

test('Challenge: live status, sticky success, progress key, and step-based checks via `<timeline>:max`', async () => {
	lessonStore('chal', { th: 60 });
	const host = mount(
		<>
			<Challenge lesson="chal" n={1} text="Make it 90." hint={['Drag A.']} success="Pythagoras." check={(v) => v.th === 90} />
			<Challenge lesson="chal" n={2} text="Find the step." check={(_, step) => (step['proof:max'] ?? 0) >= 6} />
		</>,
	);
	const status = () => [...host.querySelectorAll('.lab-ch-status')].map((e) => e.textContent);
	expect(status()).toEqual(['not yet', 'not yet']);
	expect(host.querySelector('summary')!.textContent).toBe('Hint');
	act(() => lessonStore('chal').getState().set('th', 90));
	expect(status()[0]).toBe('✓ done');
	expect(host.textContent).toContain('Pythagoras.');
	act(() => lessonStore('chal').getState().set('th', 45));
	expect(status()[0]).toBe('✓ done'); // sticky
	expect(readSolved()[challengeKey('chal', 1)]).toBe(true);
	act(() => lessonStore('chal').getState().setStep('proof:max', 6));
	expect(status()[1]).toBe('✓ done');
	expect(normalizeSolved({ 'challenge:x:1': true, 'challenge:x:y': true, 'other': true, 'challenge:x:2': 1 })).toEqual({ 'challenge:x:1': true });
});
