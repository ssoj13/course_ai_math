/**
 * Lesson 3.6 (batched matmul and einsum: contraction over a shared index): the store contract and every number,
 * string and diagram position the scenes, readouts and tests use. Pure, three-free and DOM-free.
 *
 * The lesson is ONE einsum the reader edits in three ways that stay in sync: typing the op string, picking a preset,
 * or plugging cables in the S4 socket diagram. Everything is computed by the tensor core (`parseEinsum`, `planEinsum`,
 * `einsum`, typed `SpecError` / `ExtentError`) and laid out by the DotUnit renderer (`sceneData`, `layoutEinsum`),
 * so the light box, the readout and the loops show the same plan.
 *
 * Store `einsum-lab`:
 * - `op`: the op string as UTF-16 char codes (the store holds numbers only). It may be invalid: the scene and the
 *   readout then show the typed error instead of a picture.
 * - `x_<letter>` (a..z) 1..6: the extent of every index letter (one extent per letter: that equality IS the
 *   contraction rule, so a letter shared by both operands always has one length).
 * - `seed` 0..99: 0 = the course's numbers where the op is a DotUnit preset, else seeded integers in [-3, 3].
 * - `sel`, `dive`: the selected output cell and whether the card shows its dot product (the DotUnit face 2).
 * - `bad` 0/1: a cable between two axes of different lengths was refused (a challenge asks for it).
 */
import {
	CoreError, ExtentError, parseEinsum, planEinsum, rand, SpecError,
	type Parsed, type Plan, type SpecReason, type Tensor,
} from '../../core';
import { operands, PRESETS as DU, RANGE, sceneData, type PresetId, type SceneData } from '../dotunit/model';
import { tileAt } from '../dotunit/layout';
import type { LessonState, Val } from '../store';

export const LESSON = 'einsum-lab';
type Vals = LessonState['vals'];

/** Extent fields: small enough that every tile and number stays readable (LOD 1) for the lesson's presets. */
export const EXT = { min: 1, max: 6, step: 1 } as const;
export const SEED = { min: 0, max: 99, step: 1 } as const;
export const LETTERS = 'abcdefghijklmnopqrstuvwxyz';
export const extKey = (l: string) => 'x_' + l;

/** The op string <-> store value (char codes). */
export const opVal = (s: string): number[] => Array.from(s, (c) => c.charCodeAt(0));
export function opOf(v: Vals): string {
	const x = v.op;
	if (!Array.isArray(x)) throw new Error('einsum-lab: "op" must be an array of char codes');
	return String.fromCharCode(...x);
}

/** Presets the reader can pick. `du` = the DotUnit preset with the same op (its numbers are the course's). */
export interface LabPreset {
	id: string;
	op: string;
	shapes: Readonly<Record<string, number>>;
	du?: PresetId;
}
export const LAB_PRESETS: readonly LabPreset[] = [
	{ id: 'dot', op: 'i,i->', shapes: DU.dot.shapes, du: 'dot' },
	{ id: 'matvec', op: 'ij,j->i', shapes: DU.matvec.shapes, du: 'matvec' },
	{ id: 'matmul', op: 'ij,jk->ik', shapes: DU.matmul.shapes, du: 'matmul' },
	{ id: 'frob', op: 'ij,ij->', shapes: { i: 2, j: 3 } },
	{ id: 'abt', op: 'ij,kj->ik', shapes: { i: 2, j: 3, k: 2 } },
	{ id: 'outer', op: 'i,j->ij', shapes: { i: 3, j: 2 } },
	{ id: 'batched', op: 'bij,bjk->bik', shapes: DU.batched.shapes, du: 'batched' },
	{ id: 'attention', op: 'bhqd,bhkd->bhqk', shapes: DU.attention.shapes, du: 'attention' },
];
/** Specs that the core refuses, to see the typed errors. */
export const BROKEN: readonly string[] = ['ij,jk->iz', 'iij,jk->ik', 'ij,jk'];

export const INITIAL: Record<string, Val> = {
	op: opVal('ij,jk->ik'),
	seed: 0,
	sel: 0,
	dive: 0,
	bad: 0,
	...Object.fromEntries([...LETTERS].map((l) => [extKey(l), 2])),
	...Object.fromEntries(Object.entries(DU.matmul.shapes).map(([l, n]) => [extKey(l), n])),
};

