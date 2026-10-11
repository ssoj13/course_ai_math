/**
 * Feeds the DOM instruments (Instruments.tsx) from the View camera: stage size, scale, azimuth and the projected
 * world y axis, published through the stage registry's `view` cell. Runs every rendered frame after the View's render
 * (priority 2, like the labels), compares against the published snapshot without allocating, and publishes a new
 * snapshot object only when something visible changed, so the instruments re-render on camera moves, not per frame.
 */
import { useFrame } from '@react-three/fiber';
import { Vector3 } from 'three';
import { useStage, type ViewInfo, type YTick } from './handles';
import { wppAt } from './view';

const pt = new Vector3();
const dir = new Vector3();
// Scratch tick list of the frame being measured; only copied into a snapshot when it differs from the published one.
const scratch: YTick[] = [];
const ORIGIN = [0, 0, 0] as const;

/** Tick positions and the scalar readouts are published at this resolution (CSS px, px/unit, degrees). */
const round = (x: number, q: number) => Math.round(x / q) * q;

function sameTicks(a: readonly YTick[], n: number): boolean {
	if (a.length !== n) return false;
	for (let i = 0; i < n; i++) if (a[i].v !== scratch[i].v || a[i].y !== scratch[i].y) return false;
	return true;
}

export function InstrumentSync(): null {
	const reg = useStage();
	useFrame(({ camera }) => {
		const el = reg.el;
		if (!el) return;
		const w = el.clientWidth;
		const h = el.clientHeight;
		const unitPx = round(1 / wppAt(camera, ORIGIN, h), 0.01);
		camera.getWorldDirection(dir);
		const azimuth = reg.mode === '3d' ? round((Math.atan2(-dir.x, -dir.z) * 180) / Math.PI, 0.01) : 0;
		// The world's y axis projected exactly (whole units inside the inner 95 % of the stage), not invented ticks.
		let n = 0;
		const span = Math.ceil(reg.extent * 8);
		for (let v = span; v >= -span; v--) {
			pt.set(0, v, 0).project(camera);
			if (pt.z < -1 || pt.z > 1 || pt.y < -0.95 || pt.y > 0.95) continue;
			const y = round(((1 - pt.y) * h) / 2, 0.25);
			if (n < scratch.length) {
				scratch[n].v = v;
				scratch[n].y = y;
			} else scratch.push({ v, y });
			n++;
		}
		const cur: ViewInfo = reg.view.get();
		if (cur.w === w && cur.h === h && cur.unitPx === unitPx && cur.azimuth === azimuth && sameTicks(cur.ticks, n)) return;
		reg.view.set({ w, h, unitPx, azimuth, ticks: scratch.slice(0, n).map((t) => ({ ...t })) });
	}, 2);
	return null;
}
