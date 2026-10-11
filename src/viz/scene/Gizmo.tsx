/**
 * Navigation gizmo (Blender's, top right of a 3D viewport): X/Y/Z bubbles in Blender's axis colors with letters,
 * negative axes as hollow bubbles, sorted by depth; click a bubble to align the view along that axis with the smooth
 * view (click the facing one again for the opposite side). 2D viewports show a tiny static XY corner instead.
 *
 * Why not drei `GizmoHelper` / three `ViewHelper` (fallback order of Task 6d, measured in headless Chrome with two
 * Views on one shared canvas, task-6d-evidence/exp*): GizmoHelper's Hud renders after the Views without a scissor,
 * into whatever viewport the last View set, so the gizmo of one stage appeared in another stage (renderPriority 1
 * even cleared the whole canvas); it also sizes itself from the View's stale `size`. ViewHelper renders correctly
 * once autoClear is off and its location is fed from the stage rect, but its 128 px size, colors, black negative
 * bubbles and constant-speed animation are private to its closure. So the gizmo is DOM: an SVG in the stage overlay,
 * updated per rendered frame from the camera quaternion (GizmoSync in view.tsx). It is crisp at any DPR, uses the
 * real UI font and needs no GL state juggling; keyboard users get six hidden buttons in a fixed order.
 * Three-free: the layout math is pure and unit-tested.
 */
import { useLayoutEffect, useRef, useState } from 'react';
import { roleVar } from '../palette';
import type { StageReg } from './handles';

export interface GizmoHead {
	axis: 0 | 1 | 2;
	sign: 1 | -1;
	/** Screen offset from the gizmo center in px, y down. */
	x: number;
	y: number;
	/** View-space depth; larger = nearer the viewer. */
	z: number;
}

const AXES: readonly (readonly [0 | 1 | 2, 1 | -1])[] = [
	[0, 1],
	[1, 1],
	[2, 1],
	[0, -1],
	[1, -1],
	[2, -1],
];

/**
 * Screen layout of the six axis bubbles for a camera with world orientation `q` = [x, y, z, w] (unit quaternion):
 * each axis is rotated into view space by the inverse rotation, scaled to `r` px, y flipped for the screen. Sorted
 * back to front, so later bubbles paint over earlier ones like in Blender.
 */
export function gizmoLayout(q: readonly number[], r: number): GizmoHead[] {
	// Inverse of a unit quaternion = its conjugate; v' = v + w t + u x t with t = 2 u x v, u = -q.xyz.
	const ux = -q[0];
	const uy = -q[1];
	const uz = -q[2];
	const w = q[3];
	return AXES.map(([axis, sign]) => {
		const v = [0, 0, 0];
		v[axis] = sign;
		const tx = 2 * (uy * v[2] - uz * v[1]);
		const ty = 2 * (uz * v[0] - ux * v[2]);
		const tz = 2 * (ux * v[1] - uy * v[0]);
		const x = v[0] + w * tx + (uy * tz - uz * ty);
		const y = v[1] + w * ty + (uz * tx - ux * tz);
		const z = v[2] + w * tz + (ux * ty - uy * tx);
		return { axis, sign, x: x * r, y: -y * r, z };
	}).sort((a, b) => a.z - b.z);
}

/** Which side to align to when the bubble (axis, sign) is clicked: the opposite side if the view already looks along it. */
export function alignSign(viewDir: readonly number[], axis: 0 | 1 | 2, sign: 1 | -1): 1 | -1 {
	return viewDir[axis] * sign > 0.999 ? (-sign as 1 | -1) : sign;
}

const R = 28;
const BUBBLE = 8.5;
const LETTER = ['X', 'Y', 'Z'] as const;
const ROLE = ['axisX', 'axisY', 'axisZ'] as const;
const VIEW_NAME: Record<string, string> = { '0,1': 'Right', '0,-1': 'Left', '1,1': 'Top', '1,-1': 'Bottom', '2,1': 'Front', '2,-1': 'Back' };

/** Status-bar hover id of the gizmo (see StatusBar). */
export const GIZMO_ID = '@gizmo';

