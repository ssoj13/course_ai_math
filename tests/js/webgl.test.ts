import { expect, test } from 'bun:test';
import { webglAvailable } from '../../src/viz/scene/webgl';
test('false when no context can be created', () => {
	expect(webglAvailable(() => ({ getContext: () => null }) as unknown as HTMLCanvasElement)).toBe(false);
});
test('true when webgl2 is available', () => {
	expect(webglAvailable(() => ({ getContext: (k: string) => (k === 'webgl2' ? {} : null) }) as unknown as HTMLCanvasElement)).toBe(true);
});
