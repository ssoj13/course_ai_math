import { dot, tensor } from '../../src/core/index';
import { fileURLToPath } from 'node:url';
const bits = (n: number): string => { const b = new ArrayBuffer(8); const v = new DataView(b); v.setFloat64(0,n,false); return v.getBigUint64(0,false).toString(16).padStart(16,'0'); };
const operands = [
 { name: 'pythagoras', a: [3,4], b: [3,4] },
 { name: 'cancellation-late-small', a: [1e16,1,-1e16], b: [1,1,1] },
 { name: 'cancellation-early-small', a: [1e16,-1e16,1], b: [1,1,1] },
 { name: 'ten-decimals', a: Array(10).fill(0.1), b: Array(10).fill(1) },
 { name: 'mixed-products', a: [0.125,-2.75,12345.5,1e-100], b: [8,3,0.001,-1e100] },
 { name: 'negative-zero', a: [-0], b: [1] },
];
const result = { version: 1, source: 'src/core/index.ts dot', dtype:'fp64', order:'sequential',
 cases: operands.map(c=>({...c,a_bits:c.a.map(bits),b_bits:c.b.map(bits),result_bits:bits(dot(tensor(c.a),tensor(c.b),{arith:'fp64',order:'sequential'}))})) };
const output = JSON.stringify(result,null,2)+'\n';
const path = fileURLToPath(new URL('../tests/fixtures/runtime/dot-fp64.json',import.meta.url));
if (process.argv.includes('--check')) {
 if (await Bun.file(path).text() !== output) throw new Error('runtime parity fixture differs from actual TypeScript core');
 console.log('6 fp64 sequential dot fixtures match actual TypeScript core');
} else { await Bun.write(path,output); console.log(path); }
