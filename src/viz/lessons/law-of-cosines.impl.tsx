import { useEffect, useMemo, useRef, useState } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import { DoubleSide, MeshBasicMaterial, Matrix4, Vector3, type Group, type Mesh, type Material, type BufferGeometry } from 'three';
import { useStore } from 'zustand';
import { useLocale } from '../../i18n/react';
import { lessonStore, useVals, useStep, useHot } from '../store';
import { add, sub, scale, to3, type V } from '../geom';
import { STYLE, usePalette, roleVar, type SceneRole } from '../palette';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Poly, type PolyProps } from '../scene/Poly';
import { AngleArc } from '../scene/AngleArc';
import { RightAngle } from '../scene/RightAngle';
import { Circle } from '../scene/Circle';
import { Label } from '../scene/Label';
import { Stroke } from '../scene/view';
import { Hatch } from '../scene/hatchMaterial';
import { reducedMotion } from '../scene/motion';
import { Show, useOverlay, useHoverable, useLabel, useStage, HOVER_RANK } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { LawFormula } from './law-of-cosines';
import { rowsFor, words } from './law-of-cosines.rows';
import { LESSON, derive, solveIK, square, strip, pieceAt, moveA, moveB, MAGNETS, fmt, fmtAngle } from './law-of-cosines.math';

