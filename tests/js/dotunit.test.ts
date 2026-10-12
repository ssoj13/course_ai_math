// DotUnit procedural layout, numbers and choreography (docs/design/intro-and-dotunit.md §1.2, §1.4, §2, §7).
import { describe, expect, test } from 'bun:test';
import { mulberry32, planEinsum, ShapeError, tensor } from '../../src/core';
import { assignAxes, FACE, GROUP_GAP, layoutEinsum, lodOf, PITCH, type Place } from '../../src/viz/dotunit/layout';
import { columnView, dotData, operands, PRESETS, sceneData, type PresetId } from '../../src/viz/dotunit/model';
import { DOT_BAR, dotCap, dotFrame, dotGeom, dotTops, Frame, passCap, passFrame } from '../../src/viz/dotunit/choreo';

const plan = (id: PresetId) => planEinsum(PRESETS[id].op, PRESETS[id].shapes);

describe('index-to-axis assignment (spec §1.2 table)', () => {
	test('the five lesson presets and the layer', () => {
		const rows: [PresetId, Record<string, Place>, number][] = [
			['dot', { i: 'shaft' }, 0],
			['matvec', { i: 'y', j: 'shaft' }, 0],
			['matmul', { i: 'y', k: 'x', j: 'shaft' }, 2],
			['batched', { b: 'stack', i: 'y', k: 'x', j: 'shaft' }, 2],
			['attention', { b: 'stack', h: 'row', q: 'y', k: 'x', d: 'shaft' }, 2],
			['layer', { t: 'y', o: 'x', d: 'shaft' }, 2],
		];
		for (const [id, place, shaft] of rows) {
			const ax = assignAxes(plan(id));
			expect(ax.place).toEqual(place);
			expect(ax.shaft).toBe(shaft as 0 | 1 | 2);
		}
	});

	test('stacks grow along the shaft-dependent direction (rule 5)', () => {
		expect(assignAxes(plan('dot')).stackDir).toBe(1);
		expect(assignAxes(plan('matvec')).stackDir).toBe(1);
		expect(assignAxes(plan('matmul')).stackDir).toBe(2);
	});

	test('conflict rule: the first output letter keeps the axis', () => {
		const ax = assignAxes(planEinsum('ij,kj->ik', { i: 2, j: 3, k: 2 }));
		expect(ax.place.i).toBe('y');
		expect(ax.place.k).toBe('x');
	});

	test('two summed letters flatten into one shaft with a group gap', () => {
		const L = layoutEinsum(planEinsum('ij,ij->', { i: 2, j: 3 }));
		expect(L.plan.sumLen).toBe(6);
		expect(L.shaftAt.length).toBe(6);
		expect(L.shaftAt[3] - L.shaftAt[2]).toBeCloseTo(PITCH + GROUP_GAP, 12);
		expect(L.shaftAt[2] - L.shaftAt[1]).toBeCloseTo(PITCH, 12);
	});

	test('a third batch letter is a slice selector; a bad slice throws', () => {
		const p = planEinsum('abcij,abcjk->abcik', { a: 2, b: 2, c: 4, i: 2, j: 2, k: 2 });
		const L = layoutEinsum(p, { slices: { c: 3 } });
		expect(L.axes.place).toMatchObject({ a: 'stack', b: 'row', c: 'slice' });
		expect(L.cells.n).toBe(2 * 2 * 2 * 2);
		for (let c = 0; c < L.cells.n; c++) expect(L.cells.idx[c * 5 + 2]).toBe(3);
		expect(() => layoutEinsum(p, { slices: { c: 4 } })).toThrow(ShapeError);
		expect(() => layoutEinsum(p, { slices: { a: 0 } })).toThrow(ShapeError);
	});

	test('an outer product has a shaft of length 1', () => {
		const L = layoutEinsum(planEinsum('i,j->ij', { i: 2, j: 3 }));
		expect(L.plan.sumLen).toBe(1);
		expect(L.cells.n).toBe(6);
	});
});

describe('LOD and the cap (spec §1.4)', () => {
	test('boundaries at 8/32/64 per axis and 512/4096/20 000 cells', () => {
		expect(lodOf(8, 512)).toBe(1);
		expect(lodOf(9, 10)).toBe(2);
		expect(lodOf(8, 513)).toBe(2);
		expect(lodOf(32, 4096)).toBe(2);
		expect(lodOf(33, 10)).toBe(3);
		expect(lodOf(64, 20000)).toBe(3);
		expect(() => lodOf(65, 1)).toThrow(ShapeError);
		expect(() => lodOf(64, 20001)).toThrow(ShapeError);
	});

	test('an extent above 64 throws a typed error', () => {
		expect(() => layoutEinsum(planEinsum('ij,jk->ik', { i: 65, j: 2, k: 2 }))).toThrow(ShapeError);
	});

	test('the 64 x 64 heatmap preset is LOD 3', () => {
		expect(layoutEinsum(planEinsum('ij,jk->ik', { i: 64, j: 64, k: 64 })).lod).toBe(3);
	});
});

