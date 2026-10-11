import {useFrame} from '@react-three/fiber';
import {Vector3} from 'three';
import {useStage} from './handles';
import {wppAt} from './view';
const pt=new Vector3(),direction=new Vector3();
export function InstrumentSync(){
 const reg=useStage();
 useFrame(({camera})=>{
  const el=reg.el;if(!el)return;
  const scale=(1/wppAt(camera,[0,0,0],el.clientHeight)).toFixed(3);
  camera.getWorldDirection(direction);
  const azimuth=(Math.atan2(-direction.x,-direction.z)*180/Math.PI).toFixed(3);
  if(el.dataset.unitPx!==scale)el.dataset.unitPx=scale;
  if(el.dataset.azimuth!==azimuth)el.dataset.azimuth=azimuth;
  // Exact projection of the world's y axis, not invented screen ticks.
  const ticks:number[][]=[];
  for(let y=-Math.ceil(reg.extent*8);y<=Math.ceil(reg.extent*8);y++){
   pt.set(0,y,0).project(camera);
   if(pt.z>=-1&&pt.z<=1&&pt.y>=-.95&&pt.y<=.95)ticks.push([y,(1-pt.y)*el.clientHeight/2]);
  }
  const json=JSON.stringify(ticks.map(([y,p])=>[y,+p.toFixed(2)]));
  if(el.dataset.yTicks!==json)el.dataset.yTicks=json;
 },2);
 return null;
}
