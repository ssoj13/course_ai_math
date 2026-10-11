import {useContext,useLayoutEffect,useMemo,useRef,useState} from 'react';
import {useFrame,useThree} from '@react-three/fiber';
import {DoubleSide,ShapeUtils,Vector2} from 'three';
import {to3,sub,dot,cross,normalize,type V} from '../geom';
import {STYLE,usePalette,type SceneRole} from '../palette';
import {useVals,useStep,useHot,type LessonState} from '../store';
import {useLabel,useHoverable,HOVER_RANK} from './handles';
import {useDynGeo,writeGeo,Stroke} from './view';
import {SPRING_MS,reducedMotion,springProgress} from './motion';
import {LookCtx,assertSubject} from './look';
import {Hatch} from './hatchMaterial';
import {lerpVertices,polygonInfo} from './polygon';
type Vals=LessonState['vals'];
export interface PolyProps {
 lesson:string;id:string;timeline?:string;step?:number;
 pts:(vals:Vals,step:number)=>V[];
 /** Explicit pure path, including fractional placements for area-preserving shears/rigid turns. */
 path?:(vals:Vals,from:number,to:number,progress:number)=>V[];
 phases?:number;
 role:SceneRole;fill?:number;hatched?:(vals:Vals)=>boolean;edge?:'solid'|'dashed'|'none';label?:(vals:Vals)=>string;
}
export function Poly(p:PolyProps):React.JSX.Element {
 assertSubject(useContext(LookCtx),true);
 const vals=useVals(p.lesson),timeline=useStep(p.lesson,p.timeline??'cut'),step=p.step??timeline;
 const timelinePhases=useStep(p.lesson,(p.timeline??'cut')+':phases');
 const countPhases=p.phases??Math.max(1,timelinePhases);
 if(!Number.isInteger(countPhases)||countPhases<1)throw new Error('Poly phases must be a positive integer');
 const pal=usePalette(),hot=useHot(p.lesson,p.id),invalidate=useThree(s=>s.invalidate);
 const target=p.pts(vals,step),count=useRef(target.length),last=useRef({vals,step});
 const [points,setPoints]=useState(target),shown=useRef(target),position=useRef(step);
 const motion=useRef<{start:number;from:number;to:number;pts:V[];duration:number}|null>(null);
 if(target.length!==count.current)throw new Error('Poly '+p.id+': vertex count changed');
 polygonInfo(target);
 useLayoutEffect(()=>{
  const old=last.current;last.current={vals,step};
  if(old.step!==step&&!reducedMotion()){
   motion.current={start:performance.now(),from:position.current,to:step,pts:shown.current,duration:SPRING_MS*countPhases};
   invalidate();
  }else if(old.vals!==vals||old.step!==step){motion.current=null;shown.current=target;position.current=step;setPoints(target)}
 },[vals,step]);
 useFrame(()=>{
  const m=motion.current;if(!m)return;
  const elapsed=performance.now()-m.start,n=m.duration/SPRING_MS;
  const phase=Math.floor(elapsed/SPRING_MS);
  const t=reducedMotion()||elapsed>=m.duration?1:(phase+springProgress(elapsed%SPRING_MS,SPRING_MS))/n;
  const next=p.path?p.path(vals,m.from,m.to,t):lerpVertices(m.pts,target,t);
  if(next.length!==count.current)throw new Error('Poly path changed vertex count');
  polygonInfo(next);shown.current=next;position.current=m.from+(m.to-m.from)*t;setPoints(next);
  if(t===1)motion.current=null;else invalidate();
 });
 const info=polygonInfo(points),coords=points.map(to3);
 const indices=useMemo(()=>{
  if(info.area<1e-9)return [];
  const u=normalize(sub(coords.find(v=>v.some((x,j)=>x!==coords[0][j]))!,coords[0])),v=cross(info.normal,u);
  return ShapeUtils.triangulateShape(coords.map(p=>new Vector2(dot(p,u),dot(p,v))),[]).flat();
 },[points]);
 const geo=useDynGeo(count.current,indices);
 useLayoutEffect(()=>{geo.setIndex(indices);writeGeo(geo,coords)},[geo,points,indices]);
 useLabel(p.label?{lesson:p.lesson,text:p.label(vals),role:p.role,hot:p.id,at:()=>info.center}:null);
 const hover=useHoverable(p.lesson,p.id,HOVER_RANK.fill);
 return <group {...hover}>
  {info.area>=1e-9&&<>
   <mesh geometry={geo} frustumCulled={false}><meshBasicMaterial color={pal[p.role]} side={DoubleSide} transparent opacity={p.fill??STYLE.fillA} depthWrite={false} polygonOffset polygonOffsetFactor={1}/></mesh>
   {p.hatched?.(vals)&&<Hatch geometry={geo} role={p.role}/>}
   {p.edge!=='none'&&<Stroke points={[...coords,coords[0]]} color={pal[p.role]} hot={hot} width={STYLE.edge} dashPx={p.edge==='dashed'?[5,4]:undefined}/>}
  </>}
 </group>;
}
