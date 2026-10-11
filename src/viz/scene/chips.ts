/**
 * Fixed-width model of the N-panel chip row (< 720 px, Ruling V: the chips wrap onto lines under the viewport).
 *
 * The server frame (SceneIsland.astro) must reserve the chip row's exact height before any script runs, or hydrating
 * the card shifts the page. So chip widths do not depend on live values or on which font happened to load: every
 * chip gets a width computed here from its row declaration (label length, value character budget), applied by NPanel
 * as CSS variables, and the line count follows from greedy wrapping at the narrowest supported phone (BASIS_PX). The
 * SAME rows array is passed to SceneIsland (reservation) and ViewportCard (rendering), and the card checks both agree.
 * Wider phones wrap into the same or fewer lines (greedy wrapping of fixed widths is monotone in the width), so the
 * reservation at the basis is never too small.
 *
 * Character budget: every chip text is set in the mono face at <= 11.5 px, whose advance is 0.6 em (JetBrains Mono,
 * Menlo, DejaVu Sans Mono; Consolas is narrower), i.e. <= 6.9 px; CH = 7 px per character covers every fallback.
 * Three-free and DOM-free (imported by the .astro frame on the server and by tests).
 */
import { fmtField, type ScrubSpec } from '../scrub';

/** The narrowest phone the reservation is computed for (CSS px viewport width). */
export const BASIS_PX = 320;

/** Chip geometry in CSS px; lab.css reads these through the variables NPanel sets (one source of truth). */
export const CHIP = {
	/** Starlight's content padding on each side below 50rem (the card spans the content column). */
	pagePad: 16,
	padX: 8,
	padY: 5,
	/** Top border of the chip row. */
	border: 1,
	gapX: 12,
	gapY: 4,
	/** Height of one chip (a field). */
	h: 20,
	/** Budget per character of mono text. */
	ch: 7,
	/** Between a chip's label and its value. */
	labelGap: 5,
	/** Horizontal padding of a number field (room for the hover step arrows). */
	numPad: 28,
	/** Horizontal padding of a vector component field (axis letter on the left). */
	vecPad: 32,
	/** Gap between the fields of a vector. */
	vecGap: 1,
	/** Horizontal padding of a read-only value. */
	roPad: 8,
	/** A checkbox. */
	box: 14,
	/** Value budget of a read-only row without `chars`: "−123.45". */
	roChars: 7,
} as const;

/** The part of an N-panel row the model needs (structurally a subset of NPanel's `Row`). */
export interface ChipRow {
	id: string;
	label: string;
	kind: 'num' | 'vec' | 'ro' | 'bool';
	spec?: ScrubSpec;
	fmt?: (v: number) => string;
	/** vec rows: number of components. */
	dims?: number;
	/** ro rows: value text budget in characters (default CHIP.roChars). */
	chars?: number;
}

/** Characters of a string as the reader sees them (code points, so "θ" or "²" count once). */
const len = (s: string): number => [...s].length;

/** Widest text a number field of `spec` can show on its grid (the range ends; a custom `fmt` decides the text). */
function fieldChars(r: ChipRow): number {
	const spec = r.spec;
	if (!spec) throw new Error(`chip "${r.id}": a ${r.kind} row needs \`spec\``);
	const f = r.fmt ?? ((v: number) => fmtField(v, spec.step));
	return Math.max(len(f(spec.min)), len(f(spec.max)));
}

export interface ChipBox {
	/** Label width. */
	label: number;
	/** One value field (num, each vec component) or the read-only value box; 0 for a checkbox. */
	field: number;
	/** Whole chip. */
	w: number;
}

/** Widths of one chip in CSS px. */
export function chipBox(r: ChipRow): ChipBox {
	const label = len(r.label) * CHIP.ch;
	let field = 0;
	let value: number;
	switch (r.kind) {
		case 'num':
			field = CHIP.numPad + fieldChars(r) * CHIP.ch;
			value = field;
			break;
		case 'vec': {
			if (!r.dims) throw new Error(`chip "${r.id}": a vec row needs \`dims\``);
			field = CHIP.vecPad + fieldChars(r) * CHIP.ch;
			value = r.dims * field + (r.dims - 1) * CHIP.vecGap;
			break;
		}
		case 'ro':
			field = CHIP.roPad + (r.chars ?? CHIP.roChars) * CHIP.ch;
			value = field;
			break;
		case 'bool':
			value = CHIP.box;
			break;
	}
	return { label, field, w: label + CHIP.labelGap + value };
}

/** Lines of greedy wrapping (flex-wrap) of chips `widths` into `avail` px with CHIP.gapX between chips on a line. */
export function wrapLines(widths: readonly number[], avail: number): number {
	let lines = 0;
	let used = 0;
	for (const w of widths) {
		if (lines === 0 || used + CHIP.gapX + w > avail) {
			lines++;
			used = w;
		} else used += CHIP.gapX + w;
	}
	return lines;
}

/** Inner width of the chip row on a viewport `vw` px wide. */
export const chipAvail = (vw: number): number => vw - 2 * CHIP.pagePad - 2 * CHIP.padX;

/** Height in px of a chip row with `lines` lines (0 lines = no row). */
export const chipHeight = (lines: number): number => (lines > 0 ? CHIP.border + 2 * CHIP.padY + lines * CHIP.h + (lines - 1) * CHIP.gapY : 0);

/** Lines the chips of `rows` take on a viewport `vw` px wide (every row counted: rows revealed later must fit too). */
export function chipLines(rows: readonly ChipRow[], vw = BASIS_PX): number {
	return wrapLines(
		rows.map((r) => chipBox(r).w),
		chipAvail(vw),
	);
}
