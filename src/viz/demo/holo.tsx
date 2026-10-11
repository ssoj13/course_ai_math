/** SSR-safe demonstration entry. */
import {lazyIsland} from '../island';
import {ROWSPOLAR} from './engine.rows';
export const ROWSHOLO=ROWSPOLAR.slice(0,4);
export const DemoHolo=lazyIsland(()=>import('./holo.impl').then(m=>m.DemoHolo));
export const DemoFloor=lazyIsland(()=>import('./holo.impl').then(m=>m.DemoFloor));
export const DemoClay=lazyIsland(()=>import('./holo.impl').then(m=>m.DemoClay));
