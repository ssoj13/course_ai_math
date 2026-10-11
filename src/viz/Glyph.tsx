import {useId} from 'react';
/** Miniature quantitative readouts. Signed shape and explicit zero survive grayscale. */
export function Glyph({kind,value,size='sm'}:{kind:'rect'|'disc'|'sum';value:number|readonly number[];size?:'sm'|'lg'}):React.JSX.Element {
 const clip=useId();
 const vs=typeof value==='number'?[value]:value;
 if(!vs.every(Number.isFinite))throw new Error('Glyph values must be finite');
 const max=Math.max(1,...vs.map(Math.abs)),v=vs[0]??0;
 const rectWidth=14*Math.min(1,Math.abs(v)/4),rectHeight=14*Math.min(1,Math.abs(vs[1]??4)/4);
 return <svg className={'lab-glyph is-'+size} viewBox="0 0 32 20" aria-hidden="true" data-glyph={kind} data-value={vs.join(',')}>
  {kind==='disc'?<><circle cx="16" cy="10" r="8" fill="none" stroke="currentColor"/><circle cx="16" cy="10" r="7" fill="currentColor" opacity={Math.max(0,Math.min(1,v))}/></>:
   kind==='rect'?<><path d="M16 2V18" stroke="currentColor" opacity=".5"/><rect x={v<0?16-rectWidth:16} y={10-rectHeight/2} width={rectWidth} height={rectHeight} fill="currentColor" opacity=".5"/>{v<0&&<><defs><clipPath id={clip}><rect x={16-rectWidth} y={10-rectHeight/2} width={rectWidth} height={rectHeight}/></clipPath></defs><path clipPath={`url(#${clip})`} d="M0 20L20 0M4 20L24 0M8 20L28 0M12 20L32 0" stroke="currentColor"/></>}</>:
   <>{vs.map((x,i)=><rect key={i} x={2+i*28/vs.length} y={x>=0?10-8*x/max:10} width={Math.max(1,26/vs.length-1)} height={8*Math.abs(x)/max} fill="currentColor"/>)}<path d="M1 10H31" stroke="currentColor" opacity=".5"/></>}
  {vs.every(x=>x===0)&&<path d="M13 10H19" stroke="currentColor"/>}
 </svg>;
}
