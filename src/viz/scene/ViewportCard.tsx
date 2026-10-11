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
import type { V } from '../geom';
import { createStage, type OverlayDef } from './handles';
import { Icon, Overlays } from './Header';
import { chipLines } from './chips';
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
	/** N-panel rows: the same array SceneIsland gets (it reserves the phone chip row from them). */
	rows?: readonly Row[];
	status?: StatusSpec;
	/** The lesson's one first-view draw-in. */
	drawIn?: boolean;
	children: ReactNode;
}

const NARROW = '(max-width: 719.98px)';

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
	const chev = useRef<HTMLButtonElement>(null);
	const toggleRef = useRef(() => {});
	// Closing the sidebar while a field in it holds focus would drop focus to <body> (display: none): hand it to the
	// chevron, which reopens the panel.
	const toggle = () => {
		const panel = document.getElementById(panelId);
		if (open && panel?.contains(document.activeElement) && !matchMedia(NARROW).matches) chev.current?.focus();
		setOpen((o) => !o);
	};
	toggleRef.current = toggle;
	const tip = useTip();

	// The server frame reserves the chip row from the rows SceneIsland was given; other rows here would shift the layout
	// on phones, so a mismatch is an authoring error (pass the same rows array to both).
	useEffect(() => {
		const fig = root.current?.closest<HTMLElement>('.lab-vp');
		if (!fig) return;
		if (fig.hasAttribute('data-panel') !== hasPanel)
			throw new Error(`ViewportCard "${p.id}": SceneIsland has ${fig.hasAttribute('data-panel') ? '' : 'no '}rows but the card has ${rows.length} N-panel rows`);
		const lines = hasPanel ? chipLines(rows) : 0;
		if (hasPanel && !fig.hasAttribute('data-chip-override') && Number(fig.dataset.chipLines) !== lines)
			throw new Error(`ViewportCard "${p.id}": SceneIsland reserved ${fig.dataset.chipLines} chip lines, the card's rows need ${lines} (pass the same rows to both)`);
	}, [hasPanel, rows]);

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

	// Wrapped chips (< 720 px, Ruling V): the frame reserved the chip model's height at the 320 px basis. A safety net
	// only: if the chips ever need more (a read-only value longer than its `chars` budget), grow the row to fit (a
	// shift); never shrink below the reservation, so a wider phone keeps a little spare room instead of a jump.
	useLayoutEffect(() => {
		const panel = document.getElementById(panelId);
		const fig = root.current?.closest<HTMLElement>('.lab-vp');
		const reserve = fig?.querySelector<HTMLElement>('.lab-vp-skel > :nth-child(3)');
		if (!panel || !fig || !reserve) return;
		const mq = matchMedia(NARROW);
		const upd = () => {
			fig.style.removeProperty('--lab-vp-chips');
			if (!mq.matches) return;
			const need = panel.scrollHeight;
			if (need > reserve.offsetHeight) fig.style.setProperty('--lab-vp-chips', `${need}px`);
		};
		upd();
		mq.addEventListener('change', upd);
		const ro = new ResizeObserver(upd);
		ro.observe(panel);
		return () => {
			mq.removeEventListener('change', upd);
			ro.disconnect();
			fig.style.removeProperty('--lab-vp-chips');
		};
	}, [hasPanel, panelId]);

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
			toggleRef.current();
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
						ref={chev}
						onClick={toggle}
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
