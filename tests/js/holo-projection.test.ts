import {test,expect} from 'bun:test';
import {screenDimension,worldPlaneBasis,dot,sub,norm} from '../../src/viz/geom';
test('screen dimension is 14px perpendicular under oblique perspective endpoints',()=>{
 for(const [a,b,out] of [[[21,40],[98,123],[12,60]],[[100,180],[400,162],[100,230]],[[41,82],[42,182],[80,82]]]){
 const p=screenDimension(a,b,out)!;
 expect(norm(sub(p.a,a))).toBeCloseTo(14,10);
 expect(dot(sub(p.a,a),sub(b,a))).toBeCloseTo(0,8);
 expect(norm(sub(p.b,b))).toBeCloseTo(14,10);
 }
 expect(screenDimension([1,1],[1,1],[2,2])).toBeNull();
});
test('world glass grid has absolute phase, independent of origin and skew',()=>{
 const square=worldPlaneBasis([3,0],[0,3]),skew=worldPlaneBasis([3,0],[1,3]);
 expect(square).toEqual(skew);
 const uv=(p:number[])=>square.map(axis=>dot(p,axis.slice(0,p.length)));
 expect(uv([.5,.5])).toEqual([.5,.5]);
 expect(uv([1,1])).toEqual([1,1]);
 const basis=worldPlaneBasis([2,1,0],[-1,2,3]);
 expect(norm(basis[0])).toBeCloseTo(1);expect(norm(basis[1])).toBeCloseTo(1);
 expect(dot(basis[0],basis[1])).toBeCloseTo(0);
 expect(basis).toEqual(worldPlaneBasis([4,2,0],[-2,4,6]));
});
