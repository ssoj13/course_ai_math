/**
 * Coordinate grid (visual-language.md): drei's `Grid` shader for the lines (infinite, anti-aliased, pixel-thin at
 * any zoom; it follows the camera, so panning never reaches an edge), and our own primitives on top: axes in
 * Blender's axis colors, tick marks and tick numbers in muted mono, and axis letters, so color is never the only cue.
 * 2D lies in the z = 0 plane with "infinite" axes and no fade (a sheet of graph paper); 3D lies on the floor
 * (y = 0, y-up), fades with distance like Blender's floor, and adds a vertical y axis and a z axis.
 * Drawn first and without depth writes, so anything coplanar draws on top.
 * Scene furniture (`userData.aux`): not part of the figure's bounds, so the home view frames what the lesson draws
 * (vectors, shapes), not the +-extent of the axes.
 */
import { Grid as ShaderGrid } from '@react-three/drei';
import { useLayoutEffect, useMemo, useRef } from 'react';
import { DoubleSide, type Mesh, type ShaderMaterial } from 'three';
import { tickText, tickValues } from '../geom';
import { STYLE, usePalette } from '../palette';
import { useLabel, useStage } from './handles';
import { SLine, useWorldPx } from './view';

export interface GridProps {
	/** Lesson store of the stage (tick numbers and axis letters are overlay labels). */
	lesson: string;
	extent: number;
	/** Major line spacing (also the tick spacing). */
	step: number;
	/** Optional minor line spacing. */
	minor?: number;
	axes?: boolean;
	/** 2D only: tick marks and numbers on the axes within +-extent. */
	ticks?: boolean;
	/** Grid lines; off keeps the axes, ticks and numbers (a card's "Grid" overlay hides only the lines). */
	lines?: boolean;
}

type P3 = [number, number, number];

// 2D axes reach far past any zoom-out limit (Stage clamps zoom), so they read as infinite lines.
const FAR = 400;
const TICK_PX = 5;
/** Least screen distance between two numbered ticks: closer numbers are skipped, so they never overlap. */
const NUMBER_PX = 26;

function TickLabel({ lesson, at, offset, text }: { lesson: string; at: P3; offset: readonly [number, number]; text: string }): null {
	useLabel({ lesson, text, role: 'muted', small: true, offset, at: () => at });
	return null;
}

function AxisLetter({ lesson, ch, at, offset }: { lesson: string; ch: 'x' | 'y' | 'z'; at: P3; offset: readonly [number, number] }): null {
	const role = ch === 'x' ? 'axisX' : ch === 'y' ? 'axisY' : 'axisZ';
	useLabel({ lesson, text: ch, role, offset, at: () => at });
	return null;
}

const AUX = { aux: true };

export function Grid({ lesson, extent: e, step, minor, axes = true, ticks = true, lines = true }: GridProps): React.JSX.Element {
	const reg = useStage();
	const pal = usePalette();
	const is3d = reg.mode === '3d';
	const mesh = useRef<Mesh>(null);
	// drei's material writes depth; the grid must never hide what is drawn on or under it.
	useLayoutEffect(() => {
		const m = mesh.current?.material as ShaderMaterial | undefined;
		if (m) m.depthWrite = false;
		// Switching the lines back on mounts a new drei mesh with a fresh material.
	}, [lines]);
	const ax = useMemo(() => {
		const L = is3d ? e : e * FAR;
		return { x: [[-L, 0, 0], [L, 0, 0]] as P3[], y: [[0, -L, 0], [0, L, 0]] as P3[], z: [[0, 0, -e], [0, 0, e]] as P3[] };
	}, [e, is3d]);
	const showTicks = axes && ticks && !is3d;
	const tick = useWorldPx(TICK_PX, [0, 0, 0]);
	const vals = useMemo(() => tickValues(e, step), [e, step]);
	const tickSegs = useMemo(() => {
		if (!showTicks || !tick) return null;
		const h = tick / 2;
		return vals.flatMap((v): P3[] => [[v, -h, 0], [v, h, 0], [-h, v, 0], [h, v, 0]]);
	}, [showTicks, tick, vals]);
	// Numbers on every tick up to five per half-axis and at least NUMBER_PX apart on screen (a phone-sized stage
	// thins them by its own scale), else on every second (or more) tick.
	const pxPerTick = tick ? (step * TICK_PX) / tick : Infinity;
	const every = Math.max(1, Math.ceil(vals.length / 2 / 5), Math.ceil(NUMBER_PX / pxPerTick));
	const labelled = showTicks ? vals.filter((v) => Math.round(Math.abs(v) / step) % every === 0) : [];
	const flat = { transparent: true, depthWrite: false };
	return (
		<group userData={AUX}>
			{lines && <ShaderGrid
				ref={mesh}
				args={[2, 2]}
				rotation={is3d ? [0, 0, 0] : [Math.PI / 2, 0, 0]}
				infiniteGrid
				followCamera
				cellSize={minor ?? step}
				sectionSize={step}
				cellThickness={minor ? 1 : 0}
				sectionThickness={1}
				cellColor={pal.gridMinor}
				sectionColor={pal.grid}
				fadeDistance={is3d ? e * 7 : 1e4}
				fadeStrength={is3d ? 1.6 : 0}
				side={DoubleSide}
				renderOrder={-3}
			/>}
			{axes && (
				<>
					<SLine points={ax.x} color={pal.axisX} lineWidth={STYLE.edge} {...flat} renderOrder={-1} />
					<SLine points={ax.y} color={pal.axisY} lineWidth={STYLE.edge} {...flat} renderOrder={-1} />
					{is3d && <SLine points={ax.z} color={pal.axisZ} lineWidth={STYLE.edge} {...flat} renderOrder={-1} />}
					{tickSegs && <SLine points={tickSegs} segments color={pal.muted} lineWidth={1} {...flat} renderOrder={-1} />}
					{labelled.map((v) => (
						<TickLabel key={`x${v}`} lesson={lesson} at={[v, 0, 0]} offset={[0, 11]} text={tickText(v)} />
					))}
					{labelled.map((v) => (
						<TickLabel key={`y${v}`} lesson={lesson} at={[0, v, 0]} offset={[-11, 0]} text={tickText(v)} />
					))}
					{/* 2D: letters sit inside the frame (extent reaches the nearer stage edge); 3D: just past the axis ends. */}
					<AxisLetter lesson={lesson} ch="x" at={[e, 0, 0]} offset={is3d ? [0, -12] : [-10, -12]} />
					{/* 3D: the default camera frames the floor and cuts the top of the vertical axis, so its letter sits lower. */}
					<AxisLetter lesson={lesson} ch="y" at={is3d ? [0, 0.8 * e, 0] : [0, e, 0]} offset={is3d ? [12, 0] : [12, 10]} />
					{is3d && <AxisLetter lesson={lesson} ch="z" at={[0, 0, e]} offset={[0, -12]} />}
				</>
			)}
		</group>
	);
}
