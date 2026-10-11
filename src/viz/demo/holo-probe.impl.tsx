import { t } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import {lazy,Suspense} from 'react';
const FixProbe=lazy(()=>import('./holo-fix-probe.impl'));
import {ViewportCard} from '../scene/ViewportCard';
import {Segment} from '../scene/Segment';
import {Parallelogram} from '../scene/Parallelogram';
import {Show} from '../scene/handles';
export default function Probe(){const kind=new URLSearchParams(location.search).get('case');return kind?<Suspense fallback={null}><FixProbe kind={kind}/></Suspense>:<Calibration/>;}
function Calibration(){const locale=useLocale();return <ViewportCard lesson="holo-probe" id="probe" name={t(locale, 'probe.name')} mode="2d" extent={7} overlays={[{id:'filaments',label:t(locale, 'probe.filaments'),on:true},{id:'glass',label:t(locale, 'probe.glass'),on:false}]}>
 <Show ov="filaments"><Segment lesson="holo-probe" id="horizontal" a={[-5,0]} b={[5,0]} role="vecA"/><Segment lesson="holo-probe" id="vertical" a={[0,-5]} b={[0,5]} role="vecB"/></Show>
 <Show ov="glass"><Parallelogram lesson="holo-probe" id="positive" at={[-4,1]} a={[3,0]} b={[0,3]} role="vecA"/><Parallelogram lesson="holo-probe" id="negative" at={[1,4]} a={[3,0]} b={[0,-3]} role="vecB"/></Show>
 </ViewportCard>;}
