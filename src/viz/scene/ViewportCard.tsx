/**
 * ViewportCard: every scene is a small, real DCC viewport (docs/design/visual-language.md, Blender's 3D view):
 *
 *   header   2D/3D badge | Persp/Ortho | Overlays popover | Reset view            scene name | sidebar chevron
 *   body     the Stage (dark viewport, gizmo or XY corner) with the N-panel overlaid on its right edge
 *   (chips)  under 720 px the N-panel becomes a row of chips under the viewport
 *   status   what the pointer does here
 *
 * The frame and its exact size are server-rendered by SceneIsland.astro (`.lab-vp-skel`); this component fills the
 * same box absolutely, so mounting it shifts nothing. Overlay flags live in the lesson store (`ov:<card>:<flag>`, 0/1;
 * see `useOverlay`/`Show`) so the scene, which renders in the shared canvas's React root, reads them like any value;
 * toggling one hides objects and never changes the lesson's values. `N` (pointer over the card or focus inside it)
 * and the chevron toggle the sidebar; while it is open the scene is centered in the free part (Stage `inset`).
 * Imports Stage (three/drei): lives in a lesson's .impl chunk.
 */
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { useStore } from 'zustand';
import type { V } from '../geom';
import { lessonStore } from '../store';
import { createStage, overlayKey, overlayOn, type OverlayDef } from './handles';
import { NPanel, useTip, type Row } from './NPanel';
import { Stage } from './Stage';
import { StatusBar, type StatusSpec } from './StatusBar';

export interface ViewportCardProps {
	lesson: string;
	/** Card id, unique per lesson: scopes the overlay flags. */
	id: string;
	/** Scene name in the header, e.g. "LC-1 · Ledger". */
	name: string;
	mode: '2d' | '3d';
	extent: number;
	/** 2D view center (home of Reset view). */
	center?: V;
	/** Overlay flags the scene declares (each read with `useOverlay`/`Show`). */
	overlays?: readonly OverlayDef[];
	/** N-panel rows; SceneIsland must reserve the chip row (`panel`, the default) iff there are rows. */
	rows?: readonly Row[];
	status?: StatusSpec;
	/** The lesson's one first-view draw-in. */
	drawIn?: boolean;
	children: ReactNode;
}

const NARROW = '(max-width: 719.98px)';

