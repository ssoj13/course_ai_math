/**
 * Layout of a term map: node sizes measured on the page, positions and edge routes computed by ELK's layered algorithm
 * in a Web Worker (elk.worker.ts), so a 60-term genealogy never blocks scrolling. Loaded only by the lazy map
 * implementation (TermMap.impl.tsx): pages without a map never fetch ELK.
 *
 * Orientation: `elk.direction: UP` puts the source of an edge below its target, so every layered edge is sent from
 * the more fundamental term to the one that rests on it: foundations sit at the bottom ("built on"), machine terms
 * under what they implement. `special-case-of` / `equivalent-to` are lateral relations: they do not constrain the
 * layers and are drawn afterwards as straight lines between the placed boxes.
 */
import { FIT_PAD, flow, isLayered, LAYER_GAP, type MapEdge } from './graph';

/** A node box in map coordinates (top-left origin). */
export interface Box {
	x: number;
	y: number;
	w: number;
	h: number;
}

/** A routed edge: the SVG path from the lower/source term to the upper/target term, arrowhead at the end. */
export interface Routed extends MapEdge {
	d: string;
}

export interface Placed {
	boxes: Map<string, Box>;
	edges: Routed[];
}

export interface Point {
	x: number;
	y: number;
}

/** Node paddings in CSS px (terms.css draws the same box: 12 px text, 8 px depth dot + 6 px gap). */
const PAD_X = 10;
const DOT = 14; // dot width + gap


/** Box width for a label of `textW` px. */
export const nodeW = (textW: number): number => Math.ceil(textW + 2 * PAD_X + DOT);

/**
 * SVG path through ELK spline output: start, then (c1, c2, end) triples, the last end being `end`. ELK emits
 * 3k + 2 bend points for a k + 1 segment cubic chain; anything else (or no bends) is drawn as a polyline.
 */
export function splinePath(start: Point, bends: readonly Point[], end: Point): string {
	const f = (p: Point) => `${round(p.x)},${round(p.y)}`;
	if (bends.length % 3 === 2) {
		const pts = [...bends, end];
		let d = `M${f(start)}`;
		for (let i = 0; i < pts.length; i += 3) d += ` C${f(pts[i] as Point)} ${f(pts[i + 1] as Point)} ${f(pts[i + 2] as Point)}`;
		return d;
	}
	return `M${f(start)}${[...bends, end].map((p) => ` L${f(p)}`).join('')}`;
}

const round = (v: number) => Math.round(v * 10) / 10;

/** Point where the segment from the centre of `b` toward `p` leaves the box (for lateral edges). */
export function exitPoint(b: Box, p: Point): Point {
	const cx = b.x + b.w / 2;
	const cy = b.y + b.h / 2;
	const dx = p.x - cx;
	const dy = p.y - cy;
	if (dx === 0 && dy === 0) return { x: cx, y: cy };
	const s = Math.min(dx ? b.w / 2 / Math.abs(dx) : Infinity, dy ? b.h / 2 / Math.abs(dy) : Infinity);
	return { x: cx + dx * s, y: cy + dy * s };
}

/** Straight path between two boxes, border to border. */
export function straightPath(a: Box, b: Box): string {
	const ca = { x: a.x + a.w / 2, y: a.y + a.h / 2 };
	const cb = { x: b.x + b.w / 2, y: b.y + b.h / 2 };
	return splinePath(exitPoint(a, cb), [], exitPoint(b, ca));
}

// ---- worker client: ELK's own message protocol ({ id, cmd, ... } -> { id, data } | { id, error }) ----

interface Pending {
	res: (data: unknown) => void;
	rej: (err: Error) => void;
}

/** The running worker and its registration round trip (ELK needs its algorithms registered before a layout). */
let elk: { w: Worker; ready: Promise<unknown> } | null = null;
let seq = 0;
const pending = new Map<number, Pending>();

function failAll(err: Error): void {
	for (const p of pending.values()) p.rej(err);
	pending.clear();
	elk?.w.terminate();
	elk = null; // the next layout starts a fresh worker
}

function post(w: Worker, msg: Record<string, unknown>): Promise<unknown> {
	const id = ++seq;
	return new Promise((res, rej) => {
		pending.set(id, { res, rej });
		w.postMessage({ ...msg, id });
	});
}

function start(): { w: Worker; ready: Promise<unknown> } {
	const w = new Worker(new URL('./elk.worker.ts', import.meta.url), { type: 'module' });
	w.onmessage = (e: MessageEvent<{ id: number; data?: unknown; error?: unknown }>) => {
		const p = pending.get(e.data.id);
		if (!p) return;
		pending.delete(e.data.id);
		const err = e.data.error;
		if (err === undefined) p.res(e.data.data);
		else p.rej(new Error(typeof err === 'object' && err && 'message' in err ? String(err.message) : String(err)));
	};
	w.onerror = (e) => failAll(new Error(e.message || 'the layout worker failed to start'));
	const ready = post(w, { cmd: 'register', algorithms: ['layered'] });
	// The error itself reaches every waiting send(); here a broken worker is only dropped so a retry starts afresh.
	ready.catch(() => {
		if (elk?.w !== w) return;
		w.terminate();
		elk = null;
	});
	return { w, ready };
}

