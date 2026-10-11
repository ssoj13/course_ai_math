import { useLoader } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { BackSide, SRGBColorSpace, TextureLoader } from 'three';
import { RoundedBoxGeometry } from 'three/examples/jsm/geometries/RoundedBoxGeometry.js';
import { usePalette } from '../palette';
import { useHot } from '../store';
import { HOVER_RANK, useHoverable } from './handles';
import { SLine } from './view';
export interface ClayProps {lesson:string;id:string;at?:[number,number,number];size?:[number,number,number];role?:'clayA'|'clayB'|'clayC'|'clayAccent';negative?:boolean}
export function ClayTile({lesson,id,at=[0,.12,0],size=[1,.24,1],role='clayA',negative=false}:ClayProps){
 const p=usePalette(),hot=useHot(lesson,id),hover=useHoverable(lesson,id,HOVER_RANK.fill);
 const matcap=useLoader(TextureLoader,import.meta.env.BASE_URL.replace(/\/$/, '')+'/matcap-clay.png');matcap.colorSpace=SRGBColorSpace;
 const geo=useMemo(()=>new RoundedBoxGeometry(...size,3,Math.min(...size)*.06),[...size]);
 useEffect(()=>()=>geo.dispose(),[geo]);
 const [x,y,z]=size;
 return <group position={at} {...hover}>
 <mesh rotation={[-Math.PI/2,0,0]} position={[0,-y/2-.002,0]} renderOrder={-.1} raycast={()=>{}}>
 <planeGeometry args={[x*1.4,z*1.4]}/><shaderMaterial transparent depthWrite={false}
 vertexShader={`varying vec2 st;void main(){st=uv;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}`}
 fragmentShader={`varying vec2 st;void main(){float d=length((st-.5)*2.);gl_FragColor=vec4(0.,0.,0.,.45*(1.-smoothstep(.2,1.,d)));}`}/>
 </mesh>
 {hot&&<mesh geometry={geo} scale={[1.035,1.035,1.035]}><meshBasicMaterial color={p.hot} side={BackSide}/></mesh>}
 <mesh geometry={geo}><meshMatcapMaterial matcap={matcap} color={p[role]}/></mesh>
 {negative&&<><SLine points={[[-x/2,y/2+.002,-z/2],[x/2,y/2+.002,-z/2],[x/2,y/2+.002,z/2],[-x/2,y/2+.002,z/2],[-x/2,y/2+.002,-z/2]]} color={p.clayAccent} lineWidth={2}/>
 <mesh rotation={[-Math.PI/2,0,0]} position={[0,y/2+.003,0]}><planeGeometry args={[x*.9,z*.9]}/><shaderMaterial transparent depthWrite={false} vertexShader={`varying vec2 st;void main(){st=uv;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}`} fragmentShader={`varying vec2 st;void main(){float line=step(.8,fract((st.x+st.y)*12.));gl_FragColor=vec4(.2,.15,.12,line*.65);}`}/></mesh></>}
 </group>;
}
export function ClayCube(p:ClayProps){return <ClayTile {...p} size={p.size??[1,1,1]}/>;}
