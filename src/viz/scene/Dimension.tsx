import {useEffect,useRef,useState} from 'react';
import {useFrame} from '@react-three/fiber';
import {DoubleSide,Vector3} from 'three';
import {add,norm,rightOf,screenDimension,sub,to3,type V} from '../geom';
import {usePalette} from '../palette';
import {useHot} from '../store';
import {HOVER_RANK,useHoverable,useLabel,usePos,useStage} from './handles';
import {SLine,useDynGeo,writeGeo} from './view';
type Layout={a:number[];b:number[];mid:number[];heads:number[][];ticks:number[][]};
/**
 * A dimension line (visual-language v3 measuring instrument): a 1 px line offset 14 CSS px to the right of a->b with
 * filled heads, end ticks and the value in mono. `label` replaces the default text |b - a| (e.g. a signed side "x = -3.0"
 * whose sign the length cannot carry). During the stage's draw-in the line grows from `a`, like a beam.
 */
export function Dimension({lesson,id,a:aa,b:bb,normal,label}:{lesson:string;id:string;a:V|string;b:V|string;normal?:V;label?:string}){
 const a=usePos(lesson,aa),bFull=usePos(lesson,bb),reg=useStage(),pal=usePalette(),hot=useHot(lesson,id),hover=useHoverable(lesson,id,HOVER_RANK.guide);
 const [layout,setLayout]=useState<Layout|null>(null),signature=useRef('');
 const heads=useDynGeo(6,[0,1,2,3,4,5]),point=useRef(new Vector3());
 useFrame(({camera,invalidate})=>{
  if(!reg.el)return;
  // Draw-in: the drawn end moves from a to b over the first half of the reveal (the measured value is unchanged).
  const grow=Math.min(1,reg.reveal*2),b=bFull.map((x,i)=>a[i]+(x-a[i])*grow);
  const w=reg.el.clientWidth,h=reg.el.clientHeight,n=rightOf(a,b,a.length===3?normal??[0,0,1]:undefined);
  if(!n){if(signature.current!=='hidden'){signature.current='hidden';setLayout(null);invalidate();}return;}
  const project=(p:V)=>point.current.set(...to3(p)).project(camera).toArray();
  const na=project(a),nb=project(b),no=project(add(a,n));
  const screen=(p:number[])=>[(p[0]+1)*w/2,(1-p[1])*h/2];
  const s=screenDimension(screen(na),screen(nb),screen(no));
  const observable=s&&na[2]>=-1&&na[2]<=1&&nb[2]>=-1&&nb[2]<=1;
  if(!observable){if(signature.current!=='hidden'){signature.current='hidden';setLayout(null);invalidate();}return;}
  const unproject=(p:number[],z:number)=>point.current.set(p[0]/w*2-1,1-p[1]/h*2,z).unproject(camera).toArray();
  const dx=s.b[0]-s.a[0],dy=s.b[1]-s.a[1],len=Math.hypot(dx,dy),u=[dx/len,dy/len],n2=s.normal;
  const at=(p:number[],x:number,y:number)=>[p[0]+x*u[0]+y*n2[0],p[1]+x*u[1]+y*n2[1]];
  const pa=unproject(s.a,na[2]),pb=unproject(s.b,nb[2]),hs:number[][]=[],ts:number[][]=[];
  [s.a,s.b].forEach((p,i)=>{const z=i?nb[2]:na[2],sign=i?-1:1;
   hs.push(unproject(at(p,sign*5,3),z),unproject(p,z),unproject(at(p,sign*5,-3),z));
   ts.push(unproject(at(p,0,4),z),unproject(at(p,0,-4),z));
  });
    // The value sits 12 px further out along the screen normal: outside whichever side the line was offset to.
  const next={a:pa,b:pb,mid:unproject(at([(s.a[0]+s.b[0])/2,(s.a[1]+s.b[1])/2],0,12),(na[2]+nb[2])/2),heads:hs,ticks:ts};
  const key=JSON.stringify(next,(_,v)=>typeof v==='number'?Math.round(v*1e7)/1e7:v);
  if(key!==signature.current){signature.current=key;setLayout(next);invalidate();}
 },2);
 useEffect(()=>{if(layout)writeGeo(heads,layout.heads);},[heads,layout]);
 useLabel(layout?{lesson,text:label??norm(sub(bFull,a)).toFixed(2),hot:id,at:()=>layout.mid,small:true}:null);
 if(!layout)return null;
 const color=hot?pal.hot:pal.ink;
 return <group {...hover}><mesh geometry={heads}><meshBasicMaterial color={color} side={DoubleSide} transparent depthWrite={false}/></mesh>
 <SLine points={[to3(layout.a),to3(layout.b)]} color={color} lineWidth={1} transparent depthWrite={false}/>
 <SLine points={layout.ticks.map(to3)} segments color={color} lineWidth={1} transparent depthWrite={false}/></group>;
}
