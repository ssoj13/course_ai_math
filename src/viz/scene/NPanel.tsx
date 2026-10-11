/**
 * The N-panel (Blender's sidebar): the linked "numbers view" of a scene. Each row is label | value: editable rows
 * are Blender number fields (drag horizontally to scrub, Ctrl snaps, Shift is fine, click or Enter to type, arrows and
 * PageUp/PageDown to step), vector rows have one field per component, read-only rows are flat text. Every row carries
 * a term id: hovering or focusing it lights that id everywhere (scene outline, formula term), and a row lights up
 * when its id is hot elsewhere. Under 720 px the same DOM lays out as a row of chips under the viewport (lab.css).
 *
 * One drag implementation in the codebase: fields use `startScrubDrag` (useScrub.ts) and the pure math of scrub.ts
 * (scrubValue, keyStep, ctrlSnap, parseField, fmtField), exactly like Formula and Num.
 * Three-free (DOM only), so it is tested in happy-dom.
 */
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react';
import { useStore } from 'zustand';
import type { V } from '../geom';
import { roleVar } from '../palette';
import { ctrlSnap, fmtField, keyStep, parseField, scrubValue, type ScrubSpec } from '../scrub';
import { hotOff, hotOn, lessonStore, type LessonState } from '../store';
import { renderStaticMath } from '../tex';
import { startScrubDrag } from '../useScrub';
import type { StageReg } from './handles';

type Vals = LessonState['vals'];

/** Tooltip content (Blender-like): title, one line, and optionally the formula term as TeX. */
export interface Tip {
	title?: string;
	text: string;
	tex?: string;
}

export interface Row {
	/** Term id shared with the scene object, formula term and table cell. */
	id: string;
	label: string;
	/** num: editable number; vec: editable vector (one field per component); ro: read-only value. */
	kind: 'num' | 'vec' | 'ro';
	/** Store key (required for num/vec; for ro either `key` or `value`). */
	key?: string;
	spec?: ScrubSpec;
	/** Ctrl while dragging: snap to these targets, or to multiples of this increment (default 10 steps). */
	snap?: number | readonly number[];
	/** Read-only value derived from the store. */
	value?: (vals: Vals) => number | V;
	fmt?: (v: number) => string;
	/** Row shown only while true (e.g. revealed by a timeline step). */
	visible?: (vals: Vals, step: Readonly<Record<string, number>>) => boolean;
	/** Group caption: a separator is drawn where the group changes. */
	group?: string;
	tip?: Tip;
}

/** Pixels of horizontal travel per step while scrubbing (the same rate as Formula/Num). */
const PX_PER_STEP = 4;
/** Travel below which a press counts as a click (opens the text input). */
const CLICK_PX = 3;
/** Touch/pen travel before a gesture counts as a horizontal scrub (or a vertical swipe): fingers jitter more. */
const TOUCH_SLOP = 8;
const AXIS = ['x', 'y', 'z'] as const;
const AXIS_ROLE = ['axisX', 'axisY', 'axisZ'] as const;

function checkRow(r: Row): void {
	if ((r.kind === 'num' || r.kind === 'vec') && (!r.key || !r.spec)) throw new Error(`N-panel row "${r.id}": a ${r.kind} row needs \`key\` and \`spec\``);
	if (r.kind === 'ro' && !r.key === !r.value) throw new Error(`N-panel row "${r.id}": a read-only row needs a key or value (exactly one)`);
}

