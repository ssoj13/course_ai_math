/**
 * Lesson 3.2 (data as tensors): the store contract and every derived number of its four cards. Pure and three-free:
 * the entry, the scenes, the N-panel rows and the tests share it. Values are real data (the 3.1 icon, GloVe rows of
 * lesson 1.7, a sampled tone, a mesh) and every shape, stride, address, view and gather goes through the tensor core
 * (src/core), the NumPy model of lesson 3.3.
 *
 * Store `data-as-tensors`, one link group per card (each write leaves a consistent state; indices are clamped):
 * - image (3.2-1): `chw` 0/1 memory layout (channels last H x W x C, or channels first C x H x W), `peel` 0..1 how far
 *   the colour planes stand apart, `h`, `w`, `c` the selected byte (by MEANING: row, column, channel).
 * - text (3.2-2): `look` 0..12 how many token ids have been looked up in the embedding table so far.
 * - audio (3.2-3): `fl` frame length (a divisor of 48), `s` the selected sample.
 * - mesh (3.2-4): `face` the selected triangle.
 */
import { contiguous, cStrides, flat, isContiguous, reshape, sizeOf, slice, tensor, transpose, type Tensor } from '../../core';
import type { LessonLinks, LinkGroup } from '../links';
import { finite } from '../links';
import type { LessonState, Val } from '../store';
import { GLOVE8, GLOVE_ROW, ICON, SENTENCE, SENTENCE2, type Word } from './tensor-road.data';
import { unravel } from './tensor-cells';

export const LESSON = 'data-as-tensors';
type Vals = LessonState['vals'];

const int = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`data-as-tensors: "${key}" must be a number`);
	finite(v);
	return Math.round(v);
};
const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const num = (v: Val | undefined, key: string): number => {
	if (typeof v !== 'number') throw new Error(`data-as-tensors: "${key}" must be a number`);
	finite(v);
	return v;
};

// ---------------------------------------------------------------------------------------------------------------
// 3.2-1 image: H x W x C, channels as planes.

/** The icon as stored by an image decoder: H x W x C = 8 x 8 x 3 bytes, channels last. */
export const IMG_HWC: Tensor = tensor(ICON);
/** The same bytes channels first (C x H x W): a transposed VIEW, then `contiguous()` = the one real copy. */
export const IMG_CHW: Tensor = contiguous(transpose(IMG_HWC, [2, 0, 1]));
export const IMG_H = IMG_HWC.shape[0], IMG_W = IMG_HWC.shape[1], IMG_C = IMG_HWC.shape[2];

export interface ImageModel {
	chw: boolean;
	peel: number;
	/** Selected byte by meaning. */
	h: number;
	w: number;
	c: number;
	/** The stored tensor (the layout chosen). */
	t: Tensor;
	names: string[];
	/** Selected index in the stored tensor's own axis order. */
	idx: number[];
	addr: number;
	value: number;
	/** The selected pixel's three bytes (R, G, B). */
	rgb: number[];
}

/** Address of byte (h, w, c) in the buffer of the chosen layout. */
export const imgAddr = (chw: boolean, h: number, w: number, c: number): number => (chw ? IMG_CHW.pos([c, h, w]) : IMG_HWC.pos([h, w, c]));

/** (h, w, c) by meaning of the byte at address p of the chosen layout. */
export function imgAt(chw: boolean, p: number): [number, number, number] {
	if (chw) {
		const [c, h, w] = unravel(p, IMG_CHW.shape);
		return [h, w, c];
	}
	const [h, w, c] = unravel(p, IMG_HWC.shape);
	return [h, w, c];
}

export function imageModel(v: Vals): ImageModel {
	const chw = int(v.chw, 'chw') === 1;
	const h = int(v.h, 'h'), w = int(v.w, 'w'), c = int(v.c, 'c');
	const t = chw ? IMG_CHW : IMG_HWC;
	const idx = chw ? [c, h, w] : [h, w, c];
	const addr = t.pos(idx);
	return {
		chw, peel: clamp(num(v.peel, 'peel'), 0, 1), h, w, c, t, idx, addr, value: t.data[addr],
		names: chw ? ['C', 'H', 'W'] : ['H', 'W', 'C'],
		rgb: [0, 1, 2].map((k) => IMG_HWC.get([h, w, k])),
	};
}

