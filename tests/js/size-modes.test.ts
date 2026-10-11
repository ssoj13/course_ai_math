import { expect, test } from 'bun:test';
import { NORMAL, reduceSize, viewVisible } from '../../src/viz/scene/sizeModes';
test('one mode owner across cards; toggles, Escape/system exit and sibling visibility', () => {
 const wide = reduceSize(NORMAL, { type:'toggle', id:'a', mode:'wide' });
 expect(wide).toEqual({ id:'a', mode:'wide' });
 expect(reduceSize(wide,{type:'toggle',id:'a',mode:'wide'})).toEqual(NORMAL);
 const full = reduceSize(wide,{type:'toggle',id:'b',mode:'full'});
 expect(full).toEqual({id:'b',mode:'full'});
 expect(viewVisible('a',full)).toBe(false);
 expect(viewVisible(undefined,full)).toBe(false);
 expect(viewVisible('b',full)).toBe(true);
 expect(viewVisible('b',wide)).toBe(true);
 expect(reduceSize(full,{type:'exit'})).toEqual(NORMAL);
});
