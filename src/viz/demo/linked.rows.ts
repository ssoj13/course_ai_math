import type {Row} from '../scene/NPanel';
export const LINKED_ROWS:readonly Row[]=[
 {id:'th',label:'θ',kind:'num',key:'th',spec:{min:0,max:180,step:1}},
 {id:'x',label:'x',kind:'num',key:'x',spec:{min:0,max:4,step:.1}},
 {id:'A',label:'A',kind:'num',key:'A',spec:{min:0,max:16,step:.25},glyph:'rect'},
 {id:'V',label:'V',kind:'num',key:'V',spec:{min:0,max:64,step:1}},
 {id:'n',label:'n',kind:'num',key:'n',spec:{min:1,max:3,step:1}},
 {id:'k',label:'k',kind:'num',key:'k',spec:{min:-3,max:3,step:1}},
 {id:'a',label:'a',kind:'vec',key:'a',dims:2,spec:{min:-4,max:4,step:.1}},
 {id:'b',label:'b',kind:'vec',key:'b',dims:2,spec:{min:-4,max:4,step:.1}},
 {id:'sum',label:'a·b',kind:'ro',value:v=>(v.a as number[]).reduce((s,x,i)=>s+x*(v.b as number[])[i],0),glyph:'sum',glyphValue:v=>(v.a as number[]).map((x,i)=>x*(v.b as number[])[i])},
];
