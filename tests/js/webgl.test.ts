import { expect, test } from 'bun:test';
import { webglAvailable } from '../../src/viz/scene/webgl';
test('false when no context can be created', () => {
	expect(webglAvailable(() => ({ getContext: () => null }) as unknown as HTMLCanvasElement)).toBe(false);
});
test('true when webgl2 is available', () => {
	expect(webglAvailable(() => ({ getContext: (k: string) => (k === 'webgl2' ? {} : null) }) as unknown as HTMLCanvasElement)).toBe(true);
});
test('false when getContext throws', () => {
	const make = () => ({ getContext: () => { throw new Error('blocked'); } }) as unknown as HTMLCanvasElement;
	expect(webglAvailable(make)).toBe(false);
});
test('false when the canvas cannot be created', () => {
	expect(webglAvailable(() => { throw new Error('no document'); })).toBe(false);
});
test('true when only webgl1 is available', () => {
	expect(webglAvailable(() => ({ getContext: (k: string) => (k === 'webgl' ? {} : null) }) as unknown as HTMLCanvasElement)).toBe(true);
});
