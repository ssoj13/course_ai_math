import './dom';
import { expect, test } from 'bun:test';
import { act } from 'react';
import { renderToString } from 'react-dom/server';
import { hydrateRoot } from 'react-dom/client';
import { DemoDotFormula, DemoChallenges } from '../../src/viz/demo/engine';
import { DICT } from '../../src/i18n/ui';

for (const locale of ['en','ru'] as const) test(locale+' first SSR and hydration render use the same explicit locale',async()=>{
 const element=<><DemoDotFormula locale={locale}/><DemoChallenges locale={locale}/></>;
 const html=renderToString(element), box=document.createElement('div');
 box.innerHTML=html;document.body.append(box);
 expect(box.querySelector('.lab-ch-title')?.textContent).toBe(DICT[locale]['challenge.title'].replace('{n}','1'));
 expect(box.querySelector('.lab-ch-status')?.textContent).toBe(DICT[locale]['challenge.notYet']);
 const before=box.textContent;
 const errors: unknown[]=[];
 let root: ReturnType<typeof hydrateRoot> | undefined;
 try{
  await act(async()=>{root=hydrateRoot(box,element,{onRecoverableError:e=>errors.push(e)});});
  expect(errors).toEqual([]);
  expect(box.textContent).toBe(before);
 }finally{await act(async()=>root?.unmount());box.remove();}
});