type Scene=0|1|2|3;
const sources=new Map<string,Group>();
function sourceMesh(id:string):Mesh|undefined{
 let mesh:Mesh|undefined;
 sources.get(id)?.traverse(o=>{const m=o as Mesh;if(!mesh&&m.isMesh&&m.geometry.getAttribute('position')?.count===4)mesh=m});
 return mesh;
}
function meshPoints(mesh:Mesh):V[]{
 const a=mesh.geometry.getAttribute('position');
 mesh.updateWorldMatrix(true,false);
 return Array.from({length:a.count},(_,i)=>{const v=new Vector3(a.getX(i),a.getY(i),a.getZ(i)).applyMatrix4(mesh.matrixWorld);return [v.x,v.y] as V});
}
const DETAILS=[
 {id:'sq_c',role:'vecC',key:'c2',label:'c²'},
 {id:'sq_a',role:'vecA',key:'a2',label:'a²'},
 {id:'sq_b',role:'vecB',key:'b2',label:'b²'},
 {id:'strip_a',role:'vecA',key:'strip_a',label:'a·p'},
 {id:'strip_b',role:'vecB',key:'strip_b',label:'b·q'},
 {id:'R1',role:'vecA',key:'R1',label:'a·DB'},
 {id:'R2',role:'vecB',key:'R2',label:'b·EA'},
 {id:'R1c',role:'vecC',key:'R1c',label:'c·FB'},
 {id:'R2c',role:'vecC',key:'R2c',label:'c·AF'},
 {id:'p',role:'muted',key:'p',label:'b cos θ'},
 {id:'q',role:'muted',key:'q',label:'a cos θ'}
] as const;
const PROOF_DETAILS=[
 {id:'c2',role:'vecC',key:'c2',label:'c²'},
 {id:'rest2',role:'vecA',key:'rest2',label:'(a − b cos θ)²'},
 {id:'rise2',role:'vecB',key:'rise2',label:'(b sin θ)²'},
 {id:'a2',role:'vecA',key:'a2',label:'a²'},
 {id:'bcos2',role:'vecB',key:'bcos2',label:'b² cos² θ'},
 {id:'bsin2',role:'vecB',key:'rise2',label:'b² sin² θ'},
 {id:'b2',role:'vecB',key:'b2',label:'b²'}
] as const;
type DetailPart=typeof DETAILS[number]|typeof PROOF_DETAILS[number];
const EXTENTS=[11,5.5,11,6.5];
const midpoint=(a:V,b:V)=>scale(add(a,b),.5);
function LaneText({id,at,text,role,lane}:{id:string;at:V;text:string;role:SceneRole;lane:readonly[0|1|2,number]}){
 const reg=useStage(),offset=useRef<[number,number]>([0,0]),line=useRef<SVGPathElement|null>(null),hot=useHot(LESSON,id);
 useLabel({lesson:LESSON,text,role,hot:id,at:()=>at,offset:offset.current});
 useEffect(()=>{
  if(!reg.el)return;const svg=document.createElementNS('http://www.w3.org/2000/svg','svg'),path=document.createElementNS(svg.namespaceURI,'path') as SVGPathElement;
  svg.setAttribute('aria-hidden','true');Object.assign(svg.style,{position:'absolute',inset:'0',width:'100%',height:'100%',pointerEvents:'none',zIndex:'2'});
  path.setAttribute('fill','none');path.setAttribute('stroke-width','1');svg.append(path);reg.el.append(svg);line.current=path;
  return()=>{line.current=null;svg.remove()};
 },[reg]);
 useFrame(()=>{
  const point=reg.project?.(at);if(!reg.el||!point){line.current?.setAttribute('d','');return}
  const width=reg.el.clientWidth-parseFloat(reg.el.style.getPropertyValue('--lab-vp-inset')||'0'),spec=reg.labels.list().find(s=>s.hot===id),el=spec&&reg.labelEls.get(spec.id),half=(el?.offsetWidth??100)/2;
  const x=Math.max(half+4,Math.min(width-half-4,width*(lane[0]===2?.5:lane[0]===0?.23:.77))),y=lane[0]===2?reg.el.clientHeight-14-lane[1]*22:18+lane[1]*27;
  offset.current[0]=x-point[0];offset.current[1]=y-point[1];line.current?.setAttribute('d',`M ${point[0]} ${point[1]} L ${x} ${y}`);line.current?.setAttribute('stroke',`var(${roleVar(hot?'hot':role)})`);
 },1.5);
 return null;
}
function Text({id,at,text,offset=[0,-9],role='ink',lane}:{id:string;at:V;text:string;offset?:readonly[number,number];role?:SceneRole;lane?:readonly[0|1|2,number]}){
 if(lane)return <Show ov="labels"><LaneText id={id} at={at} text={text} role={role} lane={lane}/></Show>;
 return <Show ov="labels"><Label lesson={LESSON} id={id} at={at} offset={offset} role={role}>{text}</Label></Show>;
}
function Triangle(){
 const z=derive(useVals(LESSON)),labels=useOverlay('labels');
 return <>
  <Segment lesson={LESSON} id="a" a={z.C} b={z.B} role="vecA"/>
  <Segment lesson={LESSON} id="b" a={z.C} b={z.A} role="vecB"/>
  {!z.degenerate&&<Segment lesson={LESSON} id="c" a={z.B} b={z.A} role="vecC"/>}
  <Point lesson={LESSON} id="C" at={z.C} role="ink"/>
  <Point lesson={LESSON} id="B" at={v=>derive(v).B} keys={['a']} move={moveB} role="vecA" label="B"/>
  <Point lesson={LESSON} id="A" at={v=>derive(v).A} keys={['b','th']} move={moveA} role="vecB" label="A"/>
  <Text id="C" at={z.C} text="C" offset={[-10,10]}/><Text id="A" at={z.A} text="A" offset={[-7,-13]}/><Text id="B" at={z.B} text="B" offset={[10,10]}/>
  <Text id="a" at={midpoint(z.C,z.B)} text="a" offset={[0,15]} role="vecA"/>
  <Text id="b" at={midpoint(z.C,z.A)} text="b" offset={[-32,0]} role="vecB"/>
  <Text id="c" at={midpoint(z.B,z.A)} text="c" offset={[24,0]} role="vecC"/>
  <Show ov="angle"><AngleArc lesson={LESSON} id="th" at={z.C} from={z.B} to={z.A} r={.6} ticks={MAGNETS} label={labels?'θ = '+fmtAngle(z.th)+'°':undefined}/></Show>
  <Show ov="right"><RightAngle lesson={LESSON} id="rt" at={z.C} u={z.B} v={z.A} show={z.th===90}/></Show>
 </>;
}
function Shadow({side=1,solid=false,names=true,lanes=false,lane}:{side?:1|2;solid?:boolean;names?:boolean;lanes?:boolean;lane?:readonly[0|1|2,number]}){
 const z=derive(useVals(LESSON));
 const a=side===1,zid=a?'p':'q',source=a?z.A:z.B,foot=a?z.D:z.E;
 const alias=zid;
 return <>
  <Segment lesson={LESSON} id={alias} a={z.C} b={foot} band role="muted"/>
  <Segment lesson={LESSON} id={a?'drop_a':'drop_b'} a={source} b={foot} dashed={!solid} role="muted"/>
  <Point lesson={LESSON} id={a?'D':'E'} at={foot} role="muted" size={4}/>
  <RightAngle lesson={LESSON} id={a?'rtD':'rtE'} at={foot} u={sub(source,foot)} v={a?[1,0]:z.u} role="muted"/>
  <Text id={a?'D':'E'} at={foot} text={a?'D':'E'} offset={[8,12]}/>
  {names&&<Text lane={lane??(lanes?[0,1]:undefined)} id={alias} at={midpoint(z.C,foot)} text={(a?'b cos θ = ':'a cos θ = ')+fmt(a?z.p:z.q)} offset={[0,-16]} role="muted"/>}
 </>;
}

/** LC-0's fixed overview uses two screen-space label lanes. Leaders follow the actual
 * drawn polygon, including intermediate rigid rotations; no duplicate animation clock. */
