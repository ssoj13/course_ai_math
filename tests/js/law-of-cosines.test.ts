import { describe, expect, test } from 'bun:test';
import { INITIAL, derive, solveIK, pieceAt, moveA, moveB, cutElapsed, cutProgress, cutMoves } from '../../src/viz/lessons/law-of-cosines.math';
test('reader transport resumes each spring phase without jumping and exposes only changing placements',()=>{
 for(const phases of [1,3])for(const p of [0,.01,.25,.33,1/3,.5,.66,2/3,.75,.98,.99,.999,1]){
  expect(cutProgress(cutElapsed(p,phases),phases)).toBeCloseTo(p,10);
 }
 expect(cutMoves(0,1,1)).toBe(false);expect(cutMoves(1,2,2)).toBe(true);
 expect(cutMoves(4,5,5)).toBe(false);expect(cutMoves(6,7,7)).toBe(true);
 expect(cutMoves(7.5,6,6)).toBe(true);expect(cutMoves(9,10,10)).toBe(false);
 expect(cutProgress(10000,3)).toBe(1);
});
const area=(p:readonly (readonly number[])[])=>p.reduce((s,a,i)=>{const b=p[(i+1)%p.length];return s+a[0]*b[1]-a[1]*b[0]},0)/2;
const fixtures=[[3,4,60],[3,4,90],[3,4,120],[1,4,30],[4,1,30],[3,3,0],[3,4,180],[.5,4,1]];
describe('law of cosines signed construction',()=>{
 test('independent coordinates and shoelace preserve every shear and rigid turn',()=>{
  for(const [a,b,th] of fixtures){
   const v={...INITIAL,a,b,th},z=derive(v),c2=(b*Math.cos(th*Math.PI/180)-a)**2+(b*Math.sin(th*Math.PI/180))**2;
   expect(z.c2).toBeCloseTo(c2,11);expect(z.R1+z.R2).toBeCloseTo(c2,11);
   if(z.degenerate){expect(z.FB).toBe(0);continue}
   for(const piece of [1,2] as const)for(const base of [0,1,2])for(const t of [0,.25,.5,.75,1]){
    const phase=base+t,step=piece===1?5+phase:8+phase/3,p=pieceAt(v,piece,step);
    expect(area(p)).toBeCloseTo(piece===1?-a*(a-b*Math.cos(th*Math.PI/180)):b*(b-a*Math.cos(th*Math.PI/180)),10);
    if(base===1){const initial=pieceAt(v,piece,piece===1?6:8+1/3);
     p.forEach((q,i)=>expect(Math.hypot(q[0]-p[(i+1)%4][0],q[1]-p[(i+1)%4][1])).toBeCloseTo(Math.hypot(initial[i][0]-initial[(i+1)%4][0],initial[i][1]-initial[(i+1)%4][1]),10))}
   }
  }
 });
 test('exact right angle, debts and polar keyboard magnet escape',()=>{
  expect(derive({...INITIAL,th:90}).strip_a).toBe(0);
  expect(derive({...INITIAL,a:1,th:30}).R1).toBeLessThan(0);
  expect(derive({...INITIAL,a:4,b:1,th:30}).R2).toBeLessThan(0);
  expect(derive({...INITIAL,th:120}).DB).toBeGreaterThan(3);
  expect(moveB(INITIAL,[-99,0],'key').set.a).toBe(.5);
  expect(moveA({...INITIAL,th:90},[.1,0],'key').set.th).toBeLessThan(90);
 });
 test('a pointer below CB clamps θ to the nearer end instead of flipping the triangle',()=>{
  for(const src of ['drag','key'] as const){
   expect(moveA({...INITIAL,th:180},[0,-.01],src).set.th).toBe(180);
   expect(moveA({...INITIAL,th:0},[0,-.01],src).set.th).toBe(0);
  }
  expect(moveA({...INITIAL,th:180},[1,-.5],'drag').set.th).toBe(180);
  expect(moveA({...INITIAL,th:0},[-1,-.5],'drag').set.th).toBe(0);
  expect(moveA({...INITIAL,th:170},[0,0],'drag').set.th).toBe(170);
 });
});
test('IK preserves two bone lengths and finite values for all reach boundaries',()=>{
 for(const [l1,l2] of [[2,1.5],[1,3],[2,2]])for(const d of [0,.3,Math.abs(l1-l2),2.5,l1+l2,8])for(const ik_flip of [0,1]){
  const z=solveIK({...INITIAL,ik_l1:l1,ik_l2:l2,ik_hand:[d,0],ik_flip});
  expect(Math.hypot(...z.E)).toBeCloseTo(l1,10);
  expect(Math.hypot(z.W[0]-z.E[0],z.W[1]-z.E[1])).toBeCloseTo(l2,10);
  expect(Number.isFinite(z.phi)).toBe(true);
  if(d>=z.reachMin&&d<=z.reachMax)expect(Math.hypot(z.W[0]-d,z.W[1])).toBeLessThan(1e-7);
 }
 expect(solveIK({...INITIAL,ik_hand:[4,0]}).cosraw).toBe(-1.625);
 expect(solveIK({...INITIAL,ik_hand:[2.5,0]}).phi).toBe(90);
});
