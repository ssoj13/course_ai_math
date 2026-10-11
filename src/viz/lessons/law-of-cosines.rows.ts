import type { Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { derive, solveIK, MAGNETS, fmt, fmtCos, fmtAngle } from './law-of-cosines.math';
export const words=(locale:Locale,en:string,ru:string)=>locale==='ru'?ru:en;
export const SCRUB = {a:{min:.5,max:4,step:.1},b:{min:.5,max:4,step:.1},th:{min:0,max:180,step:1}};
export function rowsFor(scene:0|1|2|3,locale:Locale,cut=0):readonly Row[] {
 const w=(en:string,ru:string)=>words(locale,en,ru);
 const inputs:Row[]=['a','b','th'].map(id=>({id,label:id==='th'?'θ':id,kind:'num',key:id,
  spec:SCRUB[id as keyof typeof SCRUB],snap:id==='th'?MAGNETS:.5,
  fmt:id==='th'?x=>fmtAngle(x)+'°':fmt,tip:{text:id==='th'?w('Angle between a and b','Угол между a и b'):w('Side length','Длина стороны')}}));
 const ro=(id:keyof ReturnType<typeof derive>,label:string,start=0,timeline='cut',group=w('AREAS','ПЛОЩАДИ')):Row=>({
  id,label,kind:'ro',chars:7,group,value:v=>derive(v)[id] as number,fmt,
  visible:(_v,s)=>(s[timeline]??0)>=start,tip:{text:label}});
 if(scene===0)return [...inputs,{...ro('c','c'),group:undefined},ro('a2','a²'),ro('b2','b²'),ro('strip_a','a·p',2),ro('strip_b','b·q',3),
  ro('R1','a·DB',4),ro('R2','b·EA',4),{...ro('R1c','c·FB',5),chars:13,fmt:x=>fmt(x)+(cut>=8?' = R1':'')},ro('R2c','c·AF',5),ro('c2','c²'),ro('ab','a·b',11)];
 if(scene===1)return [...inputs,{...ro('cos','cos θ'),group:undefined,fmt:fmtCos},{...ro('c','c'),group:undefined},ro('a2','a²'),ro('b2','b²'),ro('corr','−2ab cos θ'),ro('c2','c²')];
 if(scene===2)return [...inputs,...([['p','b cos θ',2],['rise','b sin θ',2],['rest','a − b cos θ',3],['rest2','DB²',4],['rise2','AD²',4],['c2','c²',4]] as const).map(([id,label,start])=>ro(id,label,start,'proof',w('PIECES','ЧАСТИ')))];
 const solve=(id:'d'|'cosraw'|'cosc'|'phi',label:string):Row=>({id,label,kind:'ro',chars:8,group:w('SOLVE','РЕШЕНИЕ'),value:v=>solveIK(v)[id],fmt:id.startsWith('cos')?fmtCos:id==='phi'?x=>fmtAngle(x)+'°':fmt,tip:{text:label}});
 return [
  {id:'ik_hand',label:w('Target','Цель'),kind:'vec',key:'ik_hand',dims:2,spec:{min:-6.5,max:6.5,step:.1}},
  {id:'ik_l1',label:'L₁',kind:'num',key:'ik_l1',spec:{min:.5,max:3,step:.1}},
  {id:'ik_l2',label:'L₂',kind:'num',key:'ik_l2',spec:{min:.5,max:3,step:.1}},
  {id:'ik_flip',label:w('Elbow down','Локоть вниз'),kind:'bool',key:'ik_flip'},
  solve('d','d'),solve('cosraw',w('cos φ raw','cos φ исходный')),solve('cosc',w('Clamped','С ограничением')),solve('phi','φ'),
  {id:'reach',label:w('Reach','Досягаемость'),kind:'ro',chars:13,group:w('SOLVE','РЕШЕНИЕ'),value:v=>[solveIK(v).reachMin,solveIK(v).reachMax],fmt}
 ];
}
