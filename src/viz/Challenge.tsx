/**
 * A Challenge box (lessons spec §0.6): the task, a live status and hints behind a disclosure. `check(vals, step)` is a
 * pure predicate on the lesson store, evaluated on every store change; step-based tasks read the highest step a
 * Timeline reached, which the Timeline writes as `step['<timeline>:max']`, so they are plain store reads too.
 * Success is sticky: once solved it stays solved for the visit however the reader wanders off, and it is recorded as
 * `challenge:<lesson>:<n>` (src/lib/challenges.ts). A record from an earlier visit shows as solved after hydration
 * (the server renders "not yet": storage is per reader). Page-side DOM, follows the Starlight theme.
 */
import { useEffect, useId, useRef, useState } from 'react';
import { useStore } from 'zustand';
import { challengeKey, markSolved, readSolved } from '../lib/challenges';
import { lessonStore, type LessonState } from './store';

export interface ChallengeProps {
	lesson: string;
	/** 1-based number within the lesson (part of the progress key). */
	n: number;
	text: string;
	/** Hints, revealed together behind one disclosure. */
	hint?: readonly string[];
	/** Shown once solved (e.g. why an "impossible" task succeeds at its limit). */
	success?: string;
	check: (vals: LessonState['vals'], step: LessonState['step']) => boolean;
}

export function Challenge(p: ChallengeProps): React.JSX.Element {
	if (!Number.isInteger(p.n) || p.n < 1) throw new Error(`Challenge: n must be a positive integer, got ${p.n}`);
	const key = challengeKey(p.lesson, p.n);
	const ok = useStore(lessonStore(p.lesson), (s) => p.check(s.vals, s.step));
	const [done, setDone] = useState(false);
	const id = useId();
	// After hydration only: the server cannot know what this reader solved before.
	// A check that already holds on the initial values solves itself without the reader: an authoring slip.
	const first = useRef(ok);
	useEffect(() => {
		if (import.meta.env.DEV && first.current) console.warn(`Challenge ${key}: check() is already true on the initial values`);
	}, [key]);
	useEffect(() => {
		if (readSolved()[key]) setDone(true);
	}, [key]);
	useEffect(() => {
		if (!ok || done) return;
		setDone(true);
		markSolved(key);
	}, [ok, done, key]);
	return (
		<section className={done ? 'lab-challenge not-content is-done' : 'lab-challenge not-content'} aria-labelledby={id}>
			<header className="lab-ch-head">
				<span id={id} className="lab-ch-title">
					Challenge {p.n}
				</span>
				<span className="lab-ch-status" role="status">
					<span>{done ? '✓ done' : 'not yet'}</span>
				</span>
			</header>
			<p className="lab-ch-text">{p.text}</p>
			{done && p.success && <p className="lab-ch-success">{p.success}</p>}
			{p.hint && p.hint.length > 0 && (
				<details className="lab-ch-hint">
					<summary>{p.hint.length > 1 ? 'Hints' : 'Hint'}</summary>
					<ol>
						{p.hint.map((h) => (
							<li key={h}>{h}</li>
						))}
					</ol>
				</details>
			)}
		</section>
	);
}