const Icon = {
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
 * Blender's Overlays popover: a checkbox per declared flag, written to the lesson store. Escape or a press outside
 * closes it; Escape returns focus to the button.
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

export function ViewportCard(p: ViewportCardProps): React.JSX.Element {
	const overlays = p.overlays ?? [];
	const rows = p.rows ?? [];
	const hasPanel = rows.length > 0;
	// One registry per card (mode, lesson and overlays of a mounted card never change): the stage, the N-panel, the
	// status bar and the header's camera buttons all talk through it.
	const [reg] = useState(() => createStage(p.mode, { lesson: p.lesson, id: p.id, overlays }));
	const [proj, setProj] = useState<'persp' | 'ortho'>('persp');
	const [open, setOpen] = useState(true);
	const [inset, setInset] = useState(0);
	const root = useRef<HTMLDivElement>(null);
	const panelId = useId();
	const tip = useTip();

	// The server frame reserves the chip row only for cards with a panel; a mismatch would shift layout on phones.
	useEffect(() => {
		const fig = root.current?.closest('.lab-vp');
		if (fig && fig.hasAttribute('data-panel') !== hasPanel)
			throw new Error(`ViewportCard "${p.id}": SceneIsland panel=${fig.hasAttribute('data-panel')} but the card has ${rows.length} N-panel rows`);
	}, [hasPanel]);

	// Sidebar inset: the open desktop sidebar covers the viewport's right edge (region overlap); chips do not.
	useLayoutEffect(() => {
		const panel = document.getElementById(panelId);
		const mq = matchMedia(NARROW);
		const upd = () => setInset(!panel || !open || mq.matches ? 0 : panel.offsetWidth);
		upd();
		mq.addEventListener('change', upd);
		const ro = panel ? new ResizeObserver(upd) : null;
		if (panel) ro?.observe(panel);
		return () => {
			mq.removeEventListener('change', upd);
			ro?.disconnect();
		};
	}, [open, hasPanel, panelId]);

	// `N` toggles the sidebar of the card under the pointer (or holding focus), like Blender's region toggle.
	useEffect(() => {
		const card = root.current;
		if (!card || !hasPanel) return;
		let inside = false;
		const enter = () => (inside = true);
		const leave = () => (inside = false);
		const key = (e: KeyboardEvent) => {
			if (e.key.toLowerCase() !== 'n' || e.ctrlKey || e.metaKey || e.altKey) return;
			if ((e.target as Element | null)?.closest?.('input, textarea, select, [contenteditable="true"]')) return;
			if (!inside && !card.contains(document.activeElement)) return;
			e.preventDefault();
			setOpen((o) => !o);
		};
		card.addEventListener('pointerenter', enter);
		card.addEventListener('pointerleave', leave);
		window.addEventListener('keydown', key);
		return () => {
			card.removeEventListener('pointerenter', enter);
			card.removeEventListener('pointerleave', leave);
			window.removeEventListener('keydown', key);
		};
	}, [hasPanel]);

	const is3d = p.mode === '3d';
	return (
		<div className="lab-vp-card" ref={root}>
			<header className="lab-vp-head">
				{/* Indicators, not controls: the view type and a 2D view's projection are fixed by the scene. */}
				<span className="lab-vp-seg" role="img" aria-label={is3d ? '3D view' : '2D view'}>
					<span className={is3d ? 'lab-vp-tag' : 'lab-vp-tag is-on'}>2D</span>
					<span className={is3d ? 'lab-vp-tag is-on' : 'lab-vp-tag'}>3D</span>
				</span>
				{is3d ? (
					<span className="lab-vp-seg" role="group" aria-label="Projection">
						{(['persp', 'ortho'] as const).map((k) => (
							<button key={k} type="button" className="lab-vp-btn" aria-pressed={proj === k} onClick={() => setProj(k)}>
								{k === 'persp' ? 'Persp' : 'Ortho'}
							</button>
						))}
					</span>
				) : (
					<span className="lab-vp-seg" role="img" aria-label="Orthographic">
						<span className="lab-vp-tag is-on">Ortho</span>
					</span>
				)}
				{overlays.length > 0 && <Overlays lesson={p.lesson} card={p.id} overlays={overlays} />}
				<button
					type="button"
					className="lab-vp-btn"
					aria-label="Reset view"
					onClick={() => reg.nav?.reset()}
					{...tip.bind({ title: 'Reset view', text: 'Back to the starting camera.' })}
				>
					{Icon.reset}
					<span className="lab-vp-label">Reset view</span>
				</button>
				<span className="lab-vp-name">{p.name}</span>
				{hasPanel && (
					<button
						type="button"
						className="lab-vp-btn lab-vp-chev"
						aria-expanded={open}
						aria-controls={panelId}
						aria-label="Sidebar"
						aria-keyshortcuts="N"
						onClick={() => setOpen((o) => !o)}
						{...tip.bind({ title: 'Sidebar', text: 'Show or hide the property panel (N).' })}
					>
						{Icon.chev(open)}
					</button>
				)}
			</header>
			<div className="lab-vp-body">
				<Stage lesson={p.lesson} mode={p.mode} extent={p.extent} reg={reg} proj={proj} center={p.center} inset={inset} drawIn={p.drawIn}>
					{p.children}
				</Stage>
			</div>
			{hasPanel && <NPanel id={panelId} lesson={p.lesson} rows={rows} reg={reg} open={open} />}
			<StatusBar reg={reg} lesson={p.lesson} spec={p.status ?? {}} />
			{tip.node}
		</div>
	);
}