/** Shows `tip` near the hovered element after Blender's 500 ms delay (mouse only); hidden on leave, press or scroll. */
export function useTip(): { bind: (tip: Tip | undefined) => React.HTMLAttributes<HTMLElement>; node: ReactNode } {
	const [shown, setShown] = useState<{ tip: Tip; style: React.CSSProperties } | null>(null);
	const timer = useRef(0);
	useEffect(() => {
		if (!shown) return;
		const hide = () => setShown(null);
		window.addEventListener('scroll', hide, { passive: true, capture: true });
		return () => window.removeEventListener('scroll', hide, { capture: true });
	}, [shown]);
	useEffect(() => () => clearTimeout(timer.current), []);
	const hide = () => {
		clearTimeout(timer.current);
		setShown(null);
	};
	const bind = (tip: Tip | undefined): React.HTMLAttributes<HTMLElement> =>
		tip
			? {
					onPointerEnter: (e) => {
						if (e.pointerType !== 'mouse') return;
						const el = e.currentTarget;
						clearTimeout(timer.current);
						timer.current = window.setTimeout(() => {
							const r = el.getBoundingClientRect();
							// Right half (the N-panel): open to the left of the row; elsewhere below the element.
							const style: React.CSSProperties =
								r.left > window.innerWidth / 2 ? { right: window.innerWidth - r.left + 8, top: r.top } : { left: r.left, top: r.bottom + 6 };
							setShown({ tip, style });
						}, 500);
					},
					onPointerLeave: hide,
					onPointerDown: hide,
				}
			: {};
	const html = useMemo(() => (shown?.tip.tex ? renderStaticMath(shown.tip.tex, false) : null), [shown?.tip.tex]);
	const node = shown && (
		<div className="lab-tip" role="tooltip" style={shown.style}>
			{shown.tip.title && <b>{shown.tip.title}</b>}
			<p>{shown.tip.text}</p>
			{html && <span dangerouslySetInnerHTML={{ __html: html }} />}
		</div>
	);
	return { bind, node };
}

interface FieldProps {
	value: number;
	spec: ScrubSpec;
	snap?: number | readonly number[];
	label: string;
	fmt?: (v: number) => string;
	/** Axis letter inside the field (vector components). */
	axis?: 0 | 1 | 2;
	/** Reads the live value at event time (events can outrun React renders). */
	get(): number;
	set(v: number): void;
}

/** A Blender number field: drag to scrub, click/Enter to type, arrows to step. */
function NumField(p: FieldProps): React.JSX.Element {
	const [edit, setEdit] = useState<{ text: string; err: string | null } | null>(null);
	const errId = useId();
	const input = useRef<HTMLInputElement>(null);
	const field = useRef<HTMLSpanElement>(null);
	// Closing the input with Enter/Escape hands focus back to the field (keyboard flow); a click-away does not.
	const back = useRef(false);
	const shown = p.fmt ? p.fmt(p.value) : fmtField(p.value, p.spec.step);
	useEffect(() => {
		if (edit) {
			input.current?.focus();
			input.current?.select();
		} else if (back.current) {
			back.current = false;
			field.current?.focus();
		}
	}, [edit !== null]);
	const open = () => setEdit({ text: fmtField(p.get(), p.spec.step).replace('−', '-'), err: null });
	const commit = (text: string) => {
		const r = parseField(text, p.spec);
		if (!r.ok) {
			setEdit({ text, err: r.err });
			return false;
		}
		p.set(r.v);
		setEdit(null);
		return true;
	};
	const close = (keyboard: boolean) => {
		back.current = keyboard;
		setEdit(null);
	};
	if (edit) {
		return (
			<span className="lab-np-edit">
				<input
					ref={input}
					className="lab-np-input"
					inputMode="decimal"
					aria-label={p.label}
					aria-invalid={edit.err ? 'true' : undefined}
					aria-describedby={edit.err ? errId : undefined}
					defaultValue={edit.text}
					onKeyDown={(e) => {
						if (e.key === 'Enter') {
							e.preventDefault();
							back.current = true;
							if (!commit(e.currentTarget.value)) back.current = false;
						} else if (e.key === 'Escape') {
							e.preventDefault();
							close(true);
						}
					}}
					// Leaving the field keeps a valid value (Blender confirms on click-away) and drops an invalid one.
					onBlur={(e) => {
						if (!commit(e.currentTarget.value)) close(false);
					}}
				/>
				{edit.err && (
					<span id={errId} className="lab-np-err" role="alert">
						{edit.err}
					</span>
				)}
			</span>
		);
	}
	const step = (d: number) => p.set(keyStep(p.get(), d > 0 ? 'ArrowRight' : 'ArrowLeft', p.spec) ?? p.get());
	return (
		<span
			ref={field}
			className="lab-np-field"
			role="slider"
			tabIndex={0}
			aria-label={p.label}
			aria-valuemin={p.spec.min}
			aria-valuemax={p.spec.max}
			aria-valuenow={p.value}
			aria-valuetext={shown}
			onPointerDown={(e) => {
				if (e.pointerType === 'mouse' && e.button !== 0) return;
				const d = (e.target as HTMLElement).closest<HTMLElement>('.lab-np-step')?.dataset.d;
				if (d) {
					e.preventDefault();
					step(Number(d));
					return;
				}
				e.currentTarget.focus();
				// Shift (fine) re-bases the drag at the current value, so toggling it mid-drag never jumps.
				let base = { v0: p.get(), dx0: 0, fine: false };
				let moved = false;
				// Touch/pen: the field is `touch-action: pan-y`, so a vertical swipe scrolls the page (the browser then
				// cancels the pointer). Scrubbing starts only on a clearly horizontal move; a vertical one is ignored.
				const touch = e.pointerType !== 'mouse';
				const y0 = e.clientY;
				let vertical = false;
				startScrubDrag(
					e,
					e.currentTarget,
					{ get: p.get, set: p.set },
					(dx, ev) => {
						if (vertical) return null;
						if (!moved) {
							const dy = Math.abs(ev.clientY - y0);
							if (touch) {
								if (dy >= TOUCH_SLOP && dy > Math.abs(dx)) {
									vertical = true;
									return null;
								}
								if (Math.abs(dx) < TOUCH_SLOP || Math.abs(dx) < 1.5 * dy) return null;
							} else if (Math.abs(dx) < CLICK_PX) return null;
							moved = true;
						}
						if (ev.shiftKey !== base.fine) base = { v0: p.get(), dx0: dx, fine: ev.shiftKey };
						const spec = base.fine ? { ...p.spec, step: p.spec.step / 10 } : p.spec;
						const v = scrubValue(base.v0, dx - base.dx0, spec, PX_PER_STEP);
						return ev.ctrlKey ? ctrlSnap(v, p.snap ?? p.spec.step * 10, p.spec) : v;
					},
					// A cancelled gesture is never a click (the loop has already restored the value).
					(dx, cancelled) => {
						if (!cancelled && !moved && !vertical && Math.abs(dx) < (touch ? TOUCH_SLOP : CLICK_PX)) open();
					},
				);
			}}
			onKeyDown={(e) => {
				if (e.key === 'Enter') {
					e.preventDefault();
					open();
					return;
				}
				const next = keyStep(p.get(), e.key, p.spec);
				if (next === null) return;
				e.preventDefault();
				if (next !== p.get()) p.set(next);
			}}
		>
			{p.axis !== undefined && (
				<span className="lab-np-ax" style={{ color: roleVar(AXIS_ROLE[p.axis]) }} aria-hidden="true">
					{AXIS[p.axis].toUpperCase()}
				</span>
			)}
			<span className="lab-np-step" data-d="-1" aria-hidden="true">
				{'‹'}
			</span>
			{shown}
			<span className="lab-np-step" data-d="1" aria-hidden="true">
				{'›'}
			</span>
		</span>
	);
}

const roText = (r: Row, v: number | V): string => {
	const one = (x: number) => (r.fmt ? r.fmt(x) : fmtField(x, r.spec?.step ?? 0.01));
	return typeof v === 'number' ? one(v) : `(${v.map(one).join(', ')})`;
};

