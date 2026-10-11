import { t } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { useEffect, useId, useState, useSyncExternalStore } from 'react';
import { useStore } from 'zustand';
import { lessonStore } from '../store';
import type { Row } from './NPanel';
import { overlayOn, type StageReg } from './handles';
function GalaxyPanel({flip=false}:{flip?:boolean}){
 const locale = useLocale();
 const id=useId();
 const arm=(phase:number)=>Array.from({length:48},(_,i)=>{const r=i*.43,a=i*.22+phase;return (i?'L':'M')+(25+Math.cos(a)*r).toFixed(2)+','+(25+Math.sin(a)*r*.45).toFixed(2);}).join(' ');
 return <svg className="lab-hud-galaxy" viewBox="0 0 50 50" aria-label={t(locale, 'hud.starfield')}><defs><filter id={id} x="-40%" y="-60%" width="180%" height="220%"><feTurbulence type="fractalNoise" baseFrequency=".17" numOctaves="3" seed={flip?7:3} result="dust"/><feDisplacementMap in="SourceGraphic" in2="dust" scale="5"/><feGaussianBlur stdDeviation=".65"/></filter><radialGradient id={id+"g"}><stop stopColor="#c8c3e8"/><stop offset=".18" stopColor="#8079c2"/><stop offset=".6" stopColor="#344675" stopOpacity=".6"/><stop offset="1" stopColor="#162245" stopOpacity="0"/></radialGradient></defs><rect x="2" y="2" width="46" height="46" fill="var(--viz-bg-bottom)" stroke="currentColor"/><g transform={flip?'rotate(-25 25 25)':'rotate(25 25 25)'}><ellipse cx="25" cy="25" rx="23" ry="17" fill={"url(#"+id+"g)"}/><g filter={"url(#"+id+")"}>{[0,Math.PI].map(a=><path key={a} d={arm(a)} fill="none" stroke="var(--viz-vec-a)" strokeWidth="3" opacity=".45"/>)}</g><circle cx="25" cy="25" r="1" fill="var(--viz-star)"/></g></svg>;
}
export function Instruments({reg,lesson,angleKey,rows=[]}:{reg:StageReg;lesson:string;angleKey?:string;rows?:readonly Row[]}){
 const locale = useLocale();
 const vals=useStore(lessonStore(lesson),s=>s.vals);
 const hover=useSyncExternalStore(reg.hover.subscribe,reg.hover.get,reg.hover.get);
 const [view,setView]=useState({scale:0,azimuth:0,height:1,ticks:[] as number[][]});
 const [compact,setCompact]=useState(false);
 useEffect(()=>{const mq=matchMedia('(max-width:719.98px)'),update=()=>setCompact(mq.matches);update();mq.addEventListener('change',update);return()=>mq.removeEventListener('change',update);},[]);
 useEffect(()=>{
  const update=()=>{const el=reg.el;if(!el)return;setView({scale:Number(el.dataset.unitPx??0),azimuth:Number(el.dataset.azimuth??0),height:el.clientHeight,ticks:JSON.parse(el.dataset.yTicks??'[]') as number[][]});};
  const el=reg.el;if(!el)return;update();
  const observer=new MutationObserver(update);observer.observe(el,{attributes:true,attributeFilter:['data-unit-px','data-azimuth','data-y-ticks']});return ()=>observer.disconnect();
 },[reg]);
 const def=reg.card?.overlays.find(o=>o.id==='instruments');
 const on=!def||!reg.card||overlayOn(vals,reg.card.id,def);
 const theta=angleKey?Number(vals[angleKey]??0):view.azimuth;
 const term=hover.object??hover.field??(hover.handle?reg.list().find(d=>d.id===hover.handle)?.owner:null)??angleKey;
 const row=rows.find(r=>r.id===term);
 const value=row?.value ? row.value(vals) : term ? vals[row?.key??term] : undefined;
 const measured=value??(!term?view.azimuth:undefined);
 const rulerHeight=compact?80:view.height, rulerWidth=compact?44:54;
 const ticks=view.ticks.filter((_,i)=>i%Math.max(1,Math.ceil(view.ticks.length/(compact?3:12)))===0);
 const read=typeof measured==='number'?measured.toFixed(2):measured?.map(v=>v.toFixed(2)).join(', ')??'—';
 return <div className="lab-instruments" hidden={!on} aria-label={t(locale, 'hud.instruments')}>
 <svg className="lab-hud-ruler" viewBox={`0 0 ${rulerWidth} ${rulerHeight}`} preserveAspectRatio="none" aria-label={t(locale, 'hud.scale', { value: view.scale.toFixed(1) })}>
 <path d={"M8 4V"+(rulerHeight-4)} fill="none" stroke="currentColor"/>{ticks.map(([y,px])=>{const at=compact?8+px/view.height*60:px;return <g key={y}><path d={"M8 "+at+"h10"} stroke="currentColor"/><text x="21" y={at+3}>{y}</text></g>;})}</svg>
 <svg className="lab-hud-dial" viewBox="0 0 86 90" role="img" aria-label={t(locale, 'hud.degrees', { name: t(locale, angleKey ? 'hud.angle' : 'hud.azimuth'), value: theta.toFixed(1) })}>
 <circle cx="43" cy="39" r="28" fill="none" stroke="currentColor"/><circle cx="43" cy="39" r="22" fill="none" stroke="currentColor"/>
 {Array.from({length:12},(_,i)=><path key={i} d="M43 9v5" transform={`rotate(${i*30} 43 39)`} stroke="currentColor"/>)}
 <path d="M43 39V13" transform={`rotate(${theta} 43 39)`} stroke="var(--viz-vec-a)"/><text x="43" y="82" textAnchor="middle">{theta.toFixed(1)}°</text></svg>
 <div className="lab-hud-galaxies"><GalaxyPanel/><GalaxyPanel flip/></div>
 <div className="lab-hud-readout"><span>{row?.label??(angleKey&&term===angleKey?t(locale, 'hud.angle'):t(locale, 'hud.azimuth'))}</span><output>{read}</output><span>{row?.kind==='ro'?t(locale, 'hud.computed'):row?t(locale, 'hud.linked'):t(locale, 'hud.orientation')}</span></div>
 <svg className="lab-hud-planet" viewBox="0 0 82 82" aria-label={t(locale, 'hud.sphere')}>
 <defs><radialGradient id={`planet-${reg.card?.id}`} cx="30%" cy="25%"><stop stopColor="var(--viz-vec-a)" stopOpacity=".7"/><stop offset="1" stopColor="var(--viz-bg-bottom)"/></radialGradient></defs>
 <circle cx="41" cy="41" r="24" fill={`url(#planet-${reg.card?.id})`} stroke="currentColor"/>
 <ellipse cx="41" cy="41" rx="37" ry="13" transform={`rotate(${-theta} 41 41)`} fill="none" stroke="currentColor"/><circle cx="41" cy="41" r="35" fill="none" stroke="currentColor" opacity=".5"/>
 </svg></div>;
}
