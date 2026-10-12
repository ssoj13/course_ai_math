/**
 * Guard for the shared frame clock (see frames.ts): no test may replace requestAnimationFrame / cancelAnimationFrame
 * except through `stubFrames` / `useFrameStub`, which always restore the originals.
 *
 * Why a source scan and not a runtime check: bun runs all files in one process and has no per-file hook short of a
 * preload, and a preload would put happy-dom into the pure tests. A runtime check from one file could only see the
 * state left by whichever files happened to run before it (the order differs between machines), so it would pass
 * locally and miss the leak in CI. The scan catches every direct write, in every file, in any order.
 */
import { expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = join(import.meta.dir, '..');
/** The helper itself, and this file (its self-test below spells the forbidden forms out). */
const OWNERS = new Set([join(import.meta.dir, 'frames.ts'), import.meta.path]);
/** Assignment, defineProperty or spyOn of either frame function, with any receiver (globalThis, window, ...). */
const WRITE = /\b(?:requestAnimationFrame|cancelAnimationFrame)\s*=(?!=)|(?:defineProperty|spyOn)\([^)]*['"](?:requestAnimationFrame|cancelAnimationFrame)['"]/;

function files(dir: string): string[] {
	return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
		const p = join(dir, e.name);
		return e.isDirectory() ? files(p) : /\.(ts|tsx|js|mjs)$/.test(e.name) ? [p] : [];
	});
}

test('only frames.ts writes requestAnimationFrame / cancelAnimationFrame', () => {
	const offenders = files(ROOT)
		.filter((f) => !OWNERS.has(f))
		.flatMap((f) => readFileSync(f, 'utf8').split('\n').map((line, i) => ({ f, i, line })))
		.filter(({ line }) => WRITE.test(line))
		.map(({ f, i }) => `${relative(ROOT, f)}:${i + 1}`);
	expect(offenders).toEqual([]);
});

test('the guard pattern sees the forms it forbids', () => {
	for (const bad of [
		'globalThis.requestAnimationFrame = () => 1;',
		'window.cancelAnimationFrame=()=>{}',
		"Object.defineProperty(globalThis, 'requestAnimationFrame', { value: f })",
		"spyOn(globalThis, 'cancelAnimationFrame')",
	])
		expect(WRITE.test(bad)).toBe(true);
	for (const ok of ['const raf = globalThis.requestAnimationFrame;', 'if (globalThis.requestAnimationFrame === f) {}'])
		expect(WRITE.test(ok)).toBe(false);
});
