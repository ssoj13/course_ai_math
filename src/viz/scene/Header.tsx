/**
 * DOM pieces of the ViewportCard header that need no three/drei (so they are tested without loading three): the
 * icon set and Blender's Overlays popover. ViewportCard.tsx composes them with the Stage.
 */
import { useEffect, useId, useRef, useState } from 'react';
import { useStore } from 'zustand';
import { lessonStore } from '../store';
import { overlayKey, overlayOn, type OverlayDef } from './handles';

export const Icon = {
	reset: (
		<svg viewBox="0 0 16 16" aria-hidden="true">
			<path d="M3.5 6.5A5 5 0 1 1 3 9.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
			<path d="M1.8 3.2v4h4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
		</svg>
	),
	overlays: (
		<svg viewBox="0 0 16 16" aria-hidden="true">
			<circle cx="6" cy="8" r="4.2" fill="none" stroke="currentColor" strokeWidth="1.5" />
			<circle cx="10" cy="8" r="4.2" fill="none" stroke="currentColor" strokeWidth="1.5" />
		</svg>
	),
	caret: (
		<svg viewBox="0 0 16 16" aria-hidden="true">
			<path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
		</svg>
	),
	chev: (open: boolean) => (
		<svg viewBox="0 0 16 16" aria-hidden="true">
			<path d={open ? 'M6 4l4 4-4 4' : 'M10 4l-4 4 4 4'} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
		</svg>
	),
};

/**
 * Blender's Overlays popover: a checkbox per declared flag, written to the lesson store. Escape, a press outside or
 * focus leaving it closes it; Escape returns focus to the button.
 */
export function Overlays({ lesson, card, overlays }: { lesson: string; card: string; overlays: readonly OverlayDef[] }): React.JSX.Element {
	const [open, setOpen] = useState(false);
	const wrap = useRef<HTMLDivElement>(null);
	const btn = useRef<HTMLButtonElement>(null);
	const popId = useId();
	const store = lessonStore(lesson);
	const on = useStore(store, (s) => overlays.map((o) => overlayOn(s.vals, card, o)).join());
	const flags = on.split(',').map((x) => x === 'true');
	useEffect(() => {
		if (!open) return;
		const away = (e: PointerEvent) => {
			if (!wrap.current?.contains(e.target as Node)) setOpen(false);
		};
		document.addEventListener('pointerdown', away, true);
		return () => document.removeEventListener('pointerdown', away, true);
	}, [open]);
	return (
		<div
			className="lab-vp-pop-wrap"
			ref={wrap}
			onKeyDown={(e) => {
				if (e.key !== 'Escape' || !open) return;
				e.preventDefault();
				setOpen(false);
				btn.current?.focus();
			}}
			// Tabbing out of the popover closes it, like a press outside (a stray open popover would cover the scene).
			onBlur={(e) => {
				if (open && !e.currentTarget.contains(e.relatedTarget as Node | null)) setOpen(false);
			}}
		>
			<button ref={btn} type="button" className="lab-vp-btn" aria-expanded={open} aria-controls={popId} onClick={() => setOpen((o) => !o)}>
				{Icon.overlays}
				<span className="lab-vp-label">Overlays</span>
				{Icon.caret}
			</button>
			{open && (
				<div id={popId} className="lab-vp-pop" role="group" aria-label="Viewport overlays">
					<div className="lab-vp-pop-title" aria-hidden="true">
						Viewport overlays
					</div>
					{overlays.map((o, i) => (
						<label key={o.id} className="lab-vp-check">
							<input type="checkbox" checked={flags[i]} onChange={(e) => store.getState().set(overlayKey(card, o.id), e.currentTarget.checked ? 1 : 0)} />
							{o.label}
						</label>
					))}
				</div>
			)}
		</div>
	);
}