/** Sends one command after the registration has succeeded (a failed registration rejects every command). */
async function send(msg: Record<string, unknown>): Promise<unknown> {
	elk ??= start();
	const { w, ready } = elk;
	await ready;
	return post(w, msg);
}

interface ElkOut {
	children?: { id: string; x?: number; y?: number; width?: number; height?: number }[];
	edges?: { id: string; sections?: { startPoint: Point; endPoint: Point; bendPoints?: Point[] }[] }[];
}

/** Layout options of every term map (the grid units are CSS px at zoom 1). */
const OPTIONS: Record<string, string> = {
	'elk.algorithm': 'layered',
	'elk.direction': 'UP',
	'elk.edgeRouting': 'SPLINES',
	'elk.spacing.nodeNode': '12',
	'elk.layered.spacing.nodeNodeBetweenLayers': String(LAYER_GAP),
	'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
	// Network-simplex placement packs a layer tighter than the default (dot-product card: 715 -> 640 px wide).
	'elk.layered.nodePlacement.strategy': 'NETWORK_SIMPLEX',
	'elk.padding': '[top=8,left=8,bottom=8,right=8]',
};

/**
 * Places the given nodes (id -> box size) and routes the edges. Results are memoized per graph signature, so the
 * inline map and its full-screen copy share one layout.
 */
export function place(sizes: ReadonlyMap<string, { w: number; h: number }>, edges: readonly MapEdge[]): Promise<Placed> {
	const key = JSON.stringify([[...sizes], edges]);
	let p = cache.get(key);
	if (!p) {
		p = run(sizes, edges);
		cache.set(key, p);
		p.catch(() => cache.delete(key)); // a failed layout may be retried
	}
	return p;
}

const cache = new Map<string, Promise<Placed>>();

async function run(sizes: ReadonlyMap<string, { w: number; h: number }>, edges: readonly MapEdge[]): Promise<Placed> {
	const lay = edges.map((e, i) => ({ e, i })).filter(({ e }) => isLayered(e.type));
	const graph = {
		id: 'root',
		layoutOptions: OPTIONS,
		children: [...sizes].map(([id, s]) => ({ id, width: s.w, height: s.h })),
		edges: lay.map(({ e, i }) => {
			const [lo, up] = flow(e);
			return { id: `e${i}`, sources: [lo], targets: [up] };
		}),
	};
	const out = (await send({ cmd: 'layout', graph, layoutOptions: {}, options: {} })) as ElkOut;
	const boxes = new Map<string, Box>();
	for (const c of out.children ?? []) boxes.set(c.id, { x: c.x ?? 0, y: c.y ?? 0, w: c.width ?? 0, h: c.height ?? 0 });
	for (const id of sizes.keys()) if (!boxes.has(id)) throw new Error(`layout: ELK returned no position for "${id}"`);
	const routes = new Map((out.edges ?? []).map((e) => [e.id, e.sections?.[0]]));
	const routed = edges.map((e, i): Routed => {
		const s = routes.get(`e${i}`);
		if (isLayered(e.type)) {
			if (!s) throw new Error(`layout: ELK returned no route for ${e.from} ${e.type} ${e.to}`);
			return { ...e, d: splinePath(s.startPoint, s.bendPoints ?? [], s.endPoint) };
		}
		const a = boxes.get(e.from) as Box;
		const b = boxes.get(e.to) as Box;
		return { ...e, d: straightPath(a, b) };
	});
	return { boxes, edges: routed };
}

/** How a map opens: React Flow `fitView` options. */
export interface Opening {
	padding: number;
	maxZoom: number;
	minZoom?: number;
	nodes?: { id: string }[];
}

/**
 * Opening view of a map with content `bounds` in a box: everything when that keeps the zoom at `floor` or more,
 * otherwise the focus terms at zoom `floor` (the reader pans to the rest, or uses Fit / full screen). `floor` 0 =
 * always everything (a genealogy is read as a whole tree).
 */
export function openView(bounds: { width: number; height: number }, box: { width: number; height: number }, focus: readonly string[], floor: number): Opening {
	const all = Math.min(box.width / bounds.width, box.height / bounds.height) / (1 + 2 * FIT_PAD);
	if (all >= floor || focus.length === 0) return { padding: FIT_PAD, maxZoom: 1.25 };
	// Exactly the floor zoom: zooming further in on the focus would only push more of the map out of the box.
	return { padding: FIT_PAD, nodes: focus.map((id) => ({ id })), minZoom: floor, maxZoom: floor };
}