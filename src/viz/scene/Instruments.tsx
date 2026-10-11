/**
 * HUD instruments of the holographic look (reference refs/holo-style-ref.png), drawn as one DOM/SVG overlay over the
 * free stage area: two instrument columns with rulers (the projected world y axis, fine ticks, unit labels), large
 * framing arcs, two framed galaxy panels, a target ring, a dial and a planet in a ring. Positions and sizes come from
 * `hudLayout` (hud.ts), which scales with the stage, and the camera data from the registry's `view` cell
 * (InstrumentSync), so the overlay re-renders on camera moves only.
 *
 * What the instruments read (never invented numbers): the rulers = the actual world y axis of the view; the dial = the
 * card's angle value (`angleKey`) in 2D, the view azimuth in 3D, and no needle in a 2D card without an angle; the
 * caption under the dial = the hovered term's row label and value (else the angle / azimuth). The planet's ring tilts
 * with the azimuth in 3D. The clay studio shows only the neutral measuring instruments (rulers, target ring, dial) in
 * ink, never space furniture (galaxy panels, planet).
 * Decorative parts are pointer-transparent (CSS), so they never intercept vertex drags.
 */
import { t } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useEffect, useId, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { useStore } from 'zustand';
import { lessonStore } from '../store';
import { hudLayout, hudTicks, type Box } from './hud';
import type { Row } from './NPanel';
import { overlayOn, type StageReg } from './handles';
import type { Look } from './look';

// ---------------------------------------------------------------------------------------------------------------
// Galaxy panel image: a small procedural spiral galaxy painted once per seed on a 2D canvas (particles along two
// logarithmic arms, a warm bulge, field stars) and cached as a data URL. A photograph-like thumbnail at no asset cost.

const GALAXY_PX = 128;
const galaxyCache = new Map<number, string>();

/** Deterministic pseudo-random sequence (mulberry32), so a panel looks the same on every visit. */
function rng(seed: number): () => number {
	let a = seed >>> 0;
	return () => {
		a = (a + 0x6d2b79f5) >>> 0;
		let x = Math.imul(a ^ (a >>> 15), 1 | a);
		x = (x + Math.imul(x ^ (x >>> 7), 61 | x)) ^ x;
		return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
	};
}

function galaxyImage(seed: number): string | null {
	const hit = galaxyCache.get(seed);
	if (hit) return hit;
	const canvas = document.createElement('canvas');
	canvas.width = canvas.height = GALAXY_PX;
	const g = canvas.getContext('2d');
	if (!g) return null;
	const rand = rng(seed);
	const c = GALAXY_PX / 2;
	g.fillStyle = '#03050c';
	g.fillRect(0, 0, GALAXY_PX, GALAXY_PX);
	for (let i = 0; i < 90; i++) {
		g.fillStyle = `rgba(200,215,255,${0.25 + 0.6 * rand()})`;
		g.fillRect(rand() * GALAXY_PX, rand() * GALAXY_PX, 1, 1);
	}
	g.globalCompositeOperation = 'lighter';
	const tilt = rand() * Math.PI;
	const incl = 0.45 + 0.25 * rand();
	const cos = Math.cos(tilt);
	const sin = Math.sin(tilt);
	for (let i = 0; i < 2600; i++) {
		const arm = i % 2;
		const r = Math.pow(rand(), 0.7) * c * 0.9;
		const theta = arm * Math.PI + Math.log(r / c + 0.05) * 2.6 + (rand() - 0.5) * (0.5 + (0.35 * (c - r)) / c);
		const x = Math.cos(theta) * r;
		const y = Math.sin(theta) * r * incl;
		const px = c + x * cos - y * sin;
		const py = c + x * sin + y * cos;
		const warm = Math.max(0, 1 - r / (c * 0.35));
		const R = Math.round(120 + 135 * warm);
		const G = Math.round(140 + 90 * warm);
		const B = Math.round(255 - 40 * warm);
		g.fillStyle = `rgba(${R},${G},${B},${0.08 + 0.12 * rand()})`;
		const s = 0.6 + rand() * 1.4;
		g.fillRect(px, py, s, s);
	}
	const bulge = g.createRadialGradient(c, c, 0, c, c, c * 0.32);
	bulge.addColorStop(0, 'rgba(255,240,225,0.95)');
	bulge.addColorStop(0.35, 'rgba(220,200,255,0.35)');
	bulge.addColorStop(1, 'rgba(120,120,255,0)');
	g.fillStyle = bulge;
	g.fillRect(0, 0, GALAXY_PX, GALAXY_PX);
	const url = canvas.toDataURL('image/png');
	galaxyCache.set(seed, url);
	return url;
}

