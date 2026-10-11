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
float noise(vec2 p){return .5+.1667*(sin(p.x+sin(p.y*1.3))+sin(p.y*.83+sin(p.x*.7))+sin(p.x*.73-p.y*.91));}
float cloud(vec2 p){float n=0.,a=.5;for(int i=0;i<5;i++){n+=a*noise(p);p=mat2(1.6,-1.2,1.2,1.6)*p+17.;a*=.5;}return n;}
vec3 galaxy(vec2 p,float turn){
 p=mat2(cos(turn),-sin(turn),sin(turn),cos(turn))*p;p.y*=2.1;
 float r=length(p),a=atan(p.y,p.x),n=cloud(p*19.);
 float arms=pow(.5+.5*cos(a*2.-log(r+.045)*4.+n*3.),3.);
 float dust=pow(cloud(p*46.+cloud(p*7.)*4.),1.5);
 float halo=exp(-r*3.8),lane=smoothstep(.18,.68,dust);
 return mix(nebulaB*6.,nebulaA*8.,n)*halo*(.12+arms*.65)*lane
 +vec3(.35,.31,.43)*exp(-r*25.)+vec3(.08,.10,.18)*halo*dust;
}
void main(){
 vec2 q=st-.5; float edge=smoothstep(.25,.7,length(q));
 vec3 c=mix(bottom,top,st.y);
 if(clay>.5){gl_FragColor=vec4(mix(vec3(.769,.788,.824),vec3(.839,.855,.882),st.y),1.);return;}
 c+=galaxy((st-vec2(.86,.68))*2.5,.7)+galaxy((st-vec2(.14,.84))*3.7,-.4);
 float dust=cloud(st*7.+vec2(cloud(st*5.),cloud(st*5.+30.))*3.);
 c+=mix(nebulaA,nebulaB,cloud(st*4.+8.))*dust*1.8*smoothstep(.15,.48,length(q));
 c=mix(mix(bottom,top,st.y),c,smoothstep(.30,.47,length(q)));
 c=mix(c,vignette,edge*.55);
 vec2 pix=st*resolution;
 for(int i=0;i<3;i++){
  float cell=22.+float(i)*11.; vec2 g=floor(pix/cell);
  vec2 ctr=(g+vec2(hash(g+float(i)),hash(g+17.+float(i))))*cell;
  float chance=hash(g+vec2(81.+float(i)*13.,5.));
  float rad=mix(1.,1.15,edge)*(.65+.35*hash(g+12.));
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
