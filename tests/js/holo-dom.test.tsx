import './dom';
import { expect, test } from 'bun:test';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { createStage, cell } from '../../src/viz/scene/handles';
import { lessonStore } from '../../src/viz/store';
import { Instruments } from '../../src/viz/scene/Instruments';

/** Mounts the instruments of a card whose camera feed reports a 600 x 400 stage. */
function mount(ui: (reg: ReturnType<typeof createStage>) => React.ReactNode, mode: '2d' | '3d', id: string) {
	const reg = createStage(mode, { lesson: id, id, overlays: [{ id: 'instruments', label: 'Instruments', on: true }] });
	reg.view.set({ w: 600, h: 400, unitPx: 40, azimuth: 30, ticks: [{ v: 1, y: 160 }, { v: 0, y: 200 }, { v: -1, y: 240 }] });
	const el = document.createElement('div');
	document.body.append(el);
	const root = createRoot(el);
	act(() => root.render(ui(reg)));
	return {
		el,
		reg,
		done: () => {
			act(() => root.unmount());
			el.remove();
		},
	};
}

test('dial and readout follow the store value; the overlay flag hides the instruments', () => {
	const store = lessonStore('hud-test', { theta: 60 });
	const m = mount((reg) => <Instruments reg={reg} lesson="hud-test" angleKey="theta" />, '2d', 'hud-test');
	expect(m.el.querySelector('[aria-label="Angle 60.0 degrees"]')).not.toBeNull();
	act(() => store.getState().set('theta', 91));
	expect(m.el.querySelector('[aria-label="Angle 91.0 degrees"]')).not.toBeNull();
	expect(m.el.querySelector('output')?.textContent).toBe('91.0°');
	act(() => store.getState().set('ov:hud-test:instruments', 0));
	expect(m.el.querySelector<HTMLElement>('.lab-instruments')?.hidden).toBe(true);
	m.done();
});

test('a 2D card without an angle has no needle and no azimuth; 3D reads the azimuth', () => {
	lessonStore('hud-2d', {});
	const flat = mount((reg) => <Instruments reg={reg} lesson="hud-2d" />, '2d', 'hud-2d');
	expect(flat.el.querySelectorAll('.lab-hud-hot').length).toBe(0);
	expect(flat.el.querySelector('output')).toBeNull();
	flat.done();
	lessonStore('hud-3d', {});
	const deep = mount((reg) => <Instruments reg={reg} lesson="hud-3d" />, '3d', 'hud-3d');
	expect(deep.el.querySelector('output')?.textContent).toBe('30.0°');
	expect(deep.el.querySelectorAll('.lab-hud-hot').length).toBe(2);
	deep.done();
});

test('ruler labels are the projected world units; clay shows no space furniture', () => {
	lessonStore('hud-clay', {});
	const holo = mount((reg) => <Instruments reg={reg} lesson="hud-clay" />, '2d', 'hud-holo');
	expect([...holo.el.querySelectorAll('.lab-hud-ruler text')].map((t) => t.textContent)).toEqual(['1', '0', '-1']);
	expect(holo.el.querySelectorAll('.lab-hud-galaxy').length).toBe(2);
	expect(holo.el.querySelectorAll('.lab-hud-planet').length).toBe(1);
	holo.done();
	const clay = mount((reg) => <Instruments reg={reg} lesson="hud-clay" look="clay" />, '2d', 'hud-clay');
	expect(clay.el.querySelectorAll('.lab-hud-galaxy').length).toBe(0);
	expect(clay.el.querySelectorAll('.lab-hud-planet').length).toBe(0);
	expect(clay.el.querySelectorAll('.lab-hud-ruler').length).toBe(1);
	clay.done();
});

test('a cell publishes only replaced snapshots', () => {
	const c = cell({ n: 1 });
	let calls = 0;
	const off = c.subscribe(() => calls++);
	const same = c.get();
	c.set(same);
	expect(calls).toBe(0);
	c.set({ n: 2 });
	expect(calls).toBe(1);
	expect(c.get().n).toBe(2);
	off();
});
