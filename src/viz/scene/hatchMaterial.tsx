import { useEffect,useMemo } from 'react';
import { useFrame } from '@react-three/fiber';
import { Color,DoubleSide,ShaderMaterial,type BufferGeometry } from 'three';
import { STYLE,usePalette,type SceneRole } from '../palette';
/** Screen-space stripes. gl_FragCoord / drawing-buffer DPR is CSS px in every scissored View. */
export function hatchMaterial(color:string):ShaderMaterial{
 return new ShaderMaterial({
  transparent:true,depthWrite:false,side:DoubleSide,polygonOffset:true,polygonOffsetFactor:-1,
  uniforms:{ink:{value:new Color(color)},dpr:{value:1},period:{value:STYLE.hatchPx},width:{value:STYLE.hatchW}},
  vertexShader:'void main(){gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.0);}',
  fragmentShader:`uniform vec3 ink;uniform float dpr;uniform float period;uniform float width;
   void main(){vec2 p=gl_FragCoord.xy/dpr;float x=(p.x+p.y)*0.70710678;
    float d=abs(mod(x+period*.5,period)-period*.5);
    float a=1.-smoothstep(width*.5-.5,width*.5+.5,d);
    gl_FragColor=vec4(ink,a*.65);
    #include <colorspace_fragment>
   }`,
 });
}
export function Hatch({geometry,role}:{geometry:BufferGeometry;role:SceneRole}):React.JSX.Element {
 const pal=usePalette(),mat=useMemo(()=>hatchMaterial(pal[role]),[pal[role]]);
 useEffect(()=>()=>mat.dispose(),[mat]);
 useFrame(({gl})=>{mat.uniforms.dpr.value=gl.getPixelRatio()});
 return <mesh geometry={geometry} material={mat} frustumCulled={false} renderOrder={.9}/>;
}
