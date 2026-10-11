import {add,sub,scale,cross,norm,dot,to3,type V} from '../geom';
/** Validate finite coplanar polygons; Newell area handles degenerate and concave boundaries. */
export function polygonInfo(pts:readonly V[]):{area:number;signedArea:number;center:V;normal:V}{
 if(pts.length<3||pts.some(p=>p.length!==pts[0].length||![2,3].includes(p.length)||!p.every(Number.isFinite)))throw new Error('Poly needs at least three finite matching 2D/3D vertices');
 const p=pts.map(to3);let n:V=[0,0,0];
 for(let i=0;i<p.length;i++)n=add(n,cross(p[i],p[(i+1)%p.length]));
 const area=norm(n)/2,normal=area>1e-12?scale(n,1/(2*area)):[0,0,1];
 if(area>1e-12&&p.some(v=>Math.abs(dot(sub(v,p[0]),normal))>1e-8))throw new Error('Poly vertices must be coplanar');
 const center=area<1e-9?pts[0]:scale(pts.reduce((s,v)=>add(s,v),pts[0].map(()=>0)),1/pts.length);
 return {area,signedArea:pts[0].length===2?n[2]/2:area,center,normal};
}
export function lerpVertices(a:readonly V[],b:readonly V[],t:number):V[]{
 if(a.length!==b.length)throw new Error('Poly vertex count cannot change for one id');
 return a.map((v,i)=>v.map((x,j)=>x+(b[i][j]-x)*t));
}
export function rotateVertices(pts:readonly V[],pivot:V,angle:number):V[]{
 const c=Math.cos(angle),s=Math.sin(angle);
 return pts.map(p=>{const d=sub(p,pivot);return add(pivot,[d[0]*c-d[1]*s,d[0]*s+d[1]*c])});
}
/** Correct cyclic correspondence and rigid-angle paths from the independent LC preparation oracle.
 * placement: 0 initial, 1 sheared, 2 quarter turn, 3 final. Winding is NOT mathematical debt sign.
 */
export function lcPiece(a:number,b:number,theta:number,piece:1|2,placement:number):V[]{
 if(![a,b,theta,placement].every(Number.isFinite))throw new Error('LC inputs must be finite');
 const rad=theta*Math.PI/180,c0=Math.cos(rad),cos=Math.abs(c0)<1e-12?0:c0;
 const C:V=[0,0],B:V=[a,0],u:V=[cos,Math.sin(rad)],A=scale(u,b),v=sub(A,B),c=norm(v);
 if(c<1e-9)return [A,A,A,A];
 const D:V=[A[0],0],E=scale(u,a*cos),w=scale(v,1/c),F=add(B,scale(w,dot(sub(C,B),w))),cm:V=[v[1],-v[0]];
 const origin=piece===1?B:A,far=piece===1?D:E,target=piece===1?A:B,side=piece===1?[0,-a]:scale([-u[1],u[0]],b);
 const initial=[origin,add(origin,side),add(far,side),far],sheared=[origin,add(origin,side),add(target,side),target];
 const turn=piece===1?-Math.PI/2:Math.PI/2;
 const rotated=rotateVertices(sheared,origin,turn),final=[origin,F,add(F,cm),add(origin,cm)];
 const t=Math.max(0,Math.min(3,placement));
 return t<=1?lerpVertices(initial,sheared,t):t<=2?rotateVertices(sheared,origin,turn*(t-1)):lerpVertices(rotated,final,t-2);
}
