import { useLayoutEffect, useRef } from 'react';
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

/** One cell of a NumGrid: its term id (shared with the scene, formula and N-panel) and its value. */
export interface GridCell { id: string; value: number }
/** A NumGrid column: `id` keys the cells of every row, `label` is its header. */
export interface GridCol { id: string; label: string }
/** A NumGrid row: header text (e.g. the component index, or Σ) and the cells by column id; a missing cell is blank. */
export interface GridRow { head: string; cells: Readonly<Record<string, GridCell | undefined>> }
export interface NumGridProps {
 lesson: string;
 caption: string;
 /** Header of the row-label column. */
 corner: string;
 /** The columns to show, in order (a lesson reveals columns by passing more of them). */
 cols: readonly GridCol[];
 rows: readonly GridRow[];
 marks?: Marks;
 fmt?: (v: number) => string;
 /** Column brought into view inside the table's own scroll box when it changes (a newly revealed column on a phone). */
 reveal?: string | null;
}

/**
 * Multi-column linked table (DP-1 "comp", DP-3 "comp3"): rows of values whose cells carry term ids, so hovering or
 * focusing a cell lights the same id in the scene, the formula and the N-panel, and a cell lights up when its id is hot
 * elsewhere. NumTable above is the one-row special case for a vector operation; this one shows any grid the lesson
 * computes, with marks (struck/hidden/dim) per cell id. The table scrolls horizontally inside its own box, never the
 * page; `reveal` scrolls that box (not the page) so a newly revealed column is visible.
 */
export function NumGrid(p: NumGridProps): React.JSX.Element {
 const s = lessonStore(p.lesson), hot = useStore(s, (st) => st.hot);
 const box = useRef<HTMLDivElement>(null);
 useLayoutEffect(() => {
  const el = box.current, th = p.reveal ? el?.querySelector<HTMLElement>('th[data-col="' + p.reveal + '"]') : null;
  if (!el || !th) return;
  // Only the table's own box scrolls: scrollIntoView would also move the page under the reader.
  const left = th.offsetLeft, right = left + th.offsetWidth;
  if (right > el.scrollLeft + el.clientWidth) el.scrollLeft = right - el.clientWidth;
  else if (left < el.scrollLeft) el.scrollLeft = left;
 }, [p.reveal]);
 const show = (v: number) => (p.fmt ? p.fmt(v) : String(Number(v.toPrecision(8))));
 const cell = (c: GridCell | undefined, key: string) => {
  if (!c) return <td key={key} />;
  if (!Number.isFinite(c.value)) throw new Error('NumGrid cell ' + c.id + ' must be finite');
  const m = p.marks?.[c.id];
  return <td key={key} data-t={c.id} tabIndex={m?.hidden ? -1 : 0} aria-hidden={m?.hidden || undefined}
   className={(hot.has(c.id) ? 'is-hot' : '') + markClass(m)}
   onPointerEnter={() => hotOn(p.lesson, c.id)} onPointerLeave={() => hotOff(p.lesson, c.id)}
   onFocus={() => hotOn(p.lesson, c.id)} onBlur={() => hotOff(p.lesson, c.id)}>{show(c.value)}</td>;
 };
 return <div ref={box} className="lab-num-grid not-content" style={{ overflowX: 'auto', maxWidth: '100%' }}>
  <table className="lab-num-table"><caption>{p.caption}</caption>
   <thead><tr><th scope="col">{p.corner}</th>{p.cols.map((c) => <th key={c.id} scope="col" data-col={c.id}>{c.label}</th>)}</tr></thead>
   <tbody>{p.rows.map((r, i) => <tr key={i}><th scope="row">{r.head}</th>{p.cols.map((c) => cell(r.cells[c.id], c.id))}</tr>)}</tbody>
  </table>
 </div>;
}
