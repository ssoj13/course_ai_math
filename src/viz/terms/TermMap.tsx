/**
 * TermMap: a mind map of the term graph (graph.ts), in the 6d Blender viewport look (always dark, like the scenes).
 *
 * Two modes, one component:
 *   - `local` (end of a lesson): the card draws the lesson's own terms (highlighted) and their direct foundations, small
 *     enough to read at once in a box whose height the server derived from the layer count; full screen draws the
 *     whole one-hop neighbourhood (`data`), which the list view also holds;
 *   - `genealogy` (glossary): the whole graph arrives, the reader picks a term and the map shows it with everything it
 *     is built on down to the roots, plus the machine-level terms used along the way and their own foundations.
 *
 * SSR-safe entry: the server renders the card, the legend and the list view (the accessible twin of the map: terms,
 * definitions, relations and links, keyboard- and screen-reader-friendly, and the whole content without JavaScript).
 * The drawn map (React Flow + ELK in a worker) is a lazy chunk mounted after hydration; lesson pages hydrate it with
 * `client:visible`, so pages without a map never load it. The drawing is pointer-only (`aria-hidden`).
 *
 * Interaction: hover (mouse) or a first tap (touch) shows a term's definition in the status line; a click (second tap)
 * follows the node's link: the lesson that introduces the term, or its glossary entry for a forward term. Nodes are
 * real links, so Ctrl/middle click open a new tab. Drag (one finger on touch) pans inside the box, while a swipe
 * outside it scrolls the page; plain wheel scrolls the page (Ruling T), Ctrl+wheel / pinch zoom. Full screen is a
 * modal <dialog> (top layer, Escape closes).
 */