// ---------------------------------------------------------------------------------------------------------------

function GalaxyPanel({ box, seed, label }: { box: Box; seed: number; label: string }): React.JSX.Element {
	const [href, setHref] = useState<string | null>(null);
	// Painted after mount (browser-only API); the frame is there from the first render.
	useEffect(() => setHref(galaxyImage(seed)), [seed]);
	const { x, y, size: s } = box;
	const c = s * 0.18;
	return (
		<g className="lab-hud-galaxy" role="img" aria-label={label}>
			{href && <image href={href} x={x} y={y} width={s} height={s} preserveAspectRatio="xMidYMid slice" opacity={0.92} />}
			<rect x={x} y={y} width={s} height={s} fill="none" strokeOpacity={0.45} />
			{/* Corner brackets: the instrument frame of the reference. */}
			<path
				d={`M${x - 3} ${y + c}V${y - 3}H${x + c} M${x + s - c} ${y - 3}H${x + s + 3}V${y + c} M${x + s + 3} ${y + s - c}V${y + s + 3}H${x + s - c} M${x + c} ${y + s + 3}H${x - 3}V${y + s - c}`}
				fill="none"
			/>
		</g>
	);
}

/** Target ring: two circles, a cross hair through them and a small centre ring. */
function TargetRing({ box }: { box: Box }): React.JSX.Element {
	const { x, y, size: r } = box;
	return (
		<g className="lab-hud-target">
			<circle cx={x} cy={y} r={r} fill="none" />
			<circle cx={x} cy={y} r={r * 0.32} fill="none" />
			<path d={`M${x - r * 1.9} ${y}H${x - r * 0.5} M${x + r * 0.5} ${y}H${x + r * 1.9} M${x} ${y - r * 1.9}V${y - r * 0.5} M${x} ${y + r * 0.5}V${y + r * 1.9}`} />
		</g>
	);
}

/** Dial: tick ring (every 10 degrees, long every 30), the angle as an arc from 0 and a needle; no needle without a value. */
function Dial({ box, deg, label }: { box: Box; deg: number | null; label: string }): React.JSX.Element {
	const { x, y, size: r } = box;
	const ticks = useMemo(
		() =>
			Array.from({ length: 36 }, (_, i) => {
				const a = (i * 10 * Math.PI) / 180;
				const l = i % 3 === 0 ? 0.2 : 0.1;
				return `M${x + Math.cos(a) * r} ${y - Math.sin(a) * r}L${x + Math.cos(a) * r * (1 - l)} ${y - Math.sin(a) * r * (1 - l)}`;
			}).join(''),
		[x, y, r],
	);
	let arc = '';
	let needle = '';
	if (deg !== null && Number.isFinite(deg)) {
		const a = (deg * Math.PI) / 180;
		const ra = r * 0.62;
		const large = Math.abs(deg % 360) > 180 ? 1 : 0;
		arc = `M${x + ra} ${y}A${ra} ${ra} 0 ${large} ${deg >= 0 ? 0 : 1} ${x + Math.cos(a) * ra} ${y - Math.sin(a) * ra}`;
		needle = `M${x} ${y}L${x + Math.cos(a) * r * 0.95} ${y - Math.sin(a) * r * 0.95}`;
	}
	return (
		<g className="lab-hud-dial" role="img" aria-label={label}>
			<circle cx={x} cy={y} r={r} fill="none" />
			<circle cx={x} cy={y} r={r * 1.18} fill="none" strokeOpacity={0.35} />
			<path d={ticks} />
			{arc && <path className="lab-hud-hot" d={arc} fill="none" />}
			{needle && <path className="lab-hud-hot" d={needle} />}
			<circle cx={x} cy={y} r={Math.max(1.5, r * 0.06)} />
		</g>
	);
}