function RowView({ lesson, row, vals, hot, reg, tip }: { lesson: string; row: Row; vals: Vals; hot: boolean; reg?: StageReg; tip: ReturnType<typeof useTip>['bind'] }): React.JSX.Element {
	const store = lessonStore(lesson);
	const raw = row.key ? vals[row.key] : undefined;
	if (row.key && raw === undefined) throw new Error(`N-panel row "${row.id}": store key "${row.key}" is missing`);
	const live = () => store.getState().vals[row.key!];
	let body: React.ReactNode;
	if (row.kind === 'ro') {
		const v = row.value ? row.value(vals) : (raw as number | V);
		body = <span className="lab-np-ro">{roText(row, v)}</span>;
	} else if (row.kind === 'num') {
		if (typeof raw !== 'number') throw new Error(`N-panel row "${row.id}": store key "${row.key}" is not a number`);
		body = <NumField value={raw} spec={row.spec!} snap={row.snap} label={row.label} fmt={row.fmt} get={() => live() as number} set={(v) => store.getState().set(row.key!, v)} />;
	} else {
		if (typeof raw === 'number') throw new Error(`N-panel row "${row.id}": store key "${row.key}" is not a vector`);
		const vec = raw as V;
		body = (
			<span className="lab-np-vec">
				{vec.map((x, i) => (
					<NumField
						key={i}
						value={x}
						spec={row.spec!}
						snap={row.snap}
						label={`${row.label} ${AXIS[i]}`}
						fmt={row.fmt}
						axis={i as 0 | 1 | 2}
						get={() => (live() as V)[i]}
						set={(v) => store.getState().set(row.key!, (live() as V).map((c, j) => (j === i ? v : c)))}
					/>
				))}
			</span>
		);
	}
	// The status bar explains what a field does; a read-only row behaves like hovering its scene object.
	const kind = row.kind === 'ro' ? 'object' : 'field';
	const on = () => {
		hotOn(lesson, row.id);
		reg?.hover.set({ [kind]: row.id });
	};
	const off = () => {
		hotOff(lesson, row.id);
		reg?.hover.clear(kind, row.id);
	};
	return (
		<div
			className={hot ? 'lab-np-row is-hot' : 'lab-np-row'}
			data-id={row.id}
			onPointerOver={on}
			onPointerOut={(e) => {
				// Moving between the row's own children fires pointerout; clear only when the pointer left the row.
				if (!e.currentTarget.contains(e.relatedTarget as Node | null)) off();
			}}
			onFocus={on}
			onBlur={(e) => {
				if (!e.currentTarget.contains(e.relatedTarget as Node | null)) off();
			}}
			{...tip(row.tip ? { title: row.tip.title ?? row.label, ...row.tip } : undefined)}
		>
			<span className="lab-np-label">{row.label}</span>
			{body}
		</div>
	);
}

export interface NPanelProps {
	lesson: string;
	rows: readonly Row[];
	/** Stage registry of the card: field hover feeds the status bar. */
	reg?: StageReg;
	/** Desktop sidebar open (N key / chevron); chips under 720 px are always shown. */
	open?: boolean;
	id?: string;
}

export function NPanel({ lesson, rows, reg, open = true, id }: NPanelProps): React.JSX.Element {
	rows.forEach(checkRow);
	const store = lessonStore(lesson);
	const vals = useStore(store, (s) => s.vals);
	const steps = useStore(store, (s) => s.step);
	const hot = useStore(store, (s) => s.hot);
	const tip = useTip();
	const shown = rows.filter((r) => !r.visible || r.visible(vals, steps));
	return (
		<aside id={id} className={open ? 'lab-vp-panel' : 'lab-vp-panel is-closed'} aria-label="Properties">
			{shown.map((r, i) => (
				<RowGroup key={r.id} sep={r.group && r.group !== shown[i - 1]?.group ? r.group : null}>
					<RowView lesson={lesson} row={r} vals={vals} hot={hot.has(r.id)} reg={reg} tip={tip.bind} />
				</RowGroup>
			))}
			{tip.node}
		</aside>
	);
}

function RowGroup({ sep, children }: { sep: string | null; children: ReactNode }): React.JSX.Element {
	return (
		<>
			{sep && (
				<div className="lab-np-sep" role="separator">
					{sep}
				</div>
			)}
			{children}
		</>
	);
}
