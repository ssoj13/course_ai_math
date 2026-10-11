/** Narrow, pure write contracts. No subscriptions or reactive dependency graph. */
import type { Val } from './store';
export type Values = Readonly<Record<string, Val>>;
export interface LinkGroup {
 keys: readonly string[];
 canonical: string;
 resolve(input: Readonly<Record<string, Val>>, current: Values): Record<string, Val>;
 shown(values: Values): Record<string, Val>;
}
export interface StepLink { key: string; min: number; max: number; value(i: number): number; index(v: number): number }
export interface LessonLinks { groups?: readonly LinkGroup[]; timelines?: Readonly<Record<string, StepLink>> }
export const finite = (v: Val): void => {
 if (typeof v === 'number' ? !Number.isFinite(v) : !v.every(Number.isFinite)) throw new Error('Lesson values must be finite');
};
export const boundIndex = (i: number, max: number): number => Number.isFinite(i) ? Math.max(0,Math.min(max,Math.trunc(i))) : 0;
export function powersLinks(): LessonLinks {
 const limits: Record<string,number> = {x:4,A:16,V:64};
 return {
  groups: [{
   keys:['x','A','V','from'], canonical:'x',
   resolve(input) {
    const channels=Object.keys(input).filter(k=>k in limits);
    if (!channels.length) throw new Error('Powers provenance is derived from x, A or V');
    const xs=channels.map(k=>{const v=input[k];if(typeof v!=='number')throw new Error('Powers input must be scalar');finite(v);const c=Math.max(0,Math.min(limits[k],v));return k==='x'?c:k==='A'?Math.sqrt(c):Math.cbrt(c)});
    if(xs.some(x=>Math.abs(x-xs[0])>1e-10))throw new Error('Conflicting linked powers inputs');
    const x=xs[0], from=['x','A','V'].indexOf(channels[0]);
    if ('from' in input && input.from!==from) throw new Error('Conflicting powers provenance');
    return {x,A:x*x,V:x*x*x,from};
   },
   shown(v) {const x=v.x as number;return {A:x*x,V:x*x*x}},
  }],
  timelines: {
   power: {key:'n',min:1,max:3,value:i=>i+1,index:n=>n-1},
   ladder: {key:'k',min:-3,max:3,value:i=>3-i,index:k=>3-k},
  },
 };
}
