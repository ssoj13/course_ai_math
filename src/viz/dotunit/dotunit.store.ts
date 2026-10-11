/**
 * Store of the DotUnit (lesson id `dotunit`, spec §2.1): the two vectors a, b of length n (2..8, controller ruling:
 * editable, default 3) and their 3D views a3, b3 (the first three components, the arrows of face 1). One link group per
 * vector keeps the two views of a vector and `n` consistent on every write (a drag of an arrow tip writes a3; a typed
 * component writes the full vector; a new n resizes both vectors, keeping the components that stay). Three-free.
 */
import { rand } from '../../core';
import type { LinkGroup, LessonLinks, Values } from '../links';
import type { Val } from '../store';
import { A_DEF, B_DEF, dotData, type DotData } from './model';

export const LESSON = 'dotunit';
export const N_MIN = 2;
export const N_MAX = 8;
/** Component range of a and b (spec §2.1: ±4, step 0.1). */
export const COMP = { min: -4, max: 4, step: 0.1 } as const;

/** Components beyond the defaults (n > 3): fixed seeded integers, so raising n always shows the same new numbers. */
const EXTRA_A = Array.from(rand([N_MAX], { seed: 11, range: [-3, 3], step: 1 }).data);
const EXTRA_B = Array.from(rand([N_MAX], { seed: 12, range: [-3, 3], step: 1 }).data);
const fill = (base: readonly number[], extra: readonly number[]) => Array.from({ length: N_MAX }, (_, i) => base[i] ?? extra[i]);
const DEF_A = fill(A_DEF, EXTRA_A), DEF_B = fill(B_DEF, EXTRA_B);

const clampC = (x: number) => Math.round(Math.min(COMP.max, Math.max(COMP.min, x)) * 10) / 10 + 0;
const intN = (x: number) => Math.min(N_MAX, Math.max(N_MIN, Math.round(x)));
/** The arrow of face 1: the first three components (a 2-vector lies in the plane z = 0). */
export const pad3 = (v: readonly number[]): number[] => [v[0] ?? 0, v[1] ?? 0, v[2] ?? 0];

/** a (or b) with its 3D view and n kept consistent; `def` fills components a larger n brings in. */
function vecLink(full: string, three: string, def: readonly number[]): LinkGroup {
	return {
		keys: [full, three, 'n'],
		canonical: full,
		resolve(input: Readonly<Record<string, Val>>, cur: Values) {
			const n = intN((input.n ?? cur.n ?? 3) as number);
			const v = [...((input[full] ?? cur[full] ?? def) as readonly number[])];
			if (three in input && !(full in input)) {
				const t3 = input[three] as readonly number[];
				for (let i = 0; i < Math.min(3, n); i++) v[i] = t3[i];
			}
			const out = Array.from({ length: n }, (_, i) => clampC(v[i] ?? def[i]));
			return { [full]: out, [three]: pad3(out), n };
		},
		shown: (v: Values) => ({ [three]: pad3(v[full] as readonly number[]) }),
	};
}

export const LINKS: LessonLinks = { groups: [vecLink('du_a', 'du_a3', DEF_A), vecLink('du_b', 'du_b3', DEF_B)] };
export const INITIAL: Record<string, Val> = { n: 3, du_a: [...A_DEF], du_b: [...B_DEF] };

/** The numbers of the current values (cached per pair of vector objects: rows, formula and scene ask often). */
let memo: { a: unknown; b: unknown; d: DotData } | null = null;
export function dotOf(v: Values): DotData {
	const a = v.du_a as readonly number[], b = v.du_b as readonly number[];
	if (memo && memo.a === a && memo.b === b) return memo.d;
	const d = dotData(a, b);
	memo = { a, b, d };
	return d;
}
