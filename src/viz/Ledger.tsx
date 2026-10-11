/**
 * The Ledger (lessons spec LC-1, §5 item 5): an equation as an area balance, drawn as one stacked column (DOM SVG,
 * page-side, follows the Starlight theme). The first items are non-negative areas stacked from the bottom (a², b²);
 * the LAST item is signed: negative = a hatched band hanging down from the top of the stack (the area taken away),
 * positive = a solid band added on top, zero = no band, just a `light` line labelled `zeroLabel` (e.g.
 * "= Pythagoras"). A rule at the height of `total` is the result (c²). The scale is FIXED (`scale` px per unit, the
 * SVG sized for `max` units): bars visibly grow as the reader drags, which an auto-fit would hide.
 *
 * Every bar and the rule carry term ids: hovering lights the id everywhere, and a bar gets the hot outline when its id
 * is lit elsewhere. Sign is never colour-only: hatch plus an arrow (down = subtracted, up = added).
 */
import { useId } from 'react';
import { useStore } from 'zustand';
import { roleVar, type SceneRole } from './palette';
import { hotOff, hotOn, lessonStore, type LessonState } from './store';

type Vals = LessonState['vals'];

export interface LedgerItem {
	id: string;
	label: string;
	role: SceneRole;
	value: (vals: Vals) => number;
}

export interface LedgerProps {
	lesson: string;
	/** Bottom to top; all but the last must be >= 0, the last is signed. */
	items: readonly LedgerItem[];
	total: { id: string; label: string; value: (vals: Vals) => number };
	/** px per unit of area. */
	scale: number;
	/** Largest reachable stack height in units: fixes the SVG height (no layout shift). */
	max: number;
	/** Text of the zero line where the signed item vanishes. */
	zeroLabel?: string;
	/** Number format of the labels (default 2 decimals, true minus, never -0.00). */
	fmt?: (v: number) => string;
	/** Accessible name. */
	label: string;
}

/** Below this the signed item counts as zero (the derive functions already make 90 degrees exact). */
const ZERO = 1e-9;
const COL = 72;
const PAD = 8;
const LABEL_X = COL + 14;
const W = 270;
/** Minimum vertical distance between two label baselines. */
const LINE = 16;

export const fmt2 = (v: number): string => (Number.isFinite(v) ? (v + 0).toFixed(2).replace(/^-(0\.00)$/, '$1').replace('-', '−') : '–');

export interface LedgerBar {
	id: string;
	/** Bottom and top in units. */
	lo: number;
	hi: number;
	neg: boolean;
}

/**
 * Pure layout in units: stacked bars of the first items, the signed band of the last (hanging down from the stack
 * top when negative, on top when positive, absent at zero) and the rule height. Negative non-last items are an
 * authoring error (areas of squares cannot be negative).
 */
export function ledgerLayout(ids: readonly string[], values: readonly number[]): { bars: LedgerBar[]; top: number; zero: boolean } {
	if (ids.length !== values.length || ids.length < 2) throw new Error('Ledger needs at least two items (stacked + signed)');
	const bars: LedgerBar[] = [];
	let top = 0;
	for (let i = 0; i < values.length - 1; i++) {
		const v = values[i];
		if (!(v >= 0)) throw new Error(`Ledger item "${ids[i]}" must be a non-negative area, got ${v}`);
		bars.push({ id: ids[i], lo: top, hi: top + v, neg: false });
		top += v;
	}
	const c = values[values.length - 1];
	const zero = !(Math.abs(c) >= ZERO);
	if (!zero) bars.push({ id: ids[ids.length - 1], lo: Math.min(top, top + c), hi: Math.max(top, top + c), neg: c < 0 });
	return { bars, top, zero };
}

/**
 * Moves label centers (px, y down) apart so no two are closer than `min`, keeping their order and staying within
 * [lo, hi]: pushed down from the top first, then back up from the bottom if that ran past `hi`.
 */
export function spreadLabels(ys: readonly number[], lo: number, hi: number, min = LINE): number[] {
	const order = ys.map((y, i) => [y, i] as const).sort((a, b) => a[0] - b[0]);
	const v = order.map(([y]) => y);
	for (let k = 0; k < v.length; k++) v[k] = Math.max(v[k], k ? v[k - 1] + min : lo);
	for (let k = v.length - 1; k >= 0; k--) v[k] = Math.min(v[k], k < v.length - 1 ? v[k + 1] - min : hi);
	const out = new Array<number>(ys.length);
	order.forEach(([, i], k) => (out[i] = v[k]));
	return out;
}

