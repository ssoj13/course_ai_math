/**
 * Frame-clock stubs for tests. bun runs every test file in ONE process (in an OS-dependent order), so a file that
 * replaces requestAnimationFrame / cancelAnimationFrame and never puts them back changes the frame source of every
 * later file: a leftover no-op stub froze the eased handle ring of handles-dom in CI order. Every stub goes through
 * these helpers, which always restore what was there before (including "nothing", for pure tests that run before any
 * DOM file); `frames-guard.test.ts` fails on a direct assignment anywhere else under tests/.
 *
 * DOM-free on purpose (pure store tests stub frames too); `./dom` re-exports it for component tests.
 */
import { afterEach, beforeEach } from 'bun:test';

export type Raf = (cb: FrameRequestCallback) => number;
export type Caf = (id: number) => void;
export interface FramePair {
	raf: Raf;
	caf?: Caf;
}

const KEYS = ['requestAnimationFrame', 'cancelAnimationFrame'] as const;

/** Installs `raf`/`caf` now; the returned function restores the previous pair exactly (call it in `finally`). */
export function stubFrames(raf: Raf, caf: Caf = () => {}): () => void {
	const g = globalThis as unknown as Record<string, unknown>;
	const saved = KEYS.map((k) => ({ k, own: Object.hasOwn(g, k), v: g[k] }));
	g.requestAnimationFrame = raf;
	g.cancelAnimationFrame = caf;
	return () => {
		for (const s of saved) {
			if (s.own) g[s.k] = s.v;
			else delete g[s.k];
		}
	};
}

/**
 * Stub for every test of the calling file (or describe block): `make()` builds a fresh pair before each test, the
 * previous pair is restored after each. Call it at the top level of the file, before the file's own hooks.
 */
export function useFrameStub(make: () => FramePair): void {
	let restore: (() => void) | undefined;
	beforeEach(() => {
		const { raf, caf } = make();
		restore = stubFrames(raf, caf);
	});
	afterEach(() => {
		restore?.();
		restore = undefined;
	});
}

/**
 * Captures only the frames requested from source files matching `from` (judged by the call stack) into `frames`, to be
 * run or counted by the test; every other caller keeps the clock installed before. Animations that earlier files left
 * running (lesson store eases on the real clock) must neither stall in the capture nor pollute it: an assertion like
 * "the reveal was cancelled, nothing is pending" then counts only the code under test. Captured ids are negative, so
 * they never collide with ids of the previous clock. Call `restore` in `finally`.
 */
export function captureFrames(from: RegExp): { frames: Map<number, FrameRequestCallback>; restore: () => void } {
	const prev = { raf: globalThis.requestAnimationFrame, caf: globalThis.cancelAnimationFrame };
	const frames = new Map<number, FrameRequestCallback>();
	let next = 0;
	const restore = stubFrames(
		(cb) => {
			if (from.test(new Error().stack ?? '')) {
				frames.set(--next, cb);
				return next;
			}
			if (typeof prev.raf !== 'function') throw new Error('captureFrames: no previous frame clock for a foreign caller');
			return prev.raf(cb);
		},
		(id) => {
			if (!frames.delete(id)) prev.caf?.(id);
		},
	);
	return { frames, restore };
}

/** A real (timer-driven, ~60 Hz) frame clock, for tests that need eased values to actually arrive. */
export const timerFrames = (): FramePair => ({
	raf: (cb) => setTimeout(() => cb(performance.now()), 16) as unknown as number,
	caf: (id) => clearTimeout(id),
});
