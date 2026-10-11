import { useStore } from 'zustand';
import { t } from '../i18n/core';
import { useLocale } from '../i18n/react';
import { hotOn,hotOff,lessonStore } from './store';
import { rowsFor,type TableOp } from './table';
import { markClass,type Marks } from './marks';
export interface NumTableProps {lesson:string;rows:{label:string;key:string}[];op:TableOp;labels?:string[];marks?:Marks}
export function NumTable(p:NumTableProps):React.JSX.Element {
 const s=lessonStore(p.lesson),vals=useStore(s,s=>s.vals),hot=useStore(s,s=>s.hot),locale=useLocale();
 const vectors=p.rows.map(r=>{const v=vals[r.key];if(!Array.isArray(v))throw new Error('NumTable needs vector '+r.key);return v});
 if(!vectors.length||vectors.length>2)throw new Error('NumTable needs one or two rows');
 const r=rowsFor(p.op,vectors[0],vectors[1]);
 const cell=(id:string,v:number)=><td key={id} data-t={id} tabIndex={p.marks?.[id]?.hidden?-1:0} aria-hidden={p.marks?.[id]?.hidden||undefined}
  className={(hot.has(id)?'is-hot':'')+markClass(p.marks?.[id])}
  onPointerEnter={()=>hotOn(p.lesson,id)} onPointerLeave={()=>hotOff(p.lesson,id)}
  onFocus={()=>hotOn(p.lesson,id)} onBlur={()=>hotOff(p.lesson,id)}>{Number(v.toPrecision(8))}</td>;
 return <table className="lab-num-table not-content"><caption>{p.rows.map(r=>r.label).join(' · ')}</caption>
 <thead><tr>{r.cells.map((c,i)=><th key={c.t} scope="col">{p.labels?.[i]??String(i+1)}</th>)}{r.total!==undefined&&<th scope="col">{t(locale,'linked.total')}</th>}</tr></thead>
 <tbody><tr>{r.cells.map(c=>cell(c.t,c.v))}{r.total!==undefined&&cell('sum',r.total)}</tr></tbody></table>;
}
