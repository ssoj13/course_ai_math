//! Actual copied EditorUi browser host; scene and egui share the original device, queue and Session.
use wasm_bindgen::prelude::*;
use serde::Deserialize;
use sim_editor_ui::{EditorUi,EditorProjection,projection::{RuntimeUiState,UiRole},language::{CatalogRegistry,LocaleId},intents::*,transport::{InputTransport,InputOwner}};
use sim_render::{gpu::*,geometry::SegmentGeometry,material::Material,input::InputCoordinator};
use sim_model::{ParameterId,Value,ObjectId,TimeTicks,TICKS_PER_SECOND};
use glam::Mat4;
use super::cosines_web::CosinesTrial;
fn js(e:impl std::fmt::Display)->JsValue{JsValue::from_str(&e.to_string())}
#[derive(Clone,Deserialize,Default)]#[serde(default)]struct BrowserEvent{kind:String,x:f64,y:f64,button:u8,pointer:u32,shift:bool,alt:bool,ctrl:bool,meta:bool,space:bool,text:String,key:String,pressed:bool,delta_x:f64,delta_y:f64}
impl BrowserEvent{fn modifiers(&self)->egui::Modifiers{egui::Modifiers{alt:self.alt,ctrl:self.ctrl,shift:self.shift,mac_cmd:self.meta,command:self.ctrl||self.meta}}}
struct Owner<'a>(&'a mut InputCoordinator);
impl InputOwner for Owner<'_>{
 fn begin(&mut self,s:&sim_runtime::Session,role:UiRole,keys:&[ParameterId])->Result<(),String>{if !matches!(role,UiRole::Reader){return Err("capability.pending".into());}self.0.begin(s,keys).map_err(|e|e.to_string())}
 fn update(&mut self,s:&mut sim_runtime::Session,patch:Vec<(ParameterId,Option<Value>)>)->Result<bool,String>{self.0.update(s,patch).map_err(|e|e.to_string())}
 fn commit(&mut self,s:&mut sim_runtime::Session)->Result<bool,String>{self.0.commit(s).map_err(|e|e.to_string())}
 fn cancel(&mut self,s:&mut sim_runtime::Session)->Result<bool,String>{self.0.cancel(s).map_err(|e|e.to_string())}
 fn abandon(&mut self,s:&sim_runtime::Session){self.0.abandon(s);}
}
struct SceneTargets{size:[u32;2],color:wgpu::Texture,multisample:wgpu::Texture,depth:wgpu::Texture}
struct CameraDrag{pointer:u32,kind:String,last:[f64;2]}
#[wasm_bindgen]pub struct EditorTrial{
 host:CosinesTrial,ui:EditorUi,catalogs:CatalogRegistry,ctx:egui::Context,egui:egui_wgpu::Renderer,input:InputTransport,
 targets:Option<SceneTargets>,texture:egui::TextureId,events:Vec<egui::Event>,scene_events:Vec<BrowserEvent>,camera_drag:Option<CameraDrag>,last:f64,origin:[f64;2],viewport:egui::Rect,status:String,frames:u64,space:bool,feedback:Vec<sim_editor_ui::transport::GestureFeedback>,
}
#[wasm_bindgen]impl EditorTrial{
 pub async fn create(canvas:web_sys::HtmlCanvasElement)->Result<Self,JsValue>{
  console_error_panic_hook::set_once();
  let host=CosinesTrial::create(canvas).await?;let guard=host.device.push_error_scope(wgpu::ErrorFilter::Validation);
  let egui=egui_wgpu::Renderer::new(&host.device,wgpu::TextureFormat::Bgra8Unorm,egui_wgpu::RendererOptions::default());
  if let Some(e)=guard.pop().await{return Err(js(e));}
  let ctx=egui::Context::default();ctx.set_visuals(egui::Visuals::dark());
  let mut ui=EditorUi::default();ui.view.selection=vec![ObjectId::new("triangle").map_err(js)?];ui.view.locale=LocaleId::new("ru").map_err(js)?;
  Ok(Self{host,ui,catalogs:Default::default(),ctx,egui,input:Default::default(),targets:None,texture:egui::TextureId::User(0),events:vec![],scene_events:vec![],camera_drag:None,last:0.,origin:[0.,0.],viewport:egui::Rect::NOTHING,status:String::new(),frames:0,space:false,feedback:vec![]})
 }
 pub fn set_canvas_rect(&mut self,left:f64,top:f64)->Result<(),JsValue>{if !left.is_finite()||!top.is_finite(){return Err(js("viewport"));}self.origin=[left,top];Ok(())}
 pub fn event(&mut self,source:&str)->Result<(),JsValue>{
  if source.len()>8192||self.events.len()>2048{return Err(js("input.budget"));}let e:BrowserEvent=serde_json::from_str(source).map_err(js)?;let z=f64::from(self.ctx.zoom_factor());let point=egui::pos2(((e.x-self.origin[0])/z)as f32,((e.y-self.origin[1])/z)as f32);let modifiers=e.modifiers();
  match e.kind.as_str(){
   "move"=>self.events.push(egui::Event::PointerMoved(point)),
   "down"|"up"=>{let button=match e.button{1=>egui::PointerButton::Middle,2=>egui::PointerButton::Secondary,_=>egui::PointerButton::Primary};self.events.push(egui::Event::PointerButton{pos:point,button,pressed:e.kind=="down",modifiers});},
   "leave"=>self.events.push(egui::Event::PointerGone),
   "wheel"=>self.events.push(egui::Event::MouseWheel{unit:egui::MouseWheelUnit::Point,delta:egui::vec2(e.delta_x as f32,e.delta_y as f32),phase:egui::TouchPhase::Move,modifiers}),
   "text"=>self.events.push(egui::Event::Text(e.text.clone())),
   "paste"=>self.events.push(egui::Event::Paste(e.text.clone())),
   "key"=>{self.space=if e.key=="Space"{e.pressed}else{self.space};if let Some(key)=key(&e.key){self.events.push(egui::Event::Key{key,physical_key:Some(key),pressed:e.pressed,repeat:false,modifiers});}},
   "cancel"|"blur"=>{self.cancel()?;self.space=false;self.events.push(egui::Event::PointerGone);},_=>return Err(js("input.event"))
  }
  self.scene_events.push(e);Ok(())
 }
 pub fn cancel(&mut self)->Result<(),JsValue>{
  self.camera_drag=None;let mut failure=None;
  if let Some(drag)=&self.host.input.drag{let pointer=drag.pointer;if let Err(e)=self.host.pointer_end(pointer,true){failure=Some(e);}}
  let mut owner=Owner(&mut self.host.model.input);if let Err(e)=self.input.cancel(&mut self.host.model.session,&mut owner){failure=Some(js(e));}
  if let Some(e)=failure{self.status=String::from("draft.stale");Err(e)}else{Ok(())}
 }
 pub async fn render(&mut self,width:f64,height:f64,dpr:f64,now:f64)->Result<String,JsValue>{
  if self.host.disposed{return Err(js("disposed"));}if [width,height,dpr,now].iter().any(|x|!x.is_finite())||width<=0.||height<=0.||dpr<=0.||width*dpr>4096.||height*dpr>4096.{return Err(js("viewportbudget"));}
  let backing=[(width*dpr).round()as u32,(height*dpr).round()as u32];let elapsed=if self.last==0.{0.}else{(now-self.last).clamp(0.,100.)};self.last=now;
  if self.host.model.session.is_playing(){let current=self.host.model.session.snapshot().time().ticks();let next=(current+((elapsed/5000.)*TICKS_PER_SECOND as f64).round()as i64).min(TICKS_PER_SECOND);self.host.model.session.seek(self.host.model.session.revision(),TimeTicks::new(next)).map_err(js)?;if next==TICKS_PER_SECOND{self.host.model.session.set_playing(self.host.model.session.revision(),false).map_err(js)?;}self.host.model.compile().map_err(js)?;}
  if self.host.config.width!=backing[0]||self.host.config.height!=backing[1]{self.host.canvas.set_width(backing[0]);self.host.canvas.set_height(backing[1]);self.host.config.width=backing[0];self.host.config.height=backing[1];self.host.surface.configure(&self.host.device,&self.host.config);}
  if self.targets.is_none(){self.ensure_targets([1,1])?;}
  let mut raw=egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2((width/f64::from(self.ctx.zoom_factor()))as f32,(height/f64::from(self.ctx.zoom_factor()))as f32))),time:Some(now/1000.),events:std::mem::take(&mut self.events),..Default::default()};
  if let Some(view)=raw.viewports.get_mut(&egui::ViewportId::ROOT){view.native_pixels_per_point=Some(dpr as f32);}
  let locale_before=self.ui.view.locale.clone();let frame=self.host.model.session.snapshot();let mut projection=EditorProjection::from_runtime(&frame,self.host.model.session.document().document(),RuntimeUiState{role:UiRole::Reader,playing:self.host.model.session.is_playing(),playback_intent:self.host.model.session.playback_intent(),can_undo:self.host.model.input.can_undo(),can_redo:self.host.model.input.can_redo()}).map_err(js)?;
  let language=self.catalogs.language(&self.ui.view.locale).map_err(js)?;
  for item in self.feedback.drain(..){self.ui.inspector.acknowledge(&item,Some(&projection),language);}
  projection.viewport_texture=Some(self.texture);projection.status=&self.status;
  let mut output=None;let mut full=self.ctx.run_ui(raw,|root|output=Some(self.ui.show(root,&projection,&Default::default(),&self.catalogs)));let mut output=output.ok_or_else(||js("UI publication"))?;drop(projection);
  let zoom=self.ctx.zoom_factor();let hovered=output.viewports.first().is_some_and(|x|x.hovered);if let Some(view)=output.viewports.first(){self.viewport=egui::Rect::from_min_max(view.rect.min*zoom,view.rect.max*zoom);}
  let css=[f64::from(self.viewport.width()),f64::from(self.viewport.height())];if css.iter().any(|x|*x<=0.){full.textures_delta.clear();return Err(js("viewport missing"));}
  let size=[(css[0]*dpr).ceil()as u32,(css[1]*dpr).ceil()as u32];
  let target_bytes=u64::from(size[0])*u64::from(size[1])*36+u64::from(backing[0])*u64::from(backing[1])*8;if target_bytes>256*1024*1024{full.textures_delta.clear();return Err(js("viewportbudget"));}
  self.ensure_targets(size)?;let changed=self.host.css!=css;self.host.css=css;self.host.dpr=dpr;self.host.set_viewport_rect(self.origin[0]+f64::from(self.viewport.left()),self.origin[1]+f64::from(self.viewport.top()),css[0],css[1],dpr)?;
  if changed&&!self.host.manual_camera{self.host.model.fit(css,self.host.model.perspective).map_err(js)?;}
  let draws=self.scene_draws(size)?;let displayed_values=[self.host.model.scalar("a").map_err(js)?,self.host.model.scalar("b").map_err(js)?,self.host.model.scalar("theta").map_err(js)?,self.host.model.scalar("c2").map_err(js)?];let displayed_points=["origin","A","B"].map(|id|self.host.model.position(id).map(|p|p.to_array()).unwrap_or([0.;3]));
  self.feedback=match self.apply(std::mem::take(&mut output.intents)){Ok(x)=>x,Err(error)=>{full.textures_delta.clear();return Err(error);}};
  let chrome=self.ui.view.chrome_rect.map(|r|egui::Rect::from_min_max(r.min*zoom,r.max*zoom));let scene_events=std::mem::take(&mut self.scene_events);
  for event in scene_events{
   let local=egui::pos2((event.x-self.origin[0])as f32,(event.y-self.origin[1])as f32);let in_scene=self.viewport.contains(local)&&!chrome.is_some_and(|r|r.contains(local));let eligible=hovered&&in_scene;
   match event.kind.as_str(){
    "down" if eligible=>{let navigation=self.host.camera_gesture(event.button,self.space||event.space,event.alt,event.shift);if !navigation.is_empty(){self.camera_drag=Some(CameraDrag{pointer:event.pointer,kind:navigation,last:[event.x,event.y]});}else if event.button==0&&self.ui.view.tool=="Move"{let _=self.host.pointer_begin(event.x,event.y,false,event.pointer)?;}},
    "move"=>{if let Some(drag)=&mut self.camera_drag{if drag.pointer==event.pointer{self.host.navigate(&drag.kind,drag.last[0],drag.last[1],event.x,event.y,event.shift)?;drag.last=[event.x,event.y];}}else if self.host.input.drag.is_some(){self.host.pointer_move(event.x,event.y,event.pointer)?;}else if eligible{let _=self.host.pointer_hover(event.x,event.y,false)?;}},
    "up"=>{if self.host.input.drag.is_some(){self.host.pointer_end(event.pointer,false)?;}self.camera_drag=None;},
    "wheel" if eligible&&self.camera_drag.is_none()&&self.host.input.drag.is_none()=>self.host.wheel_dolly(event.x,event.y,-event.delta_y,event.shift)?,
    "key" if event.pressed&&event.key=="Escape"=>{let _=self.cancel();},
    _=>{}
   }
  }
  let canonical_revision=self.host.model.session.revision();let deferred=canonical_revision!=frame.revision()||!self.feedback.is_empty()||self.ui.view.locale!=locale_before;
  let jobs=self.ctx.tessellate(std::mem::take(&mut full.shapes),full.pixels_per_point);
  for(id,deltas)in &full.textures_delta.set{for delta in deltas{self.egui.update_texture(&self.host.device,&self.host.queue,*id,delta);}}
  let output_texture=match self.host.surface.get_current_texture(){wgpu::CurrentSurfaceTexture::Success(x)|wgpu::CurrentSurfaceTexture::Suboptimal(x)=>x,other=>{full.textures_delta.clear();return Err(js(format!("surface:{other:?}")));}};
  let target=output_texture.texture.create_view(&Default::default());let screen=egui_wgpu::ScreenDescriptor{size_in_pixels:backing,pixels_per_point:full.pixels_per_point};let scope=self.host.device.push_error_scope(wgpu::ErrorFilter::Validation);let mut encoder=self.host.device.create_command_encoder(&Default::default());let textures=self.targets.as_ref().ok_or_else(||js("targets"))?;
  let scene_view=textures.color.create_view(&wgpu::TextureViewDescriptor{format:Some(wgpu::TextureFormat::Bgra8UnormSrgb),..Default::default()});
  self.host.renderer.encode_view(&mut encoder,&draws.iter().collect::<Vec<_>>(),&textures.multisample.create_view(&Default::default()),&textures.depth.create_view(&Default::default()),Some(&scene_view),wgpu::Color{r:0.025,g:0.03,b:0.04,a:1.},None).map_err(js)?;
  let extras=self.egui.update_buffers(&self.host.device,&self.host.queue,&mut encoder,&jobs,&screen);if !extras.is_empty(){full.textures_delta.clear();return Err(js("unexpected callback submission"));}
  {let mut pass=encoder.begin_render_pass(&wgpu::RenderPassDescriptor{label:Some("actual EditorUi same GPU host"),color_attachments:&[Some(wgpu::RenderPassColorAttachment{view:&target,depth_slice:None,resolve_target:None,ops:wgpu::Operations{load:wgpu::LoadOp::Clear(wgpu::Color::BLACK),store:wgpu::StoreOp::Store}})],..Default::default()}).forget_lifetime();self.egui.render(&mut pass,&jobs,&screen);}
  self.host.queue.submit([encoder.finish()]);self.host.queue.present(output_texture);for id in &full.textures_delta.free{self.egui.free_texture(id);}full.textures_delta.clear();if let Some(e)=scope.pop().await{return Err(js(e));}
  self.frames+=1;let repaint=full.viewport_output.get(&egui::ViewportId::ROOT).map(|x|x.repaint_delay.as_millis()).unwrap_or(u128::MAX);
  let snapshot=self.host.model.session.snapshot();let points=["origin","A","B"].map(|id|self.host.model.position(id).map(|p|p.to_array()).unwrap_or([0.;3]));
  serde_json::to_string(&serde_json::json!({"ready":true,"frames":self.frames,"a":displayed_values[0],"b":displayed_values[1],"theta":displayed_values[2],"c2":displayed_values[3],"time":frame.time(),"playing":self.host.model.session.is_playing(),"intent":self.host.model.session.playback_intent(),"revision":frame.revision(),"gui_revision":frame.revision(),"scene_revision":frame.revision(),"canonical_revision":canonical_revision,"can_undo":self.host.model.input.can_undo(),"can_redo":self.host.model.input.can_redo(),"points":displayed_points,"viewport":[self.viewport.left(),self.viewport.top(),self.viewport.width(),self.viewport.height()],"semantic_controls":output.semantics.iter().map(|x|serde_json::json!({"id":x.id,"rect":[x.rect.left(),x.rect.top(),x.rect.width(),x.rect.height()],"value":x.value})).collect::<Vec<_>>(),"device_count":1,"submissions_per_frame":1,"msaa":4,"repaint_ms":if deferred{Some(0)}else if repaint<1000{Some(repaint as u32)}else{None},"locale":self.ui.view.locale.as_str(),"status":self.status})).map_err(js)
 }
 pub fn dispose(&mut self){self.host.disposed=true;self.targets=None;self.host.device.destroy();}
}
impl EditorTrial{
 fn ensure_targets(&mut self,size:[u32;2])->Result<(),JsValue>{
  if self.targets.as_ref().is_some_and(|x|x.size==size){return Ok(());}let make=|samples,format,usage,view_formats:&[wgpu::TextureFormat]|self.host.device.create_texture(&wgpu::TextureDescriptor{label:Some("EditorUi scene target"),size:wgpu::Extent3d{width:size[0],height:size[1],depth_or_array_layers:1},mip_level_count:1,sample_count:samples,dimension:wgpu::TextureDimension::D2,format,usage,view_formats});
  let color=make(1,wgpu::TextureFormat::Bgra8Unorm,wgpu::TextureUsages::RENDER_ATTACHMENT|wgpu::TextureUsages::TEXTURE_BINDING,&[wgpu::TextureFormat::Bgra8UnormSrgb]);let multisample=make(4,wgpu::TextureFormat::Bgra8UnormSrgb,wgpu::TextureUsages::RENDER_ATTACHMENT,&[]);let depth=make(4,wgpu::TextureFormat::Depth32Float,wgpu::TextureUsages::RENDER_ATTACHMENT,&[]);
  if self.targets.is_none(){self.texture=self.egui.register_native_texture(&self.host.device,&color.create_view(&Default::default()),wgpu::FilterMode::Linear);}else{self.egui.update_egui_texture_from_wgpu_texture(&self.host.device,&color.create_view(&Default::default()),wgpu::FilterMode::Linear,self.texture);}
  self.targets=Some(SceneTargets{size,color,multisample,depth});Ok(())
 }
 fn apply(&mut self,intents:Vec<UiIntent>)->Result<Vec<sim_editor_ui::transport::GestureFeedback>,JsValue>{
  let mut gestures=Vec::new();let mut others=Vec::new();for intent in intents{match intent{UiIntent::Gesture(e)=>gestures.push(e),other=>others.push(other)}}
  let feedback={let mut owner=Owner(&mut self.host.model.input);self.input.handle_frame(&mut self.host.model.session,&mut owner,gestures).map_err(js)?};
  for item in &feedback{if let Err(error)=&item.result{self.status=error.clone();}}
  for intent in others{let result:Result<(),String>=match intent{
   UiIntent::Existing{command,..}=>self.host.model.session.dispatch(self.host.model.session.snapshot().command(command)).map(|_|()).map_err(|e|e.to_string()),
   UiIntent::Gesture(event)=>{let mut owner=Owner(&mut self.host.model.input);self.input.handle(&mut self.host.model.session,&mut owner,&event).map(|_|())},
   UiIntent::View(view)=>match view{ViewIntent::UndoInput{redo}=>self.host.model.undo_input(redo),ViewIntent::Projection{perspective,..}=>self.host.fit_view(perspective).map_err(|e|format!("{e:?}")),ViewIntent::Fit(_)=>self.host.fit_view(self.host.model.perspective).map_err(|e|format!("{e:?}")),ViewIntent::Locale(id)=>{self.ui.view.locale=id;Ok(())},ViewIntent::Select{objects,..}=>{self.ui.view.selection=objects;Ok(())},_=>Ok(())},
   UiIntent::Author(_)|UiIntent::Service(_)=>Err("capability.pending".into()),
  };if let Err(e)=result{self.status=if e.starts_with("input"){"draft.stale".into()}else{e};}}
  match self.host.model.compile(){Ok(())=>Ok(feedback),Err(error)=>{let language=self.catalogs.language(&self.ui.view.locale).map_err(js)?;for item in &feedback{self.ui.inspector.acknowledge(item,None,language);}Err(js(error))}}
 }
 fn scene_draws(&mut self,size:[u32;2])->Result<Vec<GpuDraw>,JsValue>{
  let matrices=self.host.model.camera.matrices(self.host.css).map_err(js)?;
  let frame=FrameGpu{view_projection:Mat4::from_cols_array(&matrices.view_projection().to_cols_array().map(|n|n as f32)).to_cols_array_2d(),eye:matrices.eye().as_vec3().extend(1.).to_array(),orthographic_direction:matrices.toward_eye().as_vec3().extend(if self.host.model.perspective{0.}else{1.}).to_array(),light_direction:[0.3,0.4,0.8660254,0.],light_irradiance:[1.;4],viewport:[size[0]as f32,size[1]as f32,2.5*self.host.dpr as f32,1.]};
  let positions=[self.host.model.position("origin").map_err(js)?,self.host.model.position("B").map_err(js)?,self.host.model.position("A").map_err(js)?];let vertices=positions.map(|p|Vertex{position:p.as_vec3().to_array(),normal:[0.,0.,1.],tangent:[1.,0.,0.]});let mut body=Material::default();body.surface.base=0.;body.surface.specular=0.;body.surface.emission=1.;body.surface.emission_color=[0.025,0.035,0.05];
  let mut draws=vec![self.host.renderer.prepare(&vertices,&[0,1,2],&frame,&ObjectGpu::default(),&body,&EnvironmentGpu::default(),false).map_err(js)?];
  for(a,b,color)in[(positions[0],positions[2],[0.42,0.70,0.97,1.]),(positions[0],positions[1],[0.57,0.84,0.65,1.]),(positions[1],positions[2],[0.78,0.64,0.91,1.])]{draws.push(self.host.renderer.prepare_segments(&[SegmentGeometry{a,b,width_css:2.5,head_css:None}],&frame,&ObjectGpu{edge_color:color,..Default::default()},&Material::default(),&EnvironmentGpu::default()).map_err(js)?);}
  self.host.input.refresh(&self.host.model,&matrices).map_err(js)?;
  for(_,p)in &self.host.input.handles{let mut f=frame;f.viewport[2]=8.*self.host.dpr as f32;draws.push(self.host.renderer.prepare_segments(&[SegmentGeometry{a:*p,b:*p,width_css:8.,head_css:None}],&f,&ObjectGpu::default(),&Material::default(),&EnvironmentGpu::default()).map_err(js)?);}
  for(name,a,b)in &self.host.input.gizmos{let color=if name.ends_with('x'){[0.9,0.3,0.3,1.]}else if name.ends_with('y'){[0.3,0.8,0.4,1.]}else{[0.4,0.6,0.95,1.]};draws.push(self.host.renderer.prepare_segments(&[SegmentGeometry{a:*a,b:*b,width_css:2.5,head_css:None}],&frame,&ObjectGpu{edge_color:color,..Default::default()},&Material::default(),&EnvironmentGpu::default()).map_err(js)?.as_overlay());}
  Ok(draws)
 }
}
fn key(s:&str)->Option<egui::Key>{Some(match s{"Escape"=>egui::Key::Escape,"Enter"=>egui::Key::Enter,"Tab"=>egui::Key::Tab,"Backspace"=>egui::Key::Backspace,"Delete"=>egui::Key::Delete,"ArrowLeft"=>egui::Key::ArrowLeft,"ArrowRight"=>egui::Key::ArrowRight,"ArrowUp"=>egui::Key::ArrowUp,"ArrowDown"=>egui::Key::ArrowDown,"Home"=>egui::Key::Home,"End"=>egui::Key::End,"PageUp"=>egui::Key::PageUp,"PageDown"=>egui::Key::PageDown,"Space"=>egui::Key::Space,"a"|"A"=>egui::Key::A,"c"|"C"=>egui::Key::C,"v"|"V"=>egui::Key::V,"x"|"X"=>egui::Key::X,"z"|"Z"=>egui::Key::Z,_=>return None})}
