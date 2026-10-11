import {expect,test} from 'bun:test';
import {lcPiece,polygonInfo} from '../../src/viz/scene/polygon';
const oracle={cases:[[3,4,60],[3,4,90],[3,4,120],[1,4,30],[4,1,30],[3,3,0],[3,4,180],[.5,4,1]].map(([a,b,th])=>{
 const cos=Math.cos(th*Math.PI/180),R1=a*(a-b*cos),R2=b*(b-a*cos);
 return {a,b,th,degenerate:a===b&&th===0,samples:[0,.25,.5,.75,1].map(t=>({t,values:[-R1,-R1,-R1,R2,R2,R2]}))};
})};
test('LC pieces preserve signed area during every shear and rigid rotation',()=>{
 for(const c of oracle.cases)if(!c.degenerate)for(const sample of c.samples!)for(let phase=0;phase<3;phase++)for(const piece of [1,2] as const){
  const pts=lcPiece(c.a,c.b,c.th,piece,phase+sample.t);
  expect(polygonInfo(pts).signedArea).toBeCloseTo(sample.values[(piece-1)*3+phase],9);
 }
});
test('rotation preserves edge lengths at intermediate points and coincident case stays finite',()=>{
 const a=lcPiece(3,4,60,1,1),b=lcPiece(3,4,60,1,1.5);
 for(let i=0;i<4;i++)expect(Math.hypot(...a[i].map((x,j)=>x-a[(i+1)%4][j]))).toBeCloseTo(Math.hypot(...b[i].map((x,j)=>x-b[(i+1)%4][j])),10);
 expect(polygonInfo(lcPiece(3,3,0,1,2)).area).toBe(0);
 expect(()=>polygonInfo([[0,0,0],[1,0,0],[1,1,0],[0,1,1]])).toThrow();
});
