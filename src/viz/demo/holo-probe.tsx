import {lazyIsland} from '../island';
import {lessonStore} from '../store';
lessonStore('holo-probe',{});
export default lazyIsland(()=>import('./holo-probe.impl').then(m=>m.default));
