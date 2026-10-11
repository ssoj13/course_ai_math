import type { V } from './geom';
export type TableOp = 'components' | 'product' | 'square' | 'diff';
export function rowsFor(op: TableOp, a: V, b?: V): { cells: { t: string; v: number }[]; total?: number } {
 if (!a.every(Number.isFinite) || (b && !b.every(Number.isFinite))) throw new Error('table values must be finite');
 if ((op === 'product' || op === 'diff') && (!b || b.length !== a.length)) throw new Error('table vector lengths must match');
 const prefix = { components: 'a', product: 'p', square: 'sq', diff: 'd' }[op];
 const cells = a.map((v,i) => ({t: prefix+i, v: op === 'product' ? v*b![i] : op === 'diff' ? v-b![i] : op === 'square' ? v*v : v}));
 return { cells, ...(op === 'product' || op === 'square' ? {total: cells.reduce((s,c)=>s+c.v,0)} : {}) };
}
