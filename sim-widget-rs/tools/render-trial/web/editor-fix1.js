import init,{EditorTrial} from "./pkg-editor-fix1/sim_render_trial.js";
const canvas=document.getElementById("editor"),error=document.getElementById("error"),loading=document.getElementById("loading");
let host,raf=0,busy=false,again=false,timer=0,pendingMove=null,space=false,captured=null;
const messages=[];const metrics={frames:0,events:0,coalescedMoves:0};window.editorMetrics=metrics;
function fail(reason){error.textContent=String(reason);window.editorDiagnostic=String(reason);loading.hidden=true;}
function send(kind,event={},extra={}){if(!host)return;const message={kind,x:event.clientX||0,y:event.clientY||0,button:Number.isInteger(event.button)&&event.button>=0?event.button:0,pointer:event.pointerId||1,shift:!!event.shiftKey,alt:!!event.altKey,ctrl:!!event.ctrlKey,meta:!!event.metaKey,space,...extra};const source=JSON.stringify(message);if(source.length>8192||messages.length>=2048)throw new Error("Input queue capacity exceeded");messages.push(source);invalidate();}
function invalidate(){if(!host)return;if(busy){again=true;return;}if(!raf)raf=requestAnimationFrame(frame);}
async function frame(now){raf=0;if(busy){again=true;return;}busy=true;try{
 const rect=canvas.getBoundingClientRect();host.set_canvas_rect(rect.left,rect.top);
 if(pendingMove){const move=pendingMove;pendingMove=null;send("move",move);metrics.coalescedMoves++;}
 for(const source of messages.splice(0)){host.event(source);metrics.events++;}
 const data=JSON.parse(await host.render(rect.width,rect.height,devicePixelRatio,now));window.editorFrame=data;metrics.frames++;
 loading.hidden=true;error.textContent="";document.documentElement.lang=data.locale;
 if(data.playing)again=true;
 clearTimeout(timer);if(data.repaint_ms!==null&&data.repaint_ms!==undefined)timer=setTimeout(invalidate,Math.max(1,data.repaint_ms));
 }catch(reason){fail(reason);}finally{busy=false;if(again){again=false;invalidate();}}}
for(const type of ["mousedown","auxclick"])canvas.addEventListener(type,event=>{if(event.button===1&&event.cancelable)event.preventDefault();},{capture:true,passive:false});
canvas.addEventListener("pointerdown",event=>{if(!host||captured!==null)return;try{canvas.focus({preventScroll:true});send("down",event);captured=event.pointerId;canvas.setPointerCapture(captured);if(event.cancelable)event.preventDefault();}catch(reason){fail(reason);}},{passive:false});
canvas.addEventListener("pointermove",event=>{pendingMove=event;invalidate();},{passive:false});
function finish(event,cancel=false){if(captured!==event.pointerId)return;try{
 if(pendingMove){send("move",pendingMove);pendingMove=null;}
 send(cancel?"cancel":"up",event);
 }catch(reason){fail(reason);}finally{const pointer=captured;captured=null;if(canvas.hasPointerCapture(pointer))canvas.releasePointerCapture(pointer);invalidate();}}
canvas.addEventListener("pointerup",event=>finish(event));canvas.addEventListener("pointercancel",event=>finish(event,true));canvas.addEventListener("lostpointercapture",event=>{if(captured===event.pointerId)finish(event,true);});
canvas.addEventListener("pointerleave",event=>{if(captured===null)send("leave",event);});
canvas.addEventListener("wheel",event=>{if(!host)return;if(event.cancelable)event.preventDefault();const scale=event.deltaMode===1?16:event.deltaMode===2?canvas.clientHeight:1;try{send("wheel",event,{delta_x:event.deltaX*scale,delta_y:-event.deltaY*scale});}catch(reason){fail(reason);}},{passive:false});
canvas.addEventListener("contextmenu",event=>{if(space||event.altKey||captured!==null)event.preventDefault();});
canvas.addEventListener("keydown",event=>{
 if(event.code==="Space")space=true;
 if((event.ctrlKey||event.metaKey)&&["l","t","w","n","r"].includes(event.key.toLowerCase()))return;
 try{send("key",event,{key:event.code==="Space"?"Space":event.key,pressed:true});
 if(event.key.length===1&&!event.ctrlKey&&!event.metaKey&&!event.altKey)send("text",event,{text:event.key});
 if(["Escape","Enter","Tab","Backspace","Delete","ArrowLeft","ArrowRight","ArrowUp","ArrowDown","Home","End","PageUp","PageDown"," "].includes(event.key))event.preventDefault();
 if(event.key==="Escape"){send("cancel",event);if(captured!==null){const pointer=captured;captured=null;if(canvas.hasPointerCapture(pointer))canvas.releasePointerCapture(pointer);}pendingMove=null;invalidate();}
 }catch(reason){fail(reason);}
});
canvas.addEventListener("keyup",event=>{if(event.code==="Space")space=false;try{send("key",event,{key:event.code==="Space"?"Space":event.key,pressed:false});}catch(reason){fail(reason);}});
canvas.addEventListener("paste",event=>{event.preventDefault();try{send("paste",event,{text:event.clipboardData?.getData("text/plain")||""});}catch(reason){fail(reason);}});
function release(){space=false;pendingMove=null;try{if(host)send("blur");}catch(reason){fail(reason);}finally{if(captured!==null){const pointer=captured;captured=null;if(canvas.hasPointerCapture(pointer))canvas.releasePointerCapture(pointer);}invalidate();}}
window.addEventListener("blur",release);document.addEventListener("visibilitychange",()=>{if(document.hidden)release();else invalidate();});window.addEventListener("resize",invalidate);
new ResizeObserver(invalidate).observe(canvas);
window.addEventListener("pagehide",()=>{clearTimeout(timer);if(raf)cancelAnimationFrame(raf);try{host?.dispose();}catch{}});
await init();host=await EditorTrial.create(canvas);window.editorHost=host;canvas.focus({preventScroll:true});invalidate();