function LanePoly({lane,unfold,...p}:PolyProps & {lane:readonly[0|1|2,number];unfold?:'a'|'b'|'c'}){
 const invalidate=useThree(s=>s.invalidate),lastReveal=useRef(-1);
 const reg=useStage(),group=useRef<Group>(null),center=useRef<V>([0,0]),offset=useRef<[number,number]>([0,0]);
 useEffect(()=>{
  if(group.current)sources.set(p.id,group.current);
  const own=group.current;return()=>{if(sources.get(p.id)===own)sources.delete(p.id)};
 },[p.id]);
 const line=useRef<SVGPathElement|null>(null),vals=useVals(LESSON),step=useStep(LESSON,p.timeline??'cut');
 const hot=useHot(LESSON,p.id),visible=!!p.label;
 const fallback=p.pts(vals,p.step??step);
 center.current=fallback.reduce<V>((a,b)=>add(a,scale(b,1/fallback.length)),[0,0]);
 useLabel(visible?{lesson:LESSON,text:p.label!(vals),role:p.role,hot:p.id,at:()=>center.current,offset:offset.current}:null);
 useEffect(()=>{
  if(!visible||!reg.el)return;
  const svg=document.createElementNS('http://www.w3.org/2000/svg','svg');
  svg.setAttribute('aria-hidden','true');
  Object.assign(svg.style,{position:'absolute',inset:'0',width:'100%',height:'100%',pointerEvents:'none',zIndex:'2'});
  const path=document.createElementNS(svg.namespaceURI,'path') as SVGPathElement;
  path.setAttribute('fill','none');path.setAttribute('stroke-width','1');path.dataset.lcLeader=p.id;svg.append(path);
  reg.el.append(svg);line.current=path;
  return()=>{line.current=null;svg.remove()};
 },[reg,visible]);
 useFrame(()=>{
  if(unfold&&group.current){
   const z=derive(vals),[a,b]=unfold==='a'?[z.C,z.B]:unfold==='b'?[z.A,z.C]:[z.B,z.A],d=sub(b,a),n=Math.hypot(...d);
   const x=n?d[0]/n:1,y=n?d[1]/n:0,r=reg.reveal;
   const xx=x*x+r*y*y,xy=x*y*(1-r),yy=y*y+r*x*x;
   group.current.matrixAutoUpdate=false;
   group.current.matrix.set(xx,xy,0,a[0]-xx*a[0]-xy*a[1],xy,yy,0,a[1]-xy*a[0]-yy*a[1],0,0,1,0,0,0,0,1);
   group.current.updateWorldMatrix(true,true);
   if(lastReveal.current!==r){lastReveal.current=r;invalidate()}
  }
  if(!visible||!reg.el)return;
  let found=false;
  group.current?.traverse(o=>{
   const m=o as Mesh;
   if(found||!m.isMesh)return;
   const a=m.geometry.getAttribute('position');
   if(!a||a.count!==4)return;
   found=true;
   center.current=meshPoints(m).reduce<V>((a,b)=>add(a,scale(b,.25)),[0,0]);
  });
  const anchor=reg.project?.(center.current);
  const width=reg.el.clientWidth-parseFloat(reg.el.style.getPropertyValue('--lab-vp-inset')||'0');
  const x=width*(lane[0]===2?.5:lane[0]===0?.23:.77),y=lane[0]===2?reg.el.clientHeight-14-lane[1]*22:18+lane[1]*27;
  if(anchor){
   offset.current[0]=x-anchor[0];offset.current[1]=y-anchor[1];
   const end=x+(lane[0]===0?45:-45);
   line.current?.setAttribute('d',`M ${anchor[0]} ${anchor[1]} L ${end} ${y}`);
  }else line.current?.setAttribute('d','');
  line.current?.setAttribute('stroke',`var(${roleVar(hot?'hot':p.role)})`);
 },1.5);
 return <group ref={group}><Poly {...p} label={undefined}/></group>;
}