// ---------------------------------------------------------------------------------------------------------------
// 3.2-2 text: token ids -> embedding rows -> B x T x D.

/** The two sentences of the batch, word by word. */
export const SENTENCES: readonly (readonly Word[])[] = [SENTENCE, SENTENCE2];
export const TEXT_B = SENTENCES.length, TEXT_T = SENTENCE.length;
/** Embedding numbers drawn per token (the first 8 of GloVe's 300). */
export const TEXT_D = 8;
/** The rows of the table the batch uses, in table order (row id ascending), as the card draws them. */
export const VOCAB: readonly Word[] = (Object.keys(GLOVE_ROW) as Word[]).sort((a, b) => GLOVE_ROW[a] - GLOVE_ROW[b]);
/** Token ids, shape (B, T): what a tokenizer hands the network. */
export const IDS: Tensor = tensor(SENTENCES.map((s) => s.map((w) => GLOVE_ROW[w])), 'f64');
/** The drawn slice of the embedding table: the used rows, first 8 numbers each, shape (7, 8). */
export const TABLE: Tensor = tensor(VOCAB.map((w) => [...GLOVE8[w]]), 'f32');
/** Number of lookups (one per token). */
export const LOOKUPS = TEXT_B * TEXT_T;

/** Row of the drawn table holding token id `id` (the table slice keeps the real ids, so this is a search). */
export function tableRow(id: number): number {
	const r = VOCAB.findIndex((w) => GLOVE_ROW[w] === id);
	if (r < 0) throw new Error(`data-as-tensors: token id ${id} is not in the drawn table`);
	return r;
}

/**
 * The gather X[b, t, :] = E[ids[b, t], :]: every output row is a view of one table row (core `slice`), read out in
 * order. Shape (B, T, D). This is what an embedding layer (torch.nn.Embedding, E[ids] in NumPy) computes.
 */
export const EMBEDDED: Tensor = tensor(
	Array.from({ length: TEXT_B }, (_, b) => Array.from({ length: TEXT_T }, (_, t) => flat(slice(TABLE, [tableRow(IDS.get([b, t]))])))),
	'f32',
);

/** Which lookup (b, t) is the k-th (batch-major, as a loop over the ids tensor reads them). */
export const lookupAt = (k: number): [number, number] => [Math.floor(k / TEXT_T), k % TEXT_T];

// ---------------------------------------------------------------------------------------------------------------
// 3.2-3 audio: samples -> frames.

/** Sample rate of the tone (Hz) and its pitch: the A above middle C, as a sound card would record it. */
export const RATE = 8000;
export const PITCH = 440;
export const SAMPLES = 48;
/** Frame lengths the card offers: every divisor of 48 from 2 to 24, so the samples fill whole frames. */
export const FRAMES = [2, 3, 4, 6, 8, 12, 16, 24] as const;
/**
 * The recording: 48 float32 samples of 0.8 · sin(2π · 440 · n / 8000), rounded to 3 decimals so the printed numbers are
 * the stored ones. 48 samples at 8 kHz last 6 ms.
 */
export const SIGNAL: Tensor = tensor(Array.from({ length: SAMPLES }, (_, n) => Math.round(800 * Math.sin((2 * Math.PI * PITCH * n) / RATE)) / 1000), 'f32');

/** The frame length nearest to `x` (ties go to the shorter frame). */
export const snapFrame = (x: number): number => FRAMES.reduce((best, f) => (Math.abs(f - x) < Math.abs(best - x) ? f : best), FRAMES[0]);

export interface AudioModel {
	fl: number;
	/** The frames tensor: `reshape` of the signal, a view (no copy). */
	t: Tensor;
	s: number;
	idx: [number, number];
	value: number;
}

export function audioModel(v: Vals): AudioModel {
	const fl = snapFrame(num(v.fl, 'fl'));
	const t = reshape(SIGNAL, [-1, fl]);
	const s = clamp(int(v.s, 's'), 0, SAMPLES - 1);
	const idx = unravel(s, t.shape) as [number, number];
	return { fl, t, s, idx, value: t.get(idx) };
}

