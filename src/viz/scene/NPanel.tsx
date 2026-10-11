import { Glyph } from '../Glyph';
import { t } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
/**
 * The N-panel (Blender's sidebar): the linked "numbers view" of a scene. Each row is label | value: editable rows
 * are Blender number fields (drag horizontally to scrub, Ctrl snaps, Shift is fine, click or Enter to type, arrows and
 * PageUp/PageDown to step), vector rows have one field per component, read-only rows are flat text, bool rows are
 * checkboxes writing 0/1 (`ik_flip`, `nl_raw`). Every row carries a term id: hovering or focusing it lights that id
 * everywhere (scene outline, formula term), and a row lights up when its id is hot elsewhere. Under 720 px the same
 * DOM lays out as wrapped chips under the viewport, each chip as wide as the fixed-width model of chips.ts says (CSS
 * variables set here), so the server frame can reserve the exact height.
 *
 * Motion (Ruling U): a scrub drag writes as a cut (1:1); arrow keys, the step arrows and a typed value are indirect
 * changes and write `ease`, so the scene glyph glides while the field already shows the exact value.
 *
 * One drag implementation in the codebase: fields use `startScrubDrag` (useScrub.ts) and the pure math of scrub.ts
 * (scrubValue, keyStep, ctrlSnap, parseField, fmtField), exactly like Formula and Num.
 * Three-free (DOM only), so it is tested in happy-dom.
 */
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useStore } from 'zustand';
import type { V } from '../geom';
import { roleVar } from '../palette';
import { ctrlSnap, fmtField, keyStep, parseField, scrubValue, type ScrubSpec } from '../scrub';
import { hotOff, hotOn, lessonStore, type LessonState, type WriteHow } from '../store';
import { renderStaticMath } from '../tex';
import { startScrubDrag } from '../useScrub';
import { CHIP, chipBox, type ChipRow } from './chips';
import type { Range, StageReg } from './handles';

type Vals = LessonState['vals'];

/** Tooltip content (Blender-like): title, one line, and optionally the formula term as TeX. */
export interface Tip {
	title?: string;
	text: string;
	tex?: string;
}

export interface Row extends ChipRow {
	/** Term id shared with the scene object, formula term and table cell. */
	id: string;
	label: string;
	/**
	 * num: editable number; vec: editable vector (one field per component, `dims` of them); ro: read-only value
	 * (`chars` = its text budget in the phone chip row, see chips.ts); bool: a checkbox writing 0/1 to a number key.
	 */
	kind: 'num' | 'vec' | 'ro' | 'bool';
	/** Store key (required for num/vec/bool; for ro either `key` or `value`). */
	key?: string;
	spec?: ScrubSpec;
	/** Ctrl while dragging: snap to these targets, or to multiples of this increment (default 10 steps). */
	snap?: number | readonly number[];
	/** Read-only value derived from the store. */
	value?: (vals: Vals) => number | V;
	/** Optional glyph dimensions/components, while value remains the numeric readout. */
	glyphValue?: (vals: Vals) => number | V;
	fmt?: (v: number) => string;
	/** Row shown only while true (e.g. revealed by a timeline step). */
	visible?: (vals: Vals, step: Readonly<Record<string, number>>) => boolean;
	/** Group caption: a separator is drawn where the group changes. */
	group?: string;
	tip?: Tip;
}

/**
 * Ranges of the store keys the editable rows declare (num and vec rows with a spec), for the card's handle clamp
 * (`CardInfo.ranges`): a field and a handle writing the same key share bounds. Two rows giving one key different
 * ranges is an authoring error.
 */
export function rowRanges(rows: readonly Row[]): Record<string, Range> {
	const out: Record<string, Range> = {};
	for (const r of rows) {
		if ((r.kind !== 'num' && r.kind !== 'vec') || !r.key || !r.spec) continue;
		const cur = out[r.key];
		if (cur && (cur.min !== r.spec.min || cur.max !== r.spec.max)) throw new Error(`N-panel rows give key "${r.key}" two ranges`);
		out[r.key] = { min: r.spec.min, max: r.spec.max };
	}
	return out;
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
	if (r.kind === 'vec' && !r.dims) throw new Error(`N-panel row "${r.id}": a vec row needs \`dims\` (its chip width depends on it)`);
	if (r.kind === 'bool' && !r.key) throw new Error(`N-panel row "${r.id}": a bool row needs \`key\``);
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
			{/* A row whose tip text only repeats its label (the title) shows the title once. */}
			{shown.tip.text !== shown.tip.title && <p>{shown.tip.text}</p>}
			{html && <span dangerouslySetInnerHTML={{ __html: html }} />}
		</div>
	);
	return { bind, node };
}