describe('placement', () => {
	test('matmul: cells on the front face, A on the left, B on top', () => {
		const L = layoutEinsum(plan('matmul'));
		expect(L.cells.n).toBe(4);
		expect([...L.cells.pos.subarray(0, 3)]).toEqual([0, 0, FACE]);
		expect([...L.cells.pos.subarray(9, 12)]).toEqual([PITCH, -PITCH, FACE]);
		expect([L.operands[0].normal, L.operands[0].side]).toEqual([0, -1]);
		expect([L.operands[1].normal, L.operands[1].side]).toEqual([1, 1]);
		expect(L.operands[0].pos[0]).toBeLessThan(-0.5);
		expect(L.operands[1].pos[1]).toBeGreaterThan(0.5);
	});

	test('matrix x vector: W under its tiles, x above the rows, outputs in a vertical column', () => {
		const L = layoutEinsum(plan('matvec'));
		expect([L.operands[0].normal, L.operands[0].side]).toEqual([2, 0]);
		expect([L.operands[1].normal, L.operands[1].side]).toEqual([1, 1]);
		// Both output cells share x (right of the shaft); rows are spaced to leave room for vertical stacks.
		expect(L.cells.pos[0]).toBe(L.cells.pos[3]);
		expect(L.cells.pos[1] - L.cells.pos[4]).toBeCloseTo(L.pitch[1], 12);
		expect(L.pitch[1]).toBeGreaterThan(2 * L.room);
	});

	test('dot: a below, b above the shaft', () => {
		const L = layoutEinsum(plan('dot'));
		expect([L.operands[0].side, L.operands[1].side]).toEqual([-1, 1]);
		expect(L.operands[0].pos[1]).toBeLessThan(0);
		expect(L.operands[1].pos[1]).toBeGreaterThan(0);
	});

	test('batched: index 0 of the stack is the bottom block', () => {
		const L = layoutEinsum(plan('batched'));
		expect(L.cells.n).toBe(8);
		// cell (b=1, i=0, k=0) is above cell (b=0, i=0, k=0).
		expect(L.cells.pos[4 * 3 + 1]).toBeGreaterThan(L.cells.pos[1]);
	});

	test('attention: heads are a row of stacks along x', () => {
		const L = layoutEinsum(plan('attention'));
		expect(L.cells.n).toBe(12);
		// cell (h=1, q=0, k=0) is right of every head-0 cell.
		const h0 = Math.max(...Array.from({ length: 6 }, (_, c) => L.cells.pos[3 * c]));
		expect(L.cells.pos[6 * 3]).toBeGreaterThan(h0);
	});
});

describe('numbers come from the core', () => {
	test('the presets reproduce the documented results', () => {
		const out = (id: PresetId) => {
			const p = plan(id);
			return Array.from(sceneData(p, operands(PRESETS[id], p, 0)).out);
		};
		expect(out('matvec')).toEqual([3, -1]);
		expect(out('matmul')).toEqual([3, 5, -1, 4]);
		expect(out('batched')).toEqual([3, 5, -1, 4, -3, -5, 1, -4]);
		expect(out('attention').slice(0, 6)).toEqual([5, 1, 6, 1, 0, 2]);
		expect(out('layer')).toEqual([3, -1, 3, 2]);
	});

	test('tiles of every cell sum to the cell (running sums end on the result)', () => {
		const p = plan('matmul');
		const d = sceneData(p, operands(PRESETS.matmul, p, 0));
		for (let c = 0; c < d.L.cells.n; c++) expect(d.run![c * 3 + 2]).toBe(d.out[c]);
	});

	test('seeded values: other extents or a seed draw integers in [-3, 3]', () => {
		const p = planEinsum('ij,jk->ik', { i: 3, j: 3, k: 2 });
		const [a] = operands(PRESETS.matmul, p, 0);
		expect(a.shape).toEqual([3, 3]);
		for (const x of a.data) expect(Number.isInteger(x) && x >= -3 && x <= 3).toBe(true);
	});

	test('row view and column view of matrix x vector give identical numbers', () => {
		for (let seed = 0; seed < 20; seed++) {
			const p = planEinsum('ij,j->i', { i: 2, j: 1 + (seed % 6) });
			const ts = operands(PRESETS.matvec, p, seed);
			const rows = sceneData(p, ts);
			const cols = columnView(p, ts);
			expect(cols.y).toEqual(Array.from(rows.out));
			expect(cols.heads[cols.heads.length - 1]).toEqual(Array.from(rows.out));
			// Column j scaled by x_j holds the same floats as the row view's tiles (cell i, shaft position j).
			cols.cols.forEach((col, j) => col.forEach((v, i) => expect(v).toBe(rows.prods![i * p.sumLen + j])));
		}
	});
});

describe('DotUnit (spec §2, §7)', () => {
	test('defaults: a·b = 3, |a| = √14, shadow 0.80, |a|·shadow = 3', () => {
		const d = dotData([3, 1, 2], [1, 2, -1]);
		expect(d.prods).toEqual([3, 2, -2]);
		expect(d.run).toEqual([3, 5, 3]);
		expect(d.dot).toBe(3);
		expect(d.lenA).toBeCloseTo(Math.sqrt(14), 14);
		expect(d.shadow).toBeCloseTo(0.8018, 4);
		expect(d.proj).toBeCloseTo(3, 12);
	});

	test('stack height equals the projection value for 200 random a, b', () => {
		const r = mulberry32(42);
		const q = () => Math.round((r() * 8 - 4) * 10) / 10;
		for (let k = 0; k < 200; k++) {
			const n = 2 + (k % 7);
			const a: number[] = Array.from({ length: n }, q), b: number[] = Array.from({ length: n }, q);
			if (!a.some((x) => x !== 0)) a[0] = 1;
			const d = dotData(a, b);
			const g = dotGeom(d);
			const f = new Frame(dotCap(n));
			dotFrame(g, 9, 1, f);
			const tops = dotTops(g);
			expect(Math.abs(tops.stack - tops.bar)).toBeLessThan(1e-9);
			// The drawn bar reaches the projection value and the last segment's far edge is the running sum.
			const bar = DOT_BAR(n);
			expect(Math.abs(f.c[3 * bar + 1] + Math.sign(d.proj) * f.s[3 * bar + 1] / 2 - d.proj)).toBeLessThan(1e-5);
			const last = 2 * n + n - 1;
			const edge = f.c[3 * last + 1] + (d.prods[n - 1] >= 0 ? 1 : -1) * f.s[3 * last + 1] / 2;
			if (Math.abs(d.prods[n - 1]) > 0.02) expect(Math.abs(edge - d.run[n - 1])).toBeLessThan(1e-5);
			expect(d.dot).toBe(d.run[n - 1]);
		}
	});

	test('tile area = |aᵢbᵢ| and hatch iff aᵢbᵢ < 0 (after the hatch wipe)', () => {
		const a = [3, 1, 2, -1.5], b = [1, 2, -1, 2];
		const d = dotData(a, b), g = dotGeom(d), f = new Frame(dotCap(4));
		dotFrame(g, 6, 1, f);
		for (let i = 0; i < 4; i++) {
			const k = 8 + i;
			expect(f.s[3 * k] * f.s[3 * k + 1]).toBeCloseTo(Math.abs(d.prods[i]), 5);
			expect(f.hatch[k]).toBe(d.prods[i] < 0 ? 1 : 0);
		}
	});

	test('the column passes the row during step 5: tiles appear in order', () => {
		const d = dotData([3, 1, 2], [1, 2, -1]), g = dotGeom(d), f = new Frame(dotCap(3));
		dotFrame(g, 5, 0, f);
		expect([f.alpha[6], f.alpha[7], f.alpha[8]]).toEqual([0, 0, 0]);
		dotFrame(g, 5, 0.5, f);
		expect(f.alpha[6]).toBe(1);
		expect(f.alpha[8]).toBe(0);
		dotFrame(g, 5, 1, f);
		expect([f.alpha[6], f.alpha[7], f.alpha[8]]).toEqual([1, 1, 1]);
	});
});

describe('pass frames', () => {
	test('every preset fits its capacity; tiles fall onto the stacks at the result step', () => {
		for (const id of ['matvec', 'matmul', 'batched', 'attention', 'layer'] as PresetId[]) {
			const p = plan(id);
			const d = sceneData(p, operands(PRESETS[id], p, 0));
			const f = new Frame(passCap(d));
			for (let s = 0; s < 5; s++) passFrame(d, s, 1, 0, f);
			expect(f.n).toBe(passCap(d));
			for (let i = 0; i < f.n; i++) expect(f.alpha[i]).toBeGreaterThan(0);
		}
	});

	test('the heatmap preset draws one square per cell', () => {
		const p = planEinsum('ij,jk->ik', { i: 64, j: 64, k: 64 });
		const d = sceneData(p, [tensor(Array.from({ length: 64 }, () => Array(64).fill(1))), tensor(Array.from({ length: 64 }, () => Array(64).fill(-1)))]);
		expect(d.L.lod).toBe(3);
		expect(passCap(d)).toBe(3 * 4096);
		expect(d.out[0]).toBe(-64);
	});
});
