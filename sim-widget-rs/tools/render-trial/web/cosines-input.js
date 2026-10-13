import init,{CosinesTrial} from "./pkg-input-reviewed/sim_render_trial.js";
import {Messages,hostLocale} from "./input-i18n.js";
const el=id=>document.getElementById(id),scene=el("scene"),canvas=el("canvas"),messages=new Messages(hostLocale());
let host,data,raf=0,last=0,gesture=null,pending=null,hover=null,space=false,error=null,view3=false;
const semanticName=id=>({body:messages.t("body"),theta:messages.t("theta"),"side.a":messages.t("sideA"),"side.b":messages.t("sideB"),"side.c":messages.t("sideC"),"gizmo.x":messages.t("x"),"gizmo.y":messages.t("y"),"gizmo.z":messages.t("z")}[id]||id);
const labelNodes=new Map(),metrics={frames:0,pointerEvents:0,coalescedMoves:0,queries:0};
window.cosinesInputMetrics=metrics;
function fail(reason){error=reason;window.cosinesDiagnostic=String(reason);el("status").textContent=messages.error(reason);el("status").classList.add("error");}
function invoke(fn){try{fn();error=null;invalidate();return true;}catch(reason){fail(reason);return false;}}
function invalidate(){if(host&&!raf)raf=requestAnimationFrame(frame);}
function rect(){const b=scene.getBoundingClientRect();host.set_viewport_rect(b.left,b.top,b.width,b.height,devicePixelRatio);return b;}
function localize(){
 document.documentElement.lang=messages.locale;el("locale").value=messages.locale;document.title=messages.t("title");
 for(const[id,key]of [["title","title"],["intro","intro"],["fit","fit"],["params","params"],["resultlabel","result"],["help","help"],["reset","reset"],["caption","caption"],["back","back"],["loading","loading"],["frame-label","frame"],["constraint-label","constraint"],["selection-label","selection"],["cycle","cycle"],["undo","undo"],["redo","redo"],["tool-label","profile"],["candidate-label","candidates"],["navigation-hint","navigation"],["drag-hint","dragHint"],["touch-hint","touch"],["aa-status","aa"],["time-label","timeline"]])el(id).textContent=messages.t(key);
 for(const[id,key]of [["locale","locale"],["canvas","canvas"],["a-range","sideA"],["b-range","sideB"],["theta-range","angle"],["a","sideA"],["b","sideB"],["theta","angle"],["time","time"],["nudge","nudge"]])el(id).setAttribute("aria-label",messages.t(key));
 document.querySelector(".viewport").setAttribute("aria-label",messages.t("scene"));
 for(const node of document.querySelectorAll("[data-title]")){node.title=messages.t(node.dataset.title);node.setAttribute("aria-label",node.title);}
 document.querySelectorAll(".units").forEach((node,index)=>node.textContent=index<2?messages.t("unit"):"°");
 for(const[id,options]of [["constraint",["screen","xy","yz","xz","x","y","z"]],["selection",["O","A","B","body","theta"]],["tool",["direct","orbit","pan","dolly"]]]){
  const node=el(id),value=node.value;node.replaceChildren(...options.map(key=>{const option=document.createElement("option");option.value=key;option.textContent=messages.t({O:"vertexO",A:"vertexA",B:"vertexB"}[key]||key);return option;}));if(value)node.value=value;
 }
 el("viewname").textContent=messages.t(view3?"persp":"ortho");el("play").textContent=messages.t(data?.playing?"pause":"play");
 if(host)host.set_locale(messages.locale);
}
function draw(){
 const box=rect();data=JSON.parse(host.render(box.width,box.height,devicePixelRatio));window.cosinesFrame=data;metrics.frames++;
 for(const name of ["a","b","theta"]){if(document.activeElement!==el(name))el(name).value=data.display[name].replaceAll(",",".");el(name+"-range").value=data[name];}
 el("result").value=messages.number.format(data.c2);el("formula").textContent=data.formula;el("expanded").textContent=data.expanded;
 el("time").value=data.fraction;el("play").textContent=messages.t(data.playing?"pause":"play");el("undo").disabled=!data.can_undo;el("redo").disabled=!data.can_redo;
 if(["O","A","B","body","theta"].includes(data.selected))el("selection").value=data.selected;
 el("candidates").textContent=[...new Set(data.candidates)].map(semanticName).join(" · ");scene.dataset.hover=String(Boolean(data.hovered));
 for(const label of data.labels){let node=labelNodes.get(label.id);if(!node){node=document.createElement("span");el("labels").append(node);labelNodes.set(label.id,node);}node.className="scene-label "+label.role+(data.hovered===label.id||data.selected===label.id?" hot":"");node.textContent=label.text;node.hidden=!label.position;if(label.position){node.style.left=(label.position[0]+label.offset[0])+"px";node.style.top=(label.position[1]+label.offset[1])+"px";}}
 el("receipt").textContent=JSON.stringify(data,null,2);
 if(!error){el("status").classList.remove("error");el("status").textContent=messages.t("ready")+" · "+messages.t("hintSelected")+": "+semanticName(data.selected);}
}
function flushMove(){
 if(!pending||!gesture)return;const move=pending;pending=null;metrics.coalescedMoves++;
 try{rect();if(gesture.kind==="edit")host.pointer_move(move.x,move.y,gesture.pointer);else{host.navigate(gesture.kind,gesture.x,gesture.y,move.x,move.y,move.shift);gesture.x=move.x;gesture.y=move.y;}error=null;}catch(reason){fail(reason);}
}
function frame(now){
 raf=0;flushMove();
 if(hover&&!gesture){const next=hover;hover=null;try{rect();host.pointer_hover(next.x,next.y,next.touch);metrics.queries++;}catch(reason){fail(reason);}}
 const playback=JSON.parse(host.playback_state());
 if(playback.playing){try{const next=Math.min(1,playback.fraction+Math.max(0,Math.min(100,now-last))/5000);host.advance(next);if(next>=1)host.set_playing(false);}catch(reason){fail(reason);}}
 last=now;try{draw();}catch(reason){fail(reason);}
 if(data?.playing)invalidate();
}
function stop(){host.set_playing(false);}
function seek(t){stop();host.seek(t);}
for(const name of ["a","b","theta"])for(const id of [name,name+"-range"])el(id).addEventListener("input",()=>invoke(()=>{stop();host.set_parameter(name,Number(el(id).value));}));
for(const button of document.querySelectorAll("[data-time]"))button.addEventListener("click",()=>invoke(()=>seek(Number(button.dataset.time))));
el("time").addEventListener("input",()=>invoke(()=>seek(Number(el("time").value))));el("back").addEventListener("click",()=>invoke(()=>seek(0)));
el("play").addEventListener("click",()=>invoke(()=>{if(data.playing)stop();else{if(data.fraction>=1)host.seek(0);host.set_playing(true);last=performance.now();}}));
function fit(mode=view3){if(invoke(()=>host.fit_view(mode))){view3=mode;el("mode2").setAttribute("aria-pressed",String(!mode));el("mode3").setAttribute("aria-pressed",String(mode));el("viewname").textContent=messages.t(mode?"persp":"ortho");}}
el("mode2").addEventListener("click",()=>fit(false));el("mode3").addEventListener("click",()=>fit(true));el("fit").addEventListener("click",()=>fit());
el("reset").addEventListener("click",()=>invoke(()=>{stop();host.reset_values();}));
el("locale").addEventListener("change",()=>invoke(()=>{messages.setLocale(el("locale").value);localize();}));
el("constraint").addEventListener("change",()=>invoke(()=>host.set_constraint(el("constraint").value)));
el("selection").addEventListener("change",()=>invoke(()=>host.select_handle(el("selection").value)));
el("cycle").addEventListener("click",()=>invoke(()=>host.cycle_pick(1)));
el("undo").addEventListener("click",()=>invoke(()=>host.undo_input(false)));el("redo").addEventListener("click",()=>invoke(()=>host.undo_input(true)));
for(const button of document.querySelectorAll("[data-nudge]"))button.addEventListener("click",()=>invoke(()=>host.nudge_selected(...button.dataset.nudge.split(",").map(Number).map(v=>v*0.1))));
// Middle-button autoscroll is a mousedown default, independent of pointer capture.
for(const type of ["mousedown","auxclick"])canvas.addEventListener(type,event=>{if(event.button===1)event.preventDefault();},{capture:true,passive:false});
canvas.addEventListener("touchstart",event=>{if(!host||event.touches.length!==1)return;const touch=event.touches[0];try{rect();const pick=JSON.parse(host.pointer_hover(touch.clientX,touch.clientY,true));if(pick.candidates.length||el("tool").value!=="direct")event.preventDefault();}catch(reason){fail(reason);}},{passive:false});
canvas.addEventListener("pointerdown",event=>{
 if(event.button===1)event.preventDefault();if(!host||gesture)return;try{rect();const camera=host.camera_gesture(event.button,space,event.altKey,event.shiftKey);const touchMode=event.button===0&&el("tool").value!=="direct"?el("tool").value:"";const kind=camera||touchMode;
  if(kind){gesture={kind,pointer:event.pointerId,x:event.clientX,y:event.clientY};}
  else if(event.button===0&&host.pointer_begin(event.clientX,event.clientY,event.pointerType!=="mouse",event.pointerId)){gesture={kind:"edit",pointer:event.pointerId};}
  if(gesture){canvas.focus({preventScroll:true});canvas.setPointerCapture(event.pointerId);scene.dataset.captured="true";event.preventDefault();invalidate();}
 }catch(reason){fail(reason);}
});
canvas.addEventListener("pointermove",event=>{metrics.pointerEvents++;if(gesture?.pointer===event.pointerId){pending={x:event.clientX,y:event.clientY,shift:event.shiftKey};event.preventDefault();}else if(!gesture){hover={x:event.clientX,y:event.clientY,touch:event.pointerType!=="mouse"};}invalidate();});
function finish(event,cancel){
 if(!gesture||event.pointerId!==gesture.pointer)return;
 const owned=gesture;
 if(cancel)pending=null;else{pending={x:event.clientX,y:event.clientY,shift:event.shiftKey};flushMove();}
 try{if(owned.kind==="edit")host.pointer_end(owned.pointer,cancel);error=null;}
 catch(reason){fail(reason);}
 finally{gesture=null;pending=null;last=performance.now();scene.dataset.captured="false";if(canvas.hasPointerCapture(owned.pointer))canvas.releasePointerCapture(owned.pointer);invalidate();}
}
canvas.addEventListener("pointerup",event=>finish(event,false));canvas.addEventListener("pointercancel",event=>finish(event,true));
canvas.addEventListener("lostpointercapture",event=>{if(gesture?.pointer===event.pointerId)finish(event,true);});
canvas.addEventListener("pointerleave",()=>{if(!gesture)invoke(()=>host.clear_hover());});
canvas.addEventListener("contextmenu",event=>{if(space||event.altKey||gesture)event.preventDefault();});
canvas.addEventListener("wheel",event=>{
 const profile=scene.dataset.wheelProfile||"embedded";
 if(profile!=="lab"&&!event.ctrlKey&&!event.metaKey)return;
 if(event.cancelable)event.preventDefault();
 if(gesture)return;
 invoke(()=>{const box=rect();const style=getComputedStyle(canvas);const line=parseFloat(style.lineHeight)||parseFloat(style.fontSize)*1.45;const delta=event.deltaY*(event.deltaMode===1?line:event.deltaMode===2?box.height:1);host.wheel_dolly(event.clientX,event.clientY,delta,event.shiftKey);});
},{capture:true,passive:false});
const editing=target=>target?.matches("input,select,textarea,[contenteditable=true]");
el("app").addEventListener("keydown",event=>{
 if(event.isComposing||editing(event.target))return;if(event.ctrlKey||event.metaKey){if(event.key.toLowerCase()!=="z")return;}if(event.code==="Space"){space=true;event.preventDefault();return;}
 if(!host)return;
 if(event.key==="Escape"&&gesture){finish({pointerId:gesture.pointer},true);return;}
 if(space&&["h","f"].includes(event.key.toLowerCase())){event.preventDefault();fit();return;}
 if(event.key.toLowerCase()==="c"){event.preventDefault();invoke(()=>host.cycle_pick(event.shiftKey?-1:1));return;}
 if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==="z"){event.preventDefault();invoke(()=>host.undo_input(event.shiftKey));return;}
 const delta={ArrowLeft:[-1,0,0],ArrowRight:[1,0,0],ArrowUp:[0,1,0],ArrowDown:[0,-1,0],PageUp:[0,0,1],PageDown:[0,0,-1]}[event.key];
 if(delta){event.preventDefault();invoke(()=>host.nudge_selected(...delta.map(v=>v*(event.shiftKey?0.01:0.1))));}
});
window.addEventListener("keyup",event=>{if(event.code==="Space")space=false;});
function releaseInput(){space=false;hover=null;if(gesture)finish({pointerId:gesture.pointer},true);}
window.addEventListener("blur",releaseInput);
document.addEventListener("visibilitychange",()=>{if(document.hidden)releaseInput();});
new ResizeObserver(()=>invalidate()).observe(scene);
window.addEventListener("pagehide",()=>{if(raf)cancelAnimationFrame(raf);if(gesture)finish({pointerId:gesture.pointer},true);host?.dispose();});
localize();
try{await init();host=await CosinesTrial.create(canvas);host.set_locale(messages.locale);el("app").classList.add("ready");last=performance.now();draw();fit(false);window.cosinesInputReady=true;}catch(reason){el("loading").textContent=messages.error(reason);el("loading").classList.add("error");window.cosinesDiagnostic=String(reason);}