/** Store patch that loads a preset (its op, its extents, the course's numbers, nothing selected). */
export function presetPatch(p: LabPreset): Record<string, Val> {
	return { op: opVal(p.op), seed: 0, sel: 0, dive: 0, ...Object.fromEntries(Object.entries(p.shapes).map(([l, n]) => [extKey(l), n])) };
}

const int = (v: Val | undefined): number => (typeof v === 'number' && Number.isFinite(v) ? Math.round(v) : NaN);
export const extOf = (v: Vals, l: string): number => Math.min(EXT.max, Math.max(EXT.min, int(v[extKey(l)])));

/** Distinct letters of a parsed op in order of first appearance (operands left to right). */
export const lettersOf = (p: Parsed): string[] => [...new Set(p.inputs.join(''))];

/** What happens to each letter: kept in both operands (batch), kept in one (free), summed in both (contracted), summed in one (reduced). */
export type Role = 'batch' | 'free' | 'contracted' | 'reduced';
export function roleOf(p: Parsed, l: string): Role {
	const both = p.inputs.length === 2 && p.inputs[0].includes(l) && p.inputs[1].includes(l);
	if (p.output.includes(l)) return both ? 'batch' : 'free';
	return both ? 'contracted' : 'reduced';
}

/**
 * Canonical form of an op: letters renamed a, b, c, ... in order of first appearance. Two ops with the same canonical
 * form are the same computation with other names (`bij,bjk->bik` = `abc,acd->abd`). Returns null for an invalid op.
 */
export function canon(op: string): string | null {
	try {
		const p = parseEinsum(op);
		const map = new Map(lettersOf(p).map((l, i) => [l, LETTERS[i]]));
		return p.inputs.map((t) => [...t].map((l) => map.get(l)).join('')).join(',') + '->' + [...p.output].map((l) => map.get(l)).join('');
	} catch (e) {
		if (e instanceof SpecError) return null;
		throw e;
	}
}

/** The two operand tensors: the DotUnit preset's numbers when the op and extents are a preset's and seed is 0. */
export function labOperands(plan: Plan, seed: number): [Tensor, Tensor] {
	const p = LAB_PRESETS.find((q) => q.op === plan.op && q.du);
	if (p?.du) return operands(DU[p.du], plan, seed);
	const shape = (o: number) => [...plan.inputs[o]].map((l) => plan.dims[l]);
	return [rand(shape(0), { seed, range: RANGE, step: 1 }), rand(shape(1), { seed: seed + 1, range: RANGE, step: 1 })];
}

/** A built lab state: the plan and the scene data, or the typed reason the core refused the op. */
export type Lab =
	| { ok: true; op: string; plan: Plan; d: SceneData }
	| { ok: false; op: string; error: CoreError };

let memo: { key: string; lab: Lab } | null = null;
/** The lab for these store values (memoised on the keys it reads: op, extents of its letters, seed). */
export function lab(v: Vals): Lab {
	const op = opOf(v);
	const seed = int(v.seed);
	const exts = [...new Set(op)].filter((c) => LETTERS.includes(c)).map((l) => l + extOf(v, l));
	const key = JSON.stringify([op, exts, seed]);
	if (memo?.key === key) return memo.lab;
	let out: Lab;
	try {
		const parsed = parseEinsum(op, 2);
		const plan = planEinsum(parsed, Object.fromEntries(lettersOf(parsed).map((l) => [l, extOf(v, l)])));
		out = { ok: true, op, plan, d: sceneData(plan, labOperands(plan, seed)) };
	} catch (e) {
		if (!(e instanceof CoreError)) throw e;
		out = { ok: false, op, error: e };
	}
	memo = { key, lab: out };
	return out;
}

// ---------------------------------------------------------------------------------------------------------------
// Reading an op: the loops it stands for.

/**
 * The op as plain Python loops (the reading of an einsum string: one loop per kept letter outside, one per summed
 * letter inside the accumulation). Operands are `A` and `B`, the result `C`.
 */
export function loops(plan: Plan): string {
	const ind = (n: number) => '    '.repeat(n);
	const idx = (letters: string) => (letters.length ? '[' + [...letters].join(', ') + ']' : '');
	const lines: string[] = [];
	plan.kept.forEach((l, d) => lines.push(`${ind(d)}for ${l} in range(${plan.dims[l]}):`));
	const k = plan.kept.length;
	lines.push(`${ind(k)}s = 0`);
	plan.summed.forEach((l, d) => lines.push(`${ind(k + d)}for ${l} in range(${plan.dims[l]}):`));
	const body = `s += A${idx(plan.inputs[0])} * B${idx(plan.inputs[1])}`;
	lines.push(ind(k + plan.summed.length) + body);
	lines.push(`${ind(k)}C${idx(plan.output)} = s`);
	return lines.join('\n');
}

