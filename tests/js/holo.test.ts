import {test,expect} from 'bun:test';
import {rulerTicks,dimensionLayout,dot,sub,norm} from '../../src/viz/geom';
import {assertSubject,hotAlpha,lightened} from '../../src/viz/scene/look';
test('ruler ticks preserve metre scale and suppress unreadable labels',()=>{
 expect(rulerTicks(3,23).filter(t=>t.label)).toEqual([]);
 expect(rulerTicks(3,24).filter(t=>t.label).map(t=>t.value)).toEqual([0,1,2,3]);
 expect(rulerTicks(3,40).map(t=>t.value)).toEqual([0,.5,1,1.5,2,2.5,3]);
});
test('planar dimension helper preserves Euclidean length',()=>{
 const a=[2,1],b=[5,5],wpp=.03,p=dimensionLayout(a,b,wpp);
 expect(dot(sub(b,a),p.out)).toBeCloseTo(0,12);expect(norm(p.out)/wpp).toBeCloseTo(14,12);
 expect(norm(sub(p.b,p.a))).toBeCloseTo(5,12);
});
test('hot pulse is bounded and reduced motion is static',()=>{
 expect(hotAlpha(0,false)).toBeCloseTo(.3);expect(hotAlpha(.6,false)).toBeCloseTo(.42);
 expect(hotAlpha(0,true)).toBe(hotAlpha(.6,true));expect(lightened('#000000')).toBe('#595959');
});
test('clay studio permits measurement guides, rejects holographic subjects',()=>{
 expect(()=>assertSubject('clay',false)).not.toThrow();expect(()=>assertSubject('clay',true)).toThrow();
 expect(()=>assertSubject('holo',true)).not.toThrow();
});
