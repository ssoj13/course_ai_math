/**
 * Solved Challenge boxes (src/viz/Challenge.tsx), persisted next to the lesson progress: a record of
 * `challenge:<lesson>:<n>` -> true under KEYS.challenges, so a "lesson done" view can list them. Kept apart from
 * progress.ts on purpose: that module resolves lesson URLs from course.json, which the scene bundles do not need.
 */
import { KEYS, load, save } from './store';

export type Solved = Record<string, true>;

/** Storage key of challenge `n` of `lesson` (the lesson store id, e.g. `law-of-cosines`). */
export const challengeKey = (lesson: string, n: number): string => `challenge:${lesson}:${n}`;

const KEY = /^challenge:[a-z0-9-]+:\d+$/;

/** Keeps only well-formed keys set to `true`: storage may hold anything (hand edits, another build of the course). */
export function normalizeSolved(raw: unknown): Solved {
	const out: Solved = {};
	if (raw && typeof raw === 'object' && !Array.isArray(raw))
		for (const [k, v] of Object.entries(raw as Record<string, unknown>)) if (KEY.test(k) && v === true) out[k] = true;
	return out;
}

export function readSolved(): Solved {
	return normalizeSolved(load<unknown>(KEYS.challenges, {}));
}

/** Records a solved challenge; false when storage is unavailable (it is then kept for this page only). */
export function markSolved(key: string): boolean {
	if (!KEY.test(key)) throw new Error(`malformed challenge key "${key}"`);
	return save(KEYS.challenges, { ...readSolved(), [key]: true });
}
