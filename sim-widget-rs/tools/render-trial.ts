// Real build/run interfaces. Missing required suites return NOT_RUN with nonzero exit.
import { join,resolve } from "node:path";
const root=resolve(import.meta.dir,"..");
const evidence=resolve(root,"../.superpowers/sdd/sim-widget-rs/task-03-evidence");
const trial=join(root,"tools/render-trial");
function arg(name:string,fallback=""){const i=Bun.argv.indexOf(name);return i<0?fallback:Bun.argv[i+1]??fallback;}
async function command(args:string[],cwd=root){const p=Bun.spawn(args,{cwd,stdout:"inherit",stderr:"inherit"});const exit=await p.exited;if(exit!==0)throw new Error(`${args[0]} exited${exit}`);}
await Bun.$`mkdir -p ${evidence}`.quiet();
const action=Bun.argv[2];
if(action==="build"){
  const profile=arg("--profile","raster-web");
  if(profile!=="raster-web"){console.error(`NOT_RUN build profile${profile}`);process.exit(2);}
  await command(["cargo","build","--manifest-path",join(trial,"Cargo.toml"),"--lib","--target","wasm32-unknown-unknown","--release","--target-dir",join(root,"target")]);
  await command(["wasm-bindgen",join(root,"target/wasm32-unknown-unknown/release/sim_render_trial.wasm"),"--target","web","--out-dir",join(trial,"web/pkg")]);
  const files=["sim_render_trial_bg.wasm","sim_render_trial.js"];
  const artifacts=await Promise.all(files.map(async file=>{const bytes=new Uint8Array(await Bun.file(join(trial,"web/pkg",file)).arrayBuffer());return {file,raw:bytes.length,gzip:Bun.gzipSync(bytes,{level:9}).length,sha256:new Bun.CryptoHasher("sha256").update(bytes).digest("hex")};}));
  await Bun.write(join(evidence,"raster-web-artifacts.json"),JSON.stringify({profile,artifacts,date:new Date().toISOString(),scope:"portable workload; full Task03 closure pending"},null,2));
  console.log(JSON.stringify(artifacts,null,2));
}else if(action==="serve"){
  const port=Number(arg("--port","43874"));
  const server=Bun.serve({hostname:"127.0.0.1",port,async fetch(req){const url=new URL(req.url);const path=url.pathname==="/"?"index.html":url.pathname.slice(1);if(path.includes(".."))return new Response("Forbidden",{status:403});const file=Bun.file(join(trial,"web",path));return await file.exists()?new Response(file):new Response("Not found",{status:404});}});
  console.log(`New compiled renderer trial: ${server.url}`);
}else if(action==="verify"){
  const suite=arg("--suite","portable"),required=arg("--require","native").split(",");
  if(suite!=="portable"){console.error(`NOT_RUN required suite ${suite}; no pass`);process.exit(2);}
  for(const target of required){
    if(target==="native"){await command(["cargo","run","--manifest-path",join(trial,"Cargo.toml"),"--locked","--target-dir",join(root,"target"),"--",join(evidence,"portable-native.json")]);}
    else{console.error(`NOT_RUN ${target}: execute fresh browser trial through authorized CUA; automatic driver not permitted in this session`);process.exit(2);}
  }
}else{console.log("render-trial build --profile raster-web | serve --port43874 | verify --suiteportable --require native");process.exit(action?2:0);}
