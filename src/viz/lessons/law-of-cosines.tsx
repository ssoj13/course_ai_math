import { useEffect, useState } from 'react';
import { localized, useLocale } from '../../i18n/react';
import { lessonStore, useStep, useVals } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { Timeline } from '../Timeline';
import { Challenge, type ChallengeProps } from '../Challenge';
import { CodeView } from '../CodeView';
import { Ledger } from '../Ledger';
import { reducedMotion } from '../scene/motion';
import type { Marks } from '../marks';
import { LESSON, INITIAL, derive, solveIK, fmt, fmtCos, fmtAngle, cutProgress, cutElapsed, cutMoves } from './law-of-cosines.math';
import { SCRUB, words } from './law-of-cosines.rows';
lessonStore(LESSON,INITIAL);
export const LawDetail=localized(lazyIsland(()=>import('./law-of-cosines.impl').then(m=>m.LawDetail)));
export const LawScene=localized(lazyIsland(()=>import('./law-of-cosines.impl').then(m=>m.LawScene)));
const tex=String.raw;
const keys=['a','b','th','cos','c','a2','b2','c2','corr','strip_a','strip_b','R1','R2','R1c','R2c','p','q','ab','rest','rest2','rise','rise2','bcos2','bsin2'] as const;
const derived=Object.fromEntries(keys.map(k=>[k,(v:Parameters<typeof derive>[0])=>derive(v)[k]]));
const formats=Object.fromEntries(keys.map(k=>[k,k==='cos'?fmtCos:k==='th'?(x:number)=>fmtAngle(x)+tex`^\circ`:fmt]));
const row=(key:string,body:string)=>tex`\htmlData{m=${key}}{${body}}`;
const aligned=(rows:string[])=>tex`\begin{aligned}${rows.map(x=>'&'+x).join(tex`\\ `)}\end{aligned}`;

