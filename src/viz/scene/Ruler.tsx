import { add, norm, rightOf, rulerTicks, scale, sub, to3, type V } from '../geom';
import { usePalette } from '../palette';
import { useLabel, usePos } from './handles';
import { SLine, useWorldPx } from './view';
function TickText({lesson,text,at}:{lesson:string;text:string;at:V}){useLabel({lesson,text,at:()=>at,small:true,role:'muted'});return null;}
export interface RulerProps{lesson:string;a:V|string;b:V|string;normal?:V}
export function Ruler(p:RulerProps){
 const a=usePos(p.lesson,p.a), b=usePos(p.lesson,p.b),pal=usePalette();
 const d=sub(b,a),len=norm(d),wpp=useWorldPx(1,a),n=rightOf(a,b,p.normal);
 if(!len||!wpp||!n)return null;
 const ticks=rulerTicks(len,1/wpp);
 return <group>{ticks.map(t=>{
  const pt=add(a,scale(d,t.value/len)),end=add(pt,scale(n,wpp*(t.major?8:4)));
  return <group key={t.value}><SLine points={[to3(pt),to3(end)]} color={pal.muted} lineWidth={1} transparent opacity={.7} depthWrite={false}/>{t.label&&<TickText lesson={p.lesson} text={String(t.value)} at={add(pt,scale(n,wpp*17))}/>}</group>;
 })}</group>;
}