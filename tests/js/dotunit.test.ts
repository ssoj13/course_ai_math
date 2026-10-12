// DotUnit procedural layout, numbers and choreography (docs/design/intro-and-dotunit.md §1.2, §1.4, §2, §7).
import { describe, expect, test } from 'bun:test';
import { mulberry32, planEinsum, ShapeError, tensor } from '../../src/core';
import { assignAxes, FACE, GROUP_GAP, layoutEinsum, lodOf, PITCH, type Place } from '../../src/viz/dotunit/layout';
import { columnView, dotData, operands, PRESETS, sceneData, type PresetId } from '../../src/viz/dotunit/model';
import { DOT_BAR, dotCap, dotCurrent, dotFrame, dotGeom, dotTops, Frame, passCap, passFrame } from '../../src/viz/dotunit/choreo';

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
		// The shaft keeps the plain pitch (stack room is only needed between rows of a kept stack axis).
		expect(L.shaftAt[1] - L.shaftAt[0]).toBeCloseTo(-PITCH, 12);
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
		// Two operand faces and the heat squares, plus the sweeping plane.
		expect(passCap(d)).toBe(3 * 4096 + 1);
		expect(d.out[0]).toBe(-64);
	});
});

describe('machine layer (spec §1.3) and LOD furniture (§1.4)', () => {
	test('fp32 changes the printed sum, never the geometry; the error is exact minus shown', () => {
		const a = [0.1, 0.2, 0.3, 0.7], b = [0.3, 0.7, 0.1, 0.9];
		const d64 = dotData(a, b), d32 = dotData(a, b, { arith: 'fp32' });
		expect(d32.run).toEqual(d64.run);
		expect(d32.acc.value).toBe(Math.fround(d32.acc.value));
		expect(d32.acc.err).toBe(d64.dot - d32.acc.value);
		expect(d32.acc.err).not.toBe(0);
		expect(d64.acc.err).toBe(0);
	});

	test('pairwise order: n − 1 merges, pairs first; tiles drop two at a time', () => {
		const d = dotData([1, 2, 3, 4, 5], [1, 1, 1, 1, 1], { order: 'pairwise' });
		expect(d.acc.merges.length).toBe(4);
		expect(d.acc.merges.slice(0, 2).map((m) => m.value)).toEqual([3, 7]);
		expect(d.acc.value).toBe(15);
		const g = dotGeom(d), f = new Frame(dotCap(5)), rest = new Frame(dotCap(5));
		dotFrame(g, 6, 1, rest, true);
		dotFrame(g, 7, 0.3, f, true);
		const moved = (i: number) => f.c[3 * (10 + i)] !== rest.c[3 * (10 + i)];
		// Tiles 0 and 1 drop together; tiles 2 and 3 have not started.
		expect([moved(0), moved(1), moved(2), moved(3)]).toEqual([true, true, false, false]);
		expect(dotCurrent(g, 7, 0.3, true)).toBe(0);
		expect(dotCurrent(g, 7, 0.5, true)).toBe(2);
		expect(dotCurrent(g, 9, 1, true)).toBe(-1);
	});

	test('the column passing pair i makes i the current tile', () => {
		const g = dotGeom(dotData([3, 1, 2], [1, 2, -1]));
		const seen = new Set<number>();
		for (let t = 0; t <= 1; t += 0.01) seen.add(dotCurrent(g, 5, t));
		expect([...seen].filter((x) => x >= 0).sort()).toEqual([0, 1, 2]);
	});

	test('LOD 2 draws a shaft line per cell while the shafts pass; LOD 3 sweeps one plane', () => {
		const p2 = planEinsum('ij,jk->ik', { i: 4, j: 16, k: 4 });
		const d2 = sceneData(p2, operands(PRESETS.matmul, p2, 3));
		expect(d2.L.lod).toBe(2);
		const f2 = new Frame(passCap(d2));
		passFrame(d2, 2, 1, 0, f2);
		const ops = d2.L.operands[0].n + d2.L.operands[1].n, lines = ops + d2.L.cells.n;
		for (let c = 0; c < d2.L.cells.n; c++) expect(f2.alpha[lines + c]).toBe(1);
		passFrame(d2, 4, 1, 0, f2);
		for (let c = 0; c < d2.L.cells.n; c++) expect(f2.alpha[lines + c]).toBe(0);
		const p3 = planEinsum('ij,jk->ik', { i: 40, j: 40, k: 40 });
		const d3 = sceneData(p3, operands(PRESETS.matmul, p3, 3));
		const f3 = new Frame(passCap(d3)), plane = passCap(d3) - 1;
		passFrame(d3, 1, 0, 0, f3);
		const z0 = f3.c[3 * plane + 2];
		passFrame(d3, 1, 1, 0, f3);
		expect(f3.c[3 * plane + 2]).toBeGreaterThan(z0);
		expect(f3.alpha[plane]).toBe(1);
	});
});
