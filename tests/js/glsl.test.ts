import {test,expect} from 'bun:test';
import {readFileSync} from 'node:fs';
import {compactGLSL} from '../../src/lib/compact-glsl.mjs';
test('every bundled Three shader retains its lexical tokens and preprocessor directives',()=>{
 const code=readFileSync('node_modules/three/build/three.module.js','utf8');
 const tokens=(s:string)=>s.match(/\+\+|--|==|!=|<=|>=|&&|\|\||\+=|-=|\*=|\/=|[A-Za-z_]\w*|(?:\d*\.\d+|\d+\.?\d*)(?:e[+-]?\d+)?|[^\s]/gi);
 let count=0;
 for(const match of code.matchAll(/\bvar (\w+) = ("(?:\\.|[^"\\])*");/g)){
 const shader:string=JSON.parse(match[2]);if(!shader.includes('\n')||!/\b(vec[234]|float|uniform|varying)\b|#include/.test(shader))continue;
 count++;const packed=compactGLSL(shader);
 expect(tokens(packed)).toEqual(tokens(shader));
 expect(packed.split('\n').filter(l=>l.startsWith('#'))).toEqual(shader.split('\n').map(l=>l.trim()).filter(l=>l.startsWith('#')));
 }
 expect(count).toBe(86);
});
