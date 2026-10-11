import { useLayoutEffect } from 'react';
import { useStore } from 'zustand';
import { t } from '../i18n/core';
import { useLocale } from '../i18n/react';
import { lessonStore } from './store';
import { boundIndex } from './links';
export interface TimelineStep {id:string;text:string;phases?:number}
export function Timeline({lesson,id,steps}:{lesson:string;id:string;steps:readonly TimelineStep[]}):React.JSX.Element {
 if(!steps.length||new Set(steps.map(s=>s.id)).size!==steps.length)throw new Error('Timeline requires nonempty unique steps');
 for(const s of steps)if(s.phases!==undefined&&(!Number.isInteger(s.phases)||s.phases<1))throw new Error('Timeline phases must be positive integers');
 const store=lessonStore(lesson),raw=useStore(store,s=>s.step[id]??0),locale=useLocale(),i=boundIndex(raw,steps.length-1);
 const phaseKey=steps.map(s=>s.phases??1).join(',');
 useLayoutEffect(()=>{store.getState().boundTimeline(id,steps.length,steps.map(s=>s.phases??1))},[store,id,steps.length,phaseKey]);
 const write=(n:number)=>store.getState().setStep(id,boundIndex(n,steps.length-1));
 return <div className="lab-timeline not-content" data-timeline={id}>
  <div className="lab-timeline-controls">
   <button type="button" aria-disabled={i===0} onClick={()=>write(i-1)}>{t(locale,'linked.back')}</button>
   <input type="range" min={0} max={steps.length-1} step={1} value={i} aria-label={t(locale,'linked.step')} aria-valuetext={steps[i].text} onChange={e=>write(Number(e.currentTarget.value))}/>
   <button type="button" aria-disabled={i===steps.length-1} onClick={()=>write(i+1)}>{t(locale,'linked.next')}</button>
  </div>
  <p aria-live="polite" aria-atomic="true">{i+1} / {steps.length} · {steps[i].text}</p>
 </div>;
}
