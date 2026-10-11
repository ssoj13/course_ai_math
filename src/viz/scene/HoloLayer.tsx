import type { ReactNode } from 'react';
import { useStore } from 'zustand';
import { lessonStore } from '../store';
import { InstrumentSync } from './InstrumentSync';
import { Background } from './Background';
import { FloorFade, Mirror } from './Floor';
import { useStage, overlayOn } from './handles';
import type { Look } from './look';
export function HoloLayer({children,look}:{children:ReactNode;look:Look}){
 const reg=useStage(), card=reg.card;
 const floor=useStore(lessonStore(card?.lesson??'scene'),s=>{
  const def=card?.overlays.find(o=>o.id==='floor');
  return reg.mode==='3d'&&!!card&&!!def&&overlayOn(s.vals,card.id,def);
 });
 return <><InstrumentSync/><Background look={look}/>{floor&&<><FloorFade/><Mirror>{children}</Mirror></>}</>;
}
