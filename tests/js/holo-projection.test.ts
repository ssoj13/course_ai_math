import {test,expect} from 'bun:test';
import {screenDimension,dot,sub,norm} from '../../src/viz/geom';
test('screen dimension is 14px perpendicular under oblique perspective endpoints',()=>{
 for(const [a,b,out] of [[[21,40],[98,123],[12,60]],[[100,180],[400,162],[100,230]],[[41,82],[42,182],[80,82]]]){
 const p=screenDimension(a,b,out)!;
 expect(norm(sub(p.a,a))).toBeCloseTo(14,10);
 expect(dot(sub(p.a,a),sub(b,a))).toBeCloseTo(0,8);
 expect(norm(sub(p.b,b))).toBeCloseTo(14,10);
 }
 expect(screenDimension([1,1],[1,1],[2,2])).toBeNull();
});
