import { expect, test } from 'bun:test';
import { keyStep, scrubValue } from '../../src/viz/scrub';

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
