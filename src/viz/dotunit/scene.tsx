/**
 * Shared scene helpers of the DotUnit cards (three side): the step clock that turns a timeline step into progress
 * through that step's authored motion, and `Tag`, a DOM label pinned to a point the choreography computes per frame.
 */
import { useThree } from '@react-three/fiber';
import { useEffect, useRef } from 'react';
import type { SceneRole } from '../palette';
import { lessonStore } from '../store';
import { useLabel } from '../scene/handles';
import { reducedMotion } from '../scene/motion';

export interface Clock {
	step: number;
	/** Progress through the current step's motion, 0..1 (1 = its end state). */
	t: number;
	t0: number;
}

/**
 * Progress of timeline `timeline`: stepping forward by one plays that step's motion over `msOf(step)` ms; any other
 * change (back, a jump, the first frame) and reduced motion cut to the step's end state (spec §2.5). Read in useFrame
 * through `tick`, which returns whether the progress changed. A step change requests a frame of the demand canvas.
 */
export function useClock(lesson: string, timeline: string, msOf: (step: number) => number): { clock: Clock; tick(): boolean } {
	const invalidate = useThree((s) => s.invalidate);
	const clock = useRef<Clock>({ step: -1, t: 1, t0: 0 });
	const ms = useRef(msOf);
	ms.current = msOf;
	useEffect(() => lessonStore(lesson).subscribe((s, prev) => s.step[timeline] !== prev.step[timeline] && invalidate()), [lesson, timeline, invalidate]);
	const tick = () => {
		const c = clock.current;
		const s = lessonStore(lesson).getState().step[timeline] ?? 0;
		const now = performance.now();
		if (s !== c.step) {
			const play = c.step >= 0 && s === c.step + 1 && !reducedMotion() && ms.current(s) > 0;
			c.step = s;
			c.t = play ? 0 : 1;
			c.t0 = now;
			return true;
		}
		if (c.t >= 1) return false;
		c.t = Math.min(1, (now - c.t0) / ms.current(s));
		return true;
	};
	return { clock: clock.current, tick };
}

/** A label at a point the scene computes per frame (`at` returns null to hide it). */
export function Tag(p: { lesson: string; text: string; at: () => readonly number[] | null; role?: SceneRole; hot?: string; offset?: readonly [number, number]; small?: boolean }): null {
	const at = useRef(p.at);
	at.current = p.at;
	useLabel({ lesson: p.lesson, text: p.text, role: p.role, hot: p.hot, offset: p.offset, small: p.small, at: () => at.current() });
	return null;
}
