/**
 * In-place position writes for fat lines (three-stdlib `LineGeometry` / `LineSegmentsGeometry`, drawn by drei `Line`).
 *
 * Why not the geometry's own `setPositions`: it allocates a new interleaved buffer per call, and three's WebGL
 * renderer caches the instance count of an `InstancedBufferGeometry` (`_maxInstanceCount`) the first time the
 * geometry is bound; only `geometry.dispose()` clears it. A line whose point count later grows therefore keeps
 * drawing its first segment count (measured: an epicycle trace stuck at one segment, a chain grown from 3 to 10
 * sticks showing 3), and one that shrinks draws instances past the end of its new buffer.
 *
 * `writeLine` instead keeps one buffer per geometry with spare capacity: the drawn count is `instanceCount`, a write
 * with the same or a smaller count only rewrites floats, and growth reallocates to the next power of two and disposes
 * the geometry once, which resets the renderer's cached count. Imports three core only, so it is unit-testable.
 */
import { InstancedInterleavedBuffer, InterleavedBufferAttribute, type InstancedBufferGeometry } from 'three';

/** Floats per segment instance (start xyz + end xyz) and per distance instance (start, end). */
const SEG = 6;
const DIST = 2;

/** Segment instances drawn for `pts` points: a polyline joins neighbours, `pairs` takes the points two by two. */
export function segCount(pts: number, pairs: boolean): number {
	if (pairs) {
		if (pts % 2) throw new Error(`a segment list needs an even number of points, got ${pts}`);
		return pts / 2;
	}
	return Math.max(pts - 1, 0);
}

/** Spare capacity for `n` segments: the next power of two at or above it (at least 1). */
export function capFor(n: number): number {
	let c = 1;
	while (c < n) c *= 2;
	return c;
}

/** Segment capacity of `geo`'s instance buffers (0 when the line has none of ours or drei's yet). */
export function lineCap(geo: InstancedBufferGeometry): number {
	const s = geo.getAttribute('instanceStart') as InterleavedBufferAttribute | undefined;
	const d = geo.getAttribute('instanceDistanceStart') as InterleavedBufferAttribute | undefined;
	if (!s || !d || !(s.data.array instanceof Float32Array) || !(d.data.array instanceof Float32Array)) return 0;
	// Both buffers must cover every instance: the renderer derives its instance limit from whichever it binds first.
	return Math.min(s.data.array.length / SEG, d.data.array.length / DIST);
}

/** Replaces the instance buffers of `geo` with zeroed ones for `cap` segments and drops the renderer's cached count. */
function grow(geo: InstancedBufferGeometry, cap: number): void {
	// dispose() frees the old GPU buffers and deletes the cached `_maxInstanceCount`; three re-uploads on the next draw.
	geo.dispose();
	const seg = new InstancedInterleavedBuffer(new Float32Array(cap * SEG), SEG, 1);
	geo.setAttribute('instanceStart', new InterleavedBufferAttribute(seg, 3, 0));
	geo.setAttribute('instanceEnd', new InterleavedBufferAttribute(seg, 3, 3));
	const dist = new InstancedInterleavedBuffer(new Float32Array(cap * DIST), DIST, 1);
	geo.setAttribute('instanceDistanceStart', new InterleavedBufferAttribute(dist, 1, 0));
	geo.setAttribute('instanceDistanceEnd', new InterleavedBufferAttribute(dist, 1, 1));
}

/**
 * Writes a flat xyz point list into a fat-line geometry: a polyline, or with `pairs` independent segments (drei's
 * `segments`). Draws exactly the given segments (`instanceCount`), keeps the dash distances in step (what
 * `computeLineDistances` would give) and refreshes the bounds used by hover raycasts and framing. Unused capacity is
 * filled with copies of the last segment (or the single point; the origin for an empty list) so bounds and raycasts,
 * which read the whole buffer, never see stale points. No allocation unless the count exceeds the capacity.
 */
export function writeLine(geo: InstancedBufferGeometry, flat: ArrayLike<number>, pairs = false): void {
	if (flat.length % 3) throw new Error(`expected a flat xyz list, got ${flat.length} numbers`);
	const pts = flat.length / 3;
	const n = segCount(pts, pairs);
	let cap = lineCap(geo);
	if (n > cap || cap === 0) grow(geo, (cap = capFor(n)));
	const sAttr = geo.getAttribute('instanceStart') as InterleavedBufferAttribute;
	const dAttr = geo.getAttribute('instanceDistanceStart') as InterleavedBufferAttribute;
	const seg = sAttr.data.array as Float32Array;
	const dist = dAttr.data.array as Float32Array;
	// Point index of the start of segment i: polylines share points between neighbours, pairs do not.
	const step = pairs ? 2 : 1;
	let run = 0;
	for (let i = 0; i < n; i++) {
		const a = i * step * 3;
		const o = i * SEG;
		for (let k = 0; k < 3; k++) {
			seg[o + k] = flat[a + k];
			seg[o + 3 + k] = flat[a + 3 + k];
		}
		dist[i * DIST] = run;
		run += Math.hypot(flat[a + 3] - flat[a], flat[a + 4] - flat[a + 1], flat[a + 5] - flat[a + 2]);
		dist[i * DIST + 1] = run;
	}
	if (n > 0) {
		for (let i = n; i < cap; i++) seg.copyWithin(i * SEG, (n - 1) * SEG, n * SEG);
		const last = dist[(n - 1) * DIST];
		for (let i = n; i < cap; i++) (dist[i * DIST] = last), (dist[i * DIST + 1] = run);
	} else {
		for (let i = 0; i < cap * SEG; i++) seg[i] = pts > 0 ? flat[i % 3] : 0;
		dist.fill(0);
	}
	sAttr.data.needsUpdate = true;
	dAttr.data.needsUpdate = true;
	geo.instanceCount = n;
	geo.computeBoundingBox();
	geo.computeBoundingSphere();
}
