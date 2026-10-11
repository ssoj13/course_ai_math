import {expect,test} from 'bun:test';
import {lessonStore} from '../../src/viz/store';
import {writeMove} from '../../src/viz/scene/handles';
import {radialLink} from '../../src/viz/scene/radialLink';
test('typed lamp/probe writes cannot enter the sphere or leave its surface, including zero',()=>{
 const s=lessonStore('lamp-inputs',{P:[1.5,0,0],light:[3,0,0]},{groups:[radialLink('P',1.5,'surface',[1,0,0]),radialLink('light',1.8,'outside',[1,0,0])]});

 for(const v of [[0,0,0],[.1,.2,.3],[100,0,0],[-1,-2,-3]]){
  s.getState().set('P',v);s.getState().set('light',v);
  expect(Math.hypot(...s.getState().vals.P as number[])).toBeCloseTo(1.5,12);
  expect(Math.hypot(...s.getState().vals.light as number[])).toBeGreaterThanOrEqual(1.8-1e-12);
 }
 expect(()=>s.getState().set('P',[NaN,0,0])).toThrow();
});

test('zero retains prior direction, degenerate history uses declared initial, and cancel restores the complete write',()=>{
 const link=radialLink('p',1.5,'surface',[0,1,1]);
 const fallback=link.resolve({p:[0,0,0]},{p:[0,0,0]}).p as number[];
 expect(fallback[0]).toBe(0);expect(fallback[1]).toBeCloseTo(1.5/Math.sqrt(2),12);
 const s=lessonStore('radial-zero-paths',{p:[0,1.5,0]},{groups:[link]});
 const restore=s.getState().checkpoint(['p']);
 writeMove('radial-zero-paths',{p:[0,0,0]});
 expect(s.getState().vals.p).toEqual([0,1.5,0]);
 s.getState().set('p',[1,2,3]);
 const prior=s.getState().vals.p;
 s.getState().set('p',[0,0,0]);
 expect(s.getState().vals.p).toEqual(prior);
 restore();expect(s.getState().vals.p).toEqual([0,1.5,0]);
 expect(s.getState().shown.p).toEqual([0,1.5,0]);
 const light=radialLink('p',1.8,'outside',[0,1,0]);
 expect(light.resolve({p:[100,50,25]},{p:[0,1.8,0]}).p).toEqual([100,50,25]);
});