/** FLOPs of the contraction: one multiply and one add per product, cells x shaft length of each. */
export const flops = (plan: Plan): number => 2 * plan.cells * plan.sumLen;

// ---------------------------------------------------------------------------------------------------------------
// S4: plugs and cables (pure edits of the op string; the core decides what is allowed).

/** A fresh letter not used in `op` (alphabet order, starting after the letters in use when possible). */
export function freshLetter(op: string): string {
	for (const l of LETTERS) if (!op.includes(l)) return l;
	throw new Error('einsum-lab: no free letter left');
}

/** Result of a diagram edit: the new op (and the extent of a new letter), or the typed core error that refused it. */
export type Edit = { ok: true; op: string; ext?: Readonly<Record<string, number>> } | { ok: false; error: CoreError };

/** Operand shapes of `p` under the per-letter extents. */
const shapesOf = (p: Parsed, ext: (l: string) => number) => p.inputs.map((t) => [...t].map(ext));

/**
 * Plugs socket `ia` of A into socket `ib` of B. Already the same letter = pull the cable out: B's socket gets a fresh
 * letter and both ends become kept output axes. Different letters = B's socket takes A's letter; the core checks the
 * result with the REAL operand shapes, so plugging axes of different lengths throws its `ExtentError`, and a letter
 * that B already has elsewhere throws `SpecError` (`repeatIn`). A new cable is a contraction (its letter leaves the
 * output); `keep` makes it a batch axis.
 */
export function plug(op: string, ia: number, ib: number, ext: (l: string) => number): Edit {
	try {
		const p = parseEinsum(op, 2);
		const [A, B] = p.inputs;
		const la = A[ia], lb = B[ib];
		if (la === undefined || lb === undefined) throw new Error(`einsum-lab: no socket ${ia} / ${ib} in "${op}"`);
		if (la === lb) {
			// Unplug: B's end gets a fresh letter of the same length; both ends become kept output axes.
			const f = freshLetter(op);
			const next = A + ',' + B.slice(0, ib) + f + B.slice(ib + 1) + '->' + p.output + (p.output.includes(la) ? '' : la) + f;
			parseEinsum(next, 2);
			return { ok: true, op: next, ext: { [f]: ext(lb) } };
		}
		const nb = B.slice(0, ib) + la + B.slice(ib + 1);
		// The plugged letter is contracted; B's old letter leaves the output only if no operand has it any more.
		const gone = !(A + nb).includes(lb);
		const out = [...p.output].filter((l) => l !== la && !(gone && l === lb)).join('');
		const next = A + ',' + nb + '->' + out;
		const q = parseEinsum(next, 2);
		// The cable needs one length on both sides: the core checks the REAL shapes (B's socket keeps its own length).
		planEinsum(q, shapesOf(p, ext));
		return { ok: true, op: next };
	} catch (e) {
		if (e instanceof CoreError) return { ok: false, error: e };
		throw e;
	}
}

/** Toggles letter `l` between kept (in the output) and summed. */
export function toggleKeep(op: string, l: string): Edit {
	try {
		const p = parseEinsum(op, 2);
		if (!p.inputs.join('').includes(l)) throw new Error(`einsum-lab: "${l}" is not in "${op}"`);
		const out = p.output.includes(l) ? p.output.replace(l, '') : p.output + l;
		const next = p.inputs.join(',') + '->' + out;
		parseEinsum(next, 2);
		return { ok: true, op: next };
	} catch (e) {
		if (e instanceof CoreError) return { ok: false, error: e };
		throw e;
	}
}

/** Diagram geometry in SVG user units (viewBox 0 0 W H): two operand blocks, the output block, sockets and cables. */
export const S4 = { W: 360, H: 230, blockY: 14, blockH: 44, outY: 172, outH: 44, sag: 112, gap: 30 } as const;

export interface Socket { letter: string; size: number; x: number; y: number }
export interface Cable {
	letter: string;
	role: Role;
	/** SVG path of the cable. */
	d: string;
	/** Where the Σ marker of a summed letter sits (contracted: the sag; reduced: under its socket). */
	mark: { x: number; y: number } | null;
}
export interface S4Layout {
	blocks: { name: 'A' | 'B' | 'C'; x: number; y: number; w: number; h: number; shape: number[] }[];
	sockets: { A: Socket[]; B: Socket[]; C: Socket[] };
	cables: Cable[];
}

