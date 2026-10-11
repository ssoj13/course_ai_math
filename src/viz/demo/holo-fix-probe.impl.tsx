import {Component,useState,type ReactNode} from 'react';
import {useFrame} from '@react-three/fiber';
import {Vector3,type Mesh} from 'three';
import {ViewportCard} from '../scene/ViewportCard';
import {Quad,Parallelogram} from '../scene/Parallelogram';
import {AreaTile} from '../scene/AreaTile';
import {Arrow} from '../scene/Arrow';
import {Segment} from '../scene/Segment';
import {Point} from '../scene/Point';
import {Dimension} from '../scene/Dimension';
import {Grid} from '../scene/Grid';
import {Ruler} from '../scene/Ruler';
import {ClayTile,ClayCube} from '../scene/ClayTile';
import {useStage} from '../scene/handles';
const lesson='holo-probe';
class Boundary extends Component<{children:ReactNode;report:(s:string)=>void},{failed:boolean}>{
 state={failed:false};static getDerivedStateFromError(){return {failed:true};}
 componentDidCatch(error:Error){this.props.report(error.message);}
 render(){return this.state.failed?null:this.props.children;}
}
function Inspect(){
 const reg=useStage();
 useFrame(({scene,camera})=>{
  if(!reg.el)return;
  const project=(p:number[])=>{const v=new Vector3(...p).project(camera);return[(v.x+1)*reg.el!.clientWidth/2,(1-v.y)*reg.el!.clientHeight/2];};
  const records:unknown[]=[];
  scene.traverse(o=>{const m=o as Mesh;if(!m.geometry)return;
   const materials=Array.isArray(m.material)?m.material:[m.material];for(const mat of materials){const material=mat as typeof mat & {color?:{getHexString:()=>string}};const pos=m.getWorldPosition(new Vector3()).toArray();records.push({kind:'material',type:mat.type,geometry:m.geometry.type,color:material.color?.getHexString(),opacity:mat.opacity,blending:mat.blending,side:mat.side,mirrored:m.matrixWorld.determinant()<0,screen:project(pos),order:m.renderOrder});}const g=m.geometry,at=g.getAttribute('plane');
   if(at)records.push({kind:'glass',world:Array.from(g.getAttribute('position').array),plane:Array.from(at.array),mirrored:m.matrixWorld.determinant()<0});
   if(g.getAttribute('position')?.count===6&&g.index?.count===6){const line=m.parent?.children.find(x=>(x as Mesh).geometry?.getAttribute('instanceStart')) as Mesh|undefined;
    if(line){const a=line.geometry.getAttribute('instanceStart'),b=line.geometry.getAttribute('instanceEnd');records.push({kind:'dimension',a:project([a.getX(0),a.getY(0),a.getZ(0)]),b:project([b.getX(0),b.getY(0),b.getZ(0)]),baseA:project([-2,1,0]),baseB:project([2,2,-1])});}}
  });
  reg.el.dataset.probe=JSON.stringify(records);
 },3);return null;
}
export default function FixProbe({kind}:{kind:string}){
 const [error,setError]=useState(''),invalid=kind.startsWith('reject-'),name=kind.slice(7);
 const p={lesson,id:'subject',role:'vecA' as const};
 const subjects:Record<string,ReactNode>={Quad:<Quad {...p} o={[0,0]} u={[1,0]} v={[0,1]} neg={false}/>,AreaTile:<AreaTile {...p} a={[0,0]} b={[1,0]}/>,Parallelogram:<Parallelogram {...p} a={[1,0]} b={[0,1]}/>,Arrow:<Arrow {...p} to={[1,1]}/>,Segment:<Segment {...p} a={[0,0]} b={[1,1]}/>,Point:<Point {...p} at={[1,1]}/>};
 const clay=invalid||kind==='clay',mode=kind==='grid'?'2d':'3d';
 return <><output data-probe-error style={{position:"absolute"}}>{error}</output><ViewportCard lesson={lesson} id="fix" name={kind} mode={mode} extent={4} look={clay?'clay':'holo'} overlays={[{id:'floor',label:'Reflective floor',on:mode==='3d'}]}>
 <Boundary report={setError}>{invalid?subjects[name]:kind==='grid'?<>
 <Parallelogram {...p} at={[-3.5,.5]} a={[2,0]} b={[0,2]}/><Parallelogram {...p} id="skew" at={[.5,.5]} a={[2,0]} b={[.8,2]}/>
 </>:clay?<><ClayCube lesson={lesson} id="cube" at={[-1,1,0]} size={[1,1,1]}/><ClayTile lesson={lesson} id="negative" at={[1,1,0]} size={[1,.2,1]} negative/><Grid lesson={lesson} step={1} extent={4}/><Ruler lesson={lesson} a={[-2,0,0]} b={[2,0,0]} normal={[0,1,0]}/></>:<>
 {kind==="arrow"?<Arrow {...p} from={[-2,1,0]} to={[2,2,-1]}/>:<Segment {...p} a={[-2,1,0]} b={[2,2,-1]}/>}<Dimension lesson={lesson} id="length" a={[-2,1,0]} b={[2,2,-1]} normal={[0,0,1]}/>
 <Parallelogram {...p} id="glass" at={[-1,1,1]} a={[2,0,0]} b={[.5,2,.5]}/></>}</Boundary><Inspect/>
 </ViewportCard></>;
}
