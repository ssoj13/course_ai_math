import { expect, test } from 'bun:test';
import { ctrlSnap, fmtField, keyStep, parseField, scrubValue } from '../../src/viz/scrub';

const spec = { min: 0, max: 180, step: 1 };

test('drag maps pixels to steps and clamps', () => {
	expect(scrubValue(60, 40, spec, 4)).toBe(70);
	expect(scrubValue(60, -1000, spec, 4)).toBe(0);
	expect(scrubValue(60, 1000, spec, 4)).toBe(180);
});

test('keys', () => {
	expect(keyStep(60, 'ArrowUp', spec)).toBe(61);
	expect(keyStep(60, 'PageDown', spec)).toBe(50);
	expect(keyStep(0, 'ArrowLeft', spec)).toBe(0);
	expect(keyStep(60, 'a', spec)).toBeNull();
});

test('step snapping avoids float drift', () => {
	expect(scrubValue(0.1, 8, { min: 0, max: 1, step: 0.1 }, 4)).toBe(0.3);
});

const len = { min: 0.5, max: 4, step: 0.1 };

test('parseField: numbers with a true minus, comma decimals, a trailing degree sign; snapped to the step and clamped', () => {
	expect(parseField('2.5', len)).toEqual({ ok: true, v: 2.5 });
	expect(parseField(' 2,34 ', len)).toEqual({ ok: true, v: 2.3 });
	expect(parseField('−1', { min: -5, max: 5, step: 0.1 })).toEqual({ ok: true, v: -1 });
	expect(parseField('60°', spec)).toEqual({ ok: true, v: 60 });
	expect(parseField('+7', len)).toEqual({ ok: true, v: 4 });
	expect(parseField('0', len)).toEqual({ ok: true, v: 0.5 });
	expect(parseField('1e1', len)).toEqual({ ok: true, v: 4 });
});

test('parseField: rejects text that is not a finite number', () => {
	for (const s of ['', '  ', 'abc', '1..2', 'Infinity', 'NaN', '2a', '--1', '0x10']) expect(parseField(s, len).ok).toBe(false);
});

test('ctrlSnap: nearest target from a list, or the nearest multiple of an increment; always within range', () => {
	const T = [0, 30, 45, 60, 90, 120, 135, 150, 180];
	expect(ctrlSnap(52, T, spec)).toBe(45);
	expect(ctrlSnap(53, T, spec)).toBe(60);
	expect(ctrlSnap(2.37, 0.5, len)).toBe(2.5);
	expect(ctrlSnap(0.6, 0.5, len)).toBe(0.5);
	expect(ctrlSnap(3.9, 0.5, len)).toBe(4);
});

test('fmtField: step decimals (at least 2 below 1), a true minus sign, never -0', () => {
	expect(fmtField(3, 0.1)).toBe('3.00');
	expect(fmtField(-1.5, 0.1)).toBe('−1.50');
	expect(fmtField(60, 1)).toBe('60');
	expect(fmtField(0.125, 0.001)).toBe('0.125');
	expect(fmtField(-0.001, 0.1)).toBe('0.00');
	// Between grid steps (Shift-fine drag): one more decimal so the value does not look unchanged.
	expect(fmtField(60.3, 1)).toBe('60.3');
	expect(fmtField(2.37, 0.1)).toBe('2.37');
});