/** Evenly spaced socket x positions across a block. */
const spread = (x: number, w: number, n: number) => Array.from({ length: n }, (_, i) => x + (w * (i + 1)) / (n + 1));

/** Lays out the socket diagram of a valid two-operand op with the given extents. */
export function s4Layout(p: Parsed, ext: (l: string) => number): S4Layout {
	if (p.inputs.length !== 2) throw new Error('einsum-lab: the socket diagram takes two operands');
	const { W, blockY, blockH, outY, outH, sag, gap } = S4;
	const bw = (W - 3 * gap) / 2;
	const out = [...p.output];
	const cw = Math.min(W - 2 * gap, Math.max(90, 46 * Math.max(1, out.length)));
	const blocks = [
		{ name: 'A' as const, x: gap, y: blockY, w: bw, h: blockH, shape: [...p.inputs[0]].map(ext) },
		{ name: 'B' as const, x: 2 * gap + bw, y: blockY, w: bw, h: blockH, shape: [...p.inputs[1]].map(ext) },
		{ name: 'C' as const, x: (W - cw) / 2, y: outY, w: cw, h: outH, shape: out.map(ext) },
	];
	const sock = (b: (typeof blocks)[number], letters: string, y: number): Socket[] => {
		const xs = spread(b.x, b.w, letters.length);
		return [...letters].map((l, i) => ({ letter: l, size: ext(l), x: xs[i], y }));
	};
	const A = sock(blocks[0], p.inputs[0], blockY + blockH);
	const B = sock(blocks[1], p.inputs[1], blockY + blockH);
	const C = sock(blocks[2], p.output, outY);
	const cables: Cable[] = [];
	const f = (n: number) => Math.round(n * 10) / 10;
	for (const l of lettersOf(p)) {
		const role = roleOf(p, l);
		const a = A.find((s) => s.letter === l), b = B.find((s) => s.letter === l), c = C.find((s) => s.letter === l);
		if (a && b) {
			const mx = (a.x + b.x) / 2;
			// One cable from A to B sagging between the blocks; a kept (batch) cable also drops to the output socket.
			let d = `M${f(a.x)} ${f(a.y)} C${f(a.x)} ${sag} ${f(b.x)} ${sag} ${f(b.x)} ${f(b.y)}`;
			const low = { x: mx, y: f(0.25 * a.y + 0.75 * sag) };
			if (c) d += ` M${f(mx)} ${low.y} L${f(c.x)} ${f(c.y)}`;
			cables.push({ letter: l, role, d, mark: role === 'contracted' ? low : null });
		} else {
			const s = (a ?? b)!;
			if (c) cables.push({ letter: l, role, d: `M${f(s.x)} ${f(s.y)} C${f(s.x)} ${f(s.y + 50)} ${f(c.x)} ${f(c.y - 50)} ${f(c.x)} ${f(c.y)}`, mark: null });
			else cables.push({ letter: l, role, d: `M${f(s.x)} ${f(s.y)} L${f(s.x)} ${f(s.y + 30)}`, mark: { x: s.x, y: s.y + 40 } });
		}
	}
	return { blocks, sockets: { A, B, C }, cables };
}

// ---------------------------------------------------------------------------------------------------------------
// Axis letters in the light box.

export interface AxisTag { letter: string; text: string; at: [number, number, number]; role: Role }

/**
 * Where the index letters of a valid lab are written in the 3D scene: a kept letter on a spatial axis beside the
 * output face of the first block (x below it, y right of it), the summed letters along the shaft of cell 0, and every
 * batch letter beside each of its blocks. All in world units of `layoutEinsum`.
 */
