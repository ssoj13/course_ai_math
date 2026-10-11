/** A scrubbable number inside prose: drag or arrow-key it and every linked view follows the store. */
import { lessonStore, useVal } from './store';
import { keyStep, scrubValue, type ScrubSpec } from './scrub';
import { startScrubDrag } from './useScrub';

export function Num(p: { lesson: string; k: string; spec: ScrubSpec; fmt?: (v: number) => string; label: string }): React.JSX.Element {
	const v = useVal(p.lesson, p.k, p.spec.min);
	const store = lessonStore(p.lesson);
	// Read the live value at press/key time, not the render closure: events can outrun React.
	const cur = () => store.getState().vals[p.k] as number;
	return (
		<span
			role="slider"
			tabIndex={0}
			className="lab-num not-content"
			aria-label={p.label}
			aria-valuemin={p.spec.min}
			aria-valuemax={p.spec.max}
			aria-valuenow={v}
			onPointerDown={(e) => {
				const v0 = cur();
				startScrubDrag(e, e.currentTarget, { get: cur, set: (v) => store.getState().set(p.k, v) }, (dx) => scrubValue(v0, dx, p.spec));
			}}
			onKeyDown={(e) => {
				const next = keyStep(cur(), e.key, p.spec);
				if (next === null) return;
				e.preventDefault();
				// An indirect change (Ruling U): the scene eases to the new value, the number jumps.
				store.getState().set(p.k, next, 'ease');
			}}
		>
			{p.fmt ? p.fmt(v) : String(Math.round(v * 100) / 100)}
		</span>
	);
}
