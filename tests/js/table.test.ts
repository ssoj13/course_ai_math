import { expect, test } from 'bun:test';
import { rowsFor } from '../../src/viz/table';
test('product rows and total for the dot product', () => {
 const r = rowsFor('product', [1, 2], [3, 4]);
 expect(r.cells).toEqual([{ t: 'p0', v: 3 }, { t: 'p1', v: 8 }]);
 expect(r.total).toBe(11);
});
test('length mismatch is an error', () => {
 expect(() => rowsFor('product', [1, 2], [3])).toThrow();
});
test('components, squares and differences preserve sign and sum', () => {
 expect(rowsFor('components', [-2, 0]).cells).toEqual([{t:'a0',v:-2},{t:'a1',v:0}]);
 expect(rowsFor('square', [-2,3]).total).toBe(13);
 expect(rowsFor('diff', [1,-2],[4,1]).cells).toEqual([{t:'d0',v:-3},{t:'d1',v:-3}]);
 expect(() => rowsFor('square',[NaN])).toThrow();
 expect(() => rowsFor('diff',[1])).toThrow();
});
