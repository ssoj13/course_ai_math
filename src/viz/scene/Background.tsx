/** Static NDC sky: no time uniform and no invalidation. Every View owns its resolution. */
import { useMemo } from 'react';
import { useFrame } from '@react-three/fiber';
import { Vector2 } from 'three';
import { usePalette } from '../palette';
import { useStage } from './handles';
import type { Look } from './look';
const vertex = `varying vec2 st; void main(){st=uv;gl_Position=vec4(position.xy,0.999,1.);}`;
const fragment = `
precision highp float;
varying vec2 st; uniform vec2 resolution; uniform vec3 top,bottom,star,nebulaA,nebulaB,vignette; uniform float seed,clay;
float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7))+seed)*43758.5453);}
vec3 galaxy(vec2 p, float turn){
 p=mat2(cos(turn),-sin(turn),sin(turn),cos(turn))*p; p.y*=2.7;
 float r=length(p), a=atan(p.y,p.x);
 float arms=pow(.5+.5*cos(a*2.-r*28.),2.);
 float dust=.55+.45*sin(p.x*131.+sin(p.y*73.)*4.);
 return mix(nebulaB*3.4,nebulaA*5.8,arms)*exp(-r*5.)*(.25+arms)*dust+vec3(.12,.17,.25)*exp(-r*38.);
}
void main(){
 vec2 q=st-.5; float edge=smoothstep(.25,.7,length(q));
 vec3 c=mix(bottom,top,st.y);
 if(clay>.5){gl_FragColor=vec4(mix(vec3(.769,.788,.824),vec3(.839,.855,.882),st.y),1.);return;}
 c+=galaxy((st-vec2(.86,.68))*2.5,.7)+galaxy((st-vec2(.14,.84))*3.7,-.4);
 c+=nebulaA*.22*exp(-dot(q-vec2(-.3,.25),q-vec2(-.3,.25))*15.);
 c+=nebulaB*.22*exp(-dot(q-vec2(.3,-.2),q-vec2(.3,-.2))*16.);
 c=mix(c,vignette,edge*.55);
 vec2 pix=st*resolution;
 for(int i=0;i<3;i++){
  float cell=22.+float(i)*11.; vec2 g=floor(pix/cell);
  vec2 ctr=(g+vec2(hash(g+float(i)),hash(g+17.+float(i))))*cell;
  float chance=hash(g+vec2(81.+float(i)*13.,5.));
  float rad=mix(.55,1.15,edge)*(.65+.35*hash(g+12.));
  float s=(1.-smoothstep(rad*.2,rad,length(pix-ctr)))*step(.40,chance);
  c+=star*s*mix(.25,.5+.4*edge,hash(g+32.));
 }
 gl_FragColor=vec4(c,1.);
}`;
const rgb=(hex:string)=>[1,3,5].map(i=>parseInt(hex.slice(i,i+2),16)/255);
export function Background({look='holo'}:{look?:Look}){
 const p=usePalette(), reg=useStage();
 const uniforms=useMemo(()=>({resolution:{value:new Vector2(1,1)},top:{value:rgb(p.bgTop)},bottom:{value:rgb(p.bgBottom)},star:{value:rgb(p.star)},nebulaA:{value:rgb(p.nebulaA)},nebulaB:{value:rgb(p.nebulaB)},vignette:{value:rgb(p.vignette)},seed:{value:[...(reg.card?.id??'sky')].reduce((a,c)=>a+c.charCodeAt(0),0)},clay:{value:look==='clay'?1:0}}),[p,reg,look]);
 useFrame(()=>{if(reg.el)uniforms.resolution.value.set(reg.el.clientWidth,reg.el.clientHeight);});
 return <mesh frustumCulled={false} renderOrder={-10} raycast={()=>{}}><planeGeometry args={[2,2]}/><shaderMaterial uniforms={uniforms} vertexShader={vertex} fragmentShader={fragment} depthTest={false} depthWrite={false}/></mesh>;
}
