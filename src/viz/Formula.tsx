/** Linked KaTeX, with commit-time focus preservation across markup replacement. */
import { Component, createRef, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useStore } from 'zustand';
import { hotOff, hotOn, lessonStore, type Val } from './store';
import { renderLinked, showVal, texText, type Derived } from './tex';
import { keyStep, scrubValue, type ScrubSpec } from './scrub';
import { startScrubDrag } from './useScrub';
import { markClass, type Marks } from './marks';
import { useTip, type Tip } from './scene/NPanel';

export interface FormulaProps {
 lesson: string; tex: string; texNarrow?: string;
 scrub?: Record<string, ScrubSpec>;
 fmt?: Record<string,(v:number)=>string>; derived?: Derived;
 marks?: Marks; tips?: Record<string,Tip>;
 block?: boolean; label: string;
}
interface MarkupProps {
 html:string; vals:Readonly<Record<string,Val>>; hot:ReadonlySet<string>; config:FormulaProps;
}
type Focus = {kind:'s'|'t';key:string;index:number}|null;
/** Snapshot runs before the DOM changes, never during React render or after focus has been lost. */
class Markup extends Component<MarkupProps,object,Focus> {
 root=createRef<HTMLSpanElement>();
 getSnapshotBeforeUpdate():Focus {
  const el=this.root.current,a=document.activeElement as HTMLElement|null;
  if(!a||!el?.contains(a))return null;
  const kind=a.dataset.s?'s':'t',key=a.dataset[kind];
  if(!key)return null;
  const peers=[...el.querySelectorAll<HTMLElement>('[data-'+kind+']')].filter(n=>n.dataset[kind]===key);
  return {kind,key,index:peers.indexOf(a)};
 }
 decorate() {
  const el=this.root.current; if(!el)return;
  const {config:p,vals,hot}=this.props;
  // Explicit authored mark wrappers are independent of semantic term IDs and KaTeX internals.
  el.querySelectorAll<HTMLElement>('[data-t],[data-m]').forEach(h=>{
   const k=h.dataset.m??h.dataset.t!,m=p.marks?.[k];
   h.classList.toggle('is-hot',Boolean(h.dataset.t&&hot.has(h.dataset.t)));
   h.classList.toggle('is-struck',Boolean(m?.struck));
   h.classList.toggle('is-dim',Boolean(m?.dim));
   h.classList.toggle('is-hidden',Boolean(m?.hidden));
  });
  el.querySelectorAll<HTMLElement>('[data-t],[data-m]').forEach(h=>{
   const hidden=Boolean(h.closest('.is-hidden'));
   h.setAttribute('aria-hidden',String(hidden));
   if(h.dataset.t)h.tabIndex=hidden?-1:0;
  });
  el.querySelectorAll<HTMLElement>('[data-s]').forEach(h=>{
   const k=h.dataset.s!,spec=p.scrub?.[k],v=vals[k];
   const hidden=Boolean(h.closest('.is-hidden'));
   if(!spec||typeof v!=='number'){
    h.removeAttribute('role');h.removeAttribute('tabindex');
    for(const name of ['min','max','now','text'])h.removeAttribute('aria-value'+name);
    h.style.removeProperty('min-width');return;
   }
   h.tabIndex=hidden?-1:0;h.setAttribute('role','slider');h.setAttribute('aria-label',k);
   h.setAttribute('aria-valuemin',String(spec.min));h.setAttribute('aria-valuemax',String(spec.max));
   h.setAttribute('aria-valuenow',String(v));h.setAttribute('aria-valuetext',texText(showVal(k,v,p.fmt)));
   const chars=Math.max(...[spec.min,spec.max,0].map(n=>p.fmt?.[k]?texText(showVal(k,n,p.fmt)).length:n.toFixed(2).length));
   h.style.minWidth=(chars*.6)+'em';h.style.display='inline-block';h.style.textAlign='right';
  });
 }
 componentDidMount(){this.decorate()}
 componentDidUpdate(_prev:MarkupProps,_state:object,focus:Focus){
  this.decorate();
  if(focus){
   const peers=[...this.root.current!.querySelectorAll<HTMLElement>('[data-'+focus.kind+']')].filter(n=>n.dataset[focus.kind]===focus.key);
   const target=peers[focus.index]??peers[0];
   if(target&&!target.closest('.is-hidden'))target.focus({preventScroll:true});
  }
 }
 render(){
  const {config:p,vals}=this.props;
  const html=this.props.html.replace(/(<span class="katex-html") aria-hidden="true"/g,'$1').replace(/<span([^>]*data-s="([^"]+)"[^>]*)>/g,(tag,attrs,k)=>{
   const spec=p.scrub?.[k];if(!spec||typeof vals[k]!=='number')return tag;
   const chars=Math.max(...[spec.min,spec.max,0].map(n=>p.fmt?.[k]?texText(showVal(k,n,p.fmt)).length:n.toFixed(2).length));
   return '<span'+attrs+' style="min-width:'+(chars*.6)+'em;display:inline-block;text-align:right">';
  }).replace(/<span([^>]*data-(?:t|m)="([^"]+)"[^>]*)>/g,(_tag,attrs,k)=>{
   const classes=markClass(p.marks?.[k]);
   return '<span'+attrs.replace(/class="([^"]*)"/,(_s:string,c:string)=>'class="'+c+classes+'"')+'>';
  });
  return <span ref={this.root} dangerouslySetInnerHTML={{__html:html}}/>;
 }
}
export function Formula(p:FormulaProps):React.JSX.Element {
 const store=lessonStore(p.lesson),vals=useStore(store,s=>s.vals),hot=useStore(store,s=>s.hot);
 const root=useRef<HTMLDivElement>(null),[narrow,setNarrow]=useState(false);
 const tip=useTip();
 useLayoutEffect(()=>{
  if(!p.texNarrow||!root.current)return;
  const el=root.current,update=()=>setNarrow(el.getBoundingClientRect().width<420);
  update();const ro=new ResizeObserver(update);ro.observe(el);return ()=>ro.disconnect();
 },[p.texNarrow]);
 // Rendering uses current props: formatter/derived-only changes must not leave stale numbers.
 const tex=narrow&&p.texNarrow?p.texNarrow:p.tex;
 const derivedKey=JSON.stringify(Object.entries(p.derived??{}).map(([k,f])=>[k,f(vals)]));
 const formatKey=JSON.stringify(Object.entries(p.fmt??{}).map(([k,f])=>[k,typeof (p.derived?.[k]?.(vals)??vals[k])==='number'?f((p.derived?.[k]?.(vals)??vals[k]) as number):null]));
 const html=useMemo(()=>renderLinked(tex,vals,p.fmt,p.block,p.derived),[tex,vals,p.block,derivedKey,formatKey]);
 const term=(e:React.SyntheticEvent)=>(e.target as HTMLElement).closest<HTMLElement>('[data-t]')?.dataset.t;
 const key=(e:React.SyntheticEvent)=>(e.target as HTMLElement).closest<HTMLElement>('[data-s]')?.dataset.s;
 return <div ref={root} role="group" className={'lab-formula not-content'+(p.block?' is-block':'')} aria-label={p.label}
  onPointerOver={e=>{const id=term(e);if(id){hotOn(p.lesson,id);tip.bind(p.tips?.[id]).onPointerEnter?.(e)}}}
  onPointerOut={e=>{const id=term(e);const to=(e.relatedTarget as HTMLElement|null)?.closest?.<HTMLElement>('[data-t]')?.dataset.t;if(id&&to!==id){hotOff(p.lesson,id);tip.bind(p.tips?.[id]).onPointerLeave?.(e)}}}
  onFocus={e=>{const id=term(e);if(id)hotOn(p.lesson,id)}}
  onBlur={e=>{if(!e.currentTarget.contains(e.relatedTarget as Node|null)){const id=term(e);if(id)hotOff(p.lesson,id)}}}
  onPointerDown={e=>{
   const k=key(e),spec=k?p.scrub?.[k]:undefined;if(!k||!spec)return;
   const v0=store.getState().vals[k] as number;
   startScrubDrag(e,e.currentTarget,{get:()=>store.getState().vals[k] as number,set:v=>store.getState().set(k,v),checkpoint:()=>store.getState().checkpoint([k])},dx=>scrubValue(v0,dx,spec));
  }}
  onKeyDown={e=>{
   const k=key(e),spec=k?p.scrub?.[k]:undefined;if(!k||!spec)return;
   const v=keyStep(store.getState().vals[k] as number,e.key,spec);if(v===null)return;
   e.preventDefault();store.getState().set(k,v,'ease');
  }}>
  <span className="lab-formula-stack">
   <Markup html={html} vals={vals} hot={hot} config={p}/>
   {p.texNarrow&&<span className="lab-formula-reserve" aria-hidden="true" dangerouslySetInnerHTML={{__html:renderLinked(narrow?p.tex:p.texNarrow,vals,p.fmt,p.block,p.derived)}}/>}
  </span>{tip.node}
 </div>;
}