function Strip({piece,id,lane,label}:{piece:1|2;id:string;lane:readonly[0|1|2,number];label?:PolyProps['label']}){
 const v=useVals(LESSON),z=derive(v),hot=useHot(LESSON,id);
 const phase=Math.min(1,Math.max(0,(v.cut_position as number)-(piece===1?1:2)));
 const points=(v:Parameters<typeof derive>[0],t:number)=>strip(v,piece).map((p,i)=>{
  const base=strip(v,piece)[i===1?0:3];
  return i===1||i===2?add(base,scale(sub(p,base),t)):p;
 });
 return <LanePoly lesson={LESSON} id={id} lane={lane} position={phase}
  pts={points} path={(v,a,b,t)=>points(v,a+(b-a)*t)} role={piece===1?'vecA':'vecB'}
  fill={hot?.40:.28} hatched={v=>derive(v).strip_a<0}
  edge={(piece===1?z.p>z.a:z.q>z.b)?'dashed':'solid'} label={label}/>;
}
function Missing(){
 const v=useVals(LESSON),z=derive(v),step=useStep(LESSON,'cut'),labels=useOverlay('labels');
 const reg=useStage();
 useEffect(()=>{
  if(step!==3||reducedMotion())return;
  const animations:Animation[]=[];
  const frame=requestAnimationFrame(()=>{
   reg.el?.closest('.lab-vp-card')?.querySelectorAll<HTMLElement>('.lab-np-row[data-id="strip_a"],.lab-np-row[data-id="strip_b"]').forEach(el=>{
    animations.push(el.animate([{outline:'2px solid var(--viz-hot)'},{outline:'2px solid var(--viz-hot)'},{outline:'2px solid transparent'}],{duration:300}));
   });
  });
  return()=>{cancelAnimationFrame(frame);animations.forEach(a=>a.cancel())};
 },[step,reg]);
 const squareId=(side:'a'|'b'|'c')=>'sq_'+side;
 const stripId=(side:'a'|'b')=>'strip_'+side;
 return <>
  <Triangle/>
  <Show ov="squares">
   <LanePoly lesson={LESSON} unfold="a" lane={[0,3]} id={squareId('a')} pts={v=>{const d=derive(v);return square(d.C,d.B)}} role="vecA" fill={step>=4?.08:STYLE.fillA} label={labels&&step<4?v=>'a² = '+fmt(derive(v).a2):undefined}/>
   <LanePoly lesson={LESSON} unfold="b" lane={[0,0]} id={squareId('b')} pts={v=>{const d=derive(v);return square(d.A,d.C)}} role="vecB" fill={step>=4?.08:STYLE.fillA} label={labels&&step<4?v=>'b² = '+fmt(derive(v).b2):undefined}/>
   {!z.degenerate&&<LanePoly lesson={LESSON} unfold="c" lane={[1,0]} id={squareId('c')} pts={v=>{const d=derive(v);return square(d.B,d.A)}} role="vecC" fill={STYLE.fillA} label={labels&&step<5?v=>'c² = '+fmt(derive(v).c2):undefined}/>}
  </Show>
  <Show ov="shadows">{step>=1&&<Shadow lane={[1,2]}/>}{step>=3&&<Shadow side={2} lane={[1,3]}/>}</Show>
  <Show ov="strips">
   {step>=2&&<Strip piece={1} id={stripId('a')} lane={[0,4]} label={labels?v=>(step===11?'a·b = ':'a·p = ')+fmt(derive(v).strip_a):undefined}/>}
   {step>=3&&<Strip piece={2} id={stripId('b')} lane={[0,1]} label={labels?v=>(step===11?'a·b = ':'b·q = ')+fmt(derive(v).strip_b):undefined}/>}
  </Show>
  <Show ov="pieces">
   {step>=4&&<><LanePoly lesson={LESSON} lane={[0,3]} id="R1" timeline="cut" position={v.cut_position as number} pts={(v,s)=>pieceAt(v,1,s)} path={(v,a,b,p)=>pieceAt(v,1,a+(b-a)*p)} role="vecA" hatched={v=>derive(v).R1<0} label={labels?v=>'a·DB = '+fmt(derive(v).R1):undefined}/>
    <LanePoly lesson={LESSON} lane={[0,0]} id="R2" timeline="cut" position={v.cut_position as number} pts={(v,s)=>pieceAt(v,2,s)} path={(v,a,b,p)=>pieceAt(v,2,a+(b-a)*p)} role="vecB" hatched={v=>derive(v).R2<0} label={labels?v=>'b·EA = '+fmt(derive(v).R2):undefined}/></>}
   {step>=5&&!z.degenerate&&<>
    <LanePoly lesson={LESSON} lane={[1,0]} id="R1c" pts={v=>pieceAt(v,1,8)} role="vecC" fill={0} edge="dashed" hatched={v=>derive(v).R1c<0} label={labels&&step<8?v=>'c·FB = '+fmt(derive(v).R1c):undefined}/>
    <LanePoly lesson={LESSON} lane={[1,1]} id="R2c" pts={v=>pieceAt(v,2,9)} role="vecC" fill={0} edge="dashed" hatched={v=>derive(v).R2c<0} label={labels&&step<9?v=>'c·AF = '+fmt(derive(v).R2c):undefined}/>
   </>}
  </Show>
  <Show ov="altitude">{step>=5&&!z.degenerate&&<>
   <Segment lesson={LESSON} id="drop_c" a={z.C} b={z.F} dashed role="muted"/>
   <Segment lesson={LESSON} id="drop_c" a={z.F} b={add(z.F,scale(z.m,z.c))} dashed role="muted"/>
   <Point lesson={LESSON} id="F" at={z.F} role="muted" size={4}/><Text id="F" at={z.F} text="F"/>
   <RightAngle lesson={LESSON} id="rtF" at={z.F} u={sub(z.C,z.F)} v={z.w} role="muted"/>
  </>}</Show>
 </>;
}
function RowEmphasis({scene}:{scene:Scene}){
 const reg=useStage(),v=useVals(LESSON),active=scene===1?v.th===90:scene===3&&Math.abs(solveIK(v).cosraw)>1;
 useEffect(()=>{
  const row=reg.el?.closest('.lab-vp-card')?.querySelector<HTMLElement>('.lab-np-row[data-id="'+(scene===1?'corr':'cosraw')+'"]');
  if(!row)return;
  row.style.outline=active?'2px solid var(--viz-hot)':'';
  return()=>{row.style.outline=''};
 },[reg,scene,active]);
 return null;
}
function LedgerScene(){
 const shadow=useOverlay('shadows'),p=useHot(LESSON,'p'),corr=useHot(LESSON,'corr');
 return <><Triangle/>{(shadow||p||corr)&&<Shadow/>}</>;
}
/** Fade every material and its registered DOM label together. At zero the actual
 * subtree unmounts, removing hit targets and labels, while values/formulas remain. */