interface FieldProps {
 /** A declared store radial constraint replaces component box bounds. */
 radial?: boolean;
	value: number;
	spec: ScrubSpec;
	snap?: number | readonly number[];
	label: string;
	fmt?: (v: number) => string;
	/** Axis letter inside the field (vector components). */
	axis?: 0 | 1 | 2;
	/** Reads the live value at event time (events can outrun React renders). */
	get(): number;
	checkpoint(): () => void;
	/** Drags write cuts (the default); keys, step arrows and typed values ease (see the module note). */
	set(v: number, how?: WriteHow): void;
}

/** A Blender number field: drag to scrub, click/Enter to type, arrows to step. */
function NumField(p: FieldProps): React.JSX.Element {
 const locale = useLocale();
 // Keep the common bounded engine unchanged. Radial components have a zero-origin unbounded grid.
 const radialSnap=(v:number,step=p.spec.step)=>Math.sign(v)*scrubValue(Math.abs(v),0,{min:0,max:Infinity,step});
 const nextKey=(key:string)=>{
  if(!p.radial)return keyStep(p.get(),key,p.spec);
  const delta=keyStep(0,key,{min:-10*p.spec.step,max:10*p.spec.step,step:p.spec.step});
  if(delta===null)return null;
  const value=radialSnap(p.get()+delta);return Number.isFinite(value)?value:null;
 };
 const parse=(text:string)=>{
  if(!p.radial)return parseField(text,p.spec);
  const syntax=parseField(text,{min:0,max:Infinity,step:p.spec.step});if(!syntax.ok)return syntax;
  const clean=text.trim().replace(/−/g,'-'),negative=clean.startsWith('-');
  const value=parseField(negative?clean.slice(1):clean,{min:0,max:Infinity,step:p.spec.step});
  return value.ok&&Number.isFinite(value.v)?{ok:true as const,v:negative?-value.v:value.v}:{ok:false as const,err:'finite number required'};
 };
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
		const r = parse(text);
		if (!r.ok) {
			setEdit({ text, err: t(locale, 'viewport.numberError') });
			return false;
		}
		p.set(r.v, 'ease');
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
	const step = (d: number) => p.set(nextKey(d > 0 ? 'ArrowRight' : 'ArrowLeft') ?? p.get(), 'ease');
	return (
		<span
			ref={field}
			className="lab-np-field"
			role={p.radial ? "spinbutton" : "slider"}
			tabIndex={0}
			aria-label={p.label}
			aria-valuemin={p.radial ? undefined : p.spec.min}
			aria-valuemax={p.radial ? undefined : p.spec.max}
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
					{ get: p.get, set: p.set, checkpoint: p.checkpoint },
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
						if(p.radial){
       const value=radialSnap(base.v0+Math.round((dx-base.dx0)/PX_PER_STEP)*spec.step,spec.step);
       const targets=p.snap??p.spec.step*10;
       const snapped=!ev.ctrlKey?value:typeof targets==='number'?radialSnap(value,targets):targets.reduce((best,t)=>Math.abs(t-value)<Math.abs(best-value)?t:best,targets[0]);
       return Number.isFinite(snapped)?snapped:null;
      }
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
				const next = nextKey(e.key);
				if (next === null) return;
				e.preventDefault();
				if (next !== p.get()) p.set(next, 'ease');
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
			<span className="lab-np-val">{shown}</span>
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

function RowView({ lesson, row, vals, hot, reg, tip, size='sm' }: { lesson: string; row: Row; vals: Vals; hot: boolean; reg?: StageReg; tip: ReturnType<typeof useTip>['bind']; size?: 'sm'|'lg' }): React.JSX.Element {
	const store = lessonStore(lesson);
	const raw = row.key ? vals[row.key] : undefined;
	if (row.key && raw === undefined) throw new Error(`N-panel row "${row.id}": store key "${row.key}" is missing`);
	const live = () => store.getState().vals[row.key!];
	let body: React.ReactNode;
	if (row.kind === 'ro') {
		const v = row.value ? row.value(vals) : (raw as number | V);
		body = (
			<span className="lab-np-ro">
				<span className="lab-np-val">{roText(row, v)}</span>
			</span>
		);
	} else if (row.kind === 'num') {
		if (typeof raw !== 'number') throw new Error(`N-panel row "${row.id}": store key "${row.key}" is not a number`);
		body = <NumField radial={store.getState().radialKeys.has(row.key!)} checkpoint={() => store.getState().checkpoint([row.key!])} value={raw} spec={row.spec!} snap={row.snap} label={row.label} fmt={row.fmt} get={() => live() as number} set={(v, how) => store.getState().set(row.key!, v, how)} />;
	} else if (row.kind === 'bool') {
		if (raw !== 0 && raw !== 1) throw new Error(`N-panel row "${row.id}": store key "${row.key}" must hold 0 or 1, got ${String(raw)}`);
		body = (
			<span className="lab-vp-check lab-np-check">
				<input type="checkbox" aria-label={row.label} checked={raw === 1} onChange={(e) => store.getState().set(row.key!, e.currentTarget.checked ? 1 : 0)} />
			</span>
		);
	} else {
		if (typeof raw === 'number') throw new Error(`N-panel row "${row.id}": store key "${row.key}" is not a vector`);
		const vec = raw as V;
		if (vec.length !== row.dims) throw new Error(`N-panel row "${row.id}": store key "${row.key}" has ${vec.length} components, the row declares dims ${row.dims}`);
		body = (
			<span className="lab-np-vec">
				{vec.map((x, i) => (
					<NumField
						key={i}
      radial={store.getState().radialKeys.has(row.key!)}
						checkpoint={() => store.getState().checkpoint([row.key!])}
						value={x}
						spec={row.spec!}
						snap={row.snap}
						label={`${row.label} ${AXIS[i]}`}
						fmt={row.fmt}
						axis={i as 0 | 1 | 2}
						get={() => (live() as V)[i]}
						set={(v, how) => store.getState().set(row.key!, (live() as V).map((c, j) => (j === i ? v : c)), how)}
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
	const box = chipBox(row);
	return (
		<div
			className={hot ? 'lab-np-row is-hot' : 'lab-np-row'}
			data-id={row.id}
			// Chip widths of the fixed-width model (used by lab.css under 720 px only).
			style={{ '--np-lw': `${box.label}px`, '--np-fw': `${box.field}px` } as React.CSSProperties}
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
			<span className="lab-np-label">{row.glyph && <Glyph size={size} kind={row.glyph} value={row.glyphValue ? row.glyphValue(vals) : row.value ? row.value(vals) : (raw ?? 0)} />}{row.label}</span>
			{body}
		</div>
	);
}

/** The same linked field cell outside a viewport (for page-side three-faces strips). */
export function FieldCell({lesson,row,size='lg'}:{lesson:string;row:Row;size?:'sm'|'lg'}):React.JSX.Element {
 checkRow(row);
 const store=lessonStore(lesson),vals=useStore(store,s=>s.vals),hot=useStore(store,s=>s.hot.has(row.id)),tip=useTip();
 return <div className={'lab-field-cell not-content is-'+size}><RowView lesson={lesson} row={row} vals={vals} hot={hot} tip={tip.bind} size={size}/>{tip.node}</div>;
}

/** The chip geometry of chips.ts as the CSS variables lab.css reads under 720 px (one source for model and layout). */
const CHIP_VARS = {
	'--np-gx': `${CHIP.gapX}px`,
	'--np-gy': `${CHIP.gapY}px`,
	'--np-px': `${CHIP.padX}px`,
	'--np-py': `${CHIP.padY}px`,
	'--np-h': `${CHIP.h}px`,
	'--np-lg': `${CHIP.labelGap}px`,
} as React.CSSProperties;

/**
 * Values are placed by a transform (lab.css `.lab-np-val`), so a text longer than its box no longer widens the box: it
 * would silently overlap its neighbour. That is an authoring error (a read-only row's `chars` budget, or a `fmt`
 * longer than the spec's range ends), reported once per row in the console after each render.
 */
function useOverflowCheck(panel: React.RefObject<HTMLElement | null>): void {
	const warned = useRef(new Set<string>());
	// Last text measured per value element: a render that did not change a value's text (most of a drag) costs no
	// layout read for it.
	const seen = useRef(new WeakMap<HTMLElement, string>());
	useLayoutEffect(() => {
		for (const v of panel.current?.querySelectorAll<HTMLElement>('.lab-np-val') ?? []) {
			const text = v.textContent ?? '';
			if (seen.current.get(v) === text) continue;
			seen.current.set(v, text);
			const box = v.parentElement;
			const id = v.closest<HTMLElement>('.lab-np-row')?.dataset.id ?? '?';
			if (!box || v.offsetWidth <= box.clientWidth || warned.current.has(id)) continue;
			warned.current.add(id);
			console.error(`N-panel row "${id}": value "${v.textContent}" (${v.offsetWidth} px) overflows its ${box.clientWidth} px box; raise its \`chars\` or shorten \`fmt\``);
		}
	});
}

/** Desktop label column bounds (CSS px): labels up to the max sit on one line, longer ones wrap (never truncated). */
const LABEL_W: readonly [number, number] = [50, 112];

/**
 * Sizes the desktop sidebar's label column to its longest label (EN and RU labels differ by up to 2.4x), within
 * LABEL_W, and widens the sidebar by the same amount, so "−2ab cos θ" or a Russian label is shown whole. Labels
 * longer than the maximum wrap onto a second line. Phones (chips) size labels from the chip model instead.
 */
function useLabelColumn(panel: React.RefObject<HTMLElement | null>, labels: string): void {
	// Only when the set of labels changes (a locale, a revealed row): measuring forces a layout.
	useLayoutEffect(() => {
		const el = panel.current;
		if (!el || matchMedia('(max-width: 719.98px)').matches) return;
		// Natural one-line widths: measured with wrapping off.
		el.removeAttribute('data-wrap');
		let widest = 0;
		for (const l of el.querySelectorAll<HTMLElement>('.lab-np-row:not(.is-pending) > .lab-np-label')) widest = Math.max(widest, l.scrollWidth);
		const w = Math.ceil(Math.min(LABEL_W[1], Math.max(LABEL_W[0], widest + 1)));
		el.style.setProperty('--np-label-w', `${w}px`);
		el.style.setProperty('--lab-vp-panel', `calc(200px + ${w - LABEL_W[0]}px)`);
		if (widest + 1 > LABEL_W[1]) el.setAttribute('data-wrap', '');
	}, [panel, labels]);
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
 const locale = useLocale();
	rows.forEach(checkRow);
	const store = lessonStore(lesson);
	const vals = useStore(store, (s) => s.vals);
	const steps = useStore(store, (s) => s.step);
	const hot = useStore(store, (s) => s.hot);
	const tip = useTip();
	const isShown = (r: Row) => !r.visible || r.visible(vals, steps);
	const shown = rows.filter(isShown);
	const panel = useRef<HTMLElement>(null);
	useOverflowCheck(panel);
	useLabelColumn(panel, shown.map((r) => r.label).join('|'));
	return (
		<aside ref={panel} id={id} className={open ? 'lab-vp-panel' : 'lab-vp-panel is-closed'} aria-label={t(locale, 'viewport.properties')} style={CHIP_VARS}>
			{rows.map((r) => {
				// Phones reserve a chip for every row (chips.ts), so a row a later timeline step reveals is shown as a
				// faded placeholder in its place until then: the reserved space reads as "coming", never as a gap.
				if (!isShown(r)) return <PendingChip key={r.id} row={r} />;
				const i = shown.indexOf(r);
				return (
					<RowGroup key={r.id} sep={r.group && r.group !== shown[i - 1]?.group ? r.group : null}>
						<RowView lesson={lesson} row={r} vals={vals} hot={hot.has(r.id)} reg={reg} tip={tip.bind} />
					</RowGroup>
				);
			})}
			{tip.node}
		</aside>
	);
}

/** Invisible slot of a not-yet-revealed row in the phone chip row (absent on desktop); exactly the chip's model width. */
function PendingChip({ row }: { row: Row }): React.JSX.Element {
	return (
		<div className="lab-np-row is-pending" aria-hidden="true" data-id={row.id} style={{ width: `${chipBox(row).w}px` }}>
			<span className="lab-np-label">{row.label}</span>
			<span className="lab-np-pending" />
		</div>
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
