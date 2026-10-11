/**
 * Clay studio solids (visual-language.md "Clay look", reference refs/clay-style-ref.png): a matte rounded box with
 * matcap shading (`public/matcap-clay.png`, a generated lookup, no lights needed), a soft contact shadow on the floor
 * under it, a hot outline (back faces of a slightly larger copy) and, for a negative tile, a hatched top with an
 * accent rim (sign never colour-only). `ClayTile` is a flat tile (unit square by default), `ClayCube` a unit cube.
 * Lazy: only clay cards load it.
 */
import { useLoader } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { BackSide, SRGBColorSpace, TextureLoader } from 'three';
import { RoundedBoxGeometry } from 'three/examples/jsm/geometries/RoundedBoxGeometry.js';
import { usePalette } from '../palette';
import { useHot } from '../store';
import { HOVER_RANK, useHoverable } from './handles';
import { SLine } from './view';

export interface ClayProps {
	lesson: string;
	id: string;
	/** Centre of the solid. */
	at?: [number, number, number];
	/** Size along x, y (height), z. */
	size?: [number, number, number];
	role?: 'clayA' | 'clayB' | 'clayC' | 'clayAccent';
	/** A negative (subtracted) tile: hatched top with an accent rim. */
	negative?: boolean;
}

const VERTEX = /* glsl */ `
varying vec2 st;
void main() {
	st = uv;
	gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

/** Contact shadow: a radial falloff on a quad 1.4x the footprint (a blurred contact shadow of a flat body). */
const SHADOW = /* glsl */ `
varying vec2 st;
void main() {
	float d = length((st - 0.5) * 2.0);
	gl_FragColor = vec4(0.0, 0.0, 0.0, 0.45 * (1.0 - smoothstep(0.2, 1.0, d)));
}`;

/** Diagonal hatch on the top face of a negative tile, 12 stripes across. */
const HATCH = /* glsl */ `
varying vec2 st;
void main() {
	float line = step(0.8, fract((st.x + st.y) * 12.0));
	gl_FragColor = vec4(0.2, 0.15, 0.12, line * 0.65);
}`;

const noRaycast = () => {};

export function ClayTile({ lesson, id, at = [0, 0.12, 0], size = [1, 0.24, 1], role = 'clayA', negative = false }: ClayProps): React.JSX.Element {
	const pal = usePalette();
	const hot = useHot(lesson, id);
	const hover = useHoverable(lesson, id, HOVER_RANK.fill);
	const matcap = useLoader(TextureLoader, import.meta.env.BASE_URL.replace(/\/$/, '') + '/matcap-clay.png');
	matcap.colorSpace = SRGBColorSpace;
	const [x, y, z] = size;
	// Bevel radius: 6 % of the smallest side, so thin tiles keep flat tops and cubes read as soft clay.
	const geo = useMemo(() => new RoundedBoxGeometry(x, y, z, 3, Math.min(x, y, z) * 0.06), [x, y, z]);
	useEffect(() => () => geo.dispose(), [geo]);
	const top = y / 2;
	return (
		<group position={at} {...hover}>
			<mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, -top - 0.002, 0]} renderOrder={-0.1} raycast={noRaycast}>
				<planeGeometry args={[x * 1.4, z * 1.4]} />
				<shaderMaterial transparent depthWrite={false} vertexShader={VERTEX} fragmentShader={SHADOW} />
			</mesh>
			{hot && (
				<mesh geometry={geo} scale={[1.035, 1.035, 1.035]}>
					<meshBasicMaterial color={pal.hot} side={BackSide} />
				</mesh>
			)}
			<mesh geometry={geo}>
				<meshMatcapMaterial matcap={matcap} color={pal[role]} />
			</mesh>
			{negative && (
				<>
					<SLine
						points={[
							[-x / 2, top + 0.002, -z / 2],
							[x / 2, top + 0.002, -z / 2],
							[x / 2, top + 0.002, z / 2],
							[-x / 2, top + 0.002, z / 2],
							[-x / 2, top + 0.002, -z / 2],
						]}
						color={pal.clayAccent}
						lineWidth={2}
					/>
					<mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, top + 0.003, 0]}>
						<planeGeometry args={[x * 0.9, z * 0.9]} />
						<shaderMaterial transparent depthWrite={false} vertexShader={VERTEX} fragmentShader={HATCH} />
					</mesh>
				</>
			)}
		</group>
	);
}

export function ClayCube(p: ClayProps): React.JSX.Element {
	return <ClayTile {...p} size={p.size ?? [1, 1, 1]} />;
}