export function axisTags(d: SceneData): AxisTag[] {
	const L = d.L, plan = L.plan, rank = plan.kept.length, P = [0, 0, 0];
	// Output cells of the first block (every batch index 0).
	const batchAt = L.axes.batch.slice(0, 2).map((l) => plan.kept.indexOf(l));
	const first: number[] = [];
	for (let c = 0; c < L.cells.n; c++) if (batchAt.every((d0) => L.cells.idx[c * rank + d0] === 0)) first.push(c);
	const lo = [Infinity, Infinity, Infinity], hi = [-Infinity, -Infinity, -Infinity];
	const grow = (p: ArrayLike<number>) => {
		for (let a = 0; a < 3; a++) {
			lo[a] = Math.min(lo[a], p[a]);
			hi[a] = Math.max(hi[a], p[a]);
		}
	};
	for (const c of first) for (let k = 0; k < plan.sumLen; k++) grow(tileAt(L, c, k, P));
	const mid = (a: number) => (lo[a] + hi[a]) / 2;
	const out: AxisTag[] = [];
	const name = (l: string) => `${l} = ${plan.dims[l]}`;
	for (const l of plan.kept) {
		const pl = L.axes.place[l];
		const role = roleOf(plan, l);
		if (pl === 'x') out.push({ letter: l, text: name(l), at: [mid(0), lo[1] - 0.95, hi[2]], role });
		else if (pl === 'y') out.push({ letter: l, text: name(l), at: [hi[0] + 0.95, mid(1), hi[2]], role });
		else if (pl === 'z') out.push({ letter: l, text: name(l), at: [hi[0] + 0.95, lo[1] - 0.95, mid(2)], role });
	}
	if (plan.summed.length) {
		const s = L.axes.shaft;
		const at: [number, number, number] = [mid(0), mid(1), mid(2)];
		if (s === 2) {
			at[0] = hi[0] + 0.95;
			at[1] = lo[1] - 0.95;
		} else if (s === 0) at[1] = lo[1] - 0.95;
		else at[0] = lo[0] - 0.95;
		const text = plan.summed.map(name).join(', ') + ' (Σ)';
		out.push({ letter: plan.summed[0], text, at, role: roleOf(plan, plan.summed[0]) });
	}
	// Batch letters: "b = 0", "b = 1" beside each block of the stack / row (only the first two batch letters are drawn).
	for (const [bi, l] of L.axes.batch.slice(0, 2).entries()) {
		const d0 = plan.kept.indexOf(l);
		for (let i = 0; i < plan.dims[l]; i++) {
			const c = Array.from({ length: L.cells.n }, (_, k) => k).find((k) => L.cells.idx[k * rank + d0] === i && batchAt.every((dd, j) => j === bi || L.cells.idx[k * rank + dd] === 0));
			if (c === undefined) continue;
			const dy = L.cells.pos[3 * c + 1] - L.cells.pos[3 * first[0] + 1];
			const dx = L.cells.pos[3 * c] - L.cells.pos[3 * first[0]];
			out.push({ letter: l, text: `${l} = ${i}`, at: [lo[0] + dx - 0.2, hi[1] + dy + 0.95, hi[2]], role: 'batch' });
		}
	}
	return out;
}

/** The plain-text sum an op stands for, e.g. "C[i, k] = Σ_j A[i, j] · B[j, k]" (the same in every language). */
export function sumText(p: Parsed): string {
	const ix = (s: string) => (s.length ? '[' + [...s].join(', ') + ']' : '');
	const sum = p.summed.length ? 'Σ_' + p.summed.join('') + ' ' : '';
	return `C${ix(p.output)} = ${sum}A${ix(p.inputs[0])} · B${ix(p.inputs[1])}`;
}

/** Error details a UI can localise: the spec reason and token, or the letter and both lengths of a refused cable. */
export type ErrInfo =
	| { kind: 'spec'; reason: SpecReason; token: string; pos: number }
	| { kind: 'extent'; letter: string; a: number; b: number }
	| { kind: 'other' };
export function errInfo(e: CoreError): ErrInfo {
	if (e instanceof SpecError) return { kind: 'spec', reason: e.reason, token: e.token, pos: e.pos };
	if (e instanceof ExtentError) return { kind: 'extent', letter: e.letter, a: e.first, b: e.second };
	return { kind: 'other' };
}

/** Challenge checks (store values in, boolean out). */
export const CHECKS = {
	/** ⟨A, B⟩: two matrices multiplied cell by cell and summed to one number. */
	frob: (v: Vals) => canon(opOf(v)) === 'ab,ab->',
	/** A·Bᵀ without a transpose: rows of A against rows of B. */
	abt: (v: Vals) => canon(opOf(v)) === 'ab,cb->ac',
	/** A batched matmul (any letters). */
	batched: (v: Vals) => canon(opOf(v)) === 'abc,acd->abd',
	/** A refused cable between axes of different lengths. */
	mismatch: (v: Vals) => v.bad === 1,
};
