import { useEffect, useMemo } from 'react';
import { Color, DoubleSide } from 'three';
import { gridStep } from '../geom';
import { STYLE } from '../palette';
import { useStage } from './handles';
import { useFrame, useThree } from '@react-three/fiber';
export function Glass({color,px,hot}:{color:string;px:number;hot:boolean}){
 const reg=useStage(),invalidate=useThree(s=>s.invalidate);
 useEffect(()=>invalidate(),[invalidate]);
 const uniforms=useMemo(()=>({tint:{value:new Color(color)},stepSize:{value:gridStep(px)},alpha:{value:hot?.18:STYLE.fillA},reveal:{value:1}}),[color,px,hot]);
 useFrame(()=>{uniforms.reveal.value=reg.reveal;});
 return <shaderMaterial transparent depthWrite={false} side={DoubleSide} polygonOffset polygonOffsetFactor={1} polygonOffsetUnits={1} uniforms={uniforms}
 vertexShader={`attribute vec2 plane;varying vec2 planePos;varying vec2 st;void main(){st=uv;planePos=plane;gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.);}`}
 fragmentShader={`varying vec2 planePos;varying vec2 st;uniform vec3 tint;uniform float stepSize,alpha,reveal;void main(){float a=mix(.06,alpha,st.y);if(stepSize>0.){vec2 p=planePos/stepSize;vec2 d=abs(fract(p-.5)-.5)/fwidth(p);float grid=1.-min(1.,min(d.x,d.y));a=max(a,grid*.22);}gl_FragColor=vec4(tint,a*reveal);
#include <tonemapping_fragment>
#include <colorspace_fragment>
}`}/>
}
