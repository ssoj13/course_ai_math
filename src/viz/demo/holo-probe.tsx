import { localized } from '../../i18n/react';
import {lazyIsland} from '../island';
import {lessonStore} from '../store';
lessonStore('holo-probe',{});
export default localized(lazyIsland(()=>import('./holo-probe.impl').then(m=>m.default)));
