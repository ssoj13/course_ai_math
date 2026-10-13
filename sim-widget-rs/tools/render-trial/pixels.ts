// Lossless file-format encoding of the exact GPU readback; pixels are never modified.
import { deflateSync } from "node:zlib";
const input=Bun.argv[2],output=Bun.argv[3];
if(!input||!output)throw new Error("pixels.ts input.ppm output.png");
const bytes=new Uint8Array(await Bun.file(input).arrayBuffer());
let ends:number[]=[];for(let i=0;i<bytes.length&&ends.length<3;i++)if(bytes[i]===10)ends.push(i);
const header=new TextDecoder().decode(bytes.subarray(0,ends[2]+1));
const match=/^P6\n(\d+) (\d+)\n255\n$/.exec(header);if(!match)throw new Error("Unsupported PPM readback");
const width=Number(match[1]),height=Number(match[2]),rgb=bytes.subarray(ends[2]+1);
if(rgb.length!==width*height*3)throw new Error("GPU readback dimensions mismatch");
const rows=new Uint8Array((width*3+1)*height);
for(let y=0;y<height;y++)rows.set(rgb.subarray(y*width*3,(y+1)*width*3),y*(width*3+1)+1);
function crc32(data:Uint8Array){let c=0xffffffff;for(const v of data){c^=v;for(let i=0;i<8;i++)c=(c>>>1)^(0xedb88320&-(c&1));}return (c^0xffffffff)>>>0;}
function chunk(type:string,data:Uint8Array){const out=new Uint8Array(data.length+12),view=new DataView(out.buffer);view.setUint32(0,data.length);out.set(new TextEncoder().encode(type),4);out.set(data,8);view.setUint32(data.length+8,crc32(out.subarray(4,data.length+8)));return out;}
const ihdr=new Uint8Array(13),view=new DataView(ihdr.buffer);view.setUint32(0,width);view.setUint32(4,height);ihdr[8]=8;ihdr[9]=2;
const chunks=[new Uint8Array([137,80,78,71,13,10,26,10]),chunk("IHDR",ihdr),chunk("IDAT",deflateSync(rows,{level:9})),chunk("IEND",new Uint8Array())];
const png=new Uint8Array(chunks.reduce((n,c)=>n+c.length,0));let offset=0;for(const c of chunks){png.set(c,offset);offset+=c.length;}
await Bun.write(output,png);
console.log(JSON.stringify({input,output,width,height,bytes:png.length,sha256:new Bun.CryptoHasher("sha256").update(png).digest("hex"),method:"Exact native GPU RGB readback losslessly PNG-encoded; no pixel edits"}));
