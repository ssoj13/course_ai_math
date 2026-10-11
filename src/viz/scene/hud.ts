/**
 * Layout of the HUD instruments of the holographic look (Instruments.tsx; reference refs/holo-style-ref.png: two
 * instrument columns at the left and right edges - rulers with fine ticks, framed galaxy panels, a target ring, a dial,
 * a planet in a ring - with the figure between them and the formula plate under it).
 *
 * Pure and three-free: the same numbers size the SVG instruments and the space the home view keeps free for them
 * (ViewportCard passes `pad` to the Stage), so the figure is framed between the columns and never under them.
 * Everything scales with the free stage area (`w` x `h`, the stage minus an open N-panel): the instruments of a
 * full-screen view are as big, relative to the frame, as those of a card. Narrow stages (phones) use a compact
 * arrangement that keeps every element in two slim columns.
 */

/** Below this free stage width the compact (phone) arrangement is used. */
export const COMPACT_W = 480;

export interface Box {
	x: number;
	y: number;
	size: number;
}

export interface HudLayout {
	/** Scale factor of every px length below (1 = a 520 px tall stage). */
	k: number;
	compact: boolean;
	/** Width of the left and right instrument columns: the figure is framed between them. */
	left: number;
	right: number;
	/** Vertical rulers: x of the left/right line, the span they cover, tick lengths and label font size. */
	ruler: { xl: number; xr: number; top: number; bottom: number; major: number; minor: number; font: number };
	/** Framed galaxy panels (top-left of each box). */
	panels: [Box, Box];
	/** Target ring on the left column (centre and radius in `size`). */
	target: Box;
	/** Dial on the right column (centre and radius). */
	dial: Box;
	/**
	 * Caption under the dial (hovered term or angle; 3 lines, 2 in compact): right-aligned `right` px from the right
	 * edge, clear of the right ruler's ticks, `top` px from the top, at most `width` px wide, `lines` x `font` tall.
	 */
	caption: { right: number; top: number; width: number; font: number; lines: number };
	/** Planet in its ring (centre and radius). */
	planet: Box;
	/** Large faint arcs that frame the columns: centre x of the left one (the right one is mirrored) and radius. */
	arc: { cx: number; r: number };
}

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
/** Caption line height in units of its font size. */
const LINE = 1.2;

/**
 * Instrument layout for a free stage area of `w` x `h` CSS px. `gizmo`: a 3D view's navigation gizmo occupies the top
 * right corner (84 px + 6 px margin), so the right column starts below it. `xy`: the 2D XY corner glyph sits at the
 * bottom left (38 px + 8 px), so the left ruler stops above it.
 */
export function hudLayout(w: number, h: number, gizmo = false, xy = false): HudLayout {
	const compact = w < COMPACT_W;
	if (compact) {
		const col = 40;
		const top = 8;
		const panel = 26;
		const dialY = (gizmo ? 96 : 8) + 16;
		const captionTop = dialY + 14 * 1.2 + 2;
		return {
			k: 0.5,
			compact,
			left: col,
			right: col,
			ruler: { xl: 4, xr: w - 4, top: top + panel + 10, bottom: h - (xy ? 52 : 10), major: 6, minor: 3, font: 8.5 },
			panels: [
				{ x: 8, y: top, size: panel },
				{ x: w - col + 6, y: Math.max(h * 0.45, captionTop + 2 * LINE * 8.5 + 6), size: panel },
			],
			target: { x: 22, y: h * 0.62, size: 9 },
			dial: { x: w - 20, y: dialY, size: 14 },
			caption: { right: 14, top: captionTop, width: col * 1.7, font: 8.5, lines: 2 },
			// Ring (2.05 x size) inside the column.
			planet: { x: w - 20, y: h - 24, size: 9 },
			arc: { cx: -2.2 * h, r: 2.2 * h + col + 6 },
		};
	}
	const k = clamp(Math.min(h / 520, w / 820), 0.72, 1.9);
	const col = Math.round(92 * k);
	const panel = 48 * k;
	const top = 0.07 * h;
	const dialY = (gizmo ? 96 : 0) + Math.max(top, 8) + 34 * k;
	const dial = 28 * k;
	const font = clamp(9.5 * k, 9, 15);
	const captionTop = dialY + dial * 1.25;
	// The planet's ring (2.05 x its radius) stays inside the column, so it never reaches the formula plate.
	const planet = Math.min(25 * k, (col / 2 - 2) / 2.05);
	return {
		k,
		compact,
		left: col,
		right: col,
		ruler: { xl: 10 * k, xr: w - 10 * k, top, bottom: h - (xy ? 56 : 0.05 * h), major: 9 * k, minor: 4.5 * k, font },
		panels: [
			{ x: 36 * k, y: top + 6 * k, size: panel },
			// Below the dial's caption (3 lines), never over it.
			{ x: w - 36 * k - panel, y: Math.max(0.46 * h, captionTop + 3 * LINE * font + 10 * k), size: panel },
		],
		target: { x: 36 * k + panel / 2, y: 0.5 * h, size: 15 * k },
		dial: { x: w - col / 2, y: dialY, size: dial },
		caption: { right: 19 * k + 4, top: captionTop, width: col * 1.7, font, lines: 3 },
		planet: { x: w - col / 2, y: h - 0.05 * h - 36 * k, size: planet },
		arc: { cx: -1.8 * h, r: 1.8 * h + col + 14 * k },
	};
}

/** A fine tick of a ruler: CSS px from the stage top, and whether it is a whole unit (labelled). */
export interface HudTick {
	y: number;
	major: boolean;
	label: string | null;
}

/**
 * Ruler ticks from the projected whole units of the world y axis (`units`: value and px, top to bottom). Between two
 * whole units: 4 fine ticks when a unit is at least 30 px tall, 1 half tick from 14 px, none below. Labels on every
 * whole unit, or every 2nd/5th when units are closer than the label height (`font` px + 3).
 */
export function hudTicks(units: readonly { v: number; y: number }[], font: number): HudTick[] {
	if (units.length === 0) return [];
	const gap = units.length > 1 ? Math.abs(units[1].y - units[0].y) : Infinity;
	const sub = gap >= 30 ? 5 : gap >= 14 ? 2 : 1;
	const every = gap >= font + 3 ? 1 : gap * 2 >= font + 3 ? 2 : 5;
	const out: HudTick[] = [];
	units.forEach((u, i) => {
		out.push({ y: u.y, major: true, label: u.v % every === 0 ? String(u.v) : null });
		const next = units[i + 1];
		if (!next) return;
		for (let j = 1; j < sub; j++) out.push({ y: u.y + ((next.y - u.y) * j) / sub, major: false, label: null });
	});
	return out;
}
