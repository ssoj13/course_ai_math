import { expect, test } from 'bun:test';
import { hitHandle } from '../../src/viz/scene/drag';
test('a miss returns null so the page can scroll', () => {
	expect(hitHandle([100, 100], [{ id: 'a', at: [200, 200] }])).toBeNull();
});
test('nearest handle within radius wins', () => {
	expect(hitHandle([100, 100], [{ id: 'a', at: [110, 100] }, { id: 'b', at: [104, 100] }])).toBe('b');
});
test('empty handle list is a miss', () => {
	expect(hitHandle([0, 0], [])).toBeNull();
});
test('radius is inclusive and configurable; exact ties keep the first handle', () => {
	expect(hitHandle([0, 0], [{ id: 'a', at: [14, 0] }])).toBe('a');
	expect(hitHandle([0, 0], [{ id: 'a', at: [15, 0] }])).toBeNull();
	expect(hitHandle([0, 0], [{ id: 'a', at: [15, 0] }], 20)).toBe('a');
	expect(hitHandle([0, 0], [{ id: 'a', at: [5, 0] }, { id: 'b', at: [-5, 0] }])).toBe('a');
});
