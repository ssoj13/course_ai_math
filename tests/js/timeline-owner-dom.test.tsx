import './dom';
import {expect,test} from 'bun:test';
import {act} from 'react';
import {createRoot} from 'react-dom/client';
import {Timeline} from '../../src/viz/Timeline';
import {CodeView} from '../../src/viz/CodeView';
import {lessonStore,hotOn} from '../../src/viz/store';
import '../../src/i18n/ui';

test('actual same-ID code hover/focus survives guide step/list/owner disposal until release',()=>{
 const lesson='same-owner-'+Math.random(),s=lessonStore(lesson),host=document.createElement('div');
 document.body.append(host);const root=createRoot(host);
 const steps=[{id:'a',text:'a',focus:'a'},{id:'b',text:'b',focus:'b'}];
 const render=(count=2,list=steps)=>act(()=>root.render(<>
 {count>0&&<Timeline lesson={lesson} id="one" steps={list}/>}
 {count>1&&<Timeline lesson={lesson} id="two" steps={steps}/>}
 <CodeView lesson={lesson} lang="python" code="a" lines={{1:'a'}}/>
 </>));
 const hot=()=>[...s.getState().hot],line=()=>host.querySelector('[data-line]')!;
 const event=(type:string)=>act(()=>void line().dispatchEvent(type.startsWith('pointer')?new PointerEvent(type,{bubbles:true}):new FocusEvent(type,{bubbles:true})));
 try{
 render();expect(hot()).toEqual(['a']);
 event('pointerover');act(()=>s.getState().setStep('one',1));expect(hot()).toEqual(['a']);
 event('pointerout');expect(hot()).toEqual(['b']);
 act(()=>s.getState().setStep('one',0));event('focusin');
 act(()=>s.getState().setStep('one',1));expect(hot()).toEqual(['a']);
 render(2,[{id:'c',text:'c',focus:'c'}]);expect(hot()).toEqual(['a']);
 event('focusout');expect(hot()).toEqual(['c']);
 act(()=>s.getState().setStep('two',1));act(()=>s.getState().setStep('two',0));
 event('pointerover');
 let notifications=0;const off=s.subscribe(()=>notifications++);
 for(let i=0;i<10;i++)event('pointerover');
 expect(notifications).toBe(0);off();
 render(1,[{id:'c',text:'c',focus:'c'}]);expect(hot()).toEqual(['a']);
 render(0);expect(hot()).toEqual(['a']);event('pointerout');expect(hot()).toEqual([]);
 render(1);expect(hot()).toEqual(['a']);event('pointerover');
 act(()=>hotOn(lesson+'other','z'));render(0);expect(hot()).toEqual(['a']);
 event('pointerout');expect(hot()).toEqual([]);expect([...lessonStore(lesson+'other').getState().hot]).toEqual(['z']);
 }finally{act(()=>root.unmount());host.remove()}
});
