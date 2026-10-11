import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { lazy, Suspense } from 'react';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Segment } from '../scene/Segment';
import { Point } from '../scene/Point';
import { AreaTile } from '../scene/AreaTile';
import { AngleArc } from '../scene/AngleArc';
import { Label } from '../scene/Label';
import { Dimension } from '../scene/Dimension';
import { Ruler } from '../scene/Ruler';
import { Show, type OverlayDef } from '../scene/handles';
import { useVals } from '../store';
import { Formula } from '../Formula';
import { tri, moveA, moveB } from './engine.rows';
import { ROWSHOLO } from './holo';
const ClayTile=lazy(()=>import('../scene/ClayTile').then(m=>({default:m.ClayTile})));
const ClayCube=lazy(()=>import('../scene/ClayTile').then(m=>({default:m.ClayCube})));
const overlays=(locale:Locale):readonly OverlayDef[]=>[{id:'grid',label:t(locale, 'demo.grid'),on:true},{id:'rulers',label:t(locale, 'holo.rulers'),on:true},{id:'instruments',label:t(locale, 'holo.instruments'),on:true}];
const floorOverlays=(locale:Locale):readonly OverlayDef[]=>[{id:'floor',label:t(locale, 'holo.floor'),on:true},{id:'instruments',label:t(locale, 'holo.instruments'),on:true}];
function Triangle(){
 const v=useVals('demo'),{A,B}=tri(v);
 return <>
 <Show ov="grid"><Grid lesson="demo" extent={7} step={1} axes={false}/></Show>
 <AreaTile lesson="demo" id="a2" a={[0,0]} b={B} role="vecA"/>
 <AreaTile lesson="demo" id="b2" a={A} b={[0,0]} role="vecB"/>
 <AreaTile lesson="demo" id="c2" a={B} b={A} role="vecC"/>
 <Segment lesson="demo" id="la" a={[0,0]} b={B} role="vecA"/>
 <Segment lesson="demo" id="lb" a={[0,0]} b={A} role="vecB"/>
 <Segment lesson="demo" id="lc" a={B} b={A} role="vecC"/>
 <Segment lesson="demo" id="height" a={A} b={[A[0],0]} role="ink" dashed/>
 <AngleArc lesson="demo" id="lth" at={[0,0]} from={B} to={A} r={.6} role="vecA" label="θ"/>
 <Point lesson="demo" id="A" at={v=>tri(v).A} move={moveA} keys={['lb','lth']} role="vecB"/>
 <Point lesson="demo" id="B" at={v=>tri(v).B} move={moveB} keys={['la']} role="vecA"/>
 <Point lesson="demo" id="C" at={[0,0]} role="ink"/>
 <Label lesson="demo" id="A" at={A} offset={[0,-16]}>A</Label>
 <Label lesson="demo" id="B" at={B} offset={[12,12]}>B</Label>
 <Label lesson="demo" id="C" at={[0,0]} offset={[-14,10]}>C</Label>
 <Show ov="rulers"><Ruler lesson="demo" a={[0,0]} b={B}/><Ruler lesson="demo" a={A} b={[0,0]}/><Dimension lesson="demo" id="la" a={[0,-Number(v.la)]} b={[Number(v.la),-Number(v.la)]}/></Show>
 </>;
}
const TEX='\\t{c2}{c^2}=\\t{a2}{a^2}+\\t{b2}{b^2}-2\\t{la}{a}\\t{lb}{b}\\cos(\\t{lth}{\\theta})';
export function DemoHolo(){const locale=useLocale();return <ViewportCard lesson="demo" id="holo" name={t(locale, 'holo.name')} mode="2d" extent={6.5} center={[1,0]} overlays={overlays(locale)} rows={ROWSHOLO(locale)} instruments angleKey="lth" formula={<Formula lesson="demo" tex={TEX} label={t(locale, 'holo.formula')}/>}><Triangle/></ViewportCard>;}
export function DemoFloor(){const locale=useLocale();return <ViewportCard lesson="demo" id="floor" name={t(locale, 'holo.nameFloor')} mode="3d" extent={3} instruments overlays={floorOverlays(locale)}>
 <Grid lesson="demo" extent={3} step={1}/>
 <Segment lesson="demo" id="mirror-line" a={[1,0,0]} b={[1,2.5,0]} role="vecA"/>
 <Segment lesson="demo" id="mirror-cross" a={[-1,1.2,0]} b={[2,1.2,0]} role="vecB"/>
 <Point lesson="demo" id="mirror-tip" at={[1,2.5,0]} role="vecA"/>
 <AreaTile lesson="demo" id="floor-glass" a={[-2,1,0]} b={[-1,1,0]} normal={[0,0,1]} role="vecC" flip/>
 </ViewportCard>;}
export function DemoClay(){const locale=useLocale();return <ViewportCard lesson="demo" id="clay" name={t(locale, 'holo.nameClay')} mode="3d" extent={4} look="clay" instruments overlays={[{id:'instruments',label:t(locale, 'holo.instruments'),on:true}]}>
 <Grid lesson="demo" extent={4} step={1} axes={false}/>
 <Suspense fallback={null}>
 {Array.from({length:9},(_,i)=><ClayTile key={i} lesson="demo" id={'tile'+i} at={[-2.4+i%3*.85,.13,-1+Math.floor(i/3)*.85]} size={[.8,.24,.8]} role={i===8?'clayAccent':'clayA'} negative={i===8}/>)}
 {Array.from({length:8},(_,i)=><ClayCube key={i} lesson="demo" id={'cube'+i} at={[1.2+(i%2)*.85,.42+Math.floor(i/4)*.85,-.4+(Math.floor(i/2)%2)*.85]} size={[.8,.8,.8]} role={i<4?'clayB':'clayC'}/>)}
 </Suspense>
 <Ruler lesson="demo" a={[-2.8,0,1.4]} b={[-.2,0,1.4]} normal={[0,1,0]}/>
 </ViewportCard>;}
