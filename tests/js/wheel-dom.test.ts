/** Ruling T at the DOM level: the stage's wheel gate in front of real OrbitControls (three's, same wheel logic as drei's). */
import './dom';
import { expect, test } from 'bun:test';
import { PerspectiveCamera } from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { gateWheel, wheelZooms } from '../../src/viz/scene/handles';

function setup() {
	const stage = document.createElement('div');
	const view = document.createElement('div');
	stage.appendChild(view);
	document.body.appendChild(stage);
	const cam = new PerspectiveCamera(40, 1, 0.1, 100);
	cam.position.set(0, 0, 10);
	const controls = new OrbitControls(cam, view);
	const ac = new AbortController();
	gateWheel(stage, ac.signal);
	return { view, cam, controls, done: () => (controls.dispose(), ac.abort(), stage.remove()) };
}
// happy-dom's WheelEvent ignores the modifier fields of its init dict (ctrlKey reads undefined), so set them explicitly.
const wheel = (o: { ctrlKey?: boolean }) => {
	const e = new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: 100 });
	Object.defineProperty(e, 'ctrlKey', { value: o.ctrlKey ?? false });
	Object.defineProperty(e, 'metaKey', { value: false });
	return e;
};

test('wheelZooms: only Ctrl or Cmd', () => {
	expect(wheelZooms({ ctrlKey: false, metaKey: false })).toBe(false);
	expect(wheelZooms({ ctrlKey: true, metaKey: false })).toBe(true);
	expect(wheelZooms({ ctrlKey: false, metaKey: true })).toBe(true);
});

test('a plain wheel reaches the page: not prevented, the camera does not move', () => {
	const s = setup();
	const ev = wheel({});
	s.view.dispatchEvent(ev);
	expect(ev.defaultPrevented).toBe(false);
	expect(s.cam.position.z).toBe(10);
	s.done();
});

test('Ctrl+wheel zooms: prevented (no browser zoom), the camera dollies', () => {
	const s = setup();
	const ev = wheel({ ctrlKey: true });
	s.view.dispatchEvent(ev);
	expect(ev.defaultPrevented).toBe(true);
	expect(s.cam.position.z).not.toBe(10);
	s.done();
});