/** Planet in a ring instrument: a lit sphere, its ring (back half behind, front half over), a framing lens circle. */
function Planet({ box, tilt, id, label }: { box: Box; tilt: number; id: string; label: string }): React.JSX.Element {
	const { x, y, size: r } = box;
	const ring = (front: boolean) => {
		const rx = r * 1.55;
		const ry = r * 0.42;
		// Half ellipse: the far half is drawn first and hidden behind the sphere, the near half over it.
		return `M${x - rx} ${y}A${rx} ${ry} 0 0 ${front ? 0 : 1} ${x + rx} ${y}`;
	};
	return (
		<g className="lab-hud-planet" role="img" aria-label={label} transform={`rotate(${tilt} ${x} ${y})`}>
			<defs>
				<radialGradient id={id} cx="35%" cy="30%" r="75%">
					<stop offset="0" stopColor="#bfe9ff" />
					<stop offset="0.35" stopColor="var(--viz-vec-a)" />
					<stop offset="0.8" stopColor="#123a73" />
					<stop offset="1" stopColor="#050a18" />
				</radialGradient>
			</defs>
			<circle cx={x} cy={y} r={r * 2.05} fill="none" strokeOpacity={0.5} />
			<path d={ring(false)} fill="none" strokeOpacity={0.7} />
			<circle cx={x} cy={y} r={r} fill={`url(#${id})`} stroke="none" />
			<path d={`M${x - r * 0.9} ${y - r * 0.25}Q${x} ${y - r * 0.05} ${x + r * 0.9} ${y - r * 0.3}`} fill="none" strokeOpacity={0.25} />
			<path d={ring(true)} fill="none" />
		</g>
	);
}

export interface InstrumentsProps {
	reg: StageReg;
	lesson: string;
	look?: Look;
	angleKey?: string;
	rows?: readonly Row[];
}

