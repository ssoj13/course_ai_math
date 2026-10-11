import { t } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
/**
 * DOM pieces of the ViewportCard header that need no three/drei (so they are tested without loading three): the
 * icon set and Blender's Overlays popover. ViewportCard.tsx composes them with the Stage.
 */
import { useEffect, useId, useRef, useState } from 'react';
import { useStore } from 'zustand';
import { lessonStore } from '../store';
import { toggleSize, type SizeMode } from './sizeModes';
import { useTip } from './NPanel';
import { overlayKey, overlayOn, type OverlayDef } from './handles';

export const Icon = {
 wide: <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3H3v3m7-3h3v3M3 10v3h3m7-3v3h-3M3 3l4 4m6-4L9 7M3 13l4-4m6 4L9 9" fill="none" stroke="currentColor" strokeWidth="1.4"/></svg>,
 full: <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 2H2v4m8-4h4v4M2 10v4h4m8-4v4h-4" fill="none" stroke="currentColor" strokeWidth="1.6"/></svg>,
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

export function SizeButtons({ id, mode }: { id: string; mode: SizeMode }) {
 const locale = useLocale();
 const tip = useTip();
 const names = { wide: t(locale, 'viewport.wide'), full: t(locale, 'viewport.full') };
 const hints = { wide: t(locale, 'viewport.wideTip'), full: t(locale, 'viewport.fullTip') };
 return <>{(['wide', 'full'] as const).map(k =>
  <button key={k} type="button" className="lab-vp-btn lab-vp-size" aria-label={names[k]}
   aria-pressed={mode === k} aria-keyshortcuts={k === 'wide' ? 'Control+Space' : 'Control+Alt+Space F'}
   onClick={() => toggleSize(id, k)} {...tip.bind({ title: names[k], text: hints[k] })}>
   {Icon[k]}
  </button>)}{tip.node}</>;
}

/**
 * Blender's Overlays popover: a checkbox per declared flag, written to the lesson store. Escape, a press outside or
 * focus leaving it closes it; Escape returns focus to the button.
 */
export function Overlays({ lesson, card, overlays }: { lesson: string; card: string; overlays: readonly OverlayDef[] }): React.JSX.Element {
 const locale = useLocale();
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
			const w = wrap.current;
			if (w && !e.composedPath().includes(w)) setOpen(false);
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
			// Tabbing out of the popover closes it. Only a non-null relatedTarget is a keyboard move elsewhere: a mouse press
			// on non-focusable popover content (label text, title, padding) blurs the button with relatedTarget = null, and
			// closing then would unmount the popover before the click lands (presses outside are the pointerdown rule).
			onBlur={(e) => {
				const to = e.relatedTarget as Node | null;
				if (open && to && !e.currentTarget.contains(to)) setOpen(false);
			}}
		>
			<button ref={btn} type="button" className="lab-vp-btn" aria-label={t(locale, 'viewport.overlays')} aria-expanded={open} aria-controls={popId} onClick={() => setOpen((o) => !o)}>
				{Icon.overlays}
				<span className="lab-vp-label">{t(locale, 'viewport.overlays')}</span>
				{Icon.caret}
			</button>
			{open && (
				<div id={popId} className="lab-vp-pop" role="group" aria-label={t(locale, 'viewport.overlayTitle')}>
					<div className="lab-vp-pop-title" aria-hidden="true">
						{t(locale, 'viewport.overlayTitle')}
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

