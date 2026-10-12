import { expect, test } from 'bun:test';
import { hotOff, hotOn, lessonStore } from '../../src/viz/store';

// Test-only store ids: a real lesson id would be created here without the lesson's links when this file runs first,
// and the lesson module's own lessonStore(id, init, links) would then throw (bun shares one registry across files).

test('one store per lesson id, shared by every caller (islands share it via the module singleton)', () => {
	const a = lessonStore('vs-one', { theta: 60 });
	const b = lessonStore('vs-one', { theta: 999 }); // later init must not overwrite
	expect(a).toBe(b);
	expect(a.getState().vals.theta).toBe(60);
	expect(lessonStore('vs-other')).not.toBe(a);
});

test('init merges keys that are missing (two islands may declare different defaults)', () => {
	lessonStore('vs-merge', { u1: [1, 0] });
	const s = lessonStore('vs-merge', { u2: [0, 1] });
	expect(s.getState().vals).toEqual({ u1: [1, 0], u2: [0, 1] });
});

test('set, hot and step', () => {
	const s = lessonStore('vs-length');
	s.getState().set('v', [3, 4]);
	s.getState().setHot(['norm', 'square']);
	s.getState().setStep('derive', 2);
	expect(s.getState().vals.v).toEqual([3, 4]);
	expect([...s.getState().hot].sort()).toEqual(['norm', 'square']);
	expect(s.getState().step.derive).toBe(2);
});

test('setStep clamps negative to 0', () => {
	// A test-only id: lesson ids are global singletons, and 'vector' is a real lesson that registers links on first init.
	const s = lessonStore('test-vector');
	s.getState().setStep('t', -3);
	expect(s.getState().step.t).toBe(0);
});

test('setStep maps a non-finite index (NaN, Infinity) to 0', () => {
	const s = lessonStore('vector-nonfinite');
	s.getState().setStep('t', Number.NaN);
	expect(s.getState().step.t).toBe(0);
	s.getState().setStep('t', Number.POSITIVE_INFINITY);
	expect(s.getState().step.t).toBe(0);
});

test('init seeds the initial state at creation (server snapshots see it)', () => {
	expect(lessonStore('seeded', { a: 1 }).getInitialState().vals.a).toBe(1);
});

test('hotOn/hotOff write only when the hot set actually changes', () => {
	const s = lessonStore('hot-guard');
	let writes = 0;
	const off = s.subscribe(() => writes++);
	hotOn('hot-guard', 'a');
	hotOn('hot-guard', 'a'); // already exactly {a}: no write
	expect([...s.getState().hot]).toEqual(['a']);
	hotOff('hot-guard', 'b'); // not lit: no write
	hotOff('hot-guard', 'a');
	hotOff('hot-guard', 'a');
	expect(s.getState().hot.size).toBe(0);
	expect(writes).toBe(2);
	off();
});
