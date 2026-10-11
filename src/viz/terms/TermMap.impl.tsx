/**
 * The drawn term map (lazy chunk of TermMap.tsx): measures the node labels, gets positions and spline routes from ELK
 * (layout.ts, in a worker) and renders them with React Flow as a static, pannable/zoomable picture. Nodes are links
 * (not draggable, selectable or focusable: the keyboard and screen-reader route is the list view of the card).
 *
 * Opening view: React Flow's own `fitView` prop with options from `openView` (layout.ts), applied by React Flow once
 * the nodes are in its store. Every view gets a fresh React Flow store (the provider is keyed by the view): reusing
 * the store across a genealogy retarget fitted the OLD node set (the new nodes were not adopted yet) and left the new
 * tree at zoom 1, mostly outside the box. Box resizes refit with the same options.
 */
import { getNodesBounds, Handle, MarkerType, Position, ReactFlow, ReactFlowProvider, useReactFlow, type Edge, type EdgeProps, type Node, type NodeProps } from '@xyflow/react';
import '@xyflow/react/dist/base.css';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { t } from '../../i18n/core';
import { FIT_PAD, nodeH, wrapLabel, type EdgeType, type MapNode } from './graph';
import { nodeW, openView, place, type Placed } from './layout';
import type { FlowProps } from './TermMap';

type TermData = { node: MapNode; focus: boolean; activate: FlowProps['onActivate'] };
type TermNodeT = Node<TermData, 'term'>;
type TermEdgeT = Edge<{ d: string; type: EdgeType }, 'term'>;

/** A press that moved farther than this (CSS px) was a pan, not a click on the node under it. */
const CLICK_SLOP = 5;

function TermNode({ data }: NodeProps<TermNodeT>): React.JSX.Element {
	const n = data.node;
	const down = useRef<{ x: number; y: number } | null>(null);
	return (
		<a
			href={n.href ?? n.gloss}
			tabIndex={-1}
			draggable={false}
			className={`lab-tm-node${data.focus ? ' is-focus' : ''}${n.href ? '' : ' is-later'}`}
			data-depth={n.depth}
			onPointerDown={(e) => {
				down.current = { x: e.clientX, y: e.clientY };
			}}
			onClick={(e) => {
				const d = down.current;
				const moved = d !== null && Math.hypot(e.clientX - d.x, e.clientY - d.y) > CLICK_SLOP;
				const touch = (e.nativeEvent as PointerEvent).pointerType === 'touch';
				if (moved || !data.activate(n.id, touch)) e.preventDefault();
			}}
		>
			{/* React Flow draws an edge only between handles; ours are invisible because routes come from ELK. */}
			<Handle type="target" position={Position.Top} isConnectable={false} className="lab-tm-handle" />
			<span className="lab-tm-dot" />
			<span className="lab-tm-label">
				{wrapLabel(n.label).map((s) => (
					<span key={s}>{s}</span>
				))}
			</span>
			<Handle type="source" position={Position.Bottom} isConnectable={false} className="lab-tm-handle" />
		</a>
	);
}

function TermEdge({ data, markerEnd }: EdgeProps<TermEdgeT>): React.JSX.Element | null {
	if (!data) return null;
	return (
		<g className={`lab-tm-edge t-${data.type}`}>
			{data.type === 'equivalent-to' && <path className="lab-tm-under" d={data.d} />}
			<path d={data.d} markerEnd={markerEnd} />
		</g>
	);
}

const nodeTypes = { term: TermNode };
const edgeTypes = { term: TermEdge };

/** Edge types that carry an arrowhead (pointing at the upper/general end); equivalence is symmetric. */
const ARROW: ReadonlySet<EdgeType> = new Set(['built-on', 'is-a', 'used-in', 'special-case-of']);

/** Label font of a node box (terms.css `.lab-tm-node`), read from the page so canvas measurement matches the DOM. */
function labelFont(el: Element): string {
	const family = getComputedStyle(el).getPropertyValue('--lab-font-ui').trim() || 'system-ui, sans-serif';
	return `600 12px ${family}`;
}

export default function TermFlow(p: FlowProps): React.JSX.Element {
	const key = useMemo(() => `${p.view.nodes.map((n) => n.id).join()}|${p.view.edges.length}`, [p.view]);
	return (
		<ReactFlowProvider key={key}>
			<Inner {...p} />
		</ReactFlowProvider>
	);
}

