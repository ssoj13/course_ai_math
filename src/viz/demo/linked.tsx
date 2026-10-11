import {t} from '../../i18n/core';
import {localized,useLocale} from '../../i18n/react';
import {lessonStore,useStep} from '../store';
import {powersLinks} from '../links';
import {lazyIsland} from '../island';
import {Formula} from '../Formula';
import {Num} from '../Num';
import {NumTable} from '../NumTable';
import {Timeline} from '../Timeline';
import {FieldCell} from '../scene/NPanel';
import {CodeView} from '../CodeView';
lessonStore('linked-demo',{th:60,x:3,A:9,V:27,from:0,n:2,k:3,a:[1,2],b:[3,4]},powersLinks());
export const LinkedScene=localized(lazyIsland(()=>import('./linked.impl').then(m=>m.LinkedScene)));
export const LinkedSpace=localized(lazyIsland(()=>import('./linked.impl').then(m=>m.LinkedSpace)));
export function LinkedFormula(){
 const locale=useLocale(),step=useStep('linked-demo','proof');
 return <Formula lesson="linked-demo" label={t(locale,'linked.formula')} block
  tex={String.raw`\begin{aligned}\lvert a-b\rvert^2&=\t{sq}{a^2+b^2}-2(\t{p0}{\s{p0}}+\t{p1}{\s{p1}})\\&=\t{distance}{\s{distance}}+\t{cancel}{ab-ab}\end{aligned}`}
  texNarrow={String.raw`\begin{aligned}\lvert a-b\rvert^2&=\t{sq}{a^2+b^2}\\&\quad-2(\t{p0}{\s{p0}}+\t{p1}{\s{p1}})\\&=\t{distance}{\s{distance}}+\t{cancel}{ab-ab}\end{aligned}`}
  derived={{p0:v=>(v.a as number[])[0]*(v.b as number[])[0],p1:v=>(v.a as number[])[1]*(v.b as number[])[1],distance:v=>(v.a as number[]).reduce((s,x,i)=>s+(x-(v.b as number[])[i])**2,0)}}
  marks={{sq:{hidden:step<1},p0:{hidden:step<2},p1:{hidden:step<2},distance:{hidden:step<3},cancel:{hidden:step<2,struck:step>=3}}}
  tips={{p0:{text:t(locale,'linked.productTip'),tex:'a_0 b_0'}}}/>;
}
function Controls(){
 const locale=useLocale();
 return <div data-linked-demo="">
 <Formula lesson="linked-demo" tex={String.raw`\begin{aligned}\t{x}{x=\s{x}}&\quad\t{A}{A=\s{A}}\\\t{V}{V=\s{V}}&\end{aligned}`} label={t(locale,'linked.powers')}
 scrub={{x:{min:0,max:4,step:.1},A:{min:0,max:16,step:.25},V:{min:0,max:64,step:1}}}/>
 <Num lesson="linked-demo" k="A" label="A" spec={{min:0,max:16,step:.25}}/>
 <LinkedFormula/>
 <Formula lesson="linked-demo" label="a · b" tex={String.raw`\t{sum}{a\cdot b}=\t{p0}{\s{p0}}+\t{p1}{\s{p1}}=\s{sum}`} derived={{p0:v=>(v.a as number[])[0]*(v.b as number[])[0],p1:v=>(v.a as number[])[1]*(v.b as number[])[1],sum:v=>(v.a as number[]).reduce((s,x,i)=>s+x*(v.b as number[])[i],0)}}/>
 <FieldCell lesson="linked-demo" size="lg" row={{id:'sum',label:'a · b',kind:'ro',chars:5,value:v=>(v.a as number[]).reduce((s,x,i)=>s+x*(v.b as number[])[i],0),glyph:'sum',glyphValue:v=>(v.a as number[]).map((x,i)=>x*(v.b as number[])[i])}}/>
 <NumTable lesson="linked-demo" rows={[{label:'a',key:'a'},{label:'b',key:'b'}]} op="product"/>
 <Timeline lesson="linked-demo" id="proof" steps={[0,1,2,3].map(i=>({id:String(i),text:t(locale,(['linked.start','linked.squares','linked.products','linked.cancel'] as const)[i])}))}/>
 <Timeline lesson="linked-demo" id="cut" steps={[0,1,2,3].map(i=>({id:String(i),text:t(locale,(['linked.initial','linked.shear','linked.rotate','linked.place'] as const)[i])}))}/>
 <Timeline lesson="linked-demo" id="power" steps={[1,2,3].map(n=>({id:String(n),text:'x^'+n}))}/>
 <Timeline lesson="linked-demo" id="ladder" steps={[3,2,1,0,-1,-2,-3].map(n=>({id:String(n),text:'2^'+n}))}/>
 <CodeView lesson="linked-demo" lang="python" code={'p0 = a[0] * b[0]\np1 = a[1] * b[1]\ndot = p0 + p1'} lines={{1:'p0',2:'p1',3:'sum'}}/>
 </div>;
}
export const LinkedControls=localized(Controls);