export function marksFor(scene:0|1|2|3,step:number,th:number):Marks {
 const zero=th===90;
 if(scene===0)return {'cut-0':{hidden:step<9},'cut-1':{hidden:step<10},'cut-2':{hidden:step<10},'cut-3':{hidden:step<11},'cut-4':{hidden:step<11},strip_a:{dim:zero},strip_b:{dim:zero},corr:{dim:zero}};
 if(scene===2)return {'proof-0':{hidden:step<4,dim:step===9},'proof-1':{hidden:step<6,dim:step===9},'proof-2':{hidden:step<7,dim:step===9},'proof-3':{hidden:step<8},'leg-rest':{dim:step===4},'leg-rise':{dim:step===4},ident:{struck:step>=8},corr:{dim:step===9&&zero}};
 return {corr:{dim:zero}};
}
export function LawFormula({scene,compact=false}:{scene:0|1|2|3;compact?:boolean}) {
 const locale=useLocale(),v=useVals(LESSON),step=useStep(LESSON,scene===2?'proof':'cut');
 const w=(en:string,ru:string)=>words(locale,en,ru);
 if(scene===3){
  const ids=['d','cosraw','cosc','phi'] as const;
  return <Formula lesson={LESSON} block label={w('Two-bone solution','Решение для двух костей')}
   tex={aligned([tex`\t{d}{d}=\sqrt{x^2+y^2}=\s{d}`,tex`\cos\t{phi}{\varphi}=\t{cosphi}{\frac{L_1^2+L_2^2-d^2}{2L_1L_2}}=\s{cosraw}`,tex`\t{clamp}{\operatorname{clamp}}\;\to\;\t{cosc}{\s{cosc}},\quad\t{phi}{\varphi}=\arccos(\s{cosc})=\s{phi}`])}
   texNarrow={aligned([tex`\t{d}{d}=\sqrt{x^2+y^2}=\s{d}`,tex`\cos\t{phi}{\varphi}=\t{cosphi}{\frac{L_1^2+L_2^2-d^2}{2L_1L_2}}`,tex`\t{cosraw}{\s{cosraw}}\;\to\;\t{clamp}{\operatorname{clamp}}\;\to\;\t{cosc}{\s{cosc}}`,tex`\t{phi}{\varphi}=\arccos(\s{cosc})=\s{phi}`])}
   derived={Object.fromEntries(ids.map(k=>[k,x=>solveIK(x)[k]]))} fmt={{d:fmt,cosraw:fmtCos,cosc:fmtCos,phi:x=>fmtAngle(x)+tex`^\circ`}}/>;
 }
 if(compact)return <Formula lesson={LESSON} label={w('Triangle values','Значения треугольника')}
  tex={aligned([tex`\t{a}{a=\s{a}},\;\t{b}{b=\s{b}},\;\t{th}{\theta=\s{th}}`,tex`\t{c}{c=\s{c}},\;\t{c2}{c^2=\s{c2}}`])} scrub={SCRUB} derived={derived} fmt={formats}/>;
 const cut=[
  row('cut-0',tex`\t{c2}{c^2}=\t{R1}{a\cdot DB}+\t{R2}{b\cdot EA}`),
  row('cut-1',tex`=(\t{a2}{a^2}-\t{strip_a}{a\cdot\t{p}{b\cos\theta}})+(\t{b2}{b^2}-\t{strip_b}{b\cdot\t{q}{a\cos\theta}})`),
  row('cut-2',tex`=\t{a2}{a^2}+\t{b2}{b^2}\t{corr}{-2ab\cos\theta}`),
  row('cut-3',tex`\s{c2}=\s{a2}+\s{b2}-2\cdot\s{strip_a}`),
  row('cut-4',tex`\t{ab}{\mathbf{a}\cdot\mathbf{b}}=\t{strip_a}{\lvert\mathbf{a}\rvert\,(b\cos\theta)}=\s{strip_a}`)
 ];
 const proof=[
  row('proof-0',tex`\t{c2}{c^2}=\htmlData{m=leg-rest}{\t{rest2}{(a-b\cos\theta)^2}}+\htmlData{m=leg-rise}{\t{rise2}{(b\sin\theta)^2}}`),
  row('proof-1',tex`=\t{a2}{a^2}\t{corr}{-2ab\cos\theta}+\t{bcos2}{b^2\cos^2\theta}+\t{bsin2}{b^2\sin^2\theta}`),
  row('proof-2',tex`=\t{a2}{a^2}+\t{b2}{b^2\,\t{ident}{(\cos^2\theta+\sin^2\theta)}}\t{corr}{-2ab\cos\theta}`),
  row('proof-3',tex`=\t{a2}{a^2}+\t{b2}{b^2}\t{corr}{-2ab\cos\theta}`)
 ];
 const narrow=scene===0?[cut[0],row('cut-1',tex`=(\t{a2}{a^2}-\t{strip_a}{a\cdot\t{p}{b\cos\theta}})`),row('cut-1',tex`\quad+(\t{b2}{b^2}-\t{strip_b}{b\cdot\t{q}{a\cos\theta}})`),...cut.slice(2)]
  :[row('proof-0',tex`\t{c2}{c^2}=\htmlData{m=leg-rest}{\t{rest2}{(a-b\cos\theta)^2}}`),row('proof-0',tex`\quad+\htmlData{m=leg-rise}{\t{rise2}{(b\sin\theta)^2}}`),row('proof-1',tex`=\t{a2}{a^2}\t{corr}{-2ab\cos\theta}`),row('proof-1',tex`\quad+\t{bcos2}{b^2\cos^2\theta}+\t{bsin2}{b^2\sin^2\theta}`),row('proof-2',tex`=\t{a2}{a^2}+\t{b2}{b^2\,\t{ident}{(\cos^2\theta+\sin^2\theta)}}`),row('proof-2',tex`\t{corr}{-2ab\cos\theta}`),proof[3]];
 const ledger=[tex`\t{c2}{c^2}=\t{a2}{a^2}+\t{b2}{b^2}\t{corr}{-2ab\cos\theta}`,tex`\s{c2}=\s{a2}+\s{b2}\t{corr}{-2\cdot\s{a}\cdot\s{b}\cdot\s{cos}}`,tex`\t{c}{c=\s{c}},\quad\t{th}{\theta=\s{th}},\;\cos\theta=\s{cos}`];
 return <Formula lesson={LESSON} block label={w('Law of cosines derivation','Вывод теоремы косинусов')}
  tex={aligned(scene===0?cut:scene===2?proof:ledger)} texNarrow={aligned(scene===1?[ledger[0],tex`\s{c2}=\s{a2}+\s{b2}`,tex`\t{corr}{-2\cdot\s{a}\cdot\s{b}\cdot\s{cos}}`,tex`\t{c}{c=\s{c}},\quad\t{th}{\theta=\s{th}}`,tex`\cos\theta=\s{cos}`]:narrow)}
  marks={marksFor(scene,step,v.th as number)} derived={derived} fmt={formats} scrub={SCRUB}
  tips={{corr:{text:w('Two equal signed strips','Две равные полосы со знаком'),tex:'-2ab\\cos\\theta'},p:{text:w('Signed shadow of b on a','Тень b на a со знаком')},q:{text:w('Signed shadow of a on b','Тень a на b со знаком')}}}/>;
}
const CUT_EN=["Three squares, one on each side. Pythagoras promises a² + b² = c² — but only for a square corner. Here 9 + 16 is not 13. Twelve units of area are missing. Where did they go?","Shine a light straight down onto side a. Side b casts a shadow of length b cos θ (lesson 0.4: cos is the shadow of a length-1 stick).","Cut a strip of that width off the square on a. Its area is a × (b cos θ).","Now light side b instead: a casts a shadow a cos θ on it. Cut that strip off the square on b. Its area is b × (a cos θ) — the same number. Two different rectangles, one area: a·b·cos θ.","What is left of the two squares: a rectangle a × DB and a rectangle b × EA.","The square on c can be split too: drop the height from C onto c and extend it. Two rectangles, c × FB and c × AF.","Watch the leftover rectangle on a move. First it leans: sliding the top edge sideways keeps the base and the height, so the area does not change.","Then it turns a quarter turn around B. Turning changes nothing about area.","Lean it back upright: it is exactly the rectangle c × FB. The leftover of a has become the first part of c².","The leftover of b does the same dance around A (lean, quarter turn counter-clockwise, lean) and lands as c × AF. Together they fill the square on c: c² = a·DB + b·EA.","Count the paper. The leftovers were the squares minus the strips. So c² = a² + b² − (strip) − (strip) = a² + b² − 2·a·b cos θ. Drag θ to 90°: the shadows shrink to a point, the strips vanish, and this is Pythagoras — in fact, the dance you just watched is Euclid's own proof of it. Drag past 90°: the shadows point backwards, the strips flip to hatched (negative area) and get *added*.","One more look at the strip: its area is the length of side a times the shadow of side b on it. In the next lessons the sides become arrows, written in bold **a**, **b**, with lengths |**a**|, |**b**|; the strip is |**a**| (|**b**| cos θ). Lesson 1.4 gives that product a name — the dot product **a**·**b**, a new meaning of the dot (not plain multiplication) — and shows it needs no angle at all. So the law of cosines reads |**a** − **b**|² = |**a**|² + |**b**|² − 2 **a**·**b**: a Pythagoras with two rectangles cut out."];
const CUT_RU=["Три квадрата, по одному на каждой стороне. Пифагор обещает a² + b² = c² лишь при прямом угле. Здесь 9 + 16 не равно 13. Куда делись двенадцать единиц площади?","Направим свет сверху на сторону a. Сторона b отбрасывает тень длины b cos θ. Косинус — тень палочки единичной длины.","Отрежем от квадрата на a полосу ширины тени. Её площадь a × (b cos θ).","Теперь осветим сторону b. Тень стороны a равна a cos θ. Площадь второй полосы b × (a cos θ) — то же число. Разные прямоугольники, одна площадь a·b·cos θ.","Остатки квадратов — прямоугольники a × DB и b × EA.","Опустим высоту из C на c и продолжим её через квадрат. Он разделится на прямоугольники c × FB и c × AF.","Остаток квадрата на a наклоняется. Двигается дальняя сторона, основание и высота сохраняются, поэтому площадь не меняется.","Повернём его вокруг B на четверть оборота по часовой стрелке. Поворот сохраняет площадь.","Сдвинем сторону обратно: получился прямоугольник c × FB, первая часть c².","Остаток на b повторяет три движения вокруг A: наклон, четверть оборота против часовой стрелки, наклон обратно. Получается c × AF. Вместе остатки заполняют c² = a·DB + b·EA.","Посчитаем: c² = a² + b² − полоса − полоса = a² + b² − 2ab cos θ. При 90° тени исчезают: Пифагор. При тупом угле тени отрицательны, полосы заштрихованы, и вычитание добавляет их площадь.","Площадь полосы — длина a, умноженная на тень b. В уроке 1.4 стороны станут векторами, а произведение получит имя: скалярное произведение a·b. Тогда |a − b|² = |a|² + |b|² − 2a·b. Здесь точка между векторами означает новую операцию, а не обычное умножение."];
const PROOF_EN=["Your triangle again: sides a and b meet at C with angle θ; c is the gap. No square corner, so Pythagoras does not apply — yet.","Drop a perpendicular from A onto the line of a. Call the foot D. Now there ARE right angles.","CD is the shadow of b on a: CD = b cos θ. The height AD = b sin θ.","The rest of side a is DB = a − b cos θ. (If θ > 90°, b cos θ is negative and D is behind C: DB is longer than a. The formula does not care.)","Triangle ADB is right-angled at D. Pythagoras: c² = DB² + AD².","Substitute the two legs we named.","Expand the square: (a − b cos θ)² = a² − 2ab cos θ + b² cos²θ. The middle term is the correction — it is the two strips of the first scene, written as algebra.","b² cos²θ + b² sin²θ: two pieces of the same square.","cos²θ + sin²θ = 1 (the unit-circle point is at distance 1). The two pieces merge into b².","Done: c² = a² + b² − 2ab cos θ. Drag θ to 90°: cos θ = 0 and this is Pythagoras."];
const PROOF_RU=["Стороны a и b сходятся в C под углом θ, а c — зазор. Прямого угла пока нет.","Опустим перпендикуляр из A на прямую a. Его основание D. Теперь прямой угол есть.","CD — тень b на a: CD = b cos θ. Высота AD = b sin θ.","Остаток DB = a − b cos θ. При тупом θ тень отрицательна, D позади C, а DB длиннее a.","Треугольник ADB прямоугольный: c² = DB² + AD².","Подставим выражения для двух катетов.","Раскроем квадрат: (a − b cos θ)² = a² − 2ab cos θ + b² cos² θ. Среднее слагаемое — искомая поправка, две полосы первой сцены.","b² cos² θ + b² sin² θ — две части одного квадрата.","cos² θ + sin² θ = 1: точка единичной окружности на расстоянии 1. Части складываются в b².","Готово: c² = a² + b² − 2ab cos θ. При 90° cos θ = 0 и остаётся Пифагор."];


