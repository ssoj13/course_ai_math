import {stubFrames} from './dom';
import {expect,test} from 'bun:test';
import {act} from 'react';
import {createRoot} from 'react-dom/client';
import '../../src/i18n/ui';
import {NPanel,rowRanges} from '../../src/viz/scene/NPanel';
import {createStage,type Draggable} from '../../src/viz/scene/handles';
import {lessonStore} from '../../src/viz/store';
import {ROWSLAMP,moveLight,lightLink,probeLink} from '../../src/viz/demo/engine.rows';
test('mounted radial field follows large registry truth, steps/types without box bounds and cancels touch',()=>{
 let frame:FrameRequestCallback|undefined;
 const restoreFrames=stubFrames(fn=>{frame=fn;return 1},()=>{frame=undefined});
 HTMLElement.prototype.setPointerCapture=()=>{};
 const id='radial-field-'+Math.random(),s=lessonStore(id,{nl_light:[3,1.5,-1.2],nl_pt:[0,1.5,0],nl_raw:0},{groups:[lightLink,probeLink]});
 const rows=ROWSLAMP('en'),reg=createStage('3d',{lesson:id,id:'lamp',overlays:[],ranges:rowRanges(rows)});
 const d:Draggable={id:'light',owner:'light',label:'light',lesson:id,keys:['nl_light'],at:v=>v.nl_light as number[],move:moveLight};
 const host=document.createElement('div');document.body.append(host);const root=createRoot(host);
 const key=(el:Element,k:string)=>act(()=>void el.dispatchEvent(new KeyboardEvent('keydown',{key:k,bubbles:true,cancelable:true})));
 const field=()=>host.querySelector<HTMLElement>('.lab-np-field')!;
 const x=()=> (s.getState().vals.nl_light as number[])[0];
 const type=(text:string)=>{key(field(),'Enter');const input=host.querySelector<HTMLInputElement>('input.lab-np-input')!;input.value=text;key(input,'Enter')};
 try{
 act(()=>root.render(<NPanel lesson={id} rows={rows}/>));
 act(()=>reg.apply(d,s.getState().vals,[100,50,25],'drag'));
 expect(field().getAttribute('role')).toBe('spinbutton');expect(field().getAttribute('aria-valuenow')).toBe('103');
 expect(field().hasAttribute('aria-valuemin')).toBe(false);expect(field().hasAttribute('aria-valuemax')).toBe(false);
 key(field(),'ArrowRight');expect(x()).toBe(103.1);key(field(),'ArrowLeft');expect(x()).toBe(103);
 type('205.5');expect(x()).toBe(205.5);type('-103');expect(x()).toBe(-103);
 type('--1');expect(host.querySelector('input')!.getAttribute('aria-invalid')).toBe('true');key(host.querySelector('input')!,'Escape');expect(x()).toBe(-103);
 act(()=>s.getState().set('nl_light',[3,0,0]));type('0');expect(s.getState().vals.nl_light).toEqual([1.8,0,0]);
 const before=s.getState();const f=field();
 act(()=>void f.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerId:1,pointerType:'touch',clientX:100,clientY:10})));
 act(()=>void f.dispatchEvent(new PointerEvent('pointermove',{bubbles:true,pointerId:1,pointerType:'touch',clientX:124,clientY:10})));
 act(()=>{const fn=frame;frame=undefined;fn?.(performance.now())});
 expect(x()).toBe(2.4);
 act(()=>void f.dispatchEvent(new PointerEvent('pointercancel',{bubbles:true,pointerId:1,pointerType:'touch'})));
 expect(s.getState().vals.nl_light).toEqual(before.vals.nl_light);expect(s.getState().shown.nl_light).toEqual(before.shown.nl_light);
 }finally{act(()=>root.unmount());host.remove();restoreFrames()}
});
