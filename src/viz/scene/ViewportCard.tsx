import { t } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
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
import { Icon, Overlays, SizeButtons } from './Header';
import { useCardSize } from './sizeModes';
import { frameMismatch, frameOf } from './chips';
import { NPanel, rowRanges, useTip, type Row } from './NPanel';
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

/**
 * Keeps the scene name in the header whole (EN and RU names differ a lot in length): when it does not fit, the
 * header first drops the text of its labelled buttons (icons stay, with accessible names), then lets the name wrap
 * onto two smaller lines. Re-evaluated whenever the header resizes.
 */
function useHeaderFit(root: React.RefObject<HTMLDivElement | null>, name: string): void {
	useLayoutEffect(() => {
		const head = root.current?.querySelector<HTMLElement>('.lab-vp-head');
		const label = head?.querySelector<HTMLElement>('.lab-vp-name');
		if (!head || !label) return;
		const fits = () => label.scrollWidth <= label.clientWidth + 0.5;
		const update = () => {
			head.removeAttribute('data-fit');
			if (fits()) return;
			head.setAttribute('data-fit', 'icons');
			if (!fits()) head.setAttribute('data-fit', 'wrap');
		};
		update();
		const ro = new ResizeObserver(update);
		ro.observe(head);
		return () => ro.disconnect();
	}, [root, name]);
}

export function ViewportCard(p: ViewportCardProps): React.JSX.Element {
	const locale = useLocale();
	const overlays = p.overlays ?? [];
	const rows = p.rows ?? [];
	const hasPanel = rows.length > 0;
	// One registry per card (mode, lesson and overlays of a mounted card never change): the stage, the N-panel, the
	// status bar and the header's camera buttons all talk through it.
	const [reg] = useState(() => createStage(p.mode, { lesson: p.lesson, id: p.id, overlays, ranges: rowRanges(rows) }));
	const [proj, setProj] = useState<'persp' | 'ortho'>('persp');
	const [open, setOpen] = useState(true);
	const [inset, setInset] = useState(0);
	const root = useRef<HTMLDivElement>(null);
	const sizeId = p.lesson + ':' + p.id;
	const { mode: sizeMode } = useCardSize(sizeId, root, () => reg.nav?.redraw());
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
	useHeaderFit(root, p.name);

	// The server frame reserves the chip row from the rows SceneIsland was given; other rows here would shift the layout
	// on phones, so a mismatch is an authoring error (pass the same rows array to both).
	useEffect(() => {
		const fig = root.current?.closest<HTMLElement>('.lab-vp');
		const err = fig && frameMismatch(frameOf(fig), rows);
		if (err) throw new Error(`ViewportCard "${p.id}": ${err}`);
	}, [hasPanel, rows]);

	// One observer maintains desktop overlap and the mobile chip safety net. Never shrink below the server reservation.
 useLayoutEffect(() => {
  const panel = document.getElementById(panelId);
  const fig = root.current?.closest<HTMLElement>('.lab-vp');
  const reserve = fig?.querySelector<HTMLElement>('.lab-vp-skel > :nth-child(3)');
  const mq = matchMedia(NARROW);
  const upd = () => {
   setInset(!panel || !open || mq.matches ? 0 : panel.offsetWidth);
   fig?.style.removeProperty('--lab-vp-chips');
   if (mq.matches && panel && reserve && panel.scrollHeight > reserve.offsetHeight)
    fig?.style.setProperty('--lab-vp-chips', `${panel.scrollHeight}px`);
  };
  upd();
  mq.addEventListener('change', upd);
  const ro = panel ? new ResizeObserver(upd) : null;
  if (panel) ro?.observe(panel);
  return () => {
   mq.removeEventListener('change', upd);
   ro?.disconnect();
   fig?.style.removeProperty('--lab-vp-chips');
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
		<div
			className="lab-vp-card"
			tabIndex={-1}
			data-size={sizeMode}
			data-size-id={sizeId}
			style={{ '--lab-vp-inset': `${inset}px` } as React.CSSProperties}
			ref={root}
		>
			<header className="lab-vp-head">
				{/* Indicators, not controls: the view type and a 2D view's projection are fixed by the scene. */}
				<span className="lab-vp-seg" role="img" aria-label={is3d ? t(locale, 'viewport.view3d') : t(locale, 'viewport.view2d')}>
					<span className={is3d ? 'lab-vp-tag' : 'lab-vp-tag is-on'}>2D</span>
					<span className={is3d ? 'lab-vp-tag is-on' : 'lab-vp-tag'}>3D</span>
				</span>
				{is3d ? (
					<span className="lab-vp-seg" role="group" aria-label={t(locale, 'viewport.projection')}>
						{(['persp', 'ortho'] as const).map((k) => (
							<button key={k} type="button" className="lab-vp-btn" aria-pressed={proj === k} onClick={() => setProj(k)}>
								{k === 'persp' ? t(locale, 'viewport.persp') : t(locale, 'viewport.ortho')}
							</button>
						))}
					</span>
				) : (
					<span className="lab-vp-seg" role="img" aria-label={t(locale, 'viewport.orthographic')}>
						<span className="lab-vp-tag is-on">{t(locale, 'viewport.ortho')}</span>
					</span>
				)}
				{overlays.length > 0 && <Overlays lesson={p.lesson} card={p.id} overlays={overlays} />}
				<button
					type="button"
					className="lab-vp-btn"
					aria-label={t(locale, 'viewport.reset')}
					onClick={() => reg.nav?.reset()}
					{...tip.bind({ title: t(locale, 'viewport.reset'), text: t(locale, 'viewport.resetTip') })}
				>
					{Icon.reset}
					<span className="lab-vp-label">{t(locale, 'viewport.reset')}</span>
				</button>
				<SizeButtons id={sizeId} mode={sizeMode} />
				<span className="lab-vp-name">{p.name}</span>
				{hasPanel && (
					<button
						type="button"
						className="lab-vp-btn lab-vp-chev"
						aria-expanded={open}
						aria-controls={panelId}
						aria-label={t(locale, 'viewport.sidebar')}
						aria-keyshortcuts="N"
						ref={chev}
						onClick={toggle}
						{...tip.bind({ title: t(locale, 'viewport.sidebar'), text: t(locale, 'viewport.sidebarTip') })}
					>
						{Icon.chev(open)}
					</button>
				)}
			</header>
			<div className="lab-vp-body">
				<Stage
					lesson={p.lesson}
					mode={p.mode}
					extent={p.extent}
					reg={reg}
					proj={proj}
					center={p.center}
					inset={inset}
					drawIn={p.drawIn}
				>
					{p.children}
				</Stage>
			</div>
			{hasPanel && <NPanel id={panelId} lesson={p.lesson} rows={rows} reg={reg} open={open} />}
			<StatusBar reg={reg} lesson={p.lesson} spec={p.status ?? {}} />
			{tip.node}
		</div>
	);
}