export function Gizmo({ reg }: { reg: StageReg }): React.JSX.Element {
	const heads = useRef<Map<string, SVGGElement>>(new Map());
	const lines = useRef<(SVGLineElement | null)[]>([]);
	const layer = useRef<SVGGElement>(null);

	useLayoutEffect(() => {
		let order = '';
		reg.gizmo = (q) => {
			const lay = gizmoLayout(q, R);
			for (const h of lay) {
				const g = heads.current.get(`${h.axis},${h.sign}`);
				g?.setAttribute('transform', `translate(${h.x.toFixed(2)} ${h.y.toFixed(2)})`);
				if (h.sign > 0) {
					const l = lines.current[h.axis];
					l?.setAttribute('x2', h.x.toFixed(2));
					l?.setAttribute('y2', h.y.toFixed(2));
				}
			}
			// Re-stack only when the depth order changes (orbiting moves bubbles every frame, reordering is rare). SVG has no
			// z-index, so depth is DOM order. Safe to move: the bubbles never hold focus (see the key buttons below) and
			// React never reorders this constant keyed list, so its bookkeeping does not depend on their order.
			const next = lay.map((h) => `${h.axis},${h.sign}`).join(';');
			if (next !== order && layer.current) {
				order = next;
				for (const h of lay) {
					const g = heads.current.get(`${h.axis},${h.sign}`);
					if (g) layer.current.appendChild(g);
				}
			}
		};
		reg.nav?.redraw();
		return () => {
			reg.gizmo = null;
		};
	}, [reg]);

	const align = (axis: 0 | 1 | 2, sign: 1 | -1) => {
		const nav = reg.nav;
		if (nav) nav.align(axis, alignSign(nav.viewDir(), axis, sign));
	};
	// Keyboard focus lives on visually hidden buttons in a FIXED order, apart from the bubbles: the bubbles are
	// re-stacked by depth while the view turns, and moving a focused node in the DOM drops its focus (and would make
	// the Tab order change with the orbit). The focused button's bubble draws the focus ring.
	const [focus, setFocus] = useState<string | null>(null);
	const label = (axis: number, sign: number) => `${VIEW_NAME[`${axis},${sign}`]} view (${sign < 0 ? '−' : '+'}${LETTER[axis]})`;

	return (
		<div
			className="lab-gizmo"
			role="group"
			aria-label="View navigation"
			onPointerEnter={() => reg.hover.set({ object: GIZMO_ID })}
			onPointerLeave={() => reg.hover.clear('object', GIZMO_ID)}
		>
			<svg viewBox="-42 -42 84 84" aria-hidden="true">
				<circle className="lab-gizmo-bg" r={41} />
				{([0, 1, 2] as const).map((a) => (
					<line key={a} ref={(el) => void (lines.current[a] = el)} x1={0} y1={0} x2={0} y2={0} stroke={roleVar(ROLE[a])} strokeWidth={2} strokeLinecap="round" />
				))}
				<g ref={layer}>
					{AXES.map(([axis, sign]) => {
						const k = `${axis},${sign}`;
						const color = roleVar(ROLE[axis]);
						const neg = sign < 0;
						return (
							<g
								key={k}
								ref={(el) => {
									if (el) heads.current.set(k, el);
									else heads.current.delete(k);
								}}
								className={`lab-gizmo-head${neg ? ' is-neg' : ''}${focus === k ? ' is-focus' : ''}`}
								data-k={k}
								onClick={() => align(axis, sign)}
							>
								<circle r={BUBBLE} fill={neg ? `color-mix(in srgb, ${color} 28%, ${roleVar('bgBottom')})` : color} stroke={color} />
								<text fill={roleVar(neg ? 'ink' : 'chrome2')}>{LETTER[axis]}</text>
							</g>
						);
					})}
				</g>
			</svg>
			<div className="lab-gizmo-keys">
				{AXES.map(([axis, sign]) => {
					const k = `${axis},${sign}`;
					return (
						<button
							key={k}
							type="button"
							className="lab-handle-key"
							aria-label={label(axis, sign)}
							onClick={() => align(axis, sign)}
							// Hidden buttons are reached only by keyboard (or assistive tech), so any focus draws the ring.
							onFocus={() => setFocus(k)}
							onBlur={() => setFocus((f) => (f === k ? null : f))}
						/>
					);
				})}
			</div>
		</div>
	);
}

/** 2D: a small static XY corner (bottom left), the 2D counterpart of the gizmo; decorative. */
export function XYCorner(): React.JSX.Element {
	const x = roleVar('axisX');
	const y = roleVar('axisY');
	return (
		<svg className="lab-xy" viewBox="0 0 38 38" aria-hidden="true">
			<path d="M7 31 H27" stroke={x} strokeWidth={1.75} strokeLinecap="round" />
			<path d="M27 28 L32 31 L27 34 Z" fill={x} />
			<path d="M7 31 V11" stroke={y} strokeWidth={1.75} strokeLinecap="round" />
			<path d="M4 11 L7 6 L10 11 Z" fill={y} />
			<text x={33} y={23} fill={x}>
				X
			</text>
			<text x={15} y={6} fill={y}>
				Y
			</text>
		</svg>
	);
}
