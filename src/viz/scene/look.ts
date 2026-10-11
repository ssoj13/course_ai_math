/**
 * The two scene looks (docs/design/visual-language.md v3) and their small pure rules. Three-free, so tests and the
 * DOM chrome can import it.
 *
 * - `holo`: deep-space background, glowing filament strokes, glass faces, HUD instruments, reflective floor.
 * - `clay`: a light studio with matte rounded solids; it never shows holographic furniture (space panels, planet,
 *   galaxies, floor reflection), and it accepts only solids and measurement guides as subjects.
 */
import { createContext } from 'react';

export type Look = 'holo' | 'clay';

/** Look of the enclosing Stage, provided inside its View. */
export const LookCtx = createContext<Look>('holo');

/** Throws when a holographic subject (a filament or glass primitive) is placed in a clay studio card. */
export function assertSubject(look: Look, filament: boolean): void {
	if (look === 'clay' && filament) throw new Error('Clay studio cards accept solids and measurement guides, not holographic subjects.');
}

/**
 * Alpha of the hot halo: a slow breathing pulse (period 1.2 s) so the active object reads as "live" without moving.
 * Reduced motion keeps the mid value, static.
 */
export function hotAlpha(seconds: number, reduced: boolean): number {
	return reduced ? 0.36 : 0.36 - 0.06 * Math.cos((seconds * Math.PI) / 0.6);
}

/** Share of white mixed into a filament's core: an emissive line is whiter at its hottest centre than its glow. */
const CORE_WHITE = 0.35;

/** Core colour of a filament of colour `hex` (#rrggbb): the role colour mixed with CORE_WHITE of white. */
export function lightened(hex: string): string {
	const n = parseInt(hex.slice(1), 16);
	const mix = (shift: number) => Math.round(((n >> shift) & 255) * (1 - CORE_WHITE) + 255 * CORE_WHITE);
	return '#' + [16, 8, 0].map((s) => mix(s).toString(16).padStart(2, '0')).join('');
}
