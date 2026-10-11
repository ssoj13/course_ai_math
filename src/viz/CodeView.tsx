import { useStore } from 'zustand';
import { hotOn,hotOff,lessonStore } from './store';
type Base={code:string;lang:'python'|'ts'};
export type CodeViewProps=Base&({lesson:string;lines:Record<number,string>}|{lesson?:never;lines?:never});
export function CodeView(p:CodeViewProps):React.JSX.Element {
 return p.lines?<LinkedCode {...p} lesson={p.lesson!} lines={p.lines}/>:<pre className="lab-code not-content" data-lang={p.lang}><code>{p.code}</code></pre>;
}
function LinkedCode(p:Base&{lesson:string;lines:Record<number,string>}):React.JSX.Element{
 const hot=useStore(lessonStore(p.lesson),s=>s.hot);
 return <pre className="lab-code not-content" data-lang={p.lang}><code>{p.code.split('\n').map((text,i)=>{
  const id=p.lines[i+1];
  return <span key={i} data-line={i+1} data-t={id} tabIndex={id?0:undefined} className={id&&hot.has(id)?'is-hot':undefined}
   onPointerEnter={()=>id&&hotOn(p.lesson,id)} onPointerLeave={()=>id&&hotOff(p.lesson,id)} onFocus={()=>id&&hotOn(p.lesson,id)} onBlur={()=>id&&hotOff(p.lesson,id)}>{text}{'\n'}</span>
 })}</code></pre>;
}
