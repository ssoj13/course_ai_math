import { beforeEach, expect, test } from 'bun:test';

// No `implements Storage`: the DOM Storage type has an index signature this mock does not need.
class MemStorage {
	#m = new Map<string, string>();
	get length() { return this.#m.size; }
	clear() { this.#m.clear(); }
	getItem(k: string) { return this.#m.get(k) ?? null; }
	key(i: number) { return [...this.#m.keys()][i] ?? null; }
	removeItem(k: string) { this.#m.delete(k); }
	setItem(k: string, v: string) { this.#m.set(k, v); }
}

beforeEach(() => {
	const g = globalThis as Record<string, unknown>;
	// defineProperty, not assignment: once a DOM test file has loaded (bun shares one process), happy-dom's
	// storage globals are read-only accessors and a plain assignment throws.
	const put = (k: string, value: unknown) => Object.defineProperty(g, k, { value, configurable: true, writable: true });
	put('localStorage', new MemStorage());
	put('sessionStorage', new MemStorage());
	put('window', globalThis);
	g.CustomEvent ??= class extends Event { detail: unknown; constructor(t: string, i?: { detail?: unknown }) { super(t); this.detail = i?.detail; } };
});

test('keys are namespaced so another course on the shared origin cannot collide', async () => {
	const { KEYS, load, save } = await import('../../src/lib/store');
	localStorage.setItem('progress', JSON.stringify({ foreign: true }));
	localStorage.setItem('vllm-course:progress', JSON.stringify({ foreign: true }));
	expect(load(KEYS.progress, {})).toEqual({});
	save(KEYS.progress, { pythagoras: true });
	expect(localStorage.getItem('math-course:progress')).toBe('{"pythagoras":true}');
	expect(localStorage.getItem('vllm-course:progress')).toBe('{"foreign":true}');
});