// ---------------------------------------------------------------------------------------------------------------
// 3.2-4 mesh: vertex buffer + index buffer.

/** A square pyramid: four base corners on the floor and the apex (x, y, z per row, y up). Shape (5, 3), float32. */
export const VERTS: Tensor = tensor([[-1, 0, -1], [1, 0, -1], [1, 0, 1], [-1, 0, 1], [0, 1.5, 0]], 'f32');
/**
 * Triangles as rows of three vertex indices (int32 in a real index buffer), counter-clockwise seen from outside: four
 * sides around the apex, then the base as two triangles. Shape (6, 3).
 */
export const FACES: Tensor = tensor([[1, 0, 4], [2, 1, 4], [3, 2, 4], [0, 3, 4], [0, 1, 2], [0, 2, 3]], 'f64');
export const N_FACES = FACES.shape[0];

/** The corners of triangle f: V[F[f]], shape (3, 3), each row a view of one vertex row read out (a gather). */
export function corners(f: number): number[][] {
	return flat(slice(FACES, [f])).map((i) => flat(slice(VERTS, [i])));
}

/** Every edge of the mesh once, as vertex index pairs (lower index first). */
export function edges(): [number, number][] {
	const seen = new Set<string>(), out: [number, number][] = [];
	for (let f = 0; f < N_FACES; f++) {
		const tri = flat(slice(FACES, [f]));
		for (let k = 0; k < 3; k++) {
			const a = Math.min(tri[k], tri[(k + 1) % 3]), b = Math.max(tri[k], tri[(k + 1) % 3]);
			if (!seen.has(a + ',' + b)) {
				seen.add(a + ',' + b);
				out.push([a, b]);
			}
		}
	}
	return out;
}

// ---------------------------------------------------------------------------------------------------------------
// Shape reader: real shapes from pipelines.

export type Dtype = 'uint8' | 'int32' | 'int64' | 'float32' | 'bfloat16';
/** Bytes per element (NumPy/PyTorch itemsize). */
export const ITEMSIZE: Readonly<Record<Dtype, number>> = { uint8: 1, int32: 4, int64: 8, float32: 4, bfloat16: 2 };

export type AxisId = 'B' | 'H' | 'W' | 'C' | 'T' | 'D' | 'S' | 'N' | 'xyz';
export type PresetId = 'imgLast' | 'imgFirst' | 'ids' | 'hidden' | 'audio' | 'verts';
export interface Preset {
	id: PresetId;
	shape: number[];
	axes: AxisId[];
	dtype: Dtype;
}

/**
 * Typical tensors of real pipelines: an image batch as a TensorFlow/Keras input (channels last, raw bytes) and as a
 * PyTorch input (channels first, float32), a text batch of token ids and its hidden states in a 4096-wide model in
 * bfloat16, one second of 16 kHz mono audio, and the vertex buffer of a 10 000-point mesh.
 */
export const PRESETS: readonly Preset[] = [
	{ id: 'imgLast', shape: [32, 224, 224, 3], axes: ['B', 'H', 'W', 'C'], dtype: 'uint8' },
	{ id: 'imgFirst', shape: [32, 3, 224, 224], axes: ['B', 'C', 'H', 'W'], dtype: 'float32' },
	{ id: 'ids', shape: [8, 2048], axes: ['B', 'T'], dtype: 'int64' },
	{ id: 'hidden', shape: [8, 2048, 4096], axes: ['B', 'T', 'D'], dtype: 'bfloat16' },
	{ id: 'audio', shape: [16000], axes: ['S'], dtype: 'float32' },
	{ id: 'verts', shape: [10000, 3], axes: ['N', 'xyz'], dtype: 'float32' },
];

/** Elements, bytes and strides (in elements) of a preset, from the core. */
export function presetFacts(p: Preset): { n: number; bytes: number; strides: number[] } {
	const n = sizeOf(p.shape);
	return { n, bytes: n * ITEMSIZE[p.dtype], strides: cStrides(p.shape) };
}