function ProofFade({alpha,...p}:PolyProps & {alpha:number;lane:readonly[0|1|2,number]}){
 const group=useRef<Group>(null),reg=useStage(),bases=useRef(new WeakMap<Material,{base:number;last:number}>());
 useFrame(()=>{
  group.current?.traverse(o=>{
   const value=(o as Mesh).material;if(!value)return;
   for(const material of Array.isArray(value)?value:[value]){
    const saved=bases.current.get(material),base=saved&&material.opacity===saved.last?saved.base:material.opacity;
    material.opacity=base*alpha;bases.current.set(material,{base,last:material.opacity});
   }
  });
 });
 useFrame(()=>{
  for(const s of reg.labels.list())if(s.hot===p.id){const el=reg.labelEls.get(s.id);if(el)el.style.opacity=String(alpha)}
  reg.el?.querySelectorAll<SVGPathElement>('[data-lc-leader="'+p.id+'"]').forEach(el=>el.style.opacity=String(alpha));
 },3);
 return alpha>0?<group ref={group}><LanePoly {...p}/></group>:null;
}
function Proof(){
 const vals=useVals(LESSON),fade=vals.proof_fade_position as number;
 const z=derive(vals),step=useStep(LESSON,'proof'),labels=useOverlay('labels');
 return <><Triangle/>
  <Show ov="shadows">{step>=1&&<Shadow solid names={step>=2} lanes/>}
   {step>=2&&<Text lane={[1,1]} id="rise" at={midpoint(z.D,z.A)} text={'b sin θ = '+fmt(z.h)} offset={[-34,0]}/>}
  </Show>
  <Show ov="rest">{step>=3&&<>
   <Segment lesson={LESSON} id="rest" a={add(z.D,[0,-.25])} b={add(z.B,[0,-.25])} role="vecA"/>
   <Text lane={[2,3]} id="rest" at={add(midpoint(z.D,z.B),[0,-.25])} text={'a − b cos θ = '+fmt(z.DB)} offset={[0,14]}/>
  </>}</Show>
  <Show ov="tiles">{step>=4&&<>
   {step<8&&<LanePoly lane={[2,1]} lesson={LESSON} id="rest2" timeline="proof" pts={v=>{const d=derive(v);return square(d.D,d.B)}} role="vecA" fill={step>=6?0:.16} label={labels?v=>(step===4?'DB² = ':'(a − b cos θ)² = ')+fmt(derive(v).rest2):undefined}/>}
   {step>=4&&<ProofFade lane={[2,0]} alpha={step>=7?1-fade:1} lesson={LESSON} id={step>=7?'bsin2':'rise2'} timeline="proof" pts={v=>{const d=derive(v);return square(d.A,d.D)}} role="vecB" fill={.16} label={labels?v=>(step===4?'AD² = ':'(b sin θ)² = ')+fmt(derive(v).rise2):undefined}/>}
   {!z.degenerate&&<LanePoly lane={[1,0]} lesson={LESSON} id="c2" pts={v=>{const d=derive(v);return square(d.B,d.A)}} role="vecC" fill={.16} label={labels?v=>'c² = '+fmt(derive(v).c2):undefined}/>}
   {step>=6&&<LanePoly lane={[0,0]} lesson={LESSON} id="a2" pts={v=>{const d=derive(v);return square(d.C,d.B)}} role="vecA" label={labels?v=>'a² = '+fmt(derive(v).a2):undefined}/>}
   {step>=7&&<ProofFade lane={[2,2]} alpha={1-fade} lesson={LESSON} id="bcos2" pts={v=>{const d=derive(v);return square(d.C,d.D)}} role="vecB" label={labels?v=>'b² cos² θ = '+fmt(derive(v).bcos2):undefined}/>}
   {step>=7&&<ProofFade lane={[0,1]} alpha={fade} lesson={LESSON} id="b2" pts={v=>{const d=derive(v);return square(d.A,d.C)}} role="vecB" label={labels?v=>'b² = '+fmt(derive(v).b2):undefined}/>}
  </>}</Show>
 </>;
}
function Bone({id,a,b,role}:{id:string;a:V;b:V;role:SceneRole}){
 const pal=usePalette(),own=useHot(LESSON,id),fraction=useHot(LESSON,'cosphi'),hover=useHoverable(LESSON,id,HOVER_RANK.beam);
 return <group {...hover}><Stroke points={[to3(a),to3(b)]} color={pal[role]} width={6} hot={own||fraction}/></group>;
}
function IK(){
 const z=solveIK(useVals(LESSON)),locale=useLocale(),hot=useStore(lessonStore(LESSON),s=>s.hot),distance=useOverlay('distance');
 const w=(en:string,ru:string)=>words(locale,en,ru);
 return <>
  <Show ov="reach"><Circle lesson={LESSON} id={hot.has('clamp')?'clamp':hot.has('reach')?'reach':'reach_max'} center={z.S} r={z.reachMax} role="muted" dashPx={[7,5]}/>
   <Circle lesson={LESSON} id={hot.has('reach')?'reach':'reach_min'} center={z.S} r={z.reachMin} role="muted" dashPx={[7,5]}/>
   <Text id="reach_max" at={[0,-z.reachMax]} text={w('max reach','макс. вылет')} offset={[0,14]}/>
   {z.reachMin>1e-9&&<Text id="reach_min" at={[0,-z.reachMin]} text={w('min reach','мин. вылет')} offset={[0,14]}/>}
  </Show>
  <Bone id="l1" a={z.S} b={z.E} role="vecA"/><Bone id="l2" a={z.E} b={z.W} role="vecB"/>
  <Point lesson={LESSON} id="S" at={z.S} role="ink" size={10}/>
  <Point lesson={LESSON} id="E" at={z.E} role="ink" size={12}/>
  <Point lesson={LESSON} id="W" at={z.W} role="vecB" size={8}/>
  <Point lesson={LESSON} id="hand" at="ik_hand" role="vecC" size={12} label={w('target','цель')}/>
  <Text id="S" at={z.S} text={w('shoulder','плечо')} offset={[-20,16]}/>
  <Text id="E" at={z.E} text={w('elbow','локоть')} offset={[12,-15]}/>
  <Text id="hand" at={z.T} text={w('target','цель')} offset={[20,16]}/>
  <Text id="l1" at={midpoint(z.S,z.E)} text={'L₁ = '+fmt(z.l1)} offset={[-24,0]} role="vecA"/>
  <Text id="l2" at={midpoint(z.E,z.W)} text={'L₂ = '+fmt(z.l2)} offset={[24,0]} role="vecB"/>
  {(z.outside||z.inside)&&<><Segment lesson={LESSON} id={hot.has('clamp')?'clamp':'gap'} a={z.W} b={z.T} dashed role="axisX"/><Text id="gap" at={midpoint(z.W,z.T)} text={w('unreachable','недостижимо')} role="axisX"/></>}
  {(distance||hot.has('d')||hot.has('cosphi'))&&<><Segment lesson={LESSON} id={hot.has('cosphi')?'cosphi':'d'} a={z.S} b={z.T} dashed role="vecC"/><Text id="d" at={midpoint(z.S,z.T)} text={'d = '+fmt(z.d)} offset={[0,20]}/></>}
  <Show ov="angle"><AngleArc lesson={LESSON} id="phi" at={z.E} from={sub(z.S,z.E)} to={sub(z.W,z.E)} r={.5} label={useOverlay('labels')?'φ = '+fmtAngle(z.phi)+'°':undefined}/></Show>
  <Show ov="right"><RightAngle lesson={LESSON} id="rt" at={z.E} u={sub(z.S,z.E)} v={sub(z.W,z.E)} show={Math.abs(z.cosc)<1e-12}/></Show>
 </>;
}
export function LawScene({scene}:{scene:Scene}){
 const locale=useLocale(),cut=useStep(LESSON,'cut'),w=(en:string,ru:string)=>words(locale,en,ru);
 const names=[w('Missing rectangles','Пропавшие прямоугольники'),w('Ledger','Баланс площадей'),w('Proof','Доказательство'),w('Two-bone IK','IK двух костей')];
 const overlay=(id:string,en:string,ru:string,on=true)=>({id,label:w(en,ru),on});
 const overlays=[overlay('grid','Grid','Сетка'),overlay('labels','Labels','Подписи'),overlay('angle','Angle','Угол'),...(scene!==3?[overlay('right','Right angle','Прямой угол')]:[]),
  ...(scene===0?[overlay('shadows','Shadows','Тени'),overlay('squares','Squares','Квадраты'),overlay('strips','Strips','Полосы'),overlay('pieces','Pieces','Остатки'),overlay('altitude','Altitude','Высота')]:
   scene===1?[overlay('shadows','Shadow','Тень',false)]:
   scene===2?[overlay('shadows','Shadow and height','Тень и высота'),overlay('tiles','Tiles','Квадраты'),overlay('rest','Remainder','Остаток')]:
   [overlay('reach','Reach rings','Досягаемость'),overlay('distance','Distance','Расстояние',false),overlay('right','Right angle','Прямой угол')])];
 const status:StatusSpec={hints:{
  A:{mouse:w('Drag A · b and θ follow · arrows nudge','Перетащите A · b и θ меняются · стрелки — шаг'),touch:w('Drag A · b and θ follow','Перетащите A · меняются b и θ')},
  B:w('Drag B along a · sets a','Перетащите B вдоль a · меняется a'),
  rest:w('DB = a − b cos θ · negative iff b cos θ > a','DB = a − b cos θ · отрицателен при b cos θ > a'),
  strip_a:v=>'a × (b cos θ) = '+fmt(derive(v).strip_a)+' · '+w('hatched = negative','штриховка = минус'),
  strip_b:v=>'b × (a cos θ) = '+fmt(derive(v).strip_b)+' · '+w('hatched = negative','штриховка = минус'),
  R1:v=>'a·DB = '+fmt(derive(v).R1),R2:v=>'b·EA = '+fmt(derive(v).R2),
  hand:{mouse:w('Drag target · arrows 0.1 · Shift 1.0','Цель: перетаскивание · стрелки 0.1 · Shift 1.0'),touch:w('Drag the target','Перетащите цель')},
  E:w('Elbow · solved by the law of cosines','Локоть · решение по теореме косинусов')
 },state:scene===3?v=>{const z=solveIK(v);return z.zero?w('Target on shoulder: +x direction convention','Цель на плече: условное направление +x'):z.outside?w('Out of reach · cosine clamped to −1','Вне досягаемости · косинус ограничен до −1'):z.inside?w('Too close · cosine clamped to 1','Слишком близко · косинус ограничен до 1'):null}:undefined};
 return <ViewportCard lesson={LESSON} id={'LC-'+scene} name={'LC-'+scene+' · '+names[scene]} mode="2d" extent={EXTENTS[scene]} center={scene===3?[0,0]:[1.5,scene===0||scene===2?0:1]} rows={rowsFor(scene,locale,cut)} overlays={overlays} status={status} instruments angleKey={scene===3?undefined:'th'} formula={scene===3?undefined:<LawFormula scene={scene} compact/>} drawIn={scene===0}>
  <RowEmphasis scene={scene}/>
  <Show ov="grid"><Grid lesson={LESSON} extent={EXTENTS[scene]} step={1} axes={false} ticks={false}/></Show>
  {scene===0?<Missing/>:scene===1?<LedgerScene/>:scene===2?<Proof/>:<IK/>}
 </ViewportCard>;
}

