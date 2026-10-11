/**
 * The formula plate of a scene card (reference refs/holo-style-ref.png): the lesson's linked Formula on a dark glass
 * plate with chamfered corners, a glowing edge, a faint inner grid and an outer instrument frame with corner notches.
 * The frame is an SVG drawn from the plate's measured size, so the chamfers stay 45 degrees and the strokes stay
 * 1 px at any plate size. The clay studio gets a cream rounded plate with a soft shadow instead (clay-style-ref.png).
 * Position and size are CSS (lab.css): over the floor between the instrument columns on desktop, in its own reserved
 * strip under the stage on phones. The Formula itself is unchanged (same DOM, same links).
 */
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import type { Look } from './look';

/** Chamfered rectangle path inset by `i` px in a `w` x `h` box, corner cut `c` px. */
function chamfer(w: number, h: number, i: number, c: number): string {
	const x0 = i;
	const y0 = i;
	const x1 = w - i;
	const y1 = h - i;
	return `M${x0 + c} ${y0}H${x1 - c}L${x1} ${y0 + c}V${y1 - c}L${x1 - c} ${y1}H${x0 + c}L${x0} ${y1 - c}V${y0 + c}Z`;
}

function HoloFrame({ w, h }: { w: number; h: number }): React.JSX.Element {
	const id = useId().replace(/:/g, '');
	const body = chamfer(w, h, 6, 11);
	const outer = chamfer(w, h, 0.5, 15);
	const m = h / 2;
	return (
		<>
			<defs>
				<linearGradient id={`${id}-fill`} x1="0" y1="0" x2="0" y2="1">
					<stop offset="0" stopColor="var(--viz-plate)" stopOpacity="0.62" />
					<stop offset="1" stopColor="var(--viz-plate)" stopOpacity="0.86" />
				</linearGradient>
				<pattern id={`${id}-grid`} width="10" height="10" patternUnits="userSpaceOnUse">
					<path d="M10 0H0V10" fill="none" stroke="var(--viz-vec-a)" strokeOpacity="0.07" />
				</pattern>
				<filter id={`${id}-glow`} x="-10%" y="-30%" width="120%" height="160%">
					<feGaussianBlur stdDeviation="2.4" />
				</filter>
			</defs>
			<path d={body} fill={`url(#${id}-fill)`} />
			<path d={body} fill={`url(#${id}-grid)`} />
			{/* Glow under the edge, then the crisp edge itself. */}
			<path d={body} fill="none" stroke="var(--viz-vec-a)" strokeWidth="3" strokeOpacity="0.55" filter={`url(#${id}-glow)`} />
			<path d={body} fill="none" style={{ stroke: 'color-mix(in srgb, var(--viz-vec-a) 70%, white)' }} strokeWidth="1.2" />
			<path d={outer} fill="none" stroke="var(--viz-vec-a)" strokeOpacity="0.45" />
			{/* Notches at the middle of the short sides and bright ticks at the chamfers, like the reference's frame. */}
			<path
				d={`M0.5 ${m - 9}H5 M0.5 ${m + 9}H5 M${w - 0.5} ${m - 9}H${w - 5} M${w - 0.5} ${m + 9}H${w - 5}
					M${6 + 11 + 8} 6H${6 + 11 + 26} M${w - 6 - 11 - 26} ${h - 6}H${w - 6 - 11 - 8}`}
				stroke="var(--viz-vec-a)"
				strokeWidth="1.5"
			/>
		</>
	);
}

/** Smallest text scale the plate uses to fit its formula before it would rather spill (never reached in practice). */
const MIN_SCALE = 0.6;
const px = (s: string) => parseFloat(s) || 0;

/**
 * Fits the formula into the plate by its text size, never by scrollbars: a plate is a framed caption, and a 1-2 px
 * rounding overflow used to give a two-line formula a vertical and a horizontal scrollbar (and a cut first line in a
 * narrow card). Desktop: the room between the HUD columns (card width minus N-panel and columns); phones: the plate's
 * reserved strip. Re-fitted whenever the card or the formula resizes.
 */
function useFitText(plate: React.RefObject<HTMLDivElement | null>, body: React.RefObject<HTMLDivElement | null>): void {
	useLayoutEffect(() => {
		const el = plate.current;
		const inner = body.current;
		const card = el?.parentElement;
		if (!el || !inner || !card) return;
		const fit = () => {
			inner.style.fontSize = '';
			const ps = getComputedStyle(el);
			const cs = getComputedStyle(card);
			const padX = px(ps.paddingLeft) + px(ps.paddingRight);
			const padY = px(ps.paddingTop) + px(ps.paddingBottom);
			const strip = ps.maxWidth === 'none';
			const availW = strip
				? el.clientWidth - padX
				: card.clientWidth - px(cs.getPropertyValue('--lab-vp-inset')) - px(cs.getPropertyValue('--lab-hud-l')) - px(cs.getPropertyValue('--lab-hud-r')) - padX;
			const availH = strip ? el.clientHeight - padY : Infinity;
			const scale = Math.max(MIN_SCALE, Math.min(1, availW / Math.max(1, inner.scrollWidth), availH / Math.max(1, inner.scrollHeight)));
			if (scale < 1) inner.style.fontSize = `${px(getComputedStyle(inner).fontSize) * scale}px`;
		};
		fit();
		const ro = new ResizeObserver(fit);
		ro.observe(card);
		ro.observe(inner);
		return () => ro.disconnect();
	}, [plate, body]);
}

export function FormulaPlate({ children, look = 'holo' }: { children: ReactNode; look?: Look }): React.JSX.Element {
	const frame = useRef<SVGSVGElement>(null);
	const plate = useRef<HTMLDivElement>(null);
	const body = useRef<HTMLDivElement>(null);
	useFitText(plate, body);
	const [size, setSize] = useState({ w: 0, h: 0 });
	useEffect(() => {
		const el = frame.current;
		if (!el) return;
		const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
		ro.observe(el);
		return () => ro.disconnect();
	}, []);
	const { w, h } = size;
	return (
		<div className="lab-formula-plate" data-look={look} ref={plate}>
			<svg className="lab-plate-frame" ref={frame} aria-hidden="true">
				{w > 0 &&
					h > 0 &&
					(look === 'clay' ? (
						<rect x="4" y="4" width={w - 8} height={h - 8} rx={Math.min(16, (h - 8) / 2)} fill="var(--viz-clay-plate)" style={{ filter: 'drop-shadow(0 4px 6px rgb(40 46 60 / 0.25))' }} />
					) : (
						<HoloFrame w={w} h={h} />
					))}
			</svg>
			<div className="lab-plate-body" ref={body}>
				{children}
			</div>
		</div>
	);
}
