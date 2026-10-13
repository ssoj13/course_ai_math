//! Caller-owned browser host for the authoritative cosine acceptance workload.
use wasm_bindgen::prelude::*;
use super::cosines::CosinesModel;
use sim_render::{gpu::*,geometry::SegmentGeometry,camera::Projection,material::Material};
use sim_model::*;
use glam::{DVec2,DVec3,DQuat,Mat4};
use super::cosines_input::CosinesInput;
use sim_render::input_pointer::ViewportRect;
use sim_render::interaction::Navigation;
use serde_json::json;
fn js(error:impl std::fmt::Display)->JsValue{JsValue::from_str(&error.to_string())}
fn color(hex:[u8;3])->[f32;4]{let rgb=hex.map(|v|{let s=f32::from(v)/255.0;if s<=0.04045{s/12.92}else{((s+0.055)/1.055).powf(2.4)}});[rgb[0],rgb[1],rgb[2],1.0]}
#[wasm_bindgen]
pub struct CosinesTrial {
    pub(super) model:CosinesModel,pub(super) canvas:web_sys::HtmlCanvasElement,pub(super) surface:wgpu::Surface<'static>,pub(super) device:wgpu::Device,pub(super) queue:wgpu::Queue,
    pub(super) renderer:RasterRenderer,pub(super) config:wgpu::SurfaceConfiguration,pub(super) depth:Option<wgpu::Texture>,pub(super) multisample:Option<wgpu::Texture>,pub(super) manual_camera:bool,pub(super) disposed:bool,pub(super) css:[f64;2],pub(super) dpr:f64,pub(super) input:CosinesInput,pub(super) rect:ViewportRect,
}
#[wasm_bindgen]
impl CosinesTrial {
    pub async fn create(canvas:web_sys::HtmlCanvasElement)->std::result::Result<CosinesTrial,JsValue>{
        let instance=wgpu::Instance::default();let surface=instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone())).map_err(js)?;
        let adapter=instance.request_adapter(&wgpu::RequestAdapterOptions{compatible_surface:Some(&surface),..Default::default()}).await.map_err(js)?;
        sim_render::capability::CapabilityReport::inspect(&adapter).validate_baseline().map_err(js)?;
        let(device,queue)=adapter.request_device(&wgpu::DeviceDescriptor::default()).await.map_err(js)?;
        let mut config=surface.get_default_config(&adapter,800,480).ok_or_else(||js("no surfaceconfiguration"))?;
        config.format=wgpu::TextureFormat::Bgra8Unorm;config.view_formats=vec![wgpu::TextureFormat::Bgra8UnormSrgb];
        if !adapter.get_texture_format_features(wgpu::TextureFormat::Bgra8UnormSrgb).flags.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4)||!adapter.get_texture_format_features(wgpu::TextureFormat::Depth32Float).flags.contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4){return Err(js("MSAA4 unsupported"));}
        let renderer=RasterRenderer::create(&device,wgpu::TextureFormat::Bgra8UnormSrgb,4).await.map_err(js)?;
        let mut model=CosinesModel::new().map_err(js)?;model.fit([800.0,480.0],false).map_err(js)?;
        Ok(Self{model,canvas,surface,device,queue,renderer,config,depth:None,multisample:None,manual_camera:false,disposed:false,css:[800.0,480.0],dpr:1.0,input:CosinesInput::default(),rect:ViewportRect{left:0.0,top:0.0,width:800.0,height:480.0,dpr:1.0}})
    }
    pub fn set_parameter(&mut self,name:&str,value:f64)->std::result::Result<(),JsValue>{if self.disposed{return Err(js("disposed"));}self.model.set(name,value).map_err(js)}
    pub fn playback_state(&self)->std::result::Result<String,JsValue>{serde_json::to_string(&json!({"playing":self.model.session.is_playing(),"intent":self.model.session.playback_intent(),"fraction":self.model.session.snapshot().time().ticks() as f64/TICKS_PER_SECOND as f64})).map_err(js)}
    pub fn set_playing(&mut self,playing:bool)->std::result::Result<(),JsValue>{self.model.session.set_playing(self.model.session.revision(),playing).map_err(js)?;self.model.compile().map_err(js)}
    pub fn seek(&mut self,fraction:f64)->std::result::Result<(),JsValue>{self.model.seek(fraction).map_err(js)}
    pub fn advance(&mut self,fraction:f64)->std::result::Result<(),JsValue>{self.model.advance(fraction).map_err(js)}
    pub fn set_locale(&mut self,locale:&str)->std::result::Result<(),JsValue>{if !["en","ru"].contains(&locale){return Err(js("locale"));}self.model.locale=locale.into();Ok(())}
    pub fn fit_view(&mut self,perspective:bool)->std::result::Result<(),JsValue>{if self.input.drag.is_some(){return Err(js("gesture active; cancel before changing view"));}self.manual_camera=false;self.model.fit(self.css,perspective).map_err(js)}
    pub fn reset_values(&mut self)->std::result::Result<(),JsValue>{
        self.model.session.dispatch(self.model.session.snapshot().command(sim_runtime::CommandKind::ResetParameters{expected_document_revision:self.model.session.snapshot().document_revision(),parameters:vec![ParameterId::new("a").map_err(js)?,ParameterId::new("b").map_err(js)?,ParameterId::new("theta").map_err(js)?]})).map_err(js)?;
        self.model.compile().map_err(js)
    }
    pub fn set_viewport_rect(&mut self,left:f64,top:f64,width:f64,height:f64,dpr:f64)->std::result::Result<(),JsValue>{
        let rect=ViewportRect{left,top,width,height,dpr}.validate().map_err(js)?;
        if self.rect!=rect{self.rect=rect;self.input.layout=self.input.layout.wrapping_add(1);}Ok(())
    }
    pub fn set_constraint(&mut self,profile:&str)->std::result::Result<(),JsValue>{
        if !["screen","xy","yz","xz","x","y","z"].contains(&profile){return Err(js("invalid constraint profile"));}
        if self.input.drag.is_some(){return Err(js("gesture active"));}self.input.constraint=profile.into();Ok(())
    }
    pub fn camera_gesture(&self,button:u8,space:bool,alt:bool,shift:bool)->String{
        use sim_render::input_pointer::{camera_gesture,PointerButton,Modifiers};
        let button=match button{0=>PointerButton::Primary,1=>PointerButton::Middle,2=>PointerButton::Secondary,_=>return String::new()};
        match camera_gesture(button,Modifiers{space,alt,shift}){Some(Navigation::Orbit)=>"orbit",Some(Navigation::Pan)=>"pan",Some(Navigation::Dolly)=>"dolly",None=>""}.into()
    }
    pub fn select_handle(&mut self,name:&str)->std::result::Result<(),JsValue>{
        if self.input.drag.is_some(){return Err(js("gesture active"));}
        let camera=self.model.camera.matrices(self.css).map_err(js)?;self.input.refresh(&self.model,&camera).map_err(js)?;
        let position=self.input.handles.iter().find(|(id,_)|id==name).map(|(_,p)|*p).ok_or_else(||js("unknown handle"))?;
        let projected=camera.project(position).ok_or_else(||js("handle outside viewport"))?;
        self.input.hover(&self.model,&camera,projected.truncate(),false).map_err(js)?;
        for _ in 0..self.input.selection.candidates().len(){if self.input.selection.chosen().is_some_and(|hit|hit.semantic.as_str()==name){break;}self.input.cycle(1);}
        if self.input.selection.chosen().is_none_or(|hit|hit.semantic.as_str()!=name){return Err(js("handle occluded"));}
        self.input.selection.choose();self.input.target=match name{"O"=>sim_render::bindings::TriangleTarget::Vertex(0),"A"=>sim_render::bindings::TriangleTarget::Vertex(1),"B"=>sim_render::bindings::TriangleTarget::Vertex(2),"theta"=>sim_render::bindings::TriangleTarget::Angle,"body"=>sim_render::bindings::TriangleTarget::Group,_=>return Err(js("unknown handle"))};
        self.input.sample=self.input.sample.wrapping_add(1);self.sync_view_highlights()
    }
    pub fn clear_hover(&mut self)->std::result::Result<(),JsValue>{self.input.selection.clear_hover();self.sync_view_highlights()}
    fn sync_view_highlights(&mut self)->std::result::Result<(),JsValue>{
        use sim_runtime::HighlightTarget;
        let target=|name:&str|->std::result::Result<HighlightTarget,JsValue>{Ok(match name{"O"=>HighlightTarget::Value(ValueId::new("origin").map_err(js)?),"A"|"B"=>HighlightTarget::Value(ValueId::new(name).map_err(js)?),"theta"=>HighlightTarget::Parameter(ParameterId::new("theta").map_err(js)?),"side.a"=>HighlightTarget::Parameter(ParameterId::new("a").map_err(js)?),"side.b"=>HighlightTarget::Parameter(ParameterId::new("b").map_err(js)?),"side.c"=>HighlightTarget::Value(ValueId::new("c2").map_err(js)?),_=>HighlightTarget::Object(ObjectId::new("triangle").map_err(js)?)})};
        self.model.view.set_hover(self.input.selection.chosen().map(|h|target(h.semantic.as_str())).transpose()?.into_iter().collect()).map_err(js)?;
        self.model.view.set_selection(if self.input.selection.selected.is_some(){vec![target(&self.input.selected_name())?]}else{vec![]}).map_err(js)
    }
    pub fn pointer_hover(&mut self,x:f64,y:f64,touch:bool)->std::result::Result<String,JsValue>{
        let point=self.rect.client_to_css(DVec2::new(x,y)).map_err(js)?;let camera=self.model.camera.matrices(self.css).map_err(js)?;
        let hovered=self.input.hover(&self.model,&camera,point,touch).map_err(js)?;self.sync_view_highlights()?;
        serde_json::to_string(&json!({"hovered":hovered,"candidates":self.input.candidate_names()})).map_err(js)
    }
    pub fn cycle_pick(&mut self,step:i32)->std::result::Result<String,JsValue>{
        let hovered=self.input.cycle(step);self.sync_view_highlights()?;serde_json::to_string(&json!({"hovered":hovered,"candidates":self.input.candidate_names()})).map_err(js)
    }
    pub fn pointer_begin(&mut self,x:f64,y:f64,touch:bool,pointer:u32)->std::result::Result<bool,JsValue>{
        let point=self.rect.client_to_css(DVec2::new(x,y)).map_err(js)?;let camera=self.model.camera.matrices(self.css).map_err(js)?;
        let captured=self.input.begin(&mut self.model,&camera,point,touch,pointer).map_err(js)?;if captured{self.manual_camera=true;}self.sync_view_highlights()?;Ok(captured)
    }
    pub fn pointer_move(&mut self,x:f64,y:f64,pointer:u32)->std::result::Result<(),JsValue>{
        let point=self.rect.client_to_css(DVec2::new(x,y)).map_err(js)?;let camera=self.model.camera.matrices(self.css).map_err(js)?;
        self.input.update(&mut self.model,&camera,point,pointer).map_err(js)
    }
    pub fn pointer_end(&mut self,pointer:u32,cancel:bool)->std::result::Result<(),JsValue>{self.input.end(&mut self.model,pointer,cancel).map_err(js)}
    pub fn undo_input(&mut self,redo:bool)->std::result::Result<(),JsValue>{self.model.undo_input(redo).map_err(js)}
    pub fn nudge_selected(&mut self,x:f64,y:f64,z:f64)->std::result::Result<(),JsValue>{
        let point=self.model.binding.pivot(&self.model.session.snapshot(),self.input.target).map_err(js)?+DVec3::new(x,y,z);
        self.model.begin_drag(self.input.target).map_err(js)?;
        if let Err(error)=self.model.drag(self.input.target,point){let cancel=self.model.end_drag(true);return Err(js(format!("{error}; cancel:{cancel:?}")));}
        self.model.end_drag(false).map_err(js)
    }
    pub fn navigate(&mut self,kind:&str,from_x:f64,from_y:f64,to_x:f64,to_y:f64,precision:bool)->std::result::Result<(),JsValue>{
        if self.input.drag.is_some(){return Err(js("gesture active"));}
        let kind=match kind{"orbit"=>Navigation::Orbit,"pan"=>Navigation::Pan,"dolly"=>Navigation::Dolly,_=>return Err(js("invalid navigation"))};
        let from=self.rect.client_to_css(DVec2::new(from_x,from_y)).map_err(js)?;let to=self.rect.client_to_css(DVec2::new(to_x,to_y)).map_err(js)?;
        if kind==Navigation::Orbit&&!self.model.perspective{
            let camera=self.model.camera;let pivot=DVec3::from_array(camera.target);let normal=(DVec3::from_array(camera.eye)-pivot).normalize();
            let rotation=DQuat::from_axis_angle(normal,-(to.x-from.x+to.y-from.y)*0.006*if precision{0.1}else{1.0});
            self.model.camera.up=(rotation*DVec3::from_array(camera.up)).to_array();
        }else{self.model.camera=self.model.camera.navigate(kind,from,to,self.css,precision).map_err(js)?;}
        self.persist_camera()?;Ok(())
    }
    pub fn wheel_dolly(&mut self,x:f64,y:f64,delta:f64,precision:bool)->std::result::Result<(),JsValue>{
        if self.input.drag.is_some(){return Err(js("gesture active"));}
        let point=self.rect.client_to_css(DVec2::new(x,y)).map_err(js)?;
        self.model.camera=self.model.camera.zoom_at(point,delta*0.001*if precision{0.1}else{1.0},self.css).map_err(js)?;self.persist_camera()
    }
    fn persist_camera(&mut self)->std::result::Result<(),JsValue>{
        self.manual_camera=true;self.input.sample=self.input.sample.wrapping_add(1);
        self.model.persist_camera().map_err(js)
    }
    pub fn render(&mut self,width:f64,height:f64,dpr:f64)->std::result::Result<String,JsValue>{
        if self.disposed{return Err(js("disposed"));}
        if [width,height,dpr].iter().any(|n|!n.is_finite()||*n<=0.0)||width*dpr>4096.0||height*dpr>4096.0{return Err(js("viewportbudget"));}
        let backing=[(width*dpr).round()as u32,(height*dpr).round()as u32];
        if self.css!=[width,height]||self.dpr!=dpr{self.input.layout=self.input.layout.wrapping_add(1);}self.css=[width,height];self.dpr=dpr;
        sim_render::capability::target_bytes(backing[0],backing[1],4,self.device.limits().max_texture_dimension_2d,0,128*1024*1024).map_err(js)?;
        if self.depth.is_none()||self.config.width!=backing[0]||self.config.height!=backing[1]{
            self.css=[width,height];self.dpr=dpr;self.canvas.set_width(backing[0]);self.canvas.set_height(backing[1]);self.config.width=backing[0];self.config.height=backing[1];
            self.surface.configure(&self.device,&self.config);
            self.depth=Some(self.device.create_texture(&wgpu::TextureDescriptor{label:Some("cosines reversed-Z"),size:wgpu::Extent3d{width:backing[0],height:backing[1],depth_or_array_layers:1},mip_level_count:1,sample_count:4,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Depth32Float,usage:wgpu::TextureUsages::RENDER_ATTACHMENT,view_formats:&[]}));
            self.multisample=Some(self.device.create_texture(&wgpu::TextureDescriptor{label:Some("cosines MSAA4 color"),size:wgpu::Extent3d{width:backing[0],height:backing[1],depth_or_array_layers:1},mip_level_count:1,sample_count:4,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Bgra8UnormSrgb,usage:wgpu::TextureUsages::RENDER_ATTACHMENT,view_formats:&[]}));
        }
        let mut matrices=self.model.camera.matrices(self.css).map_err(js)?;
        let actual_points=[self.model.position("origin").map_err(js)?,self.model.position("A").map_err(js)?,self.model.position("B").map_err(js)?];
        if !self.manual_camera && self.input.drag.is_none() && actual_points.iter().any(|point|matrices.project(*point).is_none_or(|p|p.x<48.0||p.y<48.0||p.x>width-48.0||p.y>height-48.0)){
            self.model.fit(self.css,self.model.perspective).map_err(js)?;
            matrices=self.model.camera.matrices(self.css).map_err(js)?;
        }
        let frame=FrameGpu{view_projection:Mat4::from_cols_array(&matrices.view_projection().to_cols_array().map(|n|n as f32)).to_cols_array_2d(),eye:matrices.eye().as_vec3().extend(1.0).to_array(),
            orthographic_direction:matrices.toward_eye().as_vec3().extend(if matches!(self.model.camera.projection,Projection::Orthographic{..}){1.0}else{0.0}).to_array(),
            light_direction:[0.3,0.4,0.8660254,0.0],light_irradiance:[1.0;4],viewport:[backing[0]as f32,backing[1]as f32,2.5*dpr as f32,1.0]};
        let origin=self.model.position("origin").map_err(js)?;let a=self.model.position("A").map_err(js)?;let b=self.model.position("B").map_err(js)?;
        let geometry=&self.model.compiler.current().ok_or_else(||js("no preparedscene"))?.objects.get(&ObjectId::new("triangle").map_err(js)?).ok_or_else(||js("missingtriangle"))?.geometry;
        let mut draws=Vec::new();
        let body=Material{surface:standard_surface_bsdf::SurfaceInputs{base:0.0,specular:0.0,emission:1.0,emission_color:color([39,49,61])[..3].try_into().map_err(js)?,..standard_surface_bsdf::SurfaceInputs::MATERIALX_DEFAULT},..Material::default()};
        let vertices=geometry.positions.iter().map(|p|Vertex{position:p.as_vec3().to_array(),normal:[0.0,0.0,1.0],tangent:[1.0,0.0,0.0]}).collect::<Vec<_>>();
        let indices=geometry.triangles.iter().flatten().copied().collect::<Vec<_>>();
        if !indices.is_empty(){draws.push(self.renderer.prepare(&vertices,&indices,&frame,&ObjectGpu::default(),&body,&EnvironmentGpu::default(),false).map_err(js)?);}
        let mut grid=Vec::new();let pose_u=self.model.position("axis-x").map_err(js)?;let pose_v=self.model.position("axis-y").map_err(js)?;let grid_point=|x:f64,y:f64|origin+pose_u*x+pose_v*y-pose_u.cross(pose_v)*0.02;
        for i in -8..=8{let t=f64::from(i);grid.push(SegmentGeometry{a:grid_point(t,-8.0),b:grid_point(t,8.0),width_css:1.0,head_css:None});grid.push(SegmentGeometry{a:grid_point(-8.0,t),b:grid_point(8.0,t),width_css:1.0,head_css:None});}
        let mut gridframe=frame;gridframe.viewport[2]=dpr as f32;
        draws.insert(0,self.renderer.prepare_segments(&grid,&gridframe,&ObjectGpu{edge_color:color([53,65,79]),..ObjectGpu::default()},&Material::default(),&EnvironmentGpu::default()).map_err(js)?);
        for(p,q,rgb)in[(origin,a,[125,202,192]),(origin,b,[169,191,241]),(a,b,[221,179,208])]{
            draws.push(self.renderer.prepare_segments(&[SegmentGeometry{a:p,b:q,width_css:2.5,head_css:None}],&frame,&ObjectGpu{edge_color:color(rgb),..ObjectGpu::default()},&Material::default(),&EnvironmentGpu::default()).map_err(js)?);
        }
        let theta=self.model.scalar("theta").map_err(js)?;let radius=0.42;
        let u=self.model.position("axis-x").map_err(js)?;let v=self.model.position("axis-y").map_err(js)?;
        let finite3=|p:DVec3|->std::result::Result<[Finite;3],JsValue>{Ok([Finite::new(p.x).map_err(js)?,Finite::new(p.y).map_err(js)?,Finite::new(p.z).map_err(js)?])};
        let arc=sim_model::ScenePrimitive::Arc{center:ScenePosition::Literal(finite3(origin)?),normal:finite3(u.cross(v))?,from:finite3(u)?,angle_radians:Finite::new(theta).map_err(js)?,radius:Finite::new(radius).map_err(js)?,width_css:Finite::new(2.0).map_err(js)?};
        let arc=sim_render::geometry::compile(&arc,|_|Ok(origin)).map_err(js)?;
        draws.push(self.renderer.prepare_segments(&arc.segments,&frame,&ObjectGpu{edge_color:color([234,196,127]),..ObjectGpu::default()},&Material::default(),&EnvironmentGpu::default()).map_err(js)?);
        self.input.refresh(&self.model,&matrices).map_err(js)?;
        for(name,position)in &self.input.handles{
            let selected=self.input.selected_name()==*name&&self.input.selection.selected.is_some();
            let rgb=match name.as_str(){"A"=>[125,202,192],"B"=>[169,191,241],"theta"=>[234,196,127],"body"=>[221,179,208],_=>[190,198,209]};
            let mut dotframe=frame;dotframe.viewport[2]=if selected{12.0}else{8.0}*dpr as f32;
            draws.push(self.renderer.prepare_segments(&[SegmentGeometry{a:*position,b:*position,width_css:8.0,head_css:None}],&dotframe,&ObjectGpu{edge_color:color(rgb),..ObjectGpu::default()},&Material::default(),&EnvironmentGpu::default()).map_err(js)?);
        }
        for(name,start,end)in &self.input.gizmos{
            let rgb=match name.as_str(){"gizmo.x"=>[235,136,136],"gizmo.y"=>[137,207,150],_=>[144,177,241]};
            draws.push(self.renderer.prepare_segments(&[SegmentGeometry{a:*start,b:*end,width_css:2.5,head_css:None}],&frame,&ObjectGpu{edge_color:color(rgb),..ObjectGpu::default()},&Material::default(),&EnvironmentGpu::default()).map_err(js)?.as_overlay());
        }
        // Arrowheads use the same CSS triangle and projected endpoint as Shape::ArrowHead.
        for(start,end,rgb,overlay)in [(origin,a,[125,202,192],false),(origin,b,[169,191,241],false)].into_iter().chain(self.input.gizmos.iter().map(|(name,start,end)|(*start,*end,match name.as_str(){"gizmo.x"=>[235,136,136],"gizmo.y"=>[137,207,150],_=>[144,177,241]},true))){
            if let(Some(pa),Some(pb))=(matrices.project(start),matrices.project(end)){
                let delta=pb.truncate()-pa.truncate();if delta.length_squared()>1e-20{
                    let direction=delta.normalize();let perp=DVec2::new(-direction.y,direction.x);
                    let corners=[pb.truncate(),pb.truncate()-direction*10.0+perp*5.0,pb.truncate()-direction*10.0-perp*5.0];
                    let vertices=corners.map(|p|matrices.unproject(p,pb.z)).into_iter().collect::<sim_render::Result<Vec<_>>>().map_err(js)?.into_iter().map(|p|Vertex{position:p.as_vec3().to_array(),normal:[0.0,0.0,1.0],tangent:[1.0,0.0,0.0]}).collect::<Vec<_>>();
                    let arrow_material=Material{surface:standard_surface_bsdf::SurfaceInputs{base:0.0,specular:0.0,emission:1.0,emission_color:color(rgb)[..3].try_into().map_err(js)?,..standard_surface_bsdf::SurfaceInputs::MATERIALX_DEFAULT},..Material::default()};
                    let draw=self.renderer.prepare(&vertices,&[0,1,2],&frame,&ObjectGpu::default(),&arrow_material,&EnvironmentGpu::default(),false).map_err(js)?;draws.push(if overlay{draw.as_overlay()}else{draw});
                }
            }
        }
        let output=match self.surface.get_current_texture(){wgpu::CurrentSurfaceTexture::Success(v)|wgpu::CurrentSurfaceTexture::Suboptimal(v)=>v,other=>return Err(js(format!("surface:{other:?}")))};
        let target=output.texture.create_view(&wgpu::TextureViewDescriptor{format:Some(wgpu::TextureFormat::Bgra8UnormSrgb),..Default::default()});
        let depth=self.depth.as_ref().ok_or_else(||js("depthmissing"))?.create_view(&Default::default());
        let mut encoder=self.device.create_command_encoder(&Default::default());let clear=color([32,38,46]);
        let msaa=self.multisample.as_ref().ok_or_else(||js("MSAA target missing"))?.create_view(&Default::default());
        self.renderer.encode_view(&mut encoder,&draws.iter().collect::<Vec<_>>(),&msaa,&depth,Some(&target),wgpu::Color{r:clear[0]as f64,g:clear[1]as f64,b:clear[2]as f64,a:1.0},None).map_err(js)?;
        self.queue.submit([encoder.finish()]);self.queue.present(output);
        let project=|p:DVec3|matrices.project(p).map(|v|[v.x,v.y]);
        let numeric=|v:f64|{let s=format!("{v:.4}");let number=s.trim_end_matches('0').trim_end_matches('.');if self.model.locale=="ru"{number.replace('.',",")}else{number.to_owned()}};
        let av=self.model.scalar("a").map_err(js)?;let bv=self.model.scalar("b").map_err(js)?;let c2=self.model.scalar("c2").map_err(js)?;
        let a2=self.model.scalar("a2").map_err(js)?;let b2=self.model.scalar("b2").map_err(js)?;let cross=self.model.scalar("cross").map_err(js)?;let cosine=self.model.scalar("cos_theta").map_err(js)?;
        let angle_degrees=DisplayUnit::Degrees.from_canonical(Finite::new(theta).map_err(js)?,Unit::ANGLE).map_err(js)?.get();
        let labels=json!([
            {"id":"a","text":format!("a = {}",numeric(av)),"position":project((origin+a)*0.5),"offset":[-36.0,-10.0],"role":"a"},
            {"id":"b","text":format!("b = {}",numeric(bv)),"position":project((origin+b)*0.5),"offset":[0.0,16.0],"role":"b"},
            {"id":"c2","text":format!("c² = {}",numeric(c2)),"position":project((a+b)*0.5),"offset":[28.0,-10.0],"role":"result"},
            {"id":"theta","text":format!("{}°",numeric(angle_degrees)),"position":project(origin+u*0.65+v*0.3),"offset":[6.0,-6.0],"role":"angle"},
            {"id":"O","text":"O","position":project(origin),"offset":[-12.0,12.0],"role":"muted"},
            {"id":"A","text":"A","position":project(a),"offset":[0.0,-16.0],"role":"muted"},
            {"id":"B","text":"B","position":project(b),"offset":[12.0,12.0],"role":"muted"}]);
        let frame_snapshot=self.model.session.snapshot();
        serde_json::to_string(&json!({"a":av,"b":bv,"theta":angle_degrees,"c2":c2,"a2":a2,"b2":b2,"cosine":cosine,"cross":cross,"labels":labels,
            "formula":format!("{}² + {}² − 2 · {} · {} · cos {}° = {}",numeric(av),numeric(bv),numeric(av),numeric(bv),numeric(angle_degrees),numeric(c2)),
            "expanded":format!("{} + {} − {} = {}",numeric(a2),numeric(b2),if cross<0.0{format!("({})",numeric(cross))}else{numeric(cross)},numeric(c2)),
            "display":{"a":numeric(av),"b":numeric(bv),"theta":numeric(angle_degrees)},
            "revision":frame_snapshot.revision(),"time":frame_snapshot.time().to_string(),"operations":frame_snapshot.evaluated_operations().len(),"view":if self.model.perspective{"3D"}else{"2D"},
            "vertices":[origin.to_array(),a.to_array(),b.to_array()],"basis":[u.to_array(),v.to_array()],"live_overrides":self.model.session.live_overrides(),"camera":self.model.view.camera(),"antialiasing":{"sample_count":4,"edge_filter":"owned capsule SDF"},"sample_count":4,"edge_filter":"owned capsule SDF","selected":self.input.selected_name(),"hovered":self.input.selection.chosen().map(|h|h.semantic.as_str()),"candidates":self.input.candidate_names(),"constraint":self.input.constraint,"can_undo":self.model.input.can_undo(),"can_redo":self.model.input.can_redo(),"fraction":frame_snapshot.time().ticks() as f64/TICKS_PER_SECOND as f64,"playing":self.model.session.is_playing(),"intent":self.model.session.playback_intent(),"gizmos":self.input.gizmos.iter().map(|(name,start,end)|json!({"id":name,"start":project(*start),"end":project(*end)})).collect::<Vec<_>>(),"handles":self.input.handles.iter().map(|(name,p)|json!({"id":name,"position":project(*p)})).collect::<Vec<_>>()})).map_err(js)
    }
    pub fn dispose(&mut self){if !self.disposed{self.depth=None;self.multisample=None;self.device.destroy();self.disposed=true;}}
}