export function Instruments({ reg, lesson, look = 'holo', angleKey, rows = [] }: InstrumentsProps): React.JSX.Element {
	const locale = useLocale();
	const uid = useId().replace(/:/g, '');
	const vals = useStore(lessonStore(lesson), (s) => s.vals);
	const hover = useSyncExternalStore(reg.hover.subscribe, reg.hover.get, reg.hover.get);
	const view = useSyncExternalStore(reg.view.subscribe, reg.view.get, reg.view.get);
	const root = useRef<HTMLDivElement>(null);
	const [size, setSize] = useState({ w: 0, h: 0 });
	useEffect(() => {
		const el = root.current;
		if (!el) return;
		const ro = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }));
		ro.observe(el);
		return () => ro.disconnect();
	}, []);

	const def = reg.card?.overlays.find((o) => o.id === 'instruments');
	const on = !def || !reg.card || overlayOn(vals, reg.card.id, def);
	const is3d = reg.mode === '3d';
	const clay = look === 'clay';

	// Angle of the dial: the card's angle value in 2D, the camera azimuth in 3D, nothing in a 2D card without one.
	const angle = angleKey ? Number(vals[angleKey] ?? 0) : is3d ? view.azimuth : null;
	// Caption: the hovered term (handle owner, scene object or N-panel field), else the angle.
	const term = hover.object ?? hover.field ?? (hover.handle ? reg.list().find((d) => d.id === hover.handle)?.owner : null) ?? null;
	const row = term ? rows.find((r) => r.id === term) : undefined;
	const raw = row ? (row.value ? row.value(vals) : vals[row.key ?? row.id]) : undefined;
	const fmt = (x: number) => (row?.fmt ? row.fmt(x) : x.toFixed(2));
	let caption: { label: string; value: string; note: string } | null = null;
	if (row && raw !== undefined)
		caption = {
			label: row.label,
			value: typeof raw === 'number' ? fmt(raw) : raw.map(fmt).join(', '),
			note: row.kind === 'ro' ? t(locale, 'hud.computed') : t(locale, 'hud.linked'),
		};
	else if (angle !== null)
		caption = {
			label: angleKey ? t(locale, 'hud.angle') : t(locale, 'hud.azimuth'),
			value: `${angle.toFixed(1)}°`,
			note: angleKey ? t(locale, 'hud.linked') : t(locale, 'hud.orientation'),
		};

	// Each card shows its own pair of galaxies, stable across visits.
	const seed = [...(reg.card?.id ?? 'hud')].reduce((a, ch) => (a * 31 + ch.charCodeAt(0)) % 100003, 17);
	// The overlay's own box; before its first ResizeObserver report, the stage size the camera feed saw.
	const w = size.w || view.w;
	const h = size.h || view.h;
	const L = w > 0 && h > 0 ? hudLayout(w, h, is3d, !is3d) : null;
	const ticks = L ? hudTicks(view.ticks, L.ruler.font) : [];
	const dialLabel = t(locale, 'hud.degrees', { name: t(locale, angleKey || !is3d ? 'hud.angle' : 'hud.azimuth'), value: (angle ?? 0).toFixed(1) });

	return (
		<div className="lab-instruments" data-look={look} hidden={!on} ref={root} aria-label={t(locale, 'hud.instruments')}>
			{L && (
				<svg className="lab-hud" width={w} height={h} viewBox={`0 0 ${w} ${h}`} aria-hidden={false}>
					{!clay && (
						<defs>
							<filter id={`${uid}-glow`} x="-20%" y="-20%" width="140%" height="140%">
								<feGaussianBlur stdDeviation={1.6 * L.k} result="b" />
								<feMerge>
									<feMergeNode in="b" />
									<feMergeNode in="SourceGraphic" />
								</feMerge>
							</filter>
						</defs>
					)}
					<g filter={clay ? undefined : `url(#${uid}-glow)`}>
						{/* Framing arcs, faint, like the large curved scales at the edges of the reference. */}
						<path
							className="lab-hud-arc"
							d={`M${L.arc.cx + Math.sqrt(Math.max(0, L.arc.r ** 2 - (h / 2) ** 2))} 0A${L.arc.r} ${L.arc.r} 0 0 1 ${L.arc.cx + Math.sqrt(Math.max(0, L.arc.r ** 2 - (h / 2) ** 2))} ${h}
								M${w - L.arc.cx - Math.sqrt(Math.max(0, L.arc.r ** 2 - (h / 2) ** 2))} 0A${L.arc.r} ${L.arc.r} 0 0 0 ${w - L.arc.cx - Math.sqrt(Math.max(0, L.arc.r ** 2 - (h / 2) ** 2))} ${h}`}
							fill="none"
						/>
						<g className="lab-hud-ruler" role="img" aria-label={t(locale, 'hud.scale', { value: view.unitPx.toFixed(1) })}>
							<path d={`M${L.ruler.xl} ${L.ruler.top}V${L.ruler.bottom}M${L.ruler.xr} ${L.ruler.top}V${L.ruler.bottom}`} />
							<path
								d={ticks
									.filter((tk) => tk.y >= L.ruler.top && tk.y <= L.ruler.bottom)
									.map((tk) => {
										const len = tk.major ? L.ruler.major : L.ruler.minor;
										return `M${L.ruler.xl} ${tk.y}h${len}M${L.ruler.xr} ${tk.y}h${-len}`;
									})
									.join('')}
							/>
							{ticks.map(
								(tk) =>
									tk.label !== null &&
									tk.y >= L.ruler.top + L.ruler.font &&
									tk.y <= L.ruler.bottom && (
										<text key={tk.y} x={L.ruler.xl + L.ruler.major + 3} y={tk.y} fontSize={L.ruler.font} dominantBaseline="central">
											{tk.label}
										</text>
									),
							)}
						</g>
						<TargetRing box={L.target} />
						<Dial box={L.dial} deg={angle} label={dialLabel} />
						{!clay && (
							<>
								<GalaxyPanel box={L.panels[0]} seed={seed} label={t(locale, 'hud.starfield')} />
								<GalaxyPanel box={L.panels[1]} seed={seed + 7} label={t(locale, 'hud.starfield')} />
								<Planet box={L.planet} tilt={is3d ? -18 + view.azimuth / 6 : -18} id={`${uid}-planet`} label={t(locale, 'hud.sphere')} />
							</>
						)}
					</g>
				</svg>
			)}
			{L && caption && (
				<div
					className="lab-hud-caption"
					style={{ right: `${L.caption.right}px`, top: `${L.caption.top}px`, maxWidth: `${L.caption.width}px`, fontSize: `${L.caption.font}px` }}
				>
					<span>{caption.label}</span>
					<output>{caption.value}</output>
					{!L.compact && <span className="lab-hud-note">{caption.note}</span>}
				</div>
			)}
		</div>
	);
}
