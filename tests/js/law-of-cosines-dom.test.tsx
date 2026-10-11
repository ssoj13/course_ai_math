import './dom';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { renderToString } from 'react-dom/server';
import { expect,test } from 'bun:test';
import { Formula } from '../../src/viz/Formula';
import { LawFormula, LawControls, LawDetailControls, marksFor } from '../../src/viz/lessons/law-of-cosines';
import { NPanel } from '../../src/viz/scene/NPanel';
import { rowsFor } from '../../src/viz/lessons/law-of-cosines.rows';
import { lessonStore } from '../../src/viz/store';
import { INITIAL, LESSON } from '../../src/viz/lessons/law-of-cosines.math';
test('actual KaTeX mark wrappers preserve semantic occurrences, ancestor hiding and accessible dim',async()=>{
 const host=document.createElement('div');document.body.append(host);const root=createRoot(host),store=lessonStore('lc-marks',{x:2});
 const tex=String.raw`\begin{aligned}&\htmlData{m=one}{\t{x}{\s{x}}+1}\\ &\htmlData{m=two}{\t{x}{\s{x}}+2}\end{aligned}`;
 const view=(hidden:boolean,dim:boolean)=><Formula lesson="lc-marks" label="marks" block tex={tex} marks={{one:{hidden},two:{dim}}} scrub={{x:{min:0,max:4,step:1}}}/>;
 expect(renderToString(view(true,true))).toContain('is-dim');
 await act(async()=>root.render(view(true,true)));
 const terms=host.querySelectorAll<HTMLElement>('[data-t="x"]'),sliders=host.querySelectorAll<HTMLElement>('[data-s="x"]');
 expect(terms).toHaveLength(2);expect(terms[0].tabIndex).toBe(-1);expect(terms[0].getAttribute('aria-hidden')).toBe('true');expect(sliders[0].tabIndex).toBe(-1);
 expect(terms[1].tabIndex).toBe(0);expect(terms[1].getAttribute('aria-hidden')).toBe('false');expect(terms[1].closest('[data-m]')!.classList.contains('is-dim')).toBe(true);
 await act(async()=>terms[1].focus());await act(async()=>store.getState().set('x',3));
 expect(document.activeElement?.getAttribute('data-t')).toBe('x');expect(document.activeElement?.closest('[data-m]')?.getAttribute('data-m')).toBe('two');
 await act(async()=>root.render(view(false,false)));expect(host.querySelector<HTMLElement>('[data-t="x"]')!.tabIndex).toBe(0);
 await act(async()=>root.render(view(true,false)));expect(host.querySelector<HTMLElement>('[data-t="x"]')!.tabIndex).toBe(-1);
 await act(async()=>root.unmount());host.remove();
});
test('proof current reveal is independent of max history; only current expansion earns sticky challenge',async()=>{
 const host=document.createElement('div');document.body.append(host);const root=createRoot(host),store=lessonStore(LESSON);
 store.getState().setMany(INITIAL);store.getState().setStep('proof',0);
 await act(async()=>root.render(<LawControls scene={2} locale="en"/>));
 await act(async()=>store.getState().setStep('proof',9));
 expect(host.querySelector('.lab-challenge')!.classList.contains('is-done')).toBe(false);
 expect(host.querySelector('[data-m="proof-0"]')!.classList.contains('is-dim')).toBe(true);
 await act(async()=>store.getState().setStep('proof',6));
 expect(host.querySelector('.lab-challenge')!.classList.contains('is-done')).toBe(true);
 await act(async()=>store.getState().setStep('proof',0));
 expect(store.getState().step['proof:max']).toBe(9);
 expect(host.querySelector('[data-m="proof-3"]')!.classList.contains('is-hidden')).toBe(true);
 expect(host.querySelector('.lab-challenge')!.classList.contains('is-done')).toBe(true);
 await act(async()=>root.unmount());host.remove();
});
test('reader pause cancels the single clock; resume and unmount preserve and clean controlled placement',async()=>{
 const originalRaf=globalThis.requestAnimationFrame,originalCancel=globalThis.cancelAnimationFrame;
 let next=0;const pending=new Map<number,FrameRequestCallback>();
 globalThis.requestAnimationFrame=cb=>{pending.set(++next,cb);return next};
 globalThis.cancelAnimationFrame=id=>{pending.delete(id)};
 const host=document.createElement('div');document.body.append(host);const root=createRoot(host),store=lessonStore(LESSON);
 store.getState().setMany(INITIAL);store.getState().setStep('cut',5);
 try{
  await act(async()=>root.render(<><LawControls scene={0} locale="en"/><LawControls scene={2} locale="en"/></>));
  expect(host.querySelector('[data-lc-transport]')).toBeNull();
  await act(async()=>store.getState().setStep('cut',6));
  expect(host.querySelector('[data-lc-transport]')).not.toBeNull();
  expect(store.getState().vals.cut_play).toBe(1);
  await act(async()=>host.querySelector<HTMLButtonElement>('[data-lc-transport] button')!.click());
  expect(store.getState().vals.cut_play).toBe(0);
  await act(async()=>store.getState().setMany({cut_progress:.5,cut_position:5.5}));
  await act(async()=>host.querySelector<HTMLButtonElement>('[data-lc-transport] button')!.click());
  expect(store.getState().vals.cut_position).toBe(5.5);expect(store.getState().vals.cut_play).toBe(1);
  await act(async()=>store.getState().setStep('proof',7));
  await act(async()=>store.getState().setStep('proof',8));
  expect(store.getState().vals.proof_fade_play).toBe(1);expect(pending.size).toBe(1);
  await act(async()=>host.querySelector<HTMLButtonElement>('[data-lc-proof-transport] button')!.click());
  expect(store.getState().vals.proof_fade_play).toBe(0);expect(store.getState().vals.cut_play).toBe(1);expect(pending.size).toBe(1);
  await act(async()=>root.unmount());
  expect(store.getState().vals.cut_play).toBe(0);expect(pending.size).toBe(0);
 }finally{host.remove();globalThis.requestAnimationFrame=originalRaf;globalThis.cancelAnimationFrame=originalCancel}
});
test('every actual formula renders both layouts and stage marks',()=>{
 lessonStore(LESSON).getState().setMany(INITIAL);
 for(const scene of [0,1,2,3] as const){
  const html=renderToString(<LawFormula scene={scene}/>);
  expect(html).toContain('katex');expect(html).not.toContain('NaN');
 }
 expect(marksFor(2,4,60)['leg-rest'].dim).toBe(true);
 expect(marksFor(2,8,60).ident.struck).toBe(true);
 expect(marksFor(0,10,90).corr.dim).toBe(true);
});

