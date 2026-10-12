import {expect,test} from 'bun:test';
import {lessonStore} from '../../src/viz/store';
import {stubFrames} from './frames';
import {powersLinks} from '../../src/viz/links';

test('cut to truth target cancels pending and applied independent/canonical springs',async()=>{
 let frame:FrameRequestCallback|undefined;
 const restoreFrames=stubFrames(cb=>{frame=cb;return 1});
 try{
 for(const applied of [false,true])for(const channel of ['q','A','x','V'] as const){
  const s=lessonStore('cut-'+Math.random(),{q:0,x:3,A:9,V:27,from:0,n:2,k:3},powersLinks());
  s.getState().set(channel==='q'?'q':'A',channel==='q'?5:4,'ease');
  if(applied){await new Promise(r=>setTimeout(r,20));const f=frame;frame=undefined;f?.(performance.now())}
  s.getState().set(channel,({q:5,A:4,x:2,V:8})[channel]);
  expect(s.getState().shown.q).toBe(s.getState().vals.q);
  expect(s.getState().shown.x).toBe(s.getState().vals.x);
  expect(s.getState().shown.A).toBe(s.getState().vals.A);
  expect(s.getState().shown.V).toBe(s.getState().vals.V);
  const shown=s.getState().shown;const f=frame;frame=undefined;f?.(performance.now());
  expect(s.getState().shown).toBe(shown);
 }
 }finally{restoreFrames()}
});
