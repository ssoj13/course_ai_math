/**
 * Stores and cached scene data of the einsum cards (three-free): the playground (`einsum`, spec §3.5: preset, editable
 * extents and seed, the selected cell and the dive), matrix x vector in its two views (`matvec`, spec §3.1/§3.2) and
 * the zoom ladder (`ladder`, spec §4). The data of a store state is built once per change of the keys it reads and
 * shared by the scene, the N-panel rows and the DOM controls (one `sceneData` = one set of floats).
 */
import { add, CoreError, einsum, explain, planEinsum, tensor, type Plan, type Tensor } from '../../core';
import type { LessonState, Val } from '../store';
import { A_DEF, B_DEF, LAYER_BIAS, operands, PRESETS, sceneData, type PresetId, type SceneData } from './model';

type Vals = LessonState['vals'];

/** Playground presets in button order. */
export const PLAY_IDS: readonly PresetId[] = ['dot', 'matvec', 'matmul', 'batched', 'attention', 'layer'];
export const PLAY = 'einsum';
/** Extent fields (spec §1.4: the slider stops at 64; the LOD picks the picture). */
export const EXT = { min: 1, max: 64, step: 1 } as const;
export const SEED = { min: 0, max: 99, step: 1 } as const;

/** Store key of the extent of `letter` in preset `id` (each preset keeps its own extents). */
export const extKey = (id: PresetId, letter: string) => `x_${id}_${letter}`;
export const letters = (id: PresetId) => Object.keys(PRESETS[id].shapes);

export const PLAY_INIT: Record<string, Val> = {
	preset: 2,
	seed: 0,
	sel: 0,
	dive: 0,
	/** Memory strip: show A through a transposed view (0/1). */
	tA: 0,
	...Object.fromEntries(PLAY_IDS.flatMap((id) => letters(id).map((l) => [extKey(id, l), PRESETS[id].shapes[l]]))),
};

export const presetOf = (v: Vals): PresetId => PLAY_IDS[Math.max(0, Math.min(PLAY_IDS.length - 1, Math.round(v.preset as number)))];

/** A scene to draw, or the typed reason it cannot be drawn (too large: spec §1.4 "Up to 64 per index"). */
export type Built = { ok: true; d: SceneData; plan: Plan } | { ok: false; error: CoreError };

const caches = new Map<string, { key: string; b: Built }>();
/** One build per distinct key per cache slot (the vals object changes on every write; the key only on relevant ones). */
function cached(slot: string, key: string, build: () => Built): Built {
	const c = caches.get(slot);
	if (c && c.key === key) return c.b;
	const b = build();
	caches.set(slot, { key, b });
	return b;
}
function attempt(f: () => { d: SceneData; plan: Plan }): Built {
	try {
		return { ok: true, ...f() };
	} catch (e) {
		if (e instanceof CoreError) return { ok: false, error: e };
		throw e;
	}
}

/** Shapes of preset `id` from the store. */
export const shapesOf = (id: PresetId, v: Vals) => Object.fromEntries(letters(id).map((l) => [l, Math.round(v[extKey(id, l)] as number)]));

export function playData(v: Vals): Built {
	const id = presetOf(v), shapes = shapesOf(id, v), seed = Math.round(v.seed as number);
	return cached('play', JSON.stringify([id, shapes, seed]), () =>
		attempt(() => {
			const plan = planEinsum(PRESETS[id].op, shapes);
			return { plan, d: sceneData(plan, operands(PRESETS[id], plan, seed)) };
		}),
	);
}

/** The vectors a dive shows: the two operand values that meet along the shaft of cell `c`, from `explain`. */
export function diveVectors(d: SceneData, c: number): { a: number[]; b: number[] } {
	const rank = d.L.plan.kept.length;
	const cell = Array.from(d.L.cells.idx.subarray(c * rank, c * rank + rank));
	const ex = explain(d.L.plan, cell, d.ts);
	return { a: ex.terms.map((tm) => tm.vals![0]), b: ex.terms.map((tm) => tm.vals![1]) };
}

// ---------------------------------------------------------------------------------------------------------------
// Matrix x vector, two views (W is 2 x j; j is editable).

export const MATVEC = 'matvec';
export const MV_J = { min: 1, max: 8, step: 1 } as const;
export const MV_INIT: Record<string, Val> = {
	mv_j: 3,
	seed: 0,
	...Object.fromEntries(Array.from({ length: MV_J.max }, (_, j) => [['cv_f' + j, [0, 0]], ['cv_t' + j, [0, 0]]]).flat()),
};
export function mvData(v: Vals): Built {
	const j = Math.round(v.mv_j as number), seed = Math.round(v.seed as number);
	return cached('mv', JSON.stringify([j, seed]), () =>
		attempt(() => {
			const plan = planEinsum(PRESETS.matvec.op, { i: 2, j });
			return { plan, d: sceneData(plan, operands(PRESETS.matvec, plan, seed)) };
		}),
	);
}

// ---------------------------------------------------------------------------------------------------------------
// The zoom ladder: fixed presets at fixed values (spec §4), the dived pair of vectors.

export const LADDER = 'ladder';
export const LAD_INIT: Record<string, Val> = { lad_a: [...A_DEF], lad_b: [...B_DEF], lad_tile: 0 };

function fixed(id: PresetId): { d: SceneData; plan: Plan } {
	const plan = planEinsum(PRESETS[id].op, PRESETS[id].shapes);
	return { plan, d: sceneData(plan, operands(PRESETS[id], plan, 0)) };
}
let ladderCache: { matmul: SceneData; layer: SceneData; attention: SceneData; bias: Tensor; biased: Tensor } | null = null;
/** The rung scenes (built once: the ladder's numbers are the section 2/3 defaults so the 3 shows at rungs 1-4). */
export function ladderData() {
	if (ladderCache) return ladderCache;
	const layer = fixed('layer').d;
	const bias = tensor([...LAYER_BIAS]);
	const biased = add(einsum(layer.L.plan, layer.ts[0], layer.ts[1]), bias);
	ladderCache = { matmul: fixed('matmul').d, layer, attention: fixed('attention').d, bias, biased };
	return ladderCache;
}