/**
 * Vertical position as a CSS transform: everything that moves while the reader drags is placed this way, because a
 * changed `y` attribute is a layout shift (measured: Chrome reported the bars and labels of a drag as shifts) and a
 * transform is not.
 */
const ty = (y: number): React.CSSProperties => ({ transform: `translateY(${+y.toFixed(2)}px)` });

export function Ledger(p: LedgerProps): React.JSX.Element {
	const vals = useStore(lessonStore(p.lesson), (s) => s.vals);
	const hot = useStore(lessonStore(p.lesson), (s) => s.hot);
	const uid = useId().replace(/:/g, '');
	const f = p.fmt ?? fmt2;
	const values = p.items.map((it) => it.value(vals));
	const total = p.total.value(vals);
	const { bars, top, zero } = ledgerLayout(
		p.items.map((it) => it.id),
		values,
	);
	const H = p.max * p.scale;
	const y = (u: number) => PAD + H - Math.min(p.max, Math.max(0, u)) * p.scale;
	const last = p.items[p.items.length - 1];
	// Labels: one per drawn bar at its middle, plus the rule; pushed apart so thin bars never overlap their text.
	const rows = [
		...bars.map((b) => {
			const i = p.items.findIndex((it) => it.id === b.id);
			const it = p.items[i];
			const arrow = it === last ? (b.neg ? '↓ ' : '↑ ') : '';
			return { id: b.id, y: (y(b.lo) + y(b.hi)) / 2, text: `${arrow}${it.label} ${f(values[i])}` };
		}),
		...(zero ? [{ id: last.id, y: y(top), text: `${last.label} ${f(0)}${p.zeroLabel ? ` ${p.zeroLabel}` : ''}` }] : []),
		{ id: p.total.id, y: y(total), text: `${p.total.label} = ${f(total)}` },
	];
	const ly = spreadLabels(
		rows.map((r) => r.y),
		PAD,
		PAD + H,
	);
	const hov = (id: string) => ({
		onPointerEnter: () => hotOn(p.lesson, id),
		onPointerLeave: () => hotOff(p.lesson, id),
	});
	const summary = `${p.items.map((it, i) => `${it.label} ${f(values[i])}`).join(', ')}; ${p.total.label} ${f(total)}`;
	return (
		<figure className="lab-ledger not-content">
			<svg width={W} height={H + 2 * PAD} viewBox={`0 0 ${W} ${H + 2 * PAD}`} role="img" aria-label={`${p.label}: ${summary}`}>
				<defs>
					<pattern id={`${uid}h`} patternUnits="userSpaceOnUse" width="6" height="6" patternTransform="rotate(45)">
						<line x1="0" y1="0" x2="0" y2="6" stroke={roleVar(last.role)} strokeWidth="1.5" />
					</pattern>
				</defs>
				<line className="lab-ledger-base" x1={0} x2={COL} y1={y(0)} y2={y(0)} />
				{bars.map((b) => {
					const it = p.items.find((i) => i.id === b.id)!;
					const lit = hot.has(b.id);
					return (
						<g key={b.id} data-id={b.id} {...hov(b.id)}>
							<rect
								x={0}
								y={0}
								style={ty(y(b.hi))}
								width={COL}
								height={Math.max(0, y(b.lo) - y(b.hi))}
								fill={b.neg ? `url(#${uid}h)` : roleVar(it.role)}
								fillOpacity={b.neg ? 1 : 0.55}
								stroke={lit ? 'var(--viz-hot)' : roleVar(it.role)}
								strokeWidth={lit ? 2 : 1}
							/>
						</g>
					);
				})}
				{zero && <line data-id={last.id} x1={0} x2={COL} y1={0} y2={0} style={ty(y(top))} className="lab-ledger-zero" strokeWidth={2} {...hov(last.id)} />}
				<line
					data-id={p.total.id}
					className="lab-ledger-rule"
					x1={-2}
					x2={COL + 8}
					y1={0}
					y2={0}
					style={ty(y(total))}
					strokeWidth={hot.has(p.total.id) ? 3 : 1.5}
					stroke={hot.has(p.total.id) ? 'var(--viz-hot)' : 'currentColor'}
					{...hov(p.total.id)}
				/>
				{rows.map((r, i) => (
					<text
						key={`${r.id}${i}`}
						x={LABEL_X}
						y={0}
						style={ty(ly[i])}
						dominantBaseline="middle"
						className={hot.has(r.id) ? 'lab-ledger-t is-hot' : 'lab-ledger-t'}
						{...hov(r.id)}
					>
						{r.text}
					</text>
				))}
			</svg>
		</figure>
	);
}