import { lazy, Suspense, useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';
import { t, type Locale, type TextKey } from '../../i18n/core';
import { DEPTHS, genealogyIn, relations, type Depth, type EdgeType, type MapData, type MapEdge, type MapNode } from './graph';
import './terms.css';

const Flow = lazy(() => import('./TermMap.impl'));

export interface TermMapProps {
	locale: Locale;
	/** local: the lesson's one-hop neighbourhood (full screen, list); genealogy: the whole graph. */
	data: MapData;
	mode: 'local' | 'genealogy';
	/** Highlighted terms (local: the lesson's own terms); genealogy: the initially selected term. */
	focus: string[];
	/** local: ids drawn in the card (own terms + direct foundations); default all of `data`. */
	inline?: string[];
	/** local: box height in CSS px reserved by the server (graph.ts `mapHeight`); default from terms.css. */
	height?: number;
}

/** Label keys of a relation, read from the term that stores the edge. */
export const OUT_KEY: Readonly<Record<EdgeType, TextKey>> = {
	'built-on': 'terms.builtOn',
	'is-a': 'terms.isA',
	'special-case-of': 'terms.specialCaseOf',
	'equivalent-to': 'terms.equivalentTo',
	'used-in': 'terms.usedIn',
};
/** Label keys of a relation read from the other end. */
export const IN_KEY: Readonly<Record<EdgeType, TextKey>> = {
	'built-on': 'terms.builds',
	'is-a': 'terms.kinds',
	'special-case-of': 'terms.specialCases',
	'equivalent-to': 'terms.equivalentTo',
	'used-in': 'terms.uses',
};
export const DEPTH_KEY: Readonly<Record<Depth, TextKey>> = {
	ground: 'terms.ground',
	vectors: 'terms.vectors',
	matrices: 'terms.matrices',
	ai: 'terms.ai',
	machine: 'terms.machine',
};

/** Edge types in legend order, with the stroke style terms.css gives each; arrowed ones as drawn on the map. */
const LEGEND: readonly EdgeType[] = ['built-on', 'is-a', 'special-case-of', 'equivalent-to', 'used-in'];
const ARROWED: ReadonlySet<EdgeType> = new Set(['built-on', 'is-a', 'used-in', 'special-case-of']);

/** Lowest opening zoom of a lesson map: node labels are 12 px at zoom 1, so 0.88 keeps them at 10.6 px. */
export const READABLE = 0.88;

/** What a drawn map shows: node list (slice order) and its edges. */
export interface View {
	nodes: MapNode[];
	edges: MapEdge[];
	focus: ReadonlySet<string>;
}

/** Props of the lazy drawn map. */
export interface FlowProps {
	view: View;
	locale: Locale;
	/** Lowest opening zoom: below it the map opens on its focus terms instead of everything (0 = always everything). */
	floor: number;
	onHover: (id: string | null) => void;
	/** A node link was activated; returns false to keep the browser from following it (first tap on touch). */
	onActivate: (id: string, touch: boolean) => boolean;
	/** Receives the fit-everything action once the map is laid out. */
	onFit: (fit: (() => void) | null) => void;
}

const sliceOf = (byId: ReadonlyMap<string, MapNode>, ids: readonly string[], edges: readonly MapEdge[], focus: ReadonlySet<string>): View => {
	const set = new Set(ids);
	return { nodes: ids.map((id) => byId.get(id) as MapNode), edges: edges.filter((e) => set.has(e.from) && set.has(e.to)), focus };
};

export function TermMap(p: TermMapProps): React.JSX.Element {
	const { locale, data, mode } = p;
	const byId = useMemo(() => new Map(data.nodes.map((n) => [n.id, n])), [data]);
	const [sel, setSel] = useState(p.focus[0] ?? data.nodes[0]?.id ?? '');
	for (const id of [sel, ...p.focus, ...(p.inline ?? [])]) if (!byId.has(id)) throw new Error(`TermMap: "${id}" is not in the map data`);
	const order = useMemo(() => data.nodes.map((n) => n.id), [data]);
	// `full` = the dialog and the list; `card` = the inline drawing.
	const { full, card } = useMemo(() => {
		if (mode === 'genealogy') {
			const s = genealogyIn(order, data.edges, sel, (id) => byId.get(id)?.depth === 'machine');
			const v = sliceOf(byId, s.ids, s.edges, new Set([sel]));
			return { full: v, card: v };
		}
		const focus = new Set(p.focus);
		return { full: sliceOf(byId, order, data.edges, focus), card: sliceOf(byId, p.inline ?? order, data.edges, focus) };
	}, [mode, order, data, sel, byId, p.focus, p.inline]);

	// The lazy map is mounted after hydration only (the server and the first client render show the SSR message).
	const [live, setLive] = useState(false);
	useEffect(() => setLive(true), []);
	const [touch, setTouch] = useState(false);
	useEffect(() => {
		const mq = matchMedia('(pointer: coarse)');
		const sync = () => setTouch(mq.matches);
		sync();
		mq.addEventListener('change', sync);
		return () => mq.removeEventListener('change', sync);
	}, []);

	// Genealogy: glossary entries carry `data-tm-term` links that retarget this map (cross-island, through the DOM).
	useEffect(() => {
		if (mode !== 'genealogy') return;
		const onClick = (e: MouseEvent) => {
			const a = (e.target as Element | null)?.closest?.('[data-tm-term]');
			const id = a?.getAttribute('data-tm-term');
			if (id && byId.has(id)) setSel(id);
		};
		document.addEventListener('click', onClick);
		return () => document.removeEventListener('click', onClick);
	}, [mode, byId]);

	const [hover, setHover] = useState<string | null>(null);
	const [picked, setPicked] = useState<string | null>(null);
	const onActivate = useCallback(
		(id: string, viaTouch: boolean) => {
			if (!viaTouch || picked === id) return true;
			setPicked(id); // first tap reads, the second one follows the link
			return false;
		},
		[picked],
	);
	const shown = byId.get(hover ?? picked ?? '');

	const fitRef = useRef<(() => void) | null>(null);
	const onFit = useCallback((f: (() => void) | null) => {
		fitRef.current = f;
	}, []);
	const noFit = useCallback(() => undefined, []);
	const dialog = useRef<HTMLDialogElement>(null);
	const [open, setOpen] = useState(false);
	const titleId = useId();
	const title = t(locale, mode === 'local' ? 'terms.title' : 'terms.genealogy');
	const floor = mode === 'local' ? READABLE : 0;

	const head = (inDialog: boolean) => (
		<div className="lab-tm-head">
			<span className="lab-tm-tag" id={inDialog ? undefined : titleId}>
				{title}
			</span>
			{mode === 'genealogy' && (
				<label className="lab-tm-pick">
					<span className="lab-tm-sr">{t(locale, 'terms.pick')}</span>
					<select value={sel} onChange={(e) => setSel(e.currentTarget.value)}>
						{[...data.nodes]
							.sort((a, b) => a.label.localeCompare(b.label, locale))
							.map((n) => (
								<option key={n.id} value={n.id}>
									{n.label}
								</option>
							))}
					</select>
				</label>
			)}
			<span className="lab-tm-spacer" />
			{!inDialog && (
				<button type="button" className="lab-tm-btn lab-tm-icon" title={t(locale, 'terms.fitTip')} aria-label={t(locale, 'terms.fitTip')} onClick={() => fitRef.current?.()}>
					<svg viewBox="0 0 16 16" aria-hidden="true">
						<path d="M2 6V2h4M10 2h4v4M14 10v4h-4M6 14H2v-4" />
					</svg>
				</button>
			)}
			{inDialog ? (
				<button type="button" className="lab-tm-btn" onClick={() => dialog.current?.close()}>
					{t(locale, 'terms.close')}
				</button>
			) : (
				<button
					type="button"
					className="lab-tm-btn lab-tm-icon"
					title={t(locale, mode === 'local' ? 'terms.fullLocal' : 'terms.full')}
					aria-label={t(locale, mode === 'local' ? 'terms.fullLocal' : 'terms.full')}
					onClick={() => {
						setOpen(true);
						dialog.current?.showModal();
					}}
				>
					<svg viewBox="0 0 16 16" aria-hidden="true">
						<path d="M9 2h5v5M14 2 9 7M7 14H2V9M2 14l5-5" />
					</svg>
				</button>
			)}
		</div>
	);

	const draw = (view: View, fit: FlowProps['onFit']) =>
		live ? (
			<Suspense fallback={<p className="lab-tm-msg">{t(locale, 'terms.loading')}</p>}>
				<Flow view={view} locale={locale} floor={floor} onHover={setHover} onActivate={onActivate} onFit={fit} />
			</Suspense>
		) : (
			<p className="lab-tm-msg">{t(locale, 'terms.noscript')}</p>
		);

	const status = shown ? (
		<>
			<b>{shown.label}</b> — {shown.def}
			{touch && picked === shown.id && (
				<>
					{' '}
					<a href={shown.href ?? shown.gloss}>{shown.href ? t(locale, 'terms.openLesson') : t(locale, 'terms.glossary')}</a>
				</>
			)}
		</>
	) : (
		t(locale, touch ? 'terms.hintTouch' : 'terms.hint')
	);

	const depths = DEPTHS.filter((d) => data.nodes.some((n) => n.depth === d));
	return (
		<figure
			className="lab-tm not-content"
			data-mode={mode}
			aria-labelledby={titleId}
			style={p.height ? ({ '--lab-tm-box': `${p.height}px` } as React.CSSProperties) : undefined}
		>
			{head(false)}
			<div className="lab-tm-box" role="img" aria-label={t(locale, 'terms.aria', { count: card.nodes.length })}>
				{draw(card, onFit)}
			</div>
			<Legend locale={locale} depths={depths} later={data.nodes.some((n) => !n.href)} />
			<p className="lab-tm-status" aria-live="polite">
				{status}
			</p>
			<TermList locale={locale} view={full} />
			<dialog ref={dialog} className="lab-tm-dialog" aria-label={title} onClose={() => setOpen(false)}>
				{head(true)}
				<div className="lab-tm-box">{open && draw(full, noFit)}</div>
				<p className="lab-tm-status">{status}</p>
			</dialog>
		</figure>
	);
}

function Legend({ locale, depths, later }: { locale: Locale; depths: readonly Depth[]; later: boolean }): React.JSX.Element {
	return (
		<div className="lab-tm-legend" aria-hidden="true">
			{depths.map((d) => (
				<span key={d} className="lab-tm-key" data-depth={d}>
					<span className="lab-tm-dot" />
					{t(locale, DEPTH_KEY[d])}
				</span>
			))}
			{later && (
				<span className="lab-tm-key">
					<span className="lab-tm-swatch" />
					{t(locale, 'terms.laterKey')}
				</span>
			)}
			{LEGEND.map((type) => (
				<span key={type} className="lab-tm-key">
					<svg width="28" height="10" viewBox="0 0 28 10" className={`lab-tm-edge t-${type}`}>
						{type === 'equivalent-to' && <path className="lab-tm-under" d="M1 5 H27" />}
						<path d={ARROWED.has(type) ? 'M1 5 H21' : 'M1 5 H27'} />
						{ARROWED.has(type) && <path className="lab-tm-head" d="M27 5 L20 1.5 L20 8.5 Z" />}
					</svg>
					{t(locale, OUT_KEY[type])}
				</span>
			))}
			<span className="lab-tm-note">{t(locale, 'terms.arrow')}</span>
		</div>
	);
}

/** The list view: every term of the map with its definition, its relations inside the map and its links. */
function TermList({ locale, view }: { locale: Locale; view: View }): React.JSX.Element {
	const byId = new Map(view.nodes.map((n) => [n.id, n]));
	return (
		<details className="lab-tm-list">
			<summary>{t(locale, 'terms.list', { count: view.nodes.length })}</summary>
			<ul>
				{view.nodes.map((n) => (
					<li key={n.id} data-depth={n.depth} className={view.focus.has(n.id) ? 'is-focus' : undefined}>
						<a href={n.gloss}>
							<b>{n.label}</b>
						</a>{' '}
						<span className="lab-tm-depth">({t(locale, DEPTH_KEY[n.depth])})</span> — {n.def}{' '}
						{n.href ? (
							<a href={n.href}>
								{t(locale, 'terms.lesson')}: {n.lesson}
							</a>
						) : (
							<i>{t(locale, 'terms.later')}</i>
						)}
						{relations(view.edges, n.id).map((r) => (
							<div key={`${r.dir}${r.type}`} className="lab-tm-rel">
								{t(locale, r.dir === 'out' ? OUT_KEY[r.type] : IN_KEY[r.type])}:{' '}
								{r.ids.map((id, i) => (
									<span key={id}>
										{i > 0 && ', '}
										{byId.get(id)?.label ?? id}
									</span>
								))}
							</div>
						))}
					</li>
				))}
			</ul>
		</details>
	);
}