/** "4 816 896" (thin-space-free: a plain space groups thousands in both languages). */
export const fmtBig = (x: number): string => String(Math.round(x)).replace(/\B(?=(\d{3})+(?!\d))/g, ' ');
/** Bytes as KiB/MiB/GiB with one decimal (binary units, as memory is counted). */
export function fmtBytes(b: number): { v: string; unit: 'B' | 'KiB' | 'MiB' | 'GiB' } {
	const units = ['B', 'KiB', 'MiB', 'GiB'] as const;
	let k = 0, x = b;
	while (x >= 1024 && k < units.length - 1) {
		x /= 1024;
		k++;
	}
	return { v: k === 0 ? String(x) : x.toFixed(1), unit: units[k] };
}

// ---------------------------------------------------------------------------------------------------------------
// The store and its link groups.

export const INITIAL: Record<string, Val> = {
	chw: 0, peel: 0.6, h: 0, w: 6, c: 0,
	look: 1,
	fl: 8, s: 0,
	face: 0,
	preset: 0,
};

/** A link group whose `fix` rounds and clamps every key of the card from the written and the current values. */
function group(keys: readonly string[], canonical: string, fix: (get: (k: string) => number) => Record<string, Val>): LinkGroup {
	return {
		keys,
		canonical,
		resolve(input, current) {
			const get = (k: string) => num(Object.hasOwn(input, k) ? input[k] : current[k], k);
			return fix(get);
		},
		shown: () => ({}),
	};
}

export const LINKS: LessonLinks = {
	groups: [
		group(['chw', 'peel', 'h', 'w', 'c'], 'peel', (get) => ({
			chw: Math.round(get('chw')) === 1 ? 1 : 0,
			peel: clamp(get('peel'), 0, 1),
			h: clamp(Math.round(get('h')), 0, IMG_H - 1),
			w: clamp(Math.round(get('w')), 0, IMG_W - 1),
			c: clamp(Math.round(get('c')), 0, IMG_C - 1),
		})),
		// `look` may ease: the scene fades the next output row in with the fraction.
		group(['look'], 'look', (get) => ({ look: clamp(Math.round(get('look')), 0, LOOKUPS) })),
		group(['fl', 's'], 's', (get) => ({ fl: snapFrame(get('fl')), s: clamp(Math.round(get('s')), 0, SAMPLES - 1) })),
		group(['face'], 'face', (get) => ({ face: clamp(Math.round(get('face')), 0, N_FACES - 1) })),
		group(['preset'], 'preset', (get) => ({ preset: clamp(Math.round(get('preset')), 0, PRESETS.length - 1) })),
	],
};

// ---------------------------------------------------------------------------------------------------------------
// Formatting and challenges.

export const fmtInt = (x: number): string => String(Math.round(x));
/** A float32 sample or embedding number as printed (3 decimals, no "-0.000"). */
export const fmt3 = (x: number): string => (Math.abs(x) < 5e-4 ? 0 : x).toFixed(3);

export const CHECKS = {
	/** Blue channel of the top-right pixel (row 0, column 7: the sun). */
	blueSun: (v: Vals) => {
		const m = imageModel(v);
		return m.h === 0 && m.w === 7 && m.c === 2;
	},
	/** Channels first, the byte at address 64 selected. */
	chw64: (v: Vals) => {
		const m = imageModel(v);
		return m.chw && m.addr === 64;
	},
	/** Exactly the lookups up to the fourth read of table row 1 ("the"). */
	fourthThe: (v: Vals) => int(v.look, 'look') === 11,
	/** A frame length that gives exactly 4 frames. */
	fourFrames: (v: Vals) => audioModel(v).t.shape[0] === 4,
	/** A base triangle (it does not use the apex, vertex 4). */
	baseFace: (v: Vals) => !flat(slice(FACES, [int(v.face, 'face')])).includes(4),
};

/** The image's two layouts are the same bytes: every (h, w, c) reads the same value through both (a test hook). */
export const sameBytes = (h: number, w: number, c: number): boolean => IMG_HWC.data[imgAddr(false, h, w, c)] === IMG_CHW.data[imgAddr(true, h, w, c)];
/** The frames tensor shares the signal's buffer (reshape is a view). */
export const framesShare = (fl: number): boolean => {
	const t = reshape(SIGNAL, [-1, fl]);
	return t.data === SIGNAL.data && isContiguous(t);
};
