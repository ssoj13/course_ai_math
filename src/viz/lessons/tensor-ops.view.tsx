/**
 * The three-side loop shared by the 3.4 and 3.5 scenes: drives a DotUnit `Frame` (rendered by `Boxes`) from a
 * timeline, the lesson's hot set and the data, and frames the figure from the front whenever its size changes.
 */
import { useThree } from '@react-three/fiber';
import { useEffect, useRef } from 'react';
import { lessonStore } from '../store';
import { useStage } from '../scene/handles';
import { useClock } from '../dotunit/scene';

const FRONT: readonly [number, number, number] = [0, 0, 1];
const AT_END = { step: Infinity, t: 1 };

export interface StepFrame {
	lesson: string;
	/** Timeline driving the motion, or null: the scene is drawn at its end state. */
	timeline: string | null;
	/** Duration (ms) of the motion of each step (0 = a cut). */
	ms: readonly number[];
	/** Changes when the figure's size changes: the view is then re-framed (value changes keep the reader's camera). */
	frameKey: string;
	/** Changes when the drawn data changes: the frame is then rewritten. */
	dataKey: unknown;
	/** Rewrites the frame for this step, progress and hot set. */
	write(step: number, t: number, hot: ReadonlySet<string>): void;
}

/** Returns the `update` callback for `Boxes`. */
export function useStepFrame(o: StepFrame): () => 'still' | 'moved' | 'moving' {
	const reg = useStage();
	const invalidate = useThree((s) => s.invalidate);
	const { clock, tick } = useClock(o.lesson, o.timeline ?? '', (s) => (o.timeline ? o.ms[s] ?? 0 : 0));
	const st = useRef({ dirty: true, refit: true, hot: null as ReadonlySet<string> | null, frameKey: '', data: null as unknown });
	const cur = useRef(o);
	cur.current = o;
	if (st.current.frameKey !== o.frameKey) {
		st.current.frameKey = o.frameKey;
		st.current.refit = true;
	}
	if (st.current.data !== o.dataKey) {
		st.current.data = o.dataKey;
		st.current.dirty = true;
	}
	useEffect(() => invalidate(), [o.dataKey, o.frameKey, invalidate]);
	return () => {
		const s = st.current, p = cur.current;
		const moved = p.timeline ? tick() : false;
		const hot = lessonStore(p.lesson).getState().hot;
		if (moved || s.dirty || hot !== s.hot) {
			s.dirty = false;
			s.hot = hot;
			const c = p.timeline ? clock : AT_END;
			p.write(c.step, c.t, hot);
			// One more frame after a data change: Boxes moves its bounds proxy right after this update, the refit reads it.
			return c.t < 1 || s.refit ? 'moving' : 'moved';
		}
		if (s.refit) {
			// The Stage binds its navigation a few frames after mount: keep the request until it can be served.
			if (!reg.nav) return 'moving';
			s.refit = false;
			reg.nav.frame(FRONT);
			return 'moved';
		}
		return 'still';
	};
}