/** Directional lesson links preserve the originating guide/interaction source.
 * A new hover starts a new set; expansions never accumulate across interactions. */
function SemanticLinks(){
 useEffect(()=>{
  const store=lessonStore(LESSON);
  const links:Record<string,readonly string[]>={
   sq_a:['a2'],a2:['sq_a'],sq_b:['b2'],b2:['sq_b'],sq_c:['c2'],c2:['sq_c'],
   p:['drop_a','D'],q:['drop_b','E'],strip_a:['p'],strip_b:['q'],
   th:['cos'],corr:['strip_a','strip_b','p','q'],ab:['strip_a'],
   R1c:['R1'],R2c:['R2'],cosraw:['cosphi'],cosphi:['cosraw','l1','l2','d'],
   cosc:['clamp'],ident:['bcos2','bsin2'],rise2:['bsin2'],bsin2:['rise2'],
   clamp:['cosc','gap','reach_max'],hand:['ik_hand'],ik_hand:['hand'],
   l1:['ik_l1'],ik_l1:['l1'],l2:['ik_l2'],ik_l2:['l2']
  };
  const sync=()=>{
   const st=store.getState(),ids=new Set(st.hot);
   for(const id of ids)for(const linked of links[id]??[])ids.add(linked);
   if(ids.size!==st.hot.size)st.setHot([...ids],st.hotSource??'interaction');
  };
  sync();return store.subscribe(sync);
 },[]);
 return null;
}


