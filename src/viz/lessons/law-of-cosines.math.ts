import type { LessonState } from '../store';
import type { V } from '../geom';
import { add, sub, scale, dot } from '../geom';
import { SPRING_MS, springProgress, omegaFor } from '../scene/motion';
import { lcPiece } from '../scene/polygon';
import { magnet, snap } from '../scene/drag';
import type { Draggable } from '../scene/handles';

export const LESSON = 'law-of-cosines';
export const INITIAL = { a:3, b:4, th:60, ik_hand:[2.5,1.5], ik_l1:2, ik_l2:1.5, ik_flip:0, cut_position:0, cut_from:0, cut_to:0, cut_progress:1, cut_play:0, cut_phases:1, proof_fade_position:0, proof_fade_from:0, proof_fade_to:0, proof_fade_progress:1, proof_fade_play:0, proof_fade_phases:1 };
export const MAGNETS = [0,30,45,60,90,120,135,150,180] as const;
type Vals = LessonState['vals'];
export const clamp = (x:number,lo:number,hi:number) => Math.max(lo,Math.min(hi,x));
export const fmt = (x:number) => Number.isFinite(x) ? (+x.toFixed(2)).toFixed(2) : '–';
export const fmtCos = (x:number) => Number.isFinite(x) ? (+x.toFixed(3)).toFixed(3) : '–';
export const fmtAngle = (x:number) => Number.isInteger(x) ? String(x) : (+x.toFixed(1)).toFixed(1);
export const cw = (p:V):V => [p[1],-p[0]];

/** Full precision signed coordinates; formatted labels never feed geometry. */
export function derive(v:Vals) {
 const a=v.a as number,b=v.b as number,th=v.th as number,rad=th*Math.PI/180;
 const cos=Math.abs(Math.cos(rad))<1e-12?0:Math.cos(rad);
 const sin=Math.abs(Math.sin(rad))<1e-12?0:Math.sin(rad);
 const C:V=[0,0],B:V=[a,0],u:V=[cos,sin],A=scale(u,b),D:V=[b*cos,0],E=scale(u,a*cos);
 const c=Math.hypot(A[0]-a,A[1]),degenerate=c<1e-9;
 const w:V=degenerate?[0,1]:scale(sub(A,B),1/c),m=cw(w);
 const FB=degenerate?0:dot(sub(C,B),w),AF=degenerate?0:c-FB,F=add(B,scale(w,FB));
 const p=b*cos,q=a*cos,h=b*sin,DB=a-p,EA=b-q;
 return {a,b,th,rad,cos,sin,C,B,A,D,E,F,u,w,m,c,degenerate,p,q,h,DB,EA,FB,AF,
  a2:a*a,b2:b*b,c2:c*c,pyth:a*a+b*b,corr:-2*a*b*cos,
  strip_a:a*p,strip_b:b*q,ab:a*p,R1:a*DB,R2:b*EA,R1c:c*FB,R2c:c*AF,
  rest:DB,rest2:DB*DB,rise:h,rise2:h*h,bcos2:p*p,bsin2:h*h};
}

/** The target can be unreachable; bones retain their exact lengths in every branch. */
export function solveIK(v:Vals) {
 const T=v.ik_hand as V,l1=v.ik_l1 as number,l2=v.ik_l2 as number,d=Math.hypot(...T);
 const reachMin=Math.abs(l1-l2),reachMax=l1+l2;
 const u:V=d<1e-9?[1,0]:scale(T,1/d);
 const cosraw=(l1*l1+l2*l2-d*d)/(2*l1*l2),cosc=clamp(cosraw,-1,1);
 const phi=Math.acos(cosc)*180/Math.PI;
 const cosb=d<1e-9?1:clamp((l1*l1+d*d-l2*l2)/(2*l1*d),-1,1);
 const beta=Math.acos(cosb)*(v.ik_flip?-1:1),cb=Math.cos(beta),sb=Math.sin(beta);
 const S:V=[0,0],E:V=[l1*(u[0]*cb-u[1]*sb),l1*(u[0]*sb+u[1]*cb)];
 const delta=sub(T,E),n=Math.hypot(...delta);
 const W=add(E,scale(n<1e-9?scale(u,-1):scale(delta,1/n),l2));
 return {S,E,W,T,u,l1,l2,d,cosraw,cosc,phi,beta,reachMin,reachMax,zero:d<1e-9,
  outside:d>reachMax+1e-9,inside:d<reachMin-1e-9};
}

export const moveA:NonNullable<Draggable['move']>=(v,raw,src)=>{
 const A=derive(v).A,x=A[0]+raw[0],y=A[1]+raw[1];
 const angle=clamp(Math.atan2(y,x)*180/Math.PI,0,180);
 const th=magnet(angle,MAGNETS,2.5,src==='key'?v.th as number:undefined);
 return {set:{b:clamp(snap(Math.hypot(x,y)),.5,4),th},ease:th!==angle?['th']:[]};
};
export const moveB:NonNullable<Draggable['move']>=(v,raw)=>({set:{a:clamp(snap((v.a as number)+raw[0]),.5,4)}});

/** Authored steps 6/7/8 are the first piece's shear/turn/shear; step 9 has three phases. */
export function placement(step:number,piece:1|2) {
 return piece===1?clamp(step-5,0,3):3*clamp(step-8,0,1);
}
export function pieceAt(v:Vals,piece:1|2,step:number):V[] {
 return lcPiece(v.a as number,v.b as number,v.th as number,piece,placement(step,piece));
}
export function square(a:V,b:V):V[] {
 const n=cw(sub(b,a));return [a,b,add(b,n),add(a,n)];
}
export function strip(v:Vals,piece:1|2):V[] {
 const z=derive(v),end=piece===1?z.D:z.E,n:V=piece===1?[0,-z.a]:scale([-z.sin,z.cos],z.b);
 return [z.C,end,add(end,n),n];
}

/** One clock for all paper pieces. A three-phase transition settles each spring in turn. */
export function cutProgress(elapsed:number,phases:number){
 if(elapsed>=SPRING_MS*phases)return 1;
 const n=Math.max(0,elapsed)/SPRING_MS,i=Math.floor(n);
 return (i+springProgress((n-i)*SPRING_MS,SPRING_MS)/springProgress(SPRING_MS,SPRING_MS))/phases;
}
/** Inverse used only when resuming a paused or reader-scrubbed position. */
export function cutElapsed(progress:number,phases:number){
 const p=clamp(progress,0,1);if(p===1)return SPRING_MS*phases;
 const n=p*phases,i=Math.floor(n),fraction=(n-i)*springProgress(SPRING_MS,SPRING_MS),omega=omegaFor(SPRING_MS)/1000;
 let lo=0,hi=SPRING_MS;
 for(let k=0;k<48;k++){const m=(lo+hi)/2,q=1-(1+omega*m)*Math.exp(-omega*m);if(q<fraction)lo=m;else hi=m}
 return i*SPRING_MS+(lo+hi)/2;
}
export function cutMoves(from:number,to:number,step:number){
 if(step>=4&&(placement(from,1)!==placement(to,1)||placement(from,2)!==placement(to,2)))return true;
 return (step>=2&&clamp(from-1,0,1)!==clamp(to-1,0,1))||(step>=3&&clamp(from-2,0,1)!==clamp(to-2,0,1));
}
