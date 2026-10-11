/**
 * The drawn term map (lazy chunk of TermMap.tsx): measures the node labels, gets positions and spline routes from ELK
 * (layout.ts, in a worker) and renders them with React Flow as a static, pannable/zoomable picture. Nodes are not
 * draggable, selectable or focusable: the keyboard and screen-reader route is the list view of the card.
 */
import { getNodesBounds, Handle, MarkerType, Position, ReactFlow, ReactFlowProvider, useReactFlow, type Edge, type EdgeProps, type Node, type NodeProps } from '@xyflow/react';
import '@xyflow/react/dist/base.css';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { t } from '../../i18n/core';
import type { EdgeType, MapNode } from './graph';
import { NODE_H, nodeW, place, type Placed } from './layout';
import type { FlowProps } from './TermMap';

type TermData = { node: MapNode; focus: boolean };
type TermNodeT = Node<TermData, 'term'>;
type TermEdgeT = Edge<{ d: string; type: EdgeType }, 'term'>;

function TermNode({ data }: NodeProps<TermNodeT>): React.JSX.Element {
	const n = data.node;
	return (
		<div className={`lab-tm-node${data.focus ? ' is-focus' : ''}${n.href ? '' : ' is-later'}`} data-depth={n.depth}>
			{/* React Flow draws an edge only between handles; ours are invisible because routes come from ELK. */}
			<Handle type="target" position={Position.Top} isConnectable={false} className="lab-tm-handle" />
			<span className="lab-tm-dot" />
			{n.label}
			<Handle type="source" position={Position.Bottom} isConnectable={false} className="lab-tm-handle" />
		</div>
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

/** Lowest opening zoom: node labels are 12 px at zoom 1, so 0.8 keeps them at about 10 px. */
const READABLE = 0.8;
/** Lowest opening zoom of a genealogy overview on a wide box (labels about 7 px: shape first, details on hover). */
const GLANCE = 0.55;
/** Fit padding, as a fraction of the view. */
const PAD = 0.06;

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
	return (
		<ReactFlowProvider>
			<Inner {...p} />
		</ReactFlowProvider>
	);
}

function Inner({ view, locale, pan, overview, onHover, onPick, onFit }: FlowProps): React.JSX.Element {
	const host = useRef<HTMLDivElement>(null);
	// A result belongs to the view it was computed for: after a view change (genealogy retarget) the previous layout
	// must not be drawn with the new nodes, even for the one render before the new layout arrives.
	const [res, setRes] = useState<{ view: FlowProps['view']; placed?: Placed; error?: string } | null>(null);
	const placed = res?.view === view ? (res.placed ?? null) : null;
	const error = res?.view === view ? (res.error ?? null) : null;
	const rf = useReactFlow();

	useEffect(() => {
		const el = host.current;
		if (!el) return;
		let alive = true;
		const ctx = document.createElement('canvas').getContext('2d');
		if (!ctx) throw new Error('TermMap: no 2D canvas context to measure labels');
		ctx.font = labelFont(el);
		const widths = new Map(view.nodes.map((n) => [n.id, nodeW(ctx.measureText(n.label).width)]));
		place(widths, view.edges).then(
			(r) => alive && setRes({ view, placed: r }),
			(e: unknown) => alive && setRes({ view, error: e instanceof Error ? e.message : String(e) }),
		);
		return () => {
			alive = false;
		};
	}, [view]);

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
							height: NODE_H,
							style: { width: b.w, height: NODE_H },
							data: { node: n, focus: view.focus.has(n.id) },
						};
					})
				: [],
		[placed, view],
	);
	const edges = useMemo<TermEdgeT[]>(
		() =>
			placed
				? placed.edges.map((e, i) => {
						// The route runs lower -> upper for layered edges (layout.ts `flow`), from -> to for lateral ones.
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

	// The opening view keeps labels readable: the whole map when it fits at the floor zoom or more,
	// otherwise the focus terms at the floor zoom (pan, full screen, or "Fit" for everything). Re-applied when
	// the box changes size (the card is fluid; the dialog is the viewport). "Fit" always shows everything.
	const fitStart = useCallback(() => {
		const el = host.current;
		if (!placed || !el) return;
		const b = getNodesBounds(nodes);
		const all = Math.min(el.clientWidth / b.width, el.clientHeight / b.height) / (1 + 2 * PAD);
		const focus = nodes.filter((n) => n.data.focus).map((n) => ({ id: n.id }));
		// A genealogy is read as a whole tree, so on a wide box it may open smaller; on a phone-wide box it may not.
		const floor = overview && el.clientWidth >= 480 ? GLANCE : READABLE;
		if (all >= floor || focus.length === 0) void rf.fitView({ padding: PAD, maxZoom: 1.25 });
		else void rf.fitView({ padding: PAD, nodes: focus, minZoom: floor, maxZoom: Math.max(floor, 1) });
	}, [placed, nodes, rf, overview]);
	useEffect(() => {
		if (!placed) return;
		onFit(() => void rf.fitView({ padding: PAD, maxZoom: 1.25 }));
		const el = host.current;
		const ro = el ? new ResizeObserver(() => fitStart()) : null;
		if (el) ro?.observe(el);
		return () => {
			ro?.disconnect();
			onFit(null);
		};
	}, [placed, rf, onFit, fitStart]);

	return (
		<div ref={host} className="lab-tm-flow" aria-hidden="true">
			{error ? (
				<p className="lab-tm-msg is-err">{t(locale, 'terms.failed', { error })}</p>
			) : !placed ? (
				<p className="lab-tm-msg">{t(locale, 'terms.loading')}</p>
			) : (
				<ReactFlow
					nodes={nodes}
					edges={edges}
					nodeTypes={nodeTypes}
					edgeTypes={edgeTypes}
					colorMode="dark"
					onInit={fitStart}
					minZoom={0.25}
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
					panOnDrag={pan}
					zoomOnPinch={pan}
					onNodeMouseEnter={(_, n) => onHover(n.id)}
					onNodeMouseLeave={() => onHover(null)}
					onNodeClick={(e, n) => onPick(n.id, (e.nativeEvent as PointerEvent).pointerType === 'touch')}
				/>
			)}
		</div>
	);
}