const TRANSPORTS=['cut','proof_fade'] as const;
type Transport=typeof TRANSPORTS[number];
/** One scheduled frame services both independent lesson motions. */
function TransportClock(){
 const store=lessonStore(LESSON);
 useEffect(()=>{
  let raf=0,disposed=false;
  const elapsed:Record<Transport,number>={cut:0,proof_fade:0},last:Record<Transport,number>={cut:0,proof_fade:0};
  const stop=()=>{cancelAnimationFrame(raf);raf=0};
  const ensure=()=>{if(!disposed&&!raf&&TRANSPORTS.some(k=>store.getState().vals[k+'_play']))raf=requestAnimationFrame(tick)};
  function tick(now:number){
   raf=0;if(disposed)return;const st=store.getState(),update:Record<string,number>={};
   for(const k of TRANSPORTS)if(st.vals[k+'_play']){
    elapsed[k]+=Math.max(0,now-last[k]);last[k]=now;
    const p=cutProgress(elapsed[k],st.vals[k+'_phases'] as number),from=st.vals[k+'_from'] as number,to=st.vals[k+'_to'] as number;
    update[k+'_progress']=p;update[k+'_position']=from+(to-from)*p;if(p===1)update[k+'_play']=0;
   }
   st.setMany(update);ensure();
  }
  const off=store.subscribe((st,prev)=>{
   for(const k of TRANSPORTS){
    const timeline=k==='cut'?'cut':'proof';
    if(st.step[timeline]!==prev.step[timeline]){
     const step=st.step[timeline]??0,from=prev.vals[k+'_position'] as number,to=k==='cut'?step:step>=8?1:0;
     const moves=k==='cut'?cutMoves(from,to,step):step>=7&&from!==to,play=moves&&!reducedMotion();
     elapsed[k]=0;last[k]=performance.now();
     st.setMany({[k+'_from']:from,[k+'_to']:to,[k+'_phases']:k==='cut'&&to===9?3:1,[k+'_progress']:play?0:1,[k+'_position']:play?from:to,[k+'_play']:play?1:0});
    }else if(st.vals[k+'_play']&&!prev.vals[k+'_play']){
     elapsed[k]=cutElapsed(st.vals[k+'_progress'] as number,st.vals[k+'_phases'] as number);last[k]=performance.now();
    }
   }
   if(TRANSPORTS.some(k=>store.getState().vals[k+'_play']))ensure();else stop();
  });
  const media=matchMedia('(prefers-reduced-motion: reduce)');
  const reduce=()=>{if(media.matches){stop();const st=store.getState();st.setMany(Object.fromEntries(TRANSPORTS.flatMap(k=>[[k+'_play',0],[k+'_progress',1],[k+'_position',st.vals[k+'_to']]])))}};
  media.addEventListener('change',reduce);
  return()=>{disposed=true;stop();off();store.getState().setMany({cut_play:0,proof_fade_play:0});media.removeEventListener('change',reduce)};
 },[store]);
 return null;
}
function MotionTransport({kind}:{kind:Transport}){
 const locale=useLocale(),v=useVals(LESSON),step=useStep(LESSON,kind==='cut'?'cut':'proof'),store=lessonStore(LESSON);
 const w=(en:string,ru:string)=>words(locale,en,ru),[reduced,setReduced]=useState(false);
 useEffect(()=>{const media=matchMedia('(prefers-reduced-motion: reduce)'),sync=()=>setReduced(media.matches);sync();media.addEventListener('change',sync);return()=>media.removeEventListener('change',sync)},[]);
 const from=v[kind+'_from'] as number,to=v[kind+'_to'] as number,progress=v[kind+'_progress'] as number,playing=!!v[kind+'_play'];
 if(kind==='cut'?!cutMoves(from,to,step):!(step>=7&&from!==to))return null;
 const scrub=(p:number)=>store.getState().setMany({[kind+'_play']:0,[kind+'_progress']:p,[kind+'_position']:from+(to-from)*p});
 return <div data-lc-transport={kind==='cut'?'':undefined} data-lc-proof-transport={kind==='proof_fade'?'':undefined} className="not-content">
  <p>{kind==='cut'?w('Movement within the selected step. Pause or scrub to inspect the shear and turn; this does not advance the explanation.','Движение внутри выбранного шага. Остановите его или двигайте ползунок, чтобы рассмотреть сдвиг и поворот; шаг объяснения не меняется.'):w('Inspect the two tiles fading into the square on b. This controls opacity, not area or the explanation step.','Рассмотрите, как две плитки исчезают и появляется квадрат на b. Ползунок меняет прозрачность, а не площадь или шаг объяснения.')}</p>
  <button type="button" disabled={reduced||(!playing&&progress===1)} onClick={()=>store.getState().set(kind+'_play',playing?0:1)}>
   {playing?w('Pause movement','Пауза движения'):w('Resume movement','Продолжить движение')}
  </button>{' '}
  <label style={{display:'grid',gridTemplateColumns:'minmax(0,1fr)',maxWidth:'100%'}}>{w('Transition progress ','Ход перехода ')}<input style={{width:'100%',minWidth:0,boxSizing:'border-box'}} type="range" min={0} max={1} step={.01} value={progress}
   aria-label={kind==='cut'?w('Movement progress within this step','Ход движения внутри шага'):w('Tile fade progress within this step','Ход смены плиток внутри шага')} aria-valuetext={Math.round(progress*100)+'%'}
   onChange={e=>scrub(Number(e.currentTarget.value))}/></label>
  <output aria-live={playing?'off':'polite'}>{Math.round(progress*100)+'%'}</output>
 </div>;
}

