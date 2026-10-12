import { useStore } from 'zustand';
import { roleVar, type SceneRole } from '../palette';
import { hotOff, hotOn, lessonStore } from '../store';
import { stripWindow } from './tensor-cells';
import './tensor-cells.css';

/**
 * A flat memory buffer as one strip of cells in address order (lessons 3.1-3.3), DOM-only and three-free. Each cell
 * shows its address (small, top), the value stored there and optionally a second small number under it (the step at
 * which a walk reads it). Hover and focus light the cell's term id, so the same id lights the matching cell in a scene;
 * a click selects it. Only a window of `max` cells around `center` is listed; the rest is counted at the ends.
 * Words come from the caller (each lesson's dictionary namespace).
 */
export interface StripCell {
	/** The value as printed. */
	text: string;
	/** Bottom number (walk step), or null for none. */
	step?: number | null;
	/** Colour of the value (e.g. the channel of an image byte). */
	role?: SceneRole;
	/** Dim a cell the current view does not reach. */
	dim?: boolean;
	/** Accessible name of the cell button. */
	label: string;
	/** Term id lit on hover (shared with the scene). */
	hot: string;
}

/** A tuple with each number in its axis colour, `(2, 3, 4)` or `(4,)`. Axis colours are the viewport's: use a dark box. */
export function Tuple({ nums, roles }: { nums: readonly (number | string)[]; roles: readonly SceneRole[] }): React.JSX.Element {
	return <span className="ts-tuple">
		(
		{nums.map((n, a) => <span key={a}>
			{a > 0 && ', '}
			<span style={{ color: roleVar(roles[a]) }}>{n}</span>
		</span>)}
		{nums.length === 1 && ','}
		)
	</span>;
}

export interface MemStripProps {
	lesson: string;
	/** Accessible name of the whole strip. */
	label: string;
	/** Number of cells in the buffer. */
	n: number;
	/** Address kept in the middle of the listed window. */
	center: number;
	/** Selected address (orange outline), if any. */
	sel?: number | null;
	cell(p: number): StripCell;
	/** "… k more" at either end. */
	more(k: number): string;
	onSelect?(p: number): void;
	max?: number;
}

export function MemStrip({ lesson, label, n, center, sel, cell, more, onSelect, max = 48 }: MemStripProps): React.JSX.Element {
	const hot = useStore(lessonStore(lesson), (s) => s.hot);
	const { from, to } = stripWindow(n, center, max);
	return <div className="ts-strip" role="list" aria-label={label}>
		{from > 0 && <span className="ts-more">{more(from)}</span>}
		{Array.from({ length: to - from }, (_, k) => {
			const p = from + k, c = cell(p);
			const cls = 'ts-cell' + (p === sel ? ' is-sel' : '') + (hot.has(c.hot) ? ' is-hot' : '') + (c.dim ? ' is-dim' : '');
			return <button key={p} type="button" role="listitem" className={cls} data-t={c.hot} aria-label={c.label}
				onPointerEnter={() => hotOn(lesson, c.hot)} onPointerLeave={() => hotOff(lesson, c.hot)}
				onFocus={() => hotOn(lesson, c.hot)} onBlur={() => hotOff(lesson, c.hot)} onClick={() => onSelect?.(p)}>
				<span className="ts-addr">{p}</span>
				<span style={c.role ? { color: roleVar(c.role) } : undefined}>{c.text}</span>
				{c.step !== undefined && <span className="ts-step">{c.step ?? '·'}</span>}
			</button>;
		})}
		{to < n && <span className="ts-more">{more(n - to)}</span>}
	</div>;
}
