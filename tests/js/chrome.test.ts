/** Pure parts of the viewport chrome: gizmo layout, status-bar hint resolution and key caps, grid ticks. */
import { expect, test } from 'bun:test';
import { alignSign, gizmoLayout, GIZMO_ID } from '../../src/viz/scene/Gizmo';
import { DEFAULT_HINTS, hintFor, hintTokens } from '../../src/viz/scene/StatusBar';
import { tickText, tickValues } from '../../src/viz/scene/Grid';

const key = (h: { axis: number; sign: number }) => `${h.sign > 0 ? '+' : '-'}${'xyz'[h.axis]}`;
const at = (lay: ReturnType<typeof gizmoLayout>, k: string) => lay.find((h) => key(h) === k)!;

test('gizmoLayout: identity camera (looking down -z) puts +x right, +y up (screen y down), +z toward the viewer', () => {
	const lay = gizmoLayout([0, 0, 0, 1], 28);
	expect(lay).toHaveLength(6);
	expect([at(lay, '+x').x, at(lay, '+x').y]).toEqual([28, -0]);
	expect(at(lay, '+y').y).toBe(-28);
	expect(at(lay, '+z').z).toBe(1);
	// back to front: the bubble facing the viewer is drawn last
	expect(key(lay[5])).toBe('+z');
	expect(key(lay[0])).toBe('-z');
});

test('gizmoLayout: a camera turned 90 degrees about y (looking down -x) sees +x facing it and +z to the left', () => {
	const s = Math.SQRT1_2;
	const lay = gizmoLayout([0, s, 0, s], 10);
	expect(at(lay, '+x').z).toBeCloseTo(1, 12);
	expect(at(lay, '+z').x).toBeCloseTo(-10, 12);
	expect(key(lay[5])).toBe('+x');
});

test('alignSign: clicking the bubble the view already looks along flips to the opposite side (Blender)', () => {
	expect(alignSign([0, 1, 0], 1, 1)).toBe(-1);
	expect(alignSign([0, -1, 0], 1, -1)).toBe(1);
	expect(alignSign([0.3, 0.9, 0.3], 1, 1)).toBe(1);
	expect(alignSign([1, 0, 0], 2, 1)).toBe(1);
});

const H = (o: Partial<{ handle: string | null; object: string | null; field: string | null }>) => ({ handle: null, object: null, field: null, ...o });

test('hintFor: state first, then field, handle (lesson or built-in), gizmo, object, idle', () => {
	const spec = { hints: { 'a:tip': 'drag a', a: (v: Record<string, unknown>) => `a = ${v.a}` }, idle: 'idle text' };
	const vals = { a: 3 };
	expect(hintFor(H({}), vals, spec, '2d')).toBe('idle text');
	expect(hintFor(H({}), vals, {}, '3d')).toBe(DEFAULT_HINTS.idle3d);
	expect(hintFor(H({ object: 'a' }), vals, spec, '2d')).toBe('a = 3');
	expect(hintFor(H({ object: 'zzz' }), vals, spec, '2d')).toBe('idle text');
	expect(hintFor(H({ object: 'a', handle: 'a:tip' }), vals, spec, '2d')).toBe('drag a');
	expect(hintFor(H({ handle: 'b:tip' }), vals, spec, '3d')).toBe(DEFAULT_HINTS.handle3d);
	expect(hintFor(H({ object: GIZMO_ID }), vals, spec, '3d')).toBe(DEFAULT_HINTS.gizmo);
	expect(hintFor(H({ field: 'a', handle: 'a:tip' }), vals, spec, '2d')).toBe(DEFAULT_HINTS.field);
	expect(hintFor(H({ field: 'a' }), vals, { ...spec, state: (v) => ((v.a as number) > 2 ? 'too far' : null) }, '2d')).toBe('too far');
});

test('hintTokens: mouse and key words become key caps; N only before "panel"', () => {
	expect(hintTokens('LMB/MMB orbit · Shift+MMB pan · N panel · N·L')).toEqual([
		{ text: 'LMB/MMB', key: true },
		{ text: ' orbit · ', key: false },
		{ text: 'Shift+MMB', key: true },
		{ text: ' pan · ', key: false },
		{ text: 'N', key: true },
		{ text: ' panel · N·L', key: false },
	]);
	expect(hintTokens('plain')).toEqual([{ text: 'plain', key: false }]);
});

test('grid ticks: multiples of the step within extent, without 0; integer text or one decimal with a true minus', () => {
	expect(tickValues(2, 1)).toEqual([-2, -1, 1, 2]);
	expect(tickValues(1, 0.5)).toEqual([-1, -0.5, 0.5, 1]);
	expect(tickValues(0.3, 0.1)).toEqual([-0.3, -0.2, -0.1, 0.1, 0.2, 0.3]);
	expect(tickText(-2)).toBe('−2');
	expect(tickText(0.5)).toBe('0.5');
});
