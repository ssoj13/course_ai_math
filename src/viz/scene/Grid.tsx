/**
 * Coordinate grid: major lines every `step`, optional minor lines every `minor`, and axes in the axis colors with
 * letter labels, so color is never the only cue. 2D lies in the z = 0 plane; 3D lies on the floor (y = 0, y-up) and
 * adds a vertical y axis and a z axis. Drawn first and without depth writes, so anything coplanar draws on top.
 * Axis letters are stroked line glyphs: no font file, no network, and they recolor with the theme like everything else.
 */
import { useFrame } from '@react-three/fiber';
import { useMemo, useRef } from 'react';
import { Vector3, type Group } from 'three';
import { usePalette } from '../palette';
import { useStage } from './handles';
import { SLine, wppAt } from './view';

export interface GridProps {
	extent: number;
	step: number;
	minor?: number;
	axes?: boolean;
}

type P3 = [number, number, number];

const isMultiple = (v: number, s: number) => Math.abs(v / s - Math.round(v / s)) < 1e-6;

/** Multiples of `step` within [-extent, extent], minus those `skip` rejects. Integer stepping avoids drift. */
function ticks(extent: number, step: number, skip: (v: number) => boolean): number[] {
	const n = Math.floor(extent / step + 1e-9);
	const out: number[] = [];
	for (let i = -n; i <= n; i++) {
		const v = Number((i * step).toFixed(10));
		if (!skip(v)) out.push(v);
	}
	return out;
}

/** Segment endpoints for lines at `ts` in both directions of the grid plane. */
function lines(ts: number[], e: number, is3d: boolean): P3[] {
	return ts.flatMap((t): P3[] =>
		is3d
			? [[t, 0, -e], [t, 0, e], [-e, 0, t], [e, 0, t]]
			: [[t, -e, 0], [t, e, 0], [-e, t, 0], [e, t, 0]],
	);
}

// Glyph strokes in a unit box centered on the origin (segment endpoint pairs).
const GLYPH: Record<'x' | 'y' | 'z', P3[]> = {
	x: [[-0.3, -0.5, 0], [0.3, 0.5, 0], [-0.3, 0.5, 0], [0.3, -0.5, 0]],
	y: [[-0.3, 0.5, 0], [0, 0, 0], [0.3, 0.5, 0], [0, 0, 0], [0, 0, 0], [0, -0.5, 0]],
	z: [[-0.3, 0.5, 0], [0.3, 0.5, 0], [0.3, 0.5, 0], [-0.3, -0.5, 0], [-0.3, -0.5, 0], [0.3, -0.5, 0]],
};
const GLYPH_PX = 11;
const right = new Vector3();
const up = new Vector3();

/** An axis letter `GLYPH_PX` tall at world point `at`, shifted by `off` screen pixels and always facing the camera. */
function Glyph({ ch, at, off, color }: { ch: 'x' | 'y' | 'z'; at: P3; off: [number, number]; color: string }): React.JSX.Element {
	const reg = useStage();
	const ref = useRef<Group>(null);
	useFrame(({ camera }) => {
		const g = ref.current;
		if (!g || !reg.el) return;
		const wpp = wppAt(camera, at, reg.el.clientHeight);
		right.set(1, 0, 0).applyQuaternion(camera.quaternion);
		up.set(0, 1, 0).applyQuaternion(camera.quaternion);
		g.position.set(...at).addScaledVector(right, off[0] * wpp).addScaledVector(up, off[1] * wpp);
		g.quaternion.copy(camera.quaternion);
		g.scale.setScalar(GLYPH_PX * wpp);
	});
	return (
		<group ref={ref}>
			<SLine points={GLYPH[ch]} segments color={color} lineWidth={1.6} renderOrder={0} />
		</group>
	);
}

export function Grid({ extent: e, step, minor, axes = true }: GridProps): React.JSX.Element {
	const { mode } = useStage();
	const pal = usePalette();
	const is3d = mode === '3d';
	const g = useMemo(() => {
		const major = lines(ticks(e, step, (v) => axes && v === 0), e, is3d);
		const fine = minor ? lines(ticks(e, minor, (v) => isMultiple(v, step)), e, is3d) : [];
		return { major, fine };
	}, [e, step, minor, axes, is3d]);
	const ax = useMemo(
		() => ({
			x: [[-e, 0, 0], [e, 0, 0]] as P3[],
			y: [[0, -e, 0], [0, e, 0]] as P3[],
			z: [[0, 0, -e], [0, 0, e]] as P3[],
		}),
		[e],
	);
	const back = { depthWrite: false, renderOrder: -1 };
	return (
		<group>
			{g.fine.length > 0 && <SLine points={g.fine} segments color={pal.gridMinor} lineWidth={1} {...back} renderOrder={-2} />}
			{g.major.length > 0 && <SLine points={g.major} segments color={pal.grid} lineWidth={1} {...back} />}
			{axes && (
				<>
					<SLine points={ax.x} color={pal.axisX} lineWidth={2} {...back} />
					<SLine points={ax.y} color={pal.axisY} lineWidth={2} {...back} />
					{is3d && <SLine points={ax.z} color={pal.axisZ} lineWidth={2} {...back} />}
					{/* 2D: letters sit inside the frame (extent reaches the nearer stage edge); 3D: just past the axis ends. */}
					<Glyph ch="x" at={[e, 0, 0]} off={is3d ? [0, 12] : [-10, -13]} color={pal.axisX} />
					{/* 3D: the default camera frames the floor and cuts the top of the vertical axis, so its letter sits lower. */}
					<Glyph ch="y" at={is3d ? [0, 0.8 * e, 0] : [0, e, 0]} off={is3d ? [12, 0] : [12, -10]} color={pal.axisY} />
					{is3d && <Glyph ch="z" at={[0, 0, e]} off={[0, 12]} color={pal.axisZ} />}
				</>
			)}
		</group>
	);
}