function Controls({scene}:{scene:0|1|2|3}){
 const locale=useLocale(),v=useVals(LESSON),z=derive(v),ik=solveIK(v);
 const w=(en:string,ru:string)=>words(locale,en,ru);
 const cut=useStep(LESSON,'cut'),proof=useStep(LESSON,'proof');
 const cutIntro=w(`Three squares: a² + b² = ${fmt(z.a2)} + ${fmt(z.b2)} = ${fmt(z.pyth)}; c² = ${fmt(z.c2)}. `,`Три квадрата: a² + b² = ${fmt(z.a2)} + ${fmt(z.b2)} = ${fmt(z.pyth)}; c² = ${fmt(z.c2)}. `)+
  (z.th===90?w('The correction is zero: Pythagoras applies.','Поправка нулевая: действует теорема Пифагора.'):
   z.corr<0?w(`Subtract ${fmt(-z.corr)} units of area. Where does this correction come from?`,`Вычитаем ${fmt(-z.corr)} единиц площади. Откуда берётся эта поправка?`):
   w(`Add ${fmt(z.corr)} units of area: subtracting negative strips adds area.`,`Прибавляем ${fmt(z.corr)} единиц площади: вычитание отрицательных полос добавляет площадь.`));
 const proofIntro=w(`Sides a and b meet at C with θ = ${fmtAngle(z.th)}°. `,`Стороны a и b сходятся в C под углом θ = ${fmtAngle(z.th)}°. `)+
  (z.th===90?w('This is a right angle, so Pythagoras already applies.','Это прямой угол, поэтому теорема Пифагора уже применима.'):
   w('This included angle is not right. Drop a height to build right triangles.','Этот угол не прямой. Опустим высоту, чтобы получить прямоугольные треугольники.'));
 const cutTexts=(locale==='ru'?CUT_RU:CUT_EN).map((text,i)=>(i===0?cutIntro:text)+
  (i===4&&(z.R1<0||z.R2<0)?w(' Here a strip is wider than its square: the hatched leftover is a signed debt.',' Здесь полоса шире квадрата: заштрихованный остаток означает долг.'): '')+
  (i>=5&&i<=9&&z.degenerate?w(' c = 0: there is no square on c; move A to continue.',' c = 0: квадрата на c нет; переместите A.'): ''));
 const proofTexts=(locale==='ru'?PROOF_RU:PROOF_EN).map((text,i)=>(i===0?proofIntro:text)+
  (i===2&&z.th===90?w(' The shadow is zero; D sits on C.',' Тень нулевая, D совпадает с C.'): '')+
  (i===4&&(z.th===0||z.th===180)?w(' Flattened case: AD = 0; the identity still holds.',' Вырожденный случай: AD = 0, равенство сохраняется.'): ''));
 const challenges:Omit<ChallengeProps,'lesson'>[]=scene===0?[
  {n:1,text:w('Make both strips disappear.','Уберите обе полосы.'),check:x=>x.th===90,hint:[w('Set θ to 90°.','Установите θ = 90°.')]},
  {n:2,text:w('Make the strips negative and c larger than √(a² + b²).','Сделайте полосы отрицательными, а c больше √(a² + b²).'),check:x=>(x.th as number)>90},
  {n:3,text:w('Find a strip wider than its square.','Найдите полосу шире её квадрата.'),check:x=>{const d=derive(x);return d.p>d.a+1e-9||d.q>d.b+1e-9},hint:[w('Short a, long b, small θ.','Короткая a, длинная b, малый θ.')]},
  {n:4,text:w('Make the strips look the same, not just measure the same.','Сделайте полосы одинаковыми по форме, а не только по площади.'),check:x=>Math.abs((x.a as number)-(x.b as number))<1e-9,success:w('Equal sides give congruent strips. At flat angles unequal sides can also give congruent rectangles after rotation.','При равных сторонах полосы равны. При вырожденном угле неравные стороны тоже могут дать равные прямоугольники после поворота.')}
 ]:scene===1?[
  {n:5,text:w('Make the correction disappear without changing a or b.','Уберите поправку, не меняя a и b.'),check:x=>x.th===90},
  {n:6,text:w('Make c longer than a + b.','Сделайте c длиннее a + b.'),check:x=>x.th===180,success:w('Impossible: c ≤ a + b always. At 180° you reach equality.','Невозможно: c ≤ a + b. При 180° достигается равенство.')},
  {n:7,text:w('Make c = 5 with a = 3 and b = 4.','Получите c = 5 при a = 3 и b = 4.'),check:x=>x.a===3&&x.b===4&&x.th===90}
 ]:scene===2?[
  {n:8,text:w('Find the step where the correction term is born.','Найдите шаг, где появляется поправка.'),check:(_x,s)=>s.proof===6,hint:[w('Select the expansion step (7 / 10), even after jumping to the end.','Выберите раскрытие квадрата (7 / 10), даже если уже перешли в конец.')]}
 ]:[
  {n:9,text:w('Put the hand at full reach: φ = 180°, still reachable.','Поставьте кисть на полный вылет: φ = 180°, цель достижима.'),check:x=>{const d=solveIK(x);return Math.abs(d.d-d.reachMax)<.05&&d.d<=d.reachMax+1e-9}},
  {n:10,text:w('Make the solver clamp.','Заставьте солвер ограничить косинус.'),check:x=>Math.abs(solveIK(x).cosraw)>1,success:w('The target cannot form a triangle with these bones. Clamp keeps this solver finite.','Цель не образует треугольник с этими костями. Ограничение сохраняет конечные углы.')},
  {n:11,text:w('Find a target with φ = 90°.','Найдите цель с φ = 90°.'),check:x=>Math.abs(solveIK(x).cosc)<1e-9,hint:[w('With lengths 2 and 1.5, use target (2.5, 0).','При длинах 2 и 1.5 поставьте цель (2.5, 0).')]}
 ];
 return <section className="not-content" data-lc-controls={scene}>
  {scene===0&&<><SemanticLinks/><TransportClock/></>}
  {scene!==3&&<LawFormula scene={scene} compact/>}
  {scene===0&&<MotionTransport kind="cut"/>}
  {scene===2&&<MotionTransport kind="proof_fade"/>}
  {scene===0&&<Timeline lesson={LESSON} id="cut" steps={cutTexts.map((text,i)=>({id:String(i),text,phases:i===9?3:1,focus:['sq_c','p','strip_a','strip_b','R1','drop_c','R1','R1','R1c','R2c','corr','ab'][i]}))}/>}
  {scene===2&&<Timeline lesson={LESSON} id="proof" steps={proofTexts.map((text,i)=>({id:String(i),text,focus:['c','drop_a','p','rest','c2','rest2','corr','ident','b2','corr'][i]}))}/>}
  <div style={scene===1?{display:'flex',flexWrap:'wrap',gap:'1rem',alignItems:'center'}:undefined}>
  <div style={{minWidth:0,flex:'1 1 260px'}}><LawFormula scene={scene}/></div>
  {scene===1&&<Ledger lesson={LESSON} label={w('Area balance','Баланс площадей')} scale={3.4} max={64} fmt={fmt} zeroLabel={w('= Pythagoras','= Пифагор')} items={[{id:'a2',label:'a²',role:'vecA',value:x=>derive(x).a2},{id:'b2',label:'b²',role:'vecB',value:x=>derive(x).b2},{id:'corr',label:'−2ab cos θ',role:'vecC',value:x=>derive(x).corr}]} total={{id:'c2',label:'c²',value:x=>derive(x).c2}}/>}
  </div>
  <p aria-live="polite">{scene===3?w('Actual wrist','Кисть')+': ('+ik.W.map(fmt).join(', ')+'); '+w('target','цель')+': ('+ik.T.map(fmt).join(', ')+'). '+(ik.zero?w('Target on shoulder: direction is a convention, +x.','Цель на плече: направление условно, +x.'):ik.outside?w('Target outside reach; arm straight.','Цель вне досягаемости, рука прямая.'):ik.inside?w('Target too close; arm folded.','Цель слишком близко, рука сложена.'):w('Target reachable.','Цель достижима.'))
   :w('Signed areas','Площади со знаком')+': a² '+fmt(z.a2)+', b² '+fmt(z.b2)+', '+w('strip','полоса')+' '+fmt(z.strip_a)+', R₁ '+fmt(z.R1)+', R₂ '+fmt(z.R2)+', c² '+fmt(z.c2)+'. '+(z.degenerate?w('c = 0: no square on c.','c = 0: квадрата на c нет.'):z.DB<0?w('DB is negative because b cos θ > a.','DB отрицателен, потому что b cos θ > a.'):z.th===90?w('Zero correction: Pythagoras.','Нулевая поправка: Пифагор.'): '')}</p>
  {scene===3&&<CodeView lesson={LESSON} lang="ts" code={'d = Math.hypot(hand[0], hand[1]);\nc = (L1*L1 + L2*L2 - d*d) / (2*L1*L2);\nc = Math.max(-1, Math.min(1, c));\nphi = Math.acos(c);'} lines={{1:'d',2:'cosphi',3:'clamp',4:'phi'}}/>}
  {challenges.map(c=><Challenge key={c.n} lesson={LESSON} {...c}/>)}
  <span hidden data-current-cut={cut} data-current-proof={proof}/>
 </section>;
}
export const LawControls=localized(Controls);

