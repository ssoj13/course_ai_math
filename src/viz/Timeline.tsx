import { useLayoutEffect, useRef } from 'react';
import { useStore } from 'zustand';
import { t } from '../i18n/core';
import { useLocale } from '../i18n/react';
import { lessonStore } from './store';
import { boundIndex } from './links';
import { plain, rich } from './rich';
/** `text` may carry the inline markup of rich.tsx (`**bold**`, `*em*`); it is rendered, never shown literally. */
export interface TimelineStep {id:string;text:string;phases?:number;focus?:string}
type Store=ReturnType<typeof lessonStore>;
type Guide={focus:string|undefined;order:number};
type Guides={items:Set<Guide>;active:Guide|null;clock:number;off:()=>void;restore:()=>void};
const guides=new WeakMap<Store,Guides>();
/** One empty-hot subscription per lesson. First mounted guide wins until a timeline changes step. */
function registerGuide(store:Store,focus:string|undefined){
 let group=guides.get(store);
 if(!group){
  const g:Guides={items:new Set(),active:null,clock:0,off:()=>{},restore:()=>{
   const id=g.active?.focus;if(id&&store.getState().hot.size===0)store.getState().setHot([id],'guide');
  }};
  g.off=store.subscribe((s,prev)=>{if(s.hot!==prev.hot&&s.hot.size===0)g.restore()});
  guides.set(store,g);group=g;
 }
 const g=group,entry:Guide={focus,order:++g.clock};g.items.add(entry);
 if(!g.active&&focus){g.active=entry;g.restore()}
 const replace=()=>{
  const {hot,hotSource}=store.getState(),owned=hotSource==='guide';
  if(owned||hot.size===0){
   const next=g.active?.focus;
   if(next){if(hot.size!==1||!hot.has(next))store.getState().setHot([next],'guide')}
   else if(owned)store.getState().setHot([]);
  }
 };
 return {
  update(next:string|undefined){
   entry.focus=next;entry.order=++g.clock;
   g.active=next?entry:[...g.items].filter(x=>x.focus).sort((a,b)=>b.order-a.order)[0]??null;
   replace();
  },
  dispose(){
   g.items.delete(entry);
   if(g.active===entry){g.active=[...g.items].filter(x=>x.focus).sort((a,b)=>b.order-a.order)[0]??null;replace()}
   if(!g.items.size){g.off();guides.delete(store)}
  },
 };
}
export function Timeline({lesson,id,steps}:{lesson:string;id:string;steps:readonly TimelineStep[]}):React.JSX.Element {
 if(!steps.length||new Set(steps.map(s=>s.id)).size!==steps.length)throw new Error('Timeline requires nonempty unique steps');
 for(const s of steps)if(s.phases!==undefined&&(!Number.isInteger(s.phases)||s.phases<1))throw new Error('Timeline phases must be positive integers');
 const store=lessonStore(lesson),raw=useStore(store,s=>s.step[id]??0),locale=useLocale(),i=boundIndex(raw,steps.length-1);
 const phaseKey=steps.map(s=>s.phases??1).join(',');
 const guide=useRef<ReturnType<typeof registerGuide>|null>(null),seen=useRef('');
 const focus=steps[i].focus,selection=JSON.stringify([i,steps[i].id,focus]);
 useLayoutEffect(()=>{guide.current=registerGuide(store,focus);seen.current=selection;return()=>{guide.current?.dispose();guide.current=null}},[store,id]);
 useLayoutEffect(()=>{if(seen.current!==selection){seen.current=selection;guide.current?.update(focus)}},[selection]);
 useLayoutEffect(()=>{store.getState().boundTimeline(id,steps.length,steps.map(s=>s.phases??1))},[store,id,steps.length,phaseKey]);
 const write=(n:number)=>store.getState().setStep(id,boundIndex(n,steps.length-1));
 return <div className="lab-timeline not-content" data-timeline={id}>
  <div className="lab-timeline-controls">
   <button type="button" aria-disabled={i===0} onClick={()=>write(i-1)}>{t(locale,'linked.back')}</button>
   <input type="range" min={0} max={steps.length-1} step={1} value={i} aria-label={t(locale,'linked.step')} aria-valuetext={plain(steps[i].text)} onChange={e=>write(Number(e.currentTarget.value))}/>
   <button type="button" aria-disabled={i===steps.length-1} onClick={()=>write(i+1)}>{t(locale,'linked.next')}</button>
  </div>
  <p aria-live="polite" aria-atomic="true">{i+1} / {steps.length} · {rich(steps[i].text)}</p>
 </div>;
}