for(const locale of ['en','ru'] as const)test('LC review regressions: live prose, row aliases and length/area readouts '+locale,async()=>{
 // Other DOM fixtures install a synchronous rAF; these text/link assertions do not advance animation time.
 const raf=globalThis.requestAnimationFrame,cancel=globalThis.cancelAnimationFrame;
 let frame=0;globalThis.requestAnimationFrame=()=>++frame;globalThis.cancelAnimationFrame=()=>{};
 const host=document.createElement('div');document.body.append(host);const root=createRoot(host),store=lessonStore(LESSON);
 store.getState().setMany(INITIAL);store.getState().setStep('cut',0);store.getState().setStep('proof',0);store.getState().setHot([]);
 const text=(id:string)=>host.querySelector('[data-timeline="'+id+'"] p')!.textContent!;
 try{
  await act(async()=>root.render(<><LawControls scene={0} locale={locale}/><LawControls scene={2} locale={locale}/><LawControls scene={3} locale={locale}/><LawDetailControls locale={locale}/><NPanel lesson={LESSON} rows={rowsFor(3,locale)}/><NPanel lesson={LESSON} rows={rowsFor(2,locale)}/></>));
  expect(text('cut')).toContain('9.00 + 16.00 = 25.00; c² = 13.00');
  expect(text('cut')).toContain(locale==='en'?'Subtract 12.00':'Вычитаем 12.00');
  await act(async()=>store.getState().set('th',90));
  expect(text('cut')).toContain('c² = 25.00');
  expect(text('cut')).toContain(locale==='en'?'correction is zero':'Поправка нулевая');
  expect(text('proof')).toContain(locale==='en'?'This is a right angle':'Это прямой угол');
  await act(async()=>store.getState().setMany({a:2,b:4,th:120}));
  expect(text('cut')).toContain('4.00 + 16.00 = 20.00; c² = 28.00');
  expect(text('cut')).toContain(locale==='en'?'Add 8.00':'Прибавляем 8.00');
  expect(text('proof')).toContain('θ = 120°');
  await act(async()=>store.getState().setStep('proof',7));
  for(const id of ['ident','bcos2','bsin2','rise2'])expect(store.getState().hot.has(id)).toBe(true);
  expect(store.getState().hotSource).toBe('guide');
  for(const [id,targets] of [['cosraw',['cosphi','l1','l2','d','ik_l1','ik_l2']],['cosc',['clamp','gap','reach_max']],['rise2',['bsin2']]] as const){
   const row=host.querySelector<HTMLElement>('.lab-np-row[data-id="'+id+'"]')!;
   for(const event of ['pointerover','focusin']){
    await act(async()=>row.dispatchEvent(new Event(event,{bubbles:true})));
    for(const target of targets)expect(store.getState().hot.has(target)).toBe(true);
    expect(store.getState().hotSource).toBe('interaction');
    await act(async()=>row.dispatchEvent(new Event(event==='pointerover'?'pointerout':'focusout',{bubbles:true})));
    expect(store.getState().hotSource).toBe('guide');expect(store.getState().hot.has('ident')).toBe(true);
   }
  }
  const term=host.querySelector<HTMLElement>('[data-t="bsin2"]')!;
  await act(async()=>term.focus());expect(store.getState().hot.has('rise2')).toBe(true);expect(store.getState().hotSource).toBe('interaction');
  await act(async()=>term.blur());
  await act(async()=>store.getState().setStep('cut',3));
  const readout=()=>host.querySelector('[data-lc-detail-controls] [aria-live]')!.textContent!;
  for(const part of [9,10]){
   await act(async()=>store.getState().set('detail_part',part));
   expect(readout()).toContain(locale==='en'?'signed projection length':'длина проекции со знаком');
   expect(readout()).not.toContain(locale==='en'?'signed area':'площадь со знаком');
  }
  await act(async()=>store.getState().set('th',90));
  expect(readout()).toContain(locale==='en'?'Zero signed projection length':'Нулевая длина проекции');
  await act(async()=>store.getState().set('detail_part',0));
  expect(readout()).toContain(locale==='en'?'signed area':'площадь со знаком');
  await act(async()=>store.getState().setMany({a:3,b:3,th:0}));
  expect(readout()).toContain(locale==='en'?'Zero signed area':'Нулевая площадь со знаком');
 }finally{await act(async()=>root.unmount());store.getState().setMany(INITIAL);host.remove();globalThis.requestAnimationFrame=raf;globalThis.cancelAnimationFrame=cancel}
});