function DetailControls({proof=false}:{proof?:boolean}){
 const locale=useLocale(),v=useVals(LESSON),step=useStep(LESSON,proof?'proof':'cut'),store=lessonStore(LESSON),z=derive(v),key=proof?'proof_detail':'detail';
 const options=proof?[['c²',4],['(a − b cos θ)²',4],['(b sin θ)²',4],['a²',6],['b² cos² θ',7],['b² sin² θ',7],['b²',8]] as const:[['c²',0],['a²',0],['b²',0],['a·p',2],['b·q',3],['R₁',4],['R₂',4],['c·FB',5],['c·AF',5],['b cos θ',1],['a cos θ',3]] as const;
 const selected=(v[key+'_part'] as number)??0,values=proof?[z.c2,z.rest2,z.rise2,z.a2,z.bcos2,z.rise2,z.b2]:[z.c2,z.a2,z.b2,z.strip_a,z.strip_b,z.R1,z.R2,z.R1c,z.R2c,z.p,z.q];
 const available=(i:number)=>proof?(i===6?step>=7&&(v.proof_fade_position as number)>0:i===4||i===5?step>=7&&(v.proof_fade_position as number)<1:step>=options[i][1]&&(i!==1||step<8)&&(i!==2||step<7)):step>=options[i][1];
 const w=(en:string,ru:string)=>words(locale,en,ru);
 return <section className="not-content" data-lc-detail-controls={proof?undefined:''} data-lc-proof-detail-controls={proof?'':undefined}>
  <p>{w('The overview keeps the whole identity in view. This detail shows the same moving piece at a readable scale. Select a piece, then Fit after moving or changing its shape. The camera stays fixed while you manipulate the lesson.','Общий вид сохраняет всю картину равенства. Здесь тот же движущийся фрагмент показан крупно. Выберите фрагмент и нажмите «Вписать» после его перемещения или изменения. Камера остаётся неподвижной при работе с уроком.')}</p>
  <label>{w('Inspect ','Рассмотреть ')}<select value={selected} onChange={e=>store.getState().set(key+'_part',Number(e.currentTarget.value))}>
   {options.map(([label],i)=><option key={label} value={i} disabled={!available(i)}>{label}</option>)}
  </select></label>{' '}
  <button type="button" onClick={()=>store.getState().set(key+'_fit',((store.getState().vals[key+'_fit'] as number)??0)+1)}>{w('Fit selected piece','Вписать фрагмент')}</button>
  <p aria-live="polite">{proof&&step<4?w('Tiles appear at the Pythagoras step (5 / 10). ','Плитки появляются на шаге Пифагора (5 / 10). '):''}{options[selected][0]+' = '+fmt(values[selected])+'. '+(!available(selected)?w('This piece is hidden at the current step.','На текущем шаге этот фрагмент скрыт.'):!proof&&selected>=9?(values[selected]===0?w('Zero signed projection length: the shadow has zero length.','Нулевая длина проекции со знаком: длина тени равна нулю.'):w('The same signed projection length as the overview.','Та же длина проекции со знаком, что на общем виде.')):values[selected]===0?w('Zero signed area: no filled shape.','Нулевая площадь со знаком: заливки нет.'):w('The same signed area as the overview.','Та же площадь со знаком, что на общем виде.'))}</p>
 </section>;
}
export const LawDetailControls=localized(DetailControls);
