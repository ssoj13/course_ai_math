import { add, dimensionLayout, norm, scale, sub, to3, type V } from '../geom';
import { usePalette } from '../palette';
import { useHot } from '../store';
import { HOVER_RANK, useHoverable, useLabel, usePos } from './handles';
import { DoubleSide } from 'three';
import { SLine, useDynGeo, writeGeo, useWorldPx } from './view';
export function Dimension({lesson,id,a:aa,b:bb,normal}:{lesson:string;id:string;a:V|string;b:V|string;normal?:V}){
 const a=usePos(lesson,aa),b=usePos(lesson,bb),wpp=useWorldPx(1,a)??.02,p=dimensionLayout(a,b,wpp,normal);
 const pal=usePalette(),hot=useHot(lesson,id),hover=useHoverable(lesson,id,HOVER_RANK.guide);
 const len=norm(sub(b,a)),unit=len?scale(sub(b,a),1/len):a.map(()=>0),tick=scale(p.out,3/14);
 useLabel({lesson,text:len.toFixed(2),hot:id,at:()=>p.mid,offset:[0,12],small:true});
 const arrow=(pt:V,sign:number)=>[to3(add(add(pt,scale(unit,sign*5*wpp)),tick)),to3(pt),to3(add(add(pt,scale(unit,sign*5*wpp)),scale(tick,-1)))];
 const heads=useDynGeo(6,[0,1,2,3,4,5]);
 writeGeo(heads,[...arrow(p.a,1),...arrow(p.b,-1)]);
 return <group {...hover}><mesh geometry={heads}><meshBasicMaterial color={hot?pal.hot:pal.ink} side={DoubleSide} transparent depthWrite={false}/></mesh><SLine points={[to3(p.a),to3(p.b)]} color={hot?pal.hot:pal.ink} lineWidth={1} transparent depthWrite={false}/>
 {[p.a,p.b].map((pt,i)=><group key={i}><SLine points={[to3(add(pt,tick)),to3(sub(pt,tick))]} color={pal.ink} lineWidth={1}/></group>)}</group>;
}