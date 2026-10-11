import {test,expect} from 'bun:test';
import {readFileSync} from 'node:fs';
import {readPalette,contrast} from '../../src/viz/palette';
const css=readFileSync(new URL('../../src/styles/lab.css',import.meta.url),'utf8');
const vars=new Map([...css.matchAll(/(--viz-[a-z0-9-]+):\s*(#[0-9a-f]{6})/gi)].map(m=>[m[1],m[2]]));
const p=readPalette(n=>vars.get(n)??'');
// Machado, Oliveira & Fernandes 2009, severity 1.0, linear RGB.
const matrices=[
 [1,0,0,0,1,0,0,0,1],
 [.152286,1.052583,-.204868,.114503,.786281,.099216,-.003882,-.048116,1.051998],
 [.367322,.860646,-.227968,.280085,.672501,.047413,-.011820,.042940,.968881]
];
function lab(hex:string,m:number[]){
 const rgb=[1,3,5].map(i=>{const x=parseInt(hex.slice(i,i+2),16)/255;return x<=.04045?x/12.92:((x+.055)/1.055)**2.4;});
 const [r,g,b]=[0,1,2].map(i=>Math.min(1,Math.max(0,rgb.reduce((a,x,j)=>a+x*m[3*i+j],0))));
 const f=(v:number)=>v>216/24389?Math.cbrt(v):v*(24389/27)/116+16/116;
 const x=f((.4124564*r+.3575761*g+.1804375*b)/.95047),y=f(.2126729*r+.7151522*g+.072175*b),z=f((.0193339*r+.119192*g+.9503041*b)/1.08883);
 return [116*y-16,500*(x-y),200*(y-z)];
}
test('v3 palette: Machado full protan/deutan CIE76 pair separation',()=>{
 for(const [keys,min] of [[['vecA','vecB','vecC','hot','light'],15],[['axisX','axisY','axisZ','vecA'],15],[['clayA','clayB','clayC','clayAccent'],14]] as const)
 for(const m of matrices){const colors=keys.map(k=>lab(p[k],m));for(let i=0;i<colors.length;i++)for(let j=i+1;j<colors.length;j++)expect(Math.hypot(...colors[i].map((v,k)=>v-colors[j][k]))).toBeGreaterThanOrEqual(min);}
 expect(contrast(p.clayInk,p.clayPlate)).toBeGreaterThanOrEqual(4.5);
 expect(contrast(p.grid,p.bgTop)).toBeLessThan(1.6);
});
