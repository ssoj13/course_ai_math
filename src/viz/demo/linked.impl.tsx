import {AreaTile} from '../scene/AreaTile';
import {t} from '../../i18n/core';
import {useLocale} from '../../i18n/react';
import {ViewportCard} from '../scene/ViewportCard';
import {Grid} from '../scene/Grid';
import {Poly} from '../scene/Poly';
import {Point} from '../scene/Point';
import {lcPiece} from '../scene/polygon';
import {LINKED_ROWS} from './linked.rows';
export function LinkedScene(){
 const locale=useLocale();
 return <ViewportCard lesson="linked-demo" id="LINKED" name={t(locale,'linked.scene')} mode="2d" extent={7} center={[0,0]} rows={LINKED_ROWS}>
  <Grid lesson="linked-demo" extent={7} step={1}/>
  <Poly lesson="linked-demo" id="R1" timeline="cut" pts={(v,s)=>lcPiece(3,4,v.th as number,1,s)} path={(v,a,b,p)=>lcPiece(3,4,v.th as number,1,a+(b-a)*p)} role="vecA" hatched={v=>3-4*Math.cos((v.th as number)*Math.PI/180)<0} label={v=>'R1 = '+(3*(3-4*Math.cos((v.th as number)*Math.PI/180))).toFixed(2)}/>
  <Poly lesson="linked-demo" id="R2" timeline="cut" pts={(v,s)=>lcPiece(3,4,v.th as number,2,s)} path={(v,a,b,p)=>lcPiece(3,4,v.th as number,2,a+(b-a)*p)} role="vecB" hatched={v=>4-3*Math.cos((v.th as number)*Math.PI/180)<0} label={v=>'R2 = '+(4*(4-3*Math.cos((v.th as number)*Math.PI/180))).toFixed(2)}/>
  <Point lesson="linked-demo" id="x" at={v=>[v.x as number,0]} keys={['x']} move={(v,d)=>({set:{x:(v.x as number)+d[0]}})} role="vecC" label="x"/>
 </ViewportCard>;
}

/** Shared-view perspective stress case: the same hatch shader on a pure polygon and a signed area tile. */
export function LinkedSpace(){
 const locale=useLocale();
 return <ViewportCard lesson="linked-demo" id="LINKED-3D" name={t(locale,'linked.scene')+' · 3D'} mode="3d" extent={4}>
  <Poly lesson="linked-demo" id="debt" pts={v=>[[-3,0,0],[-3+(v.x as number),0,0],[-3+(v.x as number),1,0],[-3,1,0]]} role="vecA" hatched={v=>(v.x as number)<4} label={v=>String(v.x)}/>
  <AreaTile lesson="linked-demo" id="signed" a={[0,0,0]} b={[0,2,0]} normal={[0,0,1]} role="vecB" signed area={v=>(v.x as number)-4}/>
  <Point lesson="linked-demo" id="size-demo" at={[0,0,0]} role="vecC" size={10}/>
 </ViewportCard>;
}
