/**
 * Semantic color roles for scenes. The values live in CSS (`--viz-*` in src/styles/lab.css: dark under `:root`,
 * light under `:root[data-theme='light']`), so the page and the WebGL scenes share one source of truth. WebGL cannot
 * use CSS vars directly: scenes need concrete hex strings, so this module reads the computed values and republishes
 * them whenever Starlight flips `data-theme`. The canvas is `flat` (no tone mapping), so the hex renders exactly.
 * Importing this module touches no DOM; only `readPalette()` without a getter and `usePalette()` do.
 */
import { useSyncExternalStore } from 'react';

export type Role =
	| 'vecA' | 'vecB' | 'vecC'
	| 'axisX' | 'axisY' | 'axisZ'
	| 'grid' | 'gridMinor' | 'shadow' | 'hot' | 'ink' | 'muted' | 'fill';

export type Palette = Record<Role, string>;

const VARS: Palette = {
	vecA: '--viz-vec-a',
	vecB: '--viz-vec-b',
	vecC: '--viz-vec-c',
	axisX: '--viz-axis-x',
	axisY: '--viz-axis-y',
	axisZ: '--viz-axis-z',
	grid: '--viz-grid',
	gridMinor: '--viz-grid-minor',
	shadow: '--viz-shadow',
	hot: '--viz-hot',
	ink: '--viz-ink',
	muted: '--viz-muted',
	fill: '--viz-fill',
};

/** CSS color of a role for DOM parts of a scene (labels): the var itself, so a theme flip needs no re-render. */
export const roleVar = (role: Role): string => `var(${VARS[role]})`;

const rootStyle = (name: string): string => getComputedStyle(document.documentElement).getPropertyValue(name);

/** Reads every role through `get` (default: computed style of <html>). A missing variable throws instead of drawing black. */
export function readPalette(get: (name: string) => string = rootStyle): Palette {
	const out = {} as Palette;
	for (const [role, name] of Object.entries(VARS) as [Role, string][]) {
		const v = get(name).trim();
		if (!v) throw new Error(`missing CSS variable ${name}`);
		out[role] = v;
	}
	return out;
}

// One observer for the whole page, however many scenes subscribe. The snapshot is cached because
// useSyncExternalStore requires getSnapshot to return the same object until the store actually changes.
const listeners = new Set<() => void>();
let snap: Palette | null = null;
let observer: MutationObserver | null = null;

function subscribe(cb: () => void): () => void {
	if (!observer) {
		observer = new MutationObserver(() => {
			snap = null;
			for (const l of listeners) l();
		});
		observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
	}
	listeners.add(cb);
	return () => {
		listeners.delete(cb);
		if (!listeners.size) {
			observer?.disconnect();
			observer = null;
			// Without an observer a theme change would go unnoticed, so the cache must not outlive it.
			snap = null;
		}
	};
}

const getSnap = (): Palette => (snap ??= readPalette());

/** Current palette; re-renders on a theme switch. Browser-only (scenes are `client:only`). */
export function usePalette(): Palette {
	return useSyncExternalStore(subscribe, getSnap);
}