function Inner({ view, locale, floor, onHover, onActivate, onFit }: FlowProps): React.JSX.Element {
	const host = useRef<HTMLDivElement>(null);
	const [res, setRes] = useState<{ placed?: Placed; error?: string } | null>(null);
	const [box, setBox] = useState<{ width: number; height: number } | null>(null);
	const rf = useReactFlow();
	// Node data must not change with the card's tap state (that would rebuild the nodes and refit the view), so nodes
	// call the latest onActivate through a ref.
	const act = useRef(onActivate);
	useEffect(() => {
		act.current = onActivate;
	}, [onActivate]);
	const activate = useCallback<FlowProps['onActivate']>((id, touch) => act.current(id, touch), []);

	useEffect(() => {
		const el = host.current;
		if (!el) return;
		let alive = true;
		const ctx = document.createElement('canvas').getContext('2d');
		if (!ctx) throw new Error('TermMap: no 2D canvas context to measure labels');
		ctx.font = labelFont(el);
		const sizes = new Map(
			view.nodes.map((n) => {
				const lines = wrapLabel(n.label);
				return [n.id, { w: nodeW(Math.max(...lines.map((s) => ctx.measureText(s).width))), h: nodeH(lines.length) }];
			}),
		);
		place(sizes, view.edges).then(
			(placed) => alive && setRes({ placed }),
			(e: unknown) => alive && setRes({ error: e instanceof Error ? e.message : String(e) }),
		);
		const ro = new ResizeObserver(() => {
			const w = el.clientWidth;
			const h = el.clientHeight;
			setBox((b) => (b && b.width === w && b.height === h ? b : { width: w, height: h }));
		});
		ro.observe(el);
		return () => {
			alive = false;
			ro.disconnect();
		};
	}, [view]);

	const placed = res?.placed ?? null;
	const nodes = useMemo<TermNodeT[]>(
		() =>
			placed
				? view.nodes.map((n) => {
						const b = placed.boxes.get(n.id);
						if (!b) throw new Error(`TermMap: no box for "${n.id}"`);
						return {
							id: n.id,
							type: 'term',
							position: { x: b.x, y: b.y },
							width: b.w,
							height: b.h,
							style: { width: b.w, height: b.h },
							data: { node: n, focus: view.focus.has(n.id), activate },
						};
					})
				: [],
		[placed, view, activate],
	);
	const edges = useMemo<TermEdgeT[]>(
		() =>
			placed
				? placed.edges.map((e, i) => {
						// The route runs lower -> upper for layered edges (graph.ts `flow`), from -> to for lateral ones.
						const [source, target] = e.type === 'used-in' || e.type === 'special-case-of' || e.type === 'equivalent-to' ? [e.from, e.to] : [e.to, e.from];
						return {
							id: `e${i}`,
							type: 'term',
							source,
							target,
							data: { d: e.d, type: e.type },
							...(ARROW.has(e.type) ? { markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14, color: 'var(--lab-tm-edge)' } } : {}),
						};
					})
				: [],
		[placed],
	);

	const opening = useMemo(() => {
		if (!placed || !box || nodes.length === 0) return null;
		const focus = nodes.filter((n) => n.data.focus).map((n) => n.id);
		return openView(getNodesBounds(nodes), box, focus, floor);
	}, [placed, box, nodes, floor]);

	// "Fit" shows everything; a box resize re-applies the opening view (React Flow's fitView prop runs only once).
	const first = useRef(true);
	useEffect(() => {
		if (!opening) return;
		onFit(() => void rf.fitView({ padding: FIT_PAD, maxZoom: 1.25 }));
		if (first.current) first.current = false;
		else void rf.fitView(opening);
		return () => onFit(null);
	}, [opening, rf, onFit]);

	// The credit link of React Flow stays, but out of the tab order: it lives inside the aria-hidden drawing.
	useEffect(() => {
		host.current?.querySelector('.react-flow__attribution a')?.setAttribute('tabindex', '-1');
	}, [opening]);

	return (
		<div ref={host} className="lab-tm-flow" aria-hidden="true">
			{res?.error ? (
				<p className="lab-tm-msg is-err">{t(locale, 'terms.failed', { error: res.error })}</p>
			) : !opening ? (
				<p className="lab-tm-msg">{t(locale, 'terms.loading')}</p>
			) : (
				<ReactFlow
					nodes={nodes}
					edges={edges}
					nodeTypes={nodeTypes}
					edgeTypes={edgeTypes}
					colorMode="dark"
					fitView
					fitViewOptions={opening}
					minZoom={0.2}
					maxZoom={2.5}
					nodesDraggable={false}
					nodesConnectable={false}
					nodesFocusable={false}
					edgesFocusable={false}
					elementsSelectable={false}
					disableKeyboardA11y
					zoomOnScroll={false}
					zoomOnDoubleClick={false}
					preventScrolling={false}
					panOnDrag
					zoomOnPinch
					onNodeMouseEnter={(_, n) => onHover(n.id)}
					onNodeMouseLeave={() => onHover(null)}
				/>
			)}
		</div>
	);
}
