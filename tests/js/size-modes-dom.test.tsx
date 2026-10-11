import './dom';
import { afterEach, expect, test } from 'bun:test';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { SizeButtons } from '../../src/viz/scene/Header';
import { registerSizeCard, toggleSize, useSizeState } from '../../src/viz/scene/sizeModes';
import { LocaleContext } from '../../src/i18n/react';
import '../../src/i18n/ui';

const clean: (()=>void)[] = [];
const roots: Root[] = [];
const originalMedia = globalThis.matchMedia;
const originalRequest = document.documentElement.requestFullscreen;
const originalExit = document.exitFullscreen;
afterEach(() => {
 for(const f of clean.splice(0)) act(f);
 for(const r of roots.splice(0)) act(()=>r.unmount());
 document.body.innerHTML='';
 globalThis.matchMedia=originalMedia;
 document.documentElement.requestFullscreen=originalRequest;
 document.exitFullscreen=originalExit;
 Object.defineProperty(document,'fullscreenElement',{configurable:true,value:null});
});
function fixture(id:string,locale:'en'|'ru'='en') {
 globalThis.matchMedia = ((q:string)=>({matches:q.includes('reduced-motion')})) as typeof matchMedia;
 const el = document.createElement('div');el.tabIndex=-1;document.body.append(el);
 clean.push(registerSizeCard(id,el,()=>{}));
 function Buttons(){const s=useSizeState();return <LocaleContext.Provider value={locale}><SizeButtons id={id} mode={s.id===id?s.mode:'normal'}/></LocaleContext.Provider>}
 const root=createRoot(el);roots.push(root);act(()=>root.render(<Buttons/>));
 return {el,buttons:[...el.querySelectorAll('button')]};
}
function key(el:HTMLElement,k:string,code=k,ctrl=false,alt=false) {
 act(()=>el.dispatchEvent(new KeyboardEvent('keydown',{key:k,code,ctrlKey:ctrl,altKey:alt,bubbles:true,cancelable:true})));
}
test('translated buttons, pressed states and focused-card precedence over another hover',()=>{
 const a=fixture('a'),b=fixture('b','ru');
 expect(a.buttons[0].getAttribute('aria-label')).toBe('Enlarge 2×');
 expect(b.buttons[1].getAttribute('aria-label')).toBe('На весь экран');
 a.el.focus();b.el.dispatchEvent(new PointerEvent('pointerenter'));
 key(a.el,' ','Space',true);
 expect(a.buttons[0].getAttribute('aria-pressed')).toBe('true');
 expect(b.buttons[0].getAttribute('aria-pressed')).toBe('false');
 key(a.el,'Escape');
 expect(a.buttons[0].getAttribute('aria-pressed')).toBe('false');
 act(()=>b.buttons[0].click());
 expect(b.buttons[0].getAttribute('aria-pressed')).toBe('true');
});
test('wide temporarily hides fixed navigation and restores exact state across owners and unmount',()=>{
 const a=fixture('a'),b=fixture('b');
 const left=document.createElement('nav'),right=document.createElement('aside');
 left.className='sidebar-pane';right.className='right-sidebar';left.tabIndex=0;
 left.style.setProperty('visibility','visible','important');right.setAttribute('inert','saved');
 document.body.append(left,right);left.focus();
 act(()=>a.buttons[0].click());
 expect(a.el.contains(document.activeElement)).toBe(true);
 expect(left.style.visibility).toBe('hidden');expect(left.inert).toBe(true);expect(right.inert).toBe(true);
 act(()=>b.buttons[0].click());
 expect(left.style.visibility).toBe('hidden');
 key(b.el,'Escape');
 expect(document.activeElement).toBe(left);
 expect(left.style.visibility).toBe('visible');expect(left.style.getPropertyPriority('visibility')).toBe('important');
 expect(left.hasAttribute('inert')).toBe(false);expect(right.getAttribute('inert')).toBe('saved');expect(right.style.visibility).toBe('');
 act(()=>a.buttons[0].click());act(()=>clean.shift()!());
 expect(left.style.visibility).toBe('visible');expect(left.hasAttribute('inert')).toBe(false);
 expect(right.getAttribute('inert')).toBe('saved');
});
test('editable F/Space remain text; prevented Escape belongs to the field/popover',()=>{
 const a=fixture('a');
 const input=document.createElement('input');a.el.append(input);input.focus();
 key(input,'f','KeyF');
 key(input,' ','Space',true);
 expect(a.buttons.every(b=>b.getAttribute('aria-pressed')==='false')).toBe(true);
 act(()=>toggleSize('a','wide'));
 const e=new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true});e.preventDefault();
 act(()=>a.el.dispatchEvent(e));
 expect(a.buttons[0].getAttribute('aria-pressed')).toBe('true');
});
test('missing and rejected fullscreen API keep overlay, restore host/overflow/focus on exit',async()=>{
 const a=fixture('a');
 const host=document.createElement('div');host.className='lab-scene-host';document.body.append(host);
 document.body.style.overflow='clip';document.documentElement.style.overflow='auto';
 document.documentElement.requestFullscreen=undefined as unknown as typeof originalRequest;
 a.buttons[1].focus();act(()=>a.buttons[1].click());
 expect(a.el.dataset.size).toBe('full');expect(host.parentElement).toBe(a.el);
 expect(document.body.style.overflow).toBe('hidden');
 key(a.el,'Escape');
 expect(host.parentElement).toBe(document.body);
 expect(document.body.style.overflow).toBe('clip');
 expect(document.documentElement.style.overflow).toBe('auto');
 expect(document.activeElement).toBe(a.buttons[1]);
 document.documentElement.requestFullscreen=()=>Promise.reject(new Error('denied'));
 await act(async()=>{a.buttons[1].click();await Promise.resolve()});
 expect(a.buttons[1].getAttribute('aria-pressed')).toBe('true');
});
test('request targets documentElement; late success after unmount exits browser and restores host',async()=>{
 const a=fixture('a');
 let resolve!:()=>void;let target:Element|undefined;let exits=0;
 document.documentElement.requestFullscreen=function(){target=this;return new Promise<void>(r=>resolve=r)};
 document.exitFullscreen=async()=>{exits++;Object.defineProperty(document,'fullscreenElement',{configurable:true,value:null})};
 act(()=>a.buttons[1].click());expect(target).toBe(document.documentElement);
 act(()=>clean.shift()!());
 await act(async()=>{Object.defineProperty(document,'fullscreenElement',{configurable:true,value:document.documentElement});resolve();await Promise.resolve()});
 expect(exits).toBe(1);expect(document.body.style.overflow).not.toBe('hidden');
});
test('browser-exit chrome reveal restores exact visibility/focus and cancels on mode change or unmount',async()=>{
 const a=fixture('a'),b=fixture('b');
 const savedRAF=globalThis.requestAnimationFrame,savedCancel=globalThis.cancelAnimationFrame;
 let next=0;const frames=new Map<number,FrameRequestCallback>();
 globalThis.requestAnimationFrame=fn=>{frames.set(++next,fn);return next};
 globalThis.cancelAnimationFrame=id=>{frames.delete(id)};
 const tick=()=>act(()=>{const pending=[...frames.values()];frames.clear();pending.forEach(fn=>fn(0))});
 document.documentElement.requestFullscreen=async()=>{Object.defineProperty(document,'fullscreenElement',{configurable:true,value:document.documentElement})};
 const full=async()=>{a.buttons[1].focus();await act(async()=>{a.buttons[1].click();await Promise.resolve()})};
 const exit=()=>act(()=>{Object.defineProperty(document,'fullscreenElement',{configurable:true,value:null});document.dispatchEvent(new Event('fullscreenchange'))});
 try {
  a.el.style.setProperty('visibility','visible','important');
  await full();exit();
  expect(a.el.style.visibility).toBe('hidden');tick();expect(a.el.style.visibility).toBe('hidden');tick();
  expect(a.el.style.visibility).toBe('visible');expect(a.el.style.getPropertyPriority('visibility')).toBe('important');expect(document.activeElement).toBe(a.buttons[1]);
  await full();exit();act(()=>toggleSize('b','wide'));
  expect(a.el.style.visibility).toBe('visible');expect(frames.size).toBe(0);expect(b.buttons[0].getAttribute('aria-pressed')).toBe('true');
  key(b.el,'Escape');await full();exit();act(()=>clean.shift()!());
  expect(a.el.style.visibility).toBe('visible');expect(frames.size).toBe(0);
 } finally {globalThis.requestAnimationFrame=savedRAF;globalThis.cancelAnimationFrame=savedCancel}
});
test('system exit normalizes full mode; switching owners keeps single host',async()=>{
 const a=fixture('a'),b=fixture('b');
 const host=document.createElement('div');host.className='lab-scene-host';document.body.append(host);
 document.documentElement.requestFullscreen=async()=>{
  Object.defineProperty(document,'fullscreenElement',{configurable:true,value:document.documentElement});
  document.dispatchEvent(new Event('fullscreenchange'));
 };
 await act(async()=>{a.buttons[1].click();await Promise.resolve()});
 act(()=>toggleSize('b','full'));
 expect(host.parentElement).toBe(b.el);
 expect(a.buttons[1].getAttribute('aria-pressed')).toBe('false');
 act(()=>{Object.defineProperty(document,'fullscreenElement',{configurable:true,value:null});document.dispatchEvent(new Event('fullscreenchange'))});
 expect(b.buttons[1].getAttribute('aria-pressed')).toBe('false');
 expect(host.parentElement).toBe(document.body);
});
