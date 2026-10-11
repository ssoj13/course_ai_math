import { useRef, type ReactNode } from 'react';
import { useFrame } from '@react-three/fiber';
import { AdditiveBlending, DoubleSide, type Group, type Mesh } from 'three';
import { STYLE, usePalette } from '../palette';
import { MirrorCtx } from './look';
import { useStage } from './handles';
const noRaycast = () => {};
/** Mirror children own their materials as normal. No cloning or per-frame allocation. */
export function Mirror({children}:{children:ReactNode}){
 const group=useRef<Group>(null), reg=useStage();
 useFrame(()=>{
  group.current?.traverse(o=>{
   o.raycast=noRaycast;
   const mesh=o as Mesh;
   if(!mesh.material)return;
   const mats=Array.isArray(mesh.material)?mesh.material:[mesh.material];
   for(const m of mats){
    if(m.userData.mirrorConfigured)continue;
    m.userData.mirrorConfigured=true;
    m.transparent=true;m.depthWrite=false;m.side=DoubleSide;
    const alpha=STYLE.mirrorA*(m.blending===AdditiveBlending?.5:1);
    const previous=m.onBeforeCompile;
    m.onBeforeCompile=(s,renderer)=>{
     previous.call(m,s,renderer);
     // Built-in materials and LineMaterial both declare worldpos_vertex or project_vertex.
     s.vertexShader='varying float mirrorDepth;\n'+s.vertexShader;
     s.vertexShader=s.vertexShader.replace('#include <project_vertex>','#include <project_vertex>\nmirrorDepth=abs((modelMatrix*vec4(transformed,1.)).y);');
     if(!s.vertexShader.includes('mirrorDepth='))
      s.vertexShader=s.vertexShader.replace('gl_Position = clip;','mirrorDepth=abs((modelMatrix*vec4(mix(instanceStart,instanceEnd,position.y<0.5?0.:1.),1.)).y); gl_Position = clip;');
     if(!s.vertexShader.includes('mirrorDepth=')) s.vertexShader=s.vertexShader.replace(/void main\s*\(\s*\)\s*\{/, 'void main(){mirrorDepth=abs((modelMatrix*vec4(position,1.)).y);');
     s.fragmentShader='varying float mirrorDepth;\n'+s.fragmentShader;
     s.fragmentShader=s.fragmentShader.replace(/void main\s*\(\s*\)\s*\{/, 'void main(){');
     const end=s.fragmentShader.lastIndexOf('}');
     s.fragmentShader=s.fragmentShader.slice(0,end)+'gl_FragColor.a *= '+alpha+'*(1.-smoothstep(0.,'+(reg.extent*.8).toFixed(5)+',mirrorDepth));\n'+s.fragmentShader.slice(end);
    };
    m.customProgramCacheKey=()=> 'mirror-'+reg.extent+'-'+alpha;
    m.needsUpdate=true;
   }
   if(!o.userData.mirrorOrder){o.renderOrder-=.5;o.userData.mirrorOrder=true;}
  });
 },.5);
 return <MirrorCtx.Provider value={true}><group ref={group} scale={[1,-1,1]}>{children}</group></MirrorCtx.Provider>;
}
export function Floor(){
 const p=usePalette(),reg=useStage();
 return <mesh rotation={[-Math.PI/2,0,0]} position={[0,-.002,0]} renderOrder={-4} raycast={()=>{}}>
  <planeGeometry args={[reg.extent*16,reg.extent*16]}/>
  <shaderMaterial transparent depthWrite={false} side={DoubleSide} uniforms={{base:{value:p.floor},extent:{value:reg.extent}}}
   vertexShader={`varying vec2 loc; void main(){loc=position.xy;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}`}
   fragmentShader={`varying vec2 loc;uniform float extent;void main(){float f=exp(-dot(loc,loc)/(extent*extent*2.25));gl_FragColor=vec4(mix(vec3(.027,.039,.082),vec3(.059,.094,.188),f*.5),.72);}`}/>
 </mesh>;
}
/** Transparent floor veil; reflection shaders additionally fade with exact world depth. */
export function FloorFade(){ return <Floor/>; }