/** A second View inspects the overview's public position buffer; no second placement
 * animation. Its own Stroke retains pixel widths at this View's resolution. */
function DetailGeometry({part,proof=false}:{part:DetailPart;proof?:boolean}){
 const vals=useVals(LESSON),step=useStep(LESSON,proof?'proof':'cut'),z=derive(vals),palette=usePalette(),hot=useHot(LESSON,part.id);
 const [sample,setSample]=useState<{geometry:BufferGeometry;points:V[];matrix:Matrix4;opacity:number}|null>(null);
 const invalidate=useThree(s=>s.invalidate),labels=useOverlay('labels'),signature=useRef('');
 const material=useMemo(()=>new MeshBasicMaterial({color:palette[part.role],side:DoubleSide,transparent:true,opacity:STYLE.fillA,depthWrite:false,polygonOffset:true,polygonOffsetFactor:1}),[palette,part.role]);
 useEffect(()=>()=>material.dispose(),[material]);
 useFrame(()=>{
  const mesh=sourceMesh(part.id);
  if(!mesh){if(signature.current!=='none'){signature.current='none';setSample(null)}return}
  const points=meshPoints(mesh),opacity=(mesh.material as Material).opacity,next=JSON.stringify([points,opacity]);
  if(next!==signature.current||sample?.geometry!==mesh.geometry){
   signature.current=next;setSample({geometry:mesh.geometry,points,matrix:mesh.matrixWorld.clone(),opacity});invalidate();
  }
 });
 const hover=useHoverable(LESSON,part.id,HOVER_RANK.fill),edgeGroup=useRef<Group>(null),reg=useStage();
 const alpha=proof?(part.id==='b2'?vals.proof_fade_position as number:['bcos2','bsin2'].includes(part.id)?1-(vals.proof_fade_position as number):1):1;
 const edgeBase=useRef(new WeakMap<Material,{base:number;last:number}>());
 useFrame(()=>{edgeGroup.current?.traverse(o=>{const m=(o as Mesh).material;if(!m)return;for(const mat of Array.isArray(m)?m:[m]){const old=edgeBase.current.get(mat),base=old&&mat.opacity===old.last?old.base:mat.opacity;mat.opacity=base*alpha;edgeBase.current.set(mat,{base,last:mat.opacity})}})});
 useFrame(()=>{for(const s of reg.labels.list())if(s.hot===part.id){const el=reg.labelEls.get(s.id);if(el)el.style.opacity=String(alpha)}},3);
 const required:Record<string,number>=proof?{c2:4,rest2:4,rise2:4,a2:6,bcos2:7,bsin2:7,b2:7}:{strip_a:2,strip_b:3,R1:4,R2:4,R1c:5,R2c:5,p:1,q:3};
 if(step<(required[part.id]??0)||(proof&&((part.id==='rest2'&&step>=8)||(part.id==='rise2'&&step>=7))))return null;
 if(part.id==='p'||part.id==='q')return <Shadow side={part.id==='p'?1:2}/>;
 if(!sample||alpha<=0)return null;
 material.opacity=sample.opacity;
 const center=sample.points.reduce<V>((a,b)=>add(a,scale(b,.25)),[0,0]);
 return <group {...hover}>
  <mesh geometry={sample.geometry} material={material} matrix={sample.matrix} matrixAutoUpdate={false} dispose={null}/>
  {z[part.key]<0&&<group matrix={sample.matrix} matrixAutoUpdate={false} dispose={null}><Hatch geometry={sample.geometry} role={part.role}/></group>}
  <group ref={edgeGroup}><Stroke points={[...sample.points,sample.points[0]].map(to3)} color={palette[part.role]} hot={hot} width={STYLE.edge}/></group>
  {labels&&<Text id={part.id} at={center} text={part.label+' = '+fmt(z[part.key])} role={part.role} offset={[0,0]}/>}
 </group>;
}
export function LawDetail({proof=false}:{proof?:boolean}){
 const locale=useLocale(),v=useVals(LESSON),step=useStep(LESSON,'proof'),parts=proof?PROOF_DETAILS:DETAILS,key=proof?'proof_detail':'detail';
 const part=parts[(v[key+'_part'] as number)??0]??parts[0],fit=(v[key+'_fit'] as number)??0;
 // Selection and explicit Fit capture framing. Ordinary value writes and animation
 // frames cannot move the camera under a reader's pointer.
 const [frame,setFrame]=useState<{center:V;extent:number;serial:number}>(()=>{
  const z=derive(v),pts=square(z.B,z.A),xs=pts.map(p=>p[0]),ys=pts.map(p=>p[1]);
  return {center:[(Math.min(...xs)+Math.max(...xs))/2,(Math.min(...ys)+Math.max(...ys))/2],extent:Math.max(Math.max(...xs)-Math.min(...xs),Math.max(...ys)-Math.min(...ys))*.7+.25,serial:0};
 });
 useEffect(()=>{
  if(proof&&step<4)return;
  const request=requestAnimationFrame(()=>{
  const mesh=sourceMesh(part.id),z=derive(v);
  const pts=part.id==='p'?[z.C,z.A,z.D]:part.id==='q'?[z.C,z.B,z.E]:mesh?meshPoints(mesh):null;
  if(!pts)return;
  const xs=pts.map(p=>p[0]),ys=pts.map(p=>p[1]);
  setFrame(old=>({center:[(Math.min(...xs)+Math.max(...xs))/2,(Math.min(...ys)+Math.max(...ys))/2],extent:Math.max(.6,Math.max(Math.max(...xs)-Math.min(...xs),Math.max(...ys)-Math.min(...ys))*.7+.25),serial:old.serial+1}));
  });
  return()=>cancelAnimationFrame(request);
 },[part.id,fit,proof&&step>=4]);
 return <ViewportCard key={frame.serial} lesson={LESSON} id={proof?'LC-2-detail':'LC-0-detail'} name={(proof?'LC-2':'LC-0')+words(locale,' · Detail: ',' · Фрагмент: ')+part.label} mode="2d" extent={frame.extent} center={frame.center}
  overlays={[{id:'labels',label:words(locale,'Labels','Подписи'),on:true}]}
  status={{idle:words(locale,'Same piece and timeline · Fit captures its current bounds','Тот же фрагмент и шаг · Вписать фиксирует текущие границы')}}>
  <DetailGeometry part={part} proof={proof}/>
 </ViewportCard>;
}
