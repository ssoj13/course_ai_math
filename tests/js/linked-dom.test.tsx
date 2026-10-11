import './dom';
import {afterEach,beforeEach,expect,test} from 'bun:test';
import {act,type ReactElement} from 'react';
import {createRoot,type Root} from 'react-dom/client';
import {Formula} from '../../src/viz/Formula';
import {Num} from '../../src/viz/Num';
import {NumTable} from '../../src/viz/NumTable';
import {Timeline} from '../../src/viz/Timeline';
import {CodeView} from '../../src/viz/CodeView';
import {NPanel,FieldCell} from '../../src/viz/scene/NPanel';
import {writeMove} from '../../src/viz/scene/handles';
import {chipBox} from '../../src/viz/scene/chips';
import {lessonStore} from '../../src/viz/store';
import {powersLinks} from '../../src/viz/links';
import '../../src/i18n/ui';
const roots:Root[]=[];
let frame:FrameRequestCallback|undefined;
beforeEach(()=>{HTMLElement.prototype.setPointerCapture=()=>{};globalThis.requestAnimationFrame=cb=>{frame=cb;return 1};globalThis.cancelAnimationFrame=()=>{frame=undefined}});
afterEach(()=>{for(const r of roots.splice(0))act(()=>r.unmount());document.body.innerHTML='';frame=undefined});
function mount(el:ReactElement){const host=document.createElement('div');document.body.append(host);const root=createRoot(host);roots.push(root);act(()=>root.render(el));return {host,render:(el:ReactElement)=>act(()=>root.render(el))}}
const ptr=(type:string,x:number)=>new PointerEvent(type,{bubbles:true,clientX:x,pointerId:1,pointerType:'mouse'});
const key=(el:Element,k:string)=>act(()=>void el.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true})));
function setup(){const id='ldom-'+Math.random();const store=lessonStore(id,{x:3,A:9,V:27,from:0,n:2,k:3,a:[1,2],b:[3,4]},powersLinks());return {id,store}}
test('actual Formula, Num, NPanel and handle writer snapshots stay coupled; pending/applied cancellations restore source',()=>{
 const {id,store}=setup();
 const {host}=mount(<><Formula lesson={id} tex={String.raw`\t{A}{\s{A}}`} scrub={{A:{min:0,max:16,step:.25}}} label="A"/>
 <Num lesson={id} k="A" spec={{min:0,max:16,step:.25}} label="A"/>
 <NPanel lesson={id} rows={[{id:'A',label:'A',kind:'num',key:'A',spec:{min:0,max:16,step:.25}}]}/></>);
 const snapshots:any[]=[];const off=store.subscribe(s=>{expect(s.vals.A).toBe((s.vals.x as number)**2);expect(s.vals.V).toBe((s.vals.x as number)**3);snapshots.push(s.vals)});
 for(const selector of ['[data-s]','.lab-num','.lab-np-field']){
  for(const applied of [false,true]){
   act(()=>store.getState().set('x',3));
   const handle=host.querySelector<HTMLElement>(selector)!,capture=selector==='[data-s]'?host.querySelector<HTMLElement>('.lab-formula')!:handle;
   act(()=>void handle.dispatchEvent(ptr('pointerdown',100)));
   act(()=>void capture.dispatchEvent(ptr('pointermove',120)));
   if(applied)act(()=>{const f=frame;frame=undefined;f?.(performance.now())});
   if(applied)expect(store.getState().vals.from).toBe(1);
   act(()=>void capture.dispatchEvent(ptr('pointercancel',120)));
   expect(store.getState().vals).toMatchObject({x:3,A:9,V:27,from:0});
   expect(host.querySelector('.lab-np-input')).toBeNull();
  }
  key(host.querySelector(selector)!,'ArrowRight');expect(store.getState().vals.from).toBe(1);
 }
 act(()=>writeMove(id,{x:2.5}));expect(store.getState().vals).toMatchObject({x:2.5,A:6.25,V:15.625,from:0});
 off();expect(snapshots.length).toBeGreaterThan(10);
});
test('Formula current formatter, live ARIA, marks and commit-time focus survive changed props',()=>{
 const {id,store}=setup();
 const props={lesson:id,tex:String.raw`\t{A}{\s{A}}`,label:'A',scrub:{A:{min:0,max:16,step:.25}}};
 const {host,render}=mount(<Formula {...props}/>);
 act(()=>host.querySelector<HTMLElement>('[data-s]')!.focus());
 render(<Formula {...props} scrub={{A:{min:1,max:20,step:1}}} fmt={{A:v=>v.toFixed(3)}} marks={{A:{struck:true}}}/>);
 const h=host.querySelector<HTMLElement>('[data-s]')!;
 expect(h.closest('[aria-hidden="true"]')).toBeNull();
 expect(h.getAttribute('aria-valuemax')).toBe('20');expect(h.getAttribute('aria-valuemin')).toBe('1');expect(h.textContent).toBe('9.000');
 expect(document.activeElement).toBe(h);expect(host.querySelector('.is-struck')).not.toBeNull();
 const outside=document.createElement('button');document.body.append(outside);act(()=>outside.focus());
 act(()=>store.getState().set('A',4));expect(document.activeElement).toBe(outside);
 render(<Formula {...props} marks={{A:{hidden:true}}}/>);
 expect(host.querySelector('[data-s]')!.getAttribute('tabindex')).toBe('-1');
});
test('table, formula and code link bidirectionally and stay lesson-scoped',()=>{
 const {id}=setup(),other=setup();
 const {host}=mount(<><Formula lesson={id} tex={String.raw`\t{p0}{3}`} label="p"/>
 <NumTable lesson={id} rows={[{label:'a',key:'a'},{label:'b',key:'b'}]} op="product"/>
 <CodeView lesson={id} lang="python" code={'p0 = 3\np1 = 8'} lines={{1:'p0',2:'p1'}}/>
 <CodeView lesson={other.id} lang="ts" code="p0" lines={{1:'p0'}}/></>);
 const cell=host.querySelector<HTMLElement>('td[data-t="p0"]')!;
 act(()=>cell.focus());expect(host.querySelectorAll('.is-hot').length).toBe(3);
 act(()=>host.querySelector<HTMLElement>('.lab-code [data-t="p1"]')!.focus());
 expect(host.querySelector('td[data-t="p1"]')!.classList.contains('is-hot')).toBe(true);
 expect(host.querySelectorAll('.lab-code')[1].querySelector('.is-hot')).toBeNull();
});
test('Timeline maintains bounded authoritative integer, history, backward reveals and focus when list changes',()=>{
 const {id,store}=setup(),steps=[1,2,3].map(n=>({id:String(n),text:'x^'+n}));
 const {host,render}=mount(<Timeline lesson={id} id="power" steps={steps}/>);
 const next=host.querySelectorAll('button')[1];act(()=>{next.focus();next.click()});
 expect(store.getState().vals.n).toBe(3);expect(store.getState().step['power:max']).toBe(2);
 act(()=>host.querySelector<HTMLButtonElement>('button')!.click());expect(store.getState().step.power).toBe(1);
 act(()=>store.getState().set('n',1.8,'ease'));expect(store.getState().shown.n).toBe(1);
 act(()=>next.focus());render(<Timeline lesson={id} id="power" steps={steps.slice(0,2)}/>);
 expect(document.activeElement).toBe(next);expect(host.querySelector('input')!.max).toBe('1');
 act(()=>store.getState().setStep('power',99));expect(store.getState().vals.n).toBe(2);expect(store.getState().step['power:max']).toBe(2);
});
test('glyph reservation and DOM share width including hidden future rows',()=>{
 const row={id:'v',label:'V',kind:'ro' as const,chars:7,value:()=>-3};
 expect(chipBox({...row,glyph:'rect'}).w-chipBox(row).w).toBe(36);
 const {id}=setup(),{host}=mount(<NPanel lesson={id} rows={[{...row,glyph:'rect'}]}/>);
 expect(host.querySelector('[data-glyph]')?.getAttribute('data-value')).toBe('-3');
 expect((host.querySelector('.lab-np-row') as HTMLElement).style.getPropertyValue('--np-lw')).toBe('43px');
});

test('large page field reuses the linked editable cell and quantitative glyph',()=>{
 const {id,store}=setup(),{host}=mount(<FieldCell lesson={id} size="lg" row={{id:'A',label:'A',key:'A',kind:'num',spec:{min:0,max:16,step:1},glyph:'rect',glyphValue:v=>[v.x as number,2]}}/>);
 expect(host.querySelector('.lab-glyph.is-lg')?.getAttribute('data-value')).toBe('3,2');
 const field=host.querySelector<HTMLElement>('.lab-np-field')!;key(field,'ArrowRight');
 expect(store.getState().vals.A).toBeCloseTo(10,12);
 expect(store.getState().vals.x).toBeCloseTo(Math.sqrt(10),12);
 expect(host.querySelector('.lab-glyph')?.getAttribute('data-value')).toBe(Math.sqrt(10)+',2');
});
