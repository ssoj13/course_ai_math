/**
 * Short plain text (a name or a number) pinned to a point of the scene. It is DOM in the Stage overlay, not drei
 * `Html`: Html creates a React root per label and positions it from r3f's View size, which is stale after a resize
 * (see Stage.tsx). Here the scene half only registers the label; the overlay renders it and LabelLayer moves it once
 * per rendered frame, so it follows drags, orbiting and resizes. `pointer-events: none`; color from a palette role.
 */
import type { V } from '../geom';
import type { SceneRole } from '../palette';
import { useLabel, usePos } from './handles';

export interface LabelProps {
	lesson: string;
	/** Literal point or store key (live). */
	at: V | string;
	children: string | number;
	role?: SceneRole;
	/** Scene object/term id: the label turns hot with it. */
	id?: string;
	/** Screen offset in CSS px, y down, e.g. [10, -12] for up-right of an arrow tip. */
	offset?: readonly [number, number];
	/** Point the label keeps clear of: it moves from `at` directly away from it until its near edge is `gap` px off. */
	away?: V;
	gap?: number;
}

export function Label(p: LabelProps): null {
	// Validates the key (a typo throws); placement reads the store values of the frame being drawn.
	usePos(p.lesson, p.at);
	const key = p.at;
	useLabel({
		lesson: p.lesson,
		text: String(p.children),
		role: p.role,
		hot: p.id,
		offset: p.offset,
		at: (v) => (typeof key === 'string' ? (v[key] as V) : key),
		away: p.away && (() => p.away!),
		gap: p.gap,
	});
	return null;
}
