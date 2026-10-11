/**
 * Visual tokens of the viewport (docs/design/visual-language.md): color roles and the stroke/fill recipe.
 *
 * Colors live in CSS (`--viz-*` in src/styles/lab.css), so the DOM chrome (header, N-panel, labels) and the WebGL
 * scenes share one source of truth. The viewport and its chrome are always dark, in both site themes (Ruling R): the
 * colors are calibrated once against one background (contrast rules are unit-tested in tests/js/tokens.test.ts).
 * WebGL cannot use CSS vars, so this module reads the computed values as concrete hex strings; the canvas is `flat`
 * (no tone mapping), so the hex renders exactly. The theme observer stays: page-side elements follow the Starlight
 * theme, and a re-read on a theme flip keeps scenes correct should a role ever become theme-dependent again.
 * Importing this module touches no DOM; only `readPalette()` without a getter and `usePalette()` do.
 */
import { useSyncExternalStore } from 'react';

export type Role =
	| 'bgTop' | 'bgBottom' | 'chrome' | 'chrome2' | 'field' | 'fieldHover'
	| 'grid' | 'gridMinor' | 'ink' | 'muted'
	| 'star' | 'nebulaA' | 'nebulaB' | 'vignette' | 'floor' | 'plate'
	| 'clayA' | 'clayB' | 'clayC' | 'clayAccent' | 'clayPlate' | 'clayInk'
	| 'axisX' | 'axisY' | 'axisZ'
	| 'vecA' | 'vecB' | 'vecC' | 'light' | 'hot';

export type Palette = Record<Role, string>;

/** CSS custom property of every role (the spec table of visual-language.md). */
export const ROLE_VARS: Readonly<Record<Role, string>> = {
	star: '--viz-star', nebulaA: '--viz-nebula-a', nebulaB: '--viz-nebula-b', vignette: '--viz-vignette', floor: '--viz-floor', plate: '--viz-plate',
	clayA: '--viz-clay-a', clayB: '--viz-clay-b', clayC: '--viz-clay-c', clayAccent: '--viz-clay-accent', clayPlate: '--viz-clay-plate', clayInk: '--viz-clay-ink',
	bgTop: '--viz-bg-top',
	bgBottom: '--viz-bg-bottom',
	chrome: '--viz-chrome',
	chrome2: '--viz-chrome-2',
	field: '--viz-field',
	fieldHover: '--viz-field-hover',
	grid: '--viz-grid',
	gridMinor: '--viz-grid-minor',
	ink: '--viz-ink',
	muted: '--viz-muted',
	axisX: '--viz-axis-x',
	axisY: '--viz-axis-y',
	axisZ: '--viz-axis-z',
	vecA: '--viz-vec-a',
	vecB: '--viz-vec-b',
	vecC: '--viz-vec-c',
	light: '--viz-light',
	hot: '--viz-hot',
};

/** Roles a scene may draw strokes, fills and labels in; each is >= 3:1 against both ends of the viewport gradient. */
export const STROKE_ROLES = ['vecA', 'vecB', 'vecC', 'axisX', 'axisY', 'axisZ', 'light', 'hot', 'ink', 'muted'] as const;
export type SceneRole = (typeof STROKE_ROLES)[number];

/**
 * Non-color tokens of the scene look (visual-language.md "Scene rendering rules"), in CSS px and alpha. Primitives
 * read these instead of private constants, so the whole course changes its look in one place.
 */
export const STYLE = {
	/** Core width of a beam (vector, segment). */
	core: 2,
	/** Halo width around a beam, and its alpha in the beam's own color. */
	halo: 18,
	haloA: 0.10,
	inner: 7,
	innerA: 0.38,
	lighten: 0.35,
	mirrorA: 0.28,
	glassBottomA: 0.06,
	glassGridA: 0.22,
	pointHalo: 14,
	pointHaloA: 0.3,
	/** Hot (Blender active outline): a hot-colored stroke `hotPad` px wider on each side, behind the core. */
	hotPad: 2,
	/** Halo alpha while hot, in the hot color. */
	hotHaloA: 0.3,
	/** Secondary lines: area edges, angle arcs, right-angle marks, legs. */
	edge: 1.5,
	/** Area fill alpha and angle sector alpha. */
	fillA: 0.12,
	sectorA: 0.1,
	/** Negative areas: 1 px hatch lines every 6 px at 45 degrees (sign never color-only). */
	hatchPx: 6,
	hatchW: 1,
	/** Arrow head length. */
	headPx: 14,
	/** Point: a 6 px disc (radius 3, Blender's vertex) inside a 1.5 px dark rim (visual-language.md). */
	pointR: 3,
	rim: 1.5,
	/** Projection shadow band on the target line, and its alpha. */
	bandPx: 6,
	bandA: 0.35,
	/** Dash and gap of drop lines and legs. */
	dash: [5, 4] as const,
} as const;

/** CSS color of a role for DOM parts of a scene (labels): the var itself. */
export const roleVar = (role: Role): string => `var(${ROLE_VARS[role]})`;

/** Relative luminance (WCAG 2.x) of a #rrggbb color. */
export function luminance(hex: string): number {
	const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim());
	if (!m) throw new Error(`expected #rrggbb, got "${hex}"`);
	const lin = (h: string) => {
		const c = parseInt(h, 16) / 255;
		return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
	};
	return 0.2126 * lin(m[1]) + 0.7152 * lin(m[2]) + 0.0722 * lin(m[3]);
}

/** WCAG contrast ratio of two #rrggbb colors (1..21, symmetric). */
export function contrast(a: string, b: string): number {
	const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
	return (hi + 0.05) / (lo + 0.05);
}

const rootStyle = (name: string): string => getComputedStyle(document.documentElement).getPropertyValue(name);

/** Reads every role through `get` (default: computed style of <html>). A missing variable throws instead of drawing black. */
export function readPalette(get: (name: string) => string = rootStyle): Palette {
	const out = {} as Palette;
	for (const [role, name] of Object.entries(ROLE_VARS) as [Role, string][]) {
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
