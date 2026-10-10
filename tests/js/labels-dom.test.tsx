import './dom';
import { afterEach, expect, test } from 'bun:test';
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { createStage, Labels, placeLabels, StageCtx, useLabel, type StageReg } from '../../src/viz/scene/handles';
import { Label } from '../../src/viz/scene/Label';
import { lessonStore } from '../../src/viz/store';

const roots: Root[] = [];

function mount(el: ReactElement): Root {
	const host = document.createElement('div');
	document.body.appendChild(host);
	const root = createRoot(host);
	roots.push(root);
	act(() => root.render(el));
	return root;
}

afterEach(() => {
	for (const r of roots.splice(0)) act(() => r.unmount());
	document.body.innerHTML = '';
});

/** Projection stub: 1 unit = 10 px, origin at (100, 100), y up. */
function stage(): StageReg {
	const reg = createStage('2d');
	reg.project = (p) => [100 + p[0] * 10, 100 - p[1] * 10];
	return reg;
}

/** Scene half (where Label lives, inside the View in the app) and overlay half, as two separate React roots. */
function setup(reg: StageReg, scene: ReactElement): Root {
	mount(<Labels reg={reg} />);
	return mount(<StageCtx.Provider value={reg}>{scene}</StageCtx.Provider>);
}

const labels = () => [...document.querySelectorAll<HTMLElement>('.lab-label')];

test('a Label renders as DOM text in the overlay, hidden until placed, then at the projected point plus offset', () => {
	const S = lessonStore('lbl1', { a: [1, 2] });
	const reg = stage();
	setup(reg, <Label lesson="lbl1" at="a" offset={[5, -5]}>a</Label>);
	const [el] = labels();
	expect(el.textContent).toBe('a');
	expect(el.closest('.lab-labels')!.getAttribute('aria-hidden')).toBe('true');
	expect(el.style.visibility).not.toBe('visible');
	placeLabels(reg);
	expect(el.style.visibility).toBe('visible');
	expect(el.style.transform).toBe('translate(115px, 75px)');
	// A store write moves the label on the next placement (the frame the write invalidates).
	act(() => S.getState().set('a', [2, 0]));
	placeLabels(reg);
	expect(el.style.transform).toBe('translate(125px, 95px)');
});

test('text, role color and hot state update in place; unmount removes the label', () => {
	const S = lessonStore('lbl2', { p: [0, 0] });
	const reg = stage();
	const root = setup(reg, <Label lesson="lbl2" at="p" role="vecA" id="p">P</Label>);
	const [el] = labels();
	expect(el.style.getPropertyValue('--lab-label-c')).toBe('var(--viz-vec-a)');
	expect(el.classList.contains('is-hot')).toBe(false);
	act(() => S.getState().setHot(['p']));
	expect(el.classList.contains('is-hot')).toBe(true);
	act(() => root.render(<StageCtx.Provider value={reg}><Label lesson="lbl2" at="p" role="vecA" id="p">{1.5}</Label></StageCtx.Provider>));
	expect(labels()).toEqual([el]);
	expect(el.textContent).toBe('1.5');
	act(() => root.render(<StageCtx.Provider value={reg}>{null}</StageCtx.Provider>));
	expect(labels()).toEqual([]);
	expect(reg.labels.list()).toEqual([]);
});

test('literal positions work; no projection yet or an off-stage point hides the label', () => {
	const reg = createStage('2d');
	setup(reg, <Label lesson="lbl3" at={[1, 1]}>x</Label>);
	const [el] = labels();
	placeLabels(reg); // DragLayer has not bound the camera yet
	expect(el.style.visibility).toBe('hidden');
	reg.project = () => null; // e.g. behind the camera
	placeLabels(reg);
	expect(el.style.visibility).toBe('hidden');
	reg.project = (p) => [p[0], p[1]];
	placeLabels(reg);
	expect(el.style.visibility).toBe('visible');
	expect(el.style.transform).toBe('translate(1px, 1px)');
});

function ArcLike({ at }: { at: number[] | null }) {
	useLabel({ lesson: 'lbl4', text: '90', at: () => at, away: () => [0, 0], gap: 10 });
	return null;
}

test('useLabel: `away` pushes the label `gap` px from a second point; a null anchor hides it (degenerate data)', () => {
	const reg = stage();
	const root = setup(reg, <ArcLike at={[3, 0]} />);
	const [el] = labels();
	placeLabels(reg);
	// (3, 0) -> (130, 100); away from the origin (100, 100) by 10 px -> (140, 100).
	expect(el.style.transform).toBe('translate(140px, 100px)');
	act(() => root.render(<StageCtx.Provider value={reg}><ArcLike at={null} /></StageCtx.Provider>));
	placeLabels(reg);
	expect(el.style.visibility).toBe('hidden');
});

test('authoring errors are loud: a Label outside a Stage, a bad store key', () => {
	lessonStore('lbl5', { s: 2 });
	const err = console.error;
	console.error = () => {};
	try {
		expect(() => mount(<Label lesson="lbl5" at={[0, 0]}>x</Label>)).toThrow(/inside <Stage>/);
		expect(() => mount(<StageCtx.Provider value={stage()}><Label lesson="lbl5" at="s">x</Label></StageCtx.Provider>)).toThrow(/"s"/);
	} finally {
		console.error = err;
	}
});
