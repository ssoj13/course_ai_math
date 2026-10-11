import './dom';
import {test,expect} from 'bun:test';
import {act} from 'react';
import {createRoot} from 'react-dom/client';
import {createStage,StageCtx,useDraggable,useLabel,useHoverable,HOVER_RANK} from '../../src/viz/scene/handles';
import {MirrorCtx} from '../../src/viz/scene/look';
import {lessonStore} from '../../src/viz/store';
import {Instruments} from '../../src/viz/scene/Instruments';
function Subject(){
 useDraggable({id:'a:tip',owner:'a',label:'a',lesson:'mirror-test',keys:['a'],at:v=>v.a as number[]});
 useLabel({lesson:'mirror-test',text:'a',at:()=>[1,2]});
 const events=useHoverable('mirror-test','a',HOVER_RANK.point);
 return <i data-events={Object.keys(events).length}/>;
}
test('mirror copies cannot duplicate handles/labels or own hover events',()=>{
 lessonStore('mirror-test',{a:[1,2]});const reg=createStage('3d');
 const el=document.createElement('div');document.body.append(el);const root=createRoot(el);
 act(()=>root.render(<StageCtx.Provider value={reg}><Subject/><MirrorCtx.Provider value={true}><Subject/></MirrorCtx.Provider></StageCtx.Provider>));
 expect(reg.list().length).toBe(1);expect(reg.labels.list().length).toBe(1);
 expect(el.querySelectorAll('i')[1].dataset.events).toBe('0');
 act(()=>root.unmount());expect(reg.list()).toEqual([]);expect(reg.labels.list()).toEqual([]);el.remove();
});
test('HUD angle/readout follows real store values; overlay choice hides it',()=>{
 const store=lessonStore('hud-test',{theta:60});const reg=createStage('2d',{lesson:'hud-test',id:'hud',overlays:[{id:'instruments',label:'Instruments',on:true}]});
 const el=document.createElement('div');document.body.append(el);const root=createRoot(el);
 act(()=>root.render(<Instruments reg={reg} lesson="hud-test" angleKey="theta"/>));
 expect(el.querySelector('[aria-label="Angle 60.0 degrees"]')).not.toBeNull();
 act(()=>store.getState().set('theta',91));expect(el.querySelector('[aria-label="Angle 91.0 degrees"]')).not.toBeNull();
 expect(el.querySelector('output')?.textContent).toBe('91.00');
 act(()=>store.getState().set('ov:hud:instruments',0));expect(el.querySelector<HTMLElement>('.lab-instruments')?.hidden).toBe(true);
 act(()=>root.unmount());el.remove();
});
