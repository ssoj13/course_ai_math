import type {LinkGroup,Values} from '../links';
import {finite} from '../links';
/** Lamp/probe input contract, shared by fields and handles. Zero retains the previous or declared initial direction. */
export function radialLink(key:string,radius:number,mode:'surface'|'outside',initial:readonly number[]):LinkGroup {
 if(initial.length!==3||!initial.every(Number.isFinite)||Math.hypot(...initial)<1e-12||!Number.isFinite(radius)||radius<=0)throw new Error('Radial input needs a nonzero initial direction and positive radius');
 const normalize=(value:Values[string],fallback:Values[string]):number[]=>{
  if(typeof value==='number'||value.length!==3)throw new Error('Radial input needs three components');
  finite(value);
  const n=Math.hypot(...value);
  // A radial bound preserves direction, including requests beyond the field's component bounds.
  const v=n>1e-12?value:typeof fallback!=='number'&&fallback?.length===3&&fallback.every(Number.isFinite)&&Math.hypot(...fallback)>1e-12?fallback:initial;
  const len=Math.hypot(...v),target=mode==='surface'||n<radius?radius:n;
  return Math.abs(target-len)<1e-12?[...v]:v.map(x=>x*target/len);
 };
 return {keys:[key],canonical:key,rangePolicy:'radial',resolve:(input,current)=>({[key]:normalize(input[key],current[key])}),shown:vals=>({[key]:normalize(vals[key],initial)})};
}
