import { expect, test } from 'bun:test';
import { lessonStore } from '../../src/viz/store';
import { powersLinks } from '../../src/viz/links';
const make = () => lessonStore('links-'+Math.random(), {x:3,A:9,V:27,from:0,n:2,k:3}, powersLinks());
test('every atomic powers snapshot is coherent, including equal input provenance', () => {
 const s=make(); let calls=0;
 s.subscribe(st=>{calls++;expect(st.vals.A).toBe((st.vals.x as number)**2);expect(st.vals.V).toBe((st.vals.x as number)**3)});
 for (const x of [0,.1,2.5,100,-10]) s.getState().set('x',x);
 s.getState().set('A',9); expect(s.getState().vals.from).toBe(1);
 s.getState().set('x',3); expect(s.getState().vals.from).toBe(0);
 expect(calls).toBe(7);
 expect(()=>s.getState().setMany({x:2,A:9})).toThrow();
 expect(()=>s.getState().set('V',NaN)).toThrow();
});
test('authoritative integers, bounded timeline history and complete cancellation', () => {
 const s=make();expect(s.getState().step.power).toBe(1);
 s.getState().setStep('power',99);expect(s.getState().vals.n).toBe(3);
 s.getState().set('n',1.9,'ease');expect(s.getState().shown.n).toBe(1);
 expect(s.getState().step.power).toBe(0);expect(s.getState().step['power:max']).toBe(2);
 s.getState().setStep('ladder',6);expect(s.getState().vals.k).toBe(-3);
 const restore=s.getState().checkpoint(['A']);
 s.getState().set('A',4);s.getState().set('other',7);restore();
 expect(s.getState().vals).toMatchObject({x:3,A:9,V:27,from:0,other:7});
});
test('shown powers derive from canonical x through ease and retarget', async () => {
 const old=globalThis.requestAnimationFrame;
 globalThis.requestAnimationFrame=cb=>setTimeout(()=>cb(performance.now()),16) as unknown as number;
 const s=make();let intermediate=0;
 const off=s.subscribe(st=>{const x=st.shown.x as number;expect(st.shown.A).toBe(x*x);expect(st.shown.V).toBe(x*x*x);if(x!==3&&x!==2&&x!==4)intermediate++});
 s.getState().set('A',4,'ease');await new Promise(r=>setTimeout(r,40));
 s.getState().set('V',64,'ease');await new Promise(r=>setTimeout(r,400));
 off();globalThis.requestAnimationFrame=old;expect(intermediate).toBeGreaterThan(1);
});
