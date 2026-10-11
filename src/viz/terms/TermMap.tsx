/**
 * TermMap: a mind map of the term graph (graph.ts), in the 6d Blender viewport look (always dark, like the scenes).
 *
 * Two modes, one component:
 *   - `local`: the slice the server computed for a lesson (its own terms, highlighted, plus one hop around them);
 *   - `genealogy`: the whole graph arrives, the reader picks a term and the map shows it with everything it is built
 *     on, down to the roots, plus the machine-level terms used in that chain.
 *
 * SSR-safe entry: the server renders the card, the legend and the list view (the accessible twin of the map: the same
 * terms, definitions, relations and links, keyboard- and screen-reader-friendly, and the whole content without
 * JavaScript). The drawn map (React Flow + ELK in a worker) is a lazy chunk fetched after hydration, which the
 * wrapper schedules with `client:visible`, so pages without a map never load it and pages with one load it only when
 * the map scrolls into view. The map itself is pointer-only (`aria-hidden`); the list carries the semantics.
 *
 * Interaction: hover (mouse) or tap (touch) shows a term's definition in the status line; click opens the lesson that
 * introduces the term (forward terms: their glossary entry); touch needs a second tap so reading never navigates.
 * Plain wheel scrolls the page (Ruling T); Ctrl+wheel / pinch zoom. Inline on touch the map does not pan (a swipe
 * scrolls the page); the full-screen view (a modal <dialog>, top layer, Escape closes) pans and zooms.
 */
import { lazy, Suspense, useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';
import { t, type Locale, type TextKey } from '../../i18n/core';
import { genealogyIn, relations, type Depth, type EdgeType, type MapData, type MapEdge, type MapNode } from './graph';
import './terms.css';

const Flow = lazy(() => import('./TermMap.impl'));

export interface TermMapProps {
	locale: Locale;
	data: MapData;
	mode: 'local' | 'genealogy';
	/** Highlighted terms (local: the lesson's own terms); genealogy: the initially selected term. */
	focus: string[];
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

/** Edge types in legend order, with the stroke style terms.css gives each. */
const LEGEND: readonly EdgeType[] = ['built-on', 'is-a', 'special-case-of', 'equivalent-to', 'used-in'];

/** What the drawn map shows: node list (slice order) and its edges. */
export interface View {
	nodes: MapNode[];
	edges: MapEdge[];
	focus: ReadonlySet<string>;
}

/** Callbacks the lazy map reports to the card. */
export interface FlowProps {
	view: View;
	locale: Locale;
	/** Pan with a drag and zoom with a pinch (full screen, or any fine pointer). */
	pan: boolean;
	/** A whole tree (genealogy): on a wide box it opens on everything down to a lower zoom than a lesson map. */
	overview: boolean;
	onHover: (id: string | null) => void;
	onPick: (id: string, touch: boolean) => void;
	/** Receives the fit-view action once the map is laid out. */
	onFit: (fit: (() => void) | null) => void;
}

export function TermMap(p: TermMapProps): React.JSX.Element {
	const { locale, data, mode } = p;
	const byId = useMemo(() => new Map(data.nodes.map((n) => [n.id, n])), [data]);
	const [sel, setSel] = useState(p.focus[0] ?? data.nodes[0]?.id ?? '');
	if (!byId.has(sel)) throw new Error(`TermMap: focus "${sel}" is not in the map data`);
	const view = useMemo<View>(() => {
		if (mode === 'local') return { nodes: data.nodes, edges: data.edges, focus: new Set(p.focus) };
		const order = data.nodes.map((n) => n.id);
		const s = genealogyIn(order, data.edges, sel, (id) => byId.get(id)?.depth === 'machine');
		return { nodes: s.ids.map((id) => byId.get(id) as MapNode), edges: s.edges, focus: new Set([sel]) };
	}, [mode, data, sel, byId, p.focus]);

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
	const onPick = useCallback(
		(id: string, viaTouch: boolean) => {
			const n = byId.get(id);
			if (!n) return;
			if (viaTouch && picked !== id) {
				setPicked(id);
				return;
			}
			location.assign(n.href ?? n.gloss);
		},
		[byId, picked],
	);
	const shown = byId.get(hover ?? picked ?? '');

	const fitRef = useRef<(() => void) | null>(null);
	const onFit = useCallback((f: (() => void) | null) => {
		fitRef.current = f;
	}, []);
	const dialog = useRef<HTMLDialogElement>(null);
	const [full, setFull] = useState(false);
	const titleId = useId();
	const title = t(locale, mode === 'local' ? 'terms.title' : 'terms.genealogy');

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
					title={t(locale, 'terms.full')}
					aria-label={t(locale, 'terms.full')}
					onClick={() => {
						setFull(true);
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

	const flow = (pan: boolean, fit: FlowProps['onFit']) =>
		live ? (
			<Suspense fallback={<p className="lab-tm-msg">{t(locale, 'terms.loading')}</p>}>
				<Flow view={view} locale={locale} pan={pan} overview={mode === 'genealogy'} onHover={setHover} onPick={onPick} onFit={fit} />
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

	return (
		<figure className="lab-tm not-content" data-mode={mode} aria-labelledby={titleId}>
			{head(false)}
			<div className="lab-tm-box" role="img" aria-label={t(locale, 'terms.aria', { count: view.nodes.length })}>
				{flow(!touch, onFit)}
			</div>
			<Legend locale={locale} />
			<p className="lab-tm-status" aria-live="polite">
				{status}
			</p>
			<TermList locale={locale} view={view} />
			<dialog ref={dialog} className="lab-tm-dialog" aria-label={title} onClose={() => setFull(false)}>
				{head(true)}
				<div className="lab-tm-box">{full && flow(true, () => undefined)}</div>
				<p className="lab-tm-status">{status}</p>
			</dialog>
		</figure>
	);
}

function Legend({ locale }: { locale: Locale }): React.JSX.Element {
	return (
		<div className="lab-tm-legend" aria-hidden="true">
			{(['ground', 'vectors', 'machine'] as const).map((d) => (
				<span key={d} className="lab-tm-key" data-depth={d}>
					<span className="lab-tm-dot" />
					{t(locale, DEPTH_KEY[d])}
				</span>
			))}
			{LEGEND.map((type) => (
				<span key={type} className="lab-tm-key">
					<svg width="26" height="10" viewBox="0 0 26 10" className={`lab-tm-edge t-${type}`}>
						{type === 'equivalent-to' && <path className="lab-tm-under" d="M1 5 H25" />}
						<path d="M1 5 H25" />
					</svg>
					{t(locale, OUT_KEY[type])}
				</span>
			))}
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
