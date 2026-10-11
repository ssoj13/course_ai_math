import {lazy,Suspense,useContext,useLayoutEffect,useMemo,useRef,useState} from 'react';
import {useFrame,useThree} from '@react-three/fiber';
import {DoubleSide,Float32BufferAttribute,ShapeUtils,Vector2} from 'three';
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
const Glass=lazy(()=>import('./Glass').then(m=>({default:m.Glass})));
/**
 * Glass coordinates of a polygon: `local` = position in world units along the first edge and its in-plane normal
 * (the glass grid then counts unit cells aligned with that edge: squares and strips show their own cells), `uv` =
 * the same normalised to the polygon's extent in those directions (rim glow; exact for rectangles).
 */
function glassCoords(coords:number[][],u:number[],v:number[]):{local:number[];uv:number[]}{
 const o=coords[0],l=coords.map(p=>[dot(sub(p,o),u),dot(sub(p,o),v)]);
 const lo=[Math.min(...l.map(q=>q[0])),Math.min(...l.map(q=>q[1]))],hi=[Math.max(...l.map(q=>q[0])),Math.max(...l.map(q=>q[1]))];
 const span=[Math.max(hi[0]-lo[0],1e-9),Math.max(hi[1]-lo[1],1e-9)];
 return {local:l.flatMap(q=>[q[0]-lo[0],q[1]-lo[1]]),uv:l.flatMap(q=>[(q[0]-lo[0])/span[0],(q[1]-lo[1])/span[1]])};
}
export interface PolyProps {
 lesson:string;id:string;timeline?:string;step?:number;
 /** Reader-controlled fractional placement; supplied positions bypass the internal clock. */
 position?:number;
 pts:(vals:Vals,step:number)=>V[];
 /** Explicit pure path, including fractional placements for area-preserving shears/rigid turns. */
 path?:(vals:Vals,from:number,to:number,progress:number)=>V[];
 phases?:number;
 role:SceneRole;fill?:number;hatched?:(vals:Vals)=>boolean;edge?:'solid'|'dashed'|'none';label?:(vals:Vals)=>string;
}
export function Poly(p:PolyProps):React.JSX.Element {
 assertSubject(useContext(LookCtx),true);
 const vals=useVals(p.lesson),timeline=useStep(p.lesson,p.timeline??'cut'),step=p.position??p.step??timeline;
 if(!Number.isFinite(step))throw new Error('Poly position must be finite');
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
  if(p.position!==undefined){motion.current=null;shown.current=target;position.current=step;setPoints(target)}
  else if(old.step!==step&&!reducedMotion()){
   motion.current={start:performance.now(),from:position.current,to:step,pts:shown.current,duration:SPRING_MS*countPhases};
   invalidate();
  }else if(old.vals!==vals&&motion.current&&p.path&&!reducedMotion()){
   // A direct value write reshapes the current placement without restarting its clock.
   const next=p.path(vals,position.current,position.current,0);
   shown.current=next;setPoints(next);invalidate();
  }else if(old.vals!==vals||old.step!==step){motion.current=null;shown.current=target;position.current=step;setPoints(target)}
 },[vals,step,p.position]);
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
 useLayoutEffect(()=>{
  geo.setIndex(indices);writeGeo(geo,coords);
  if(info.area<1e-9)return;
  const u=normalize(sub(coords.find(q=>q.some((x,j)=>x!==coords[0][j]))!,coords[0])),g=glassCoords(coords,u,cross(info.normal,u));
  geo.setAttribute('local',new Float32BufferAttribute(g.local,2));geo.setAttribute('uv',new Float32BufferAttribute(g.uv,2));
 },[geo,points,indices]);
 useLabel(p.label?{lesson:p.lesson,text:p.label(vals),role:p.role,hot:p.id,at:()=>info.center}:null);
 const hover=useHoverable(p.lesson,p.id,HOVER_RANK.fill);
 return <group {...hover}>
  {info.area>=1e-9&&<>
   <mesh geometry={geo} frustumCulled={false}>
    <Suspense fallback={<meshBasicMaterial color={pal[p.role]} side={DoubleSide} transparent opacity={p.fill??STYLE.fillA} depthWrite={false} polygonOffset polygonOffsetFactor={1}/>}>
     <Glass color={pal[p.role]} hot={hot} alpha={p.fill??STYLE.fillA}/>
    </Suspense>
   </mesh>
   {p.hatched?.(vals)&&<Hatch geometry={geo} role={p.role}/>}
   {p.edge!=='none'&&<Stroke points={[...coords,coords[0]]} color={pal[p.role]} hot={hot} width={STYLE.edge} dashPx={p.edge==='dashed'?[5,4]:undefined}/>}
  </>}
 </group>;
}
