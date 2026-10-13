//! Acceptance consumer of the shared spatial picker and bounded live-input coordinator.
use crate::cosines::CosinesModel;
use glam::{DVec2,DVec3,DMat4};
use sim_model::{ObjectId};
use sim_render::picking::Shape;
use sim_render::{camera::CameraMatrices,frame::FrameStamp,picking::*,input_pointer::SelectionState,interaction::DragConstraint,bindings::TriangleTarget};
pub struct PointerDrag{pub pointer:u32,pub target:TriangleTarget,pub constraint:DragConstraint,pub anchor:DVec3,pub pivot:DVec3,pub press:DVec2,pub started:bool,pub signature:[u64;18],pub identity:FrameStamp}
pub struct CosinesInput{pub index:Option<PickIndex>,pub selection:SelectionState,pub target:TriangleTarget,pub constraint:String,pub drag:Option<PointerDrag>,pub sample:u64,pub layout:u64,pub handles:Vec<(String,DVec3)>,pub gizmos:Vec<(String,DVec3,DVec3)>}
impl Default for CosinesInput{fn default()->Self{Self{index:None,selection:SelectionState::default(),target:TriangleTarget::Group,constraint:"screen".into(),drag:None,sample:0,layout:0,handles:Vec::new(),gizmos:Vec::new()}}}
fn error(e:impl std::fmt::Display)->String{e.to_string()}
fn semantic(s:&str)->std::result::Result<SemanticId,String>{SemanticId::new(s).map_err(error)}
fn tool(s:&str)->Option<TriangleTarget>{match s{"O"=>Some(TriangleTarget::Vertex(0)),"A"=>Some(TriangleTarget::Vertex(1)),"B"=>Some(TriangleTarget::Vertex(2)),"body"=>Some(TriangleTarget::Group),"theta"=>Some(TriangleTarget::Angle),"side.a"=>Some(TriangleTarget::Side(0)),"side.b"=>Some(TriangleTarget::Side(1)),"side.c"=>Some(TriangleTarget::Side(2)),_=>None}}
impl CosinesInput{
 pub fn refresh(&mut self,model:&CosinesModel,camera:&CameraMatrices)->std::result::Result<(),String>{
  let stamp=DisplayStamp{frame:FrameStamp::from_frame(&model.session.snapshot()),scene_revision:model.scene.revision,topology_revision:0,view:model.view.id().clone(),layout_generation:self.layout,device_generation:0,sample:self.sample,view_signature:DisplayStamp::camera_signature(camera)};
  if self.index.as_ref().is_some_and(|index|index.stamp==stamp){return Ok(());}
  let p=model.binding.points(&model.session.snapshot()).map_err(error)?;let mut primitives=Vec::with_capacity(32);self.handles.clear();self.gizmos.clear();
  
  let handles=[("O",p[0]),("A",p[1]),("B",p[2]),("body",(p[0]+p[1]+p[2])/3.0),("theta",model.binding.pivot(&model.session.snapshot(),TriangleTarget::Angle).map_err(error)?)];
  for(name,position)in handles{let id=semantic(name)?;primitives.push(Primitive{component:Component::Handle(id.clone()),semantic:id,shape:Shape::Point{position,radius_css:5.0},priority:3});self.handles.push((name.into(),position));}
  for(index,(a,b,name))in [(0,1,"side.a"),(0,2,"side.b"),(1,2,"side.c")].into_iter().enumerate(){primitives.push(Primitive{component:Component::Segment(index as u32),semantic:semantic(name)?,shape:Shape::Segment{a:p[a],b:p[b],radius_css:5.0},priority:2});}
  for(index,(a,b,name))in [(0,1,"A"),(0,2,"B")].into_iter().enumerate(){primitives.push(Primitive{component:Component::ArrowHead(index as u32),semantic:semantic(name)?,shape:Shape::ArrowHead{a:p[a],b:p[b],length_css:10.0},priority:2});}
  if (p[1]-p[0]).cross(p[2]-p[0]).length_squared()>1e-20{primitives.push(Primitive{component:Component::Triangle(0),semantic:semantic("body")?,shape:Shape::Triangle{a:p[0],b:p[1],c:p[2]},priority:1});}
  let geometry=GeometryAsset::new(primitives,stamp.frame.revision).map_err(error)?;
  let scene=PickObject{object:ObjectId::new("triangle").map_err(error)?,instance:0,geometry,world:DMat4::IDENTITY,visible:true,pickable:true,locked:false,opacity:Opacity::Opaque,layer:Layer::Scene};
  let mut objects=vec![scene];
  if self.selection.selected.is_some(){
   let pivot=model.binding.pivot(&model.session.snapshot(),self.target).map_err(error)?;
   if let Some(projected)=camera.project(pivot){
    let offset=camera.unproject(projected.truncate()+DVec2::new(72.0,0.0),projected.z).map_err(error)?.distance(pivot);
    let mut primitives=Vec::with_capacity(6);
    for(index,(name,direction))in [("gizmo.x",DVec3::X),("gizmo.y",DVec3::Y),("gizmo.z",DVec3::Z)].into_iter().enumerate(){
     let end=pivot+direction*offset;let start=pivot+direction*offset*0.25;
     if camera.project(end).zip(camera.project(pivot)).is_none_or(|(end,pivot)|end.truncate().distance(pivot.truncate())<8.0){continue;}
     primitives.push(Primitive{component:Component::Segment(index as u32),semantic:semantic(name)?,shape:Shape::Segment{a:start,b:end,radius_css:5.0},priority:4});
     primitives.push(Primitive{component:Component::ArrowHead(index as u32),semantic:semantic(name)?,shape:Shape::ArrowHead{a:start,b:end,length_css:10.0},priority:4});
     self.gizmos.push((name.into(),start,end));
    }
    objects.push(PickObject{object:ObjectId::new("gizmo").map_err(error)?,instance:0,geometry:GeometryAsset::new(primitives,stamp.sample).map_err(error)?,world:DMat4::IDENTITY,visible:true,pickable:true,locked:false,opacity:Opacity::Opaque,layer:Layer::GizmoOverlay});
   }
  }
  self.index=Some(PickIndex::new(objects,stamp).map_err(error)?);Ok(())
 }
 pub fn hover(&mut self,model:&CosinesModel,camera:&CameraMatrices,point:DVec2,touch:bool)->std::result::Result<Option<String>,String>{
  self.refresh(model,camera)?;let index=self.index.as_ref().ok_or("no pick index")?;
  let query=PickQuery{stamp:&index.stamp,camera,pointer:point,clip:[0.0,0.0,camera.css()[0],camera.css()[1]],touch,filter:PickFilter::default(),offset:0,limit:256};
  Ok(self.selection.resolve(index,&query).map_err(error)?.map(|hit|hit.semantic.as_str().to_owned()))
 }
 pub fn cycle(&mut self,step:i32)->Option<String>{self.selection.cycle(step).map(|h|h.semantic.as_str().to_owned())}
 pub fn candidate_names(&self)->Vec<String>{self.selection.candidates().iter().map(|h|h.semantic.as_str().to_owned()).collect()}
 pub fn begin(&mut self,model:&mut CosinesModel,camera:&CameraMatrices,point:DVec2,touch:bool,pointer:u32)->std::result::Result<bool,String>{
  if self.drag.is_some(){return Err("pointer already captured".into());}
  self.hover(model,camera,point,touch)?;let Some(hit)=self.selection.choose()else{return Ok(false);};
  let name=hit.semantic.as_str();
  let profile=if let Some(axis)=name.strip_prefix("gizmo."){axis}else{self.target=tool(name).ok_or("unbound semantic component")?;&self.constraint};
  let pivot=model.binding.pivot(&model.session.snapshot(),self.target).map_err(error)?;
  let axis=match profile{"x"=>Some(DVec3::X),"y"=>Some(DVec3::Y),"z"=>Some(DVec3::Z),_=>None};
  let normal=match profile{"xy"=>DVec3::Z,"yz"=>DVec3::X,"xz"=>DVec3::Y,_=>if self.target==TriangleTarget::Angle{model.position("axis-x")?.cross(model.position("axis-y")?)}else{camera.toward_eye()}};
  let constraint=if let Some(direction)=axis{DragConstraint::Axis{origin:pivot,direction}}else{DragConstraint::Plane{origin:pivot,normal}};
  let anchor=constraint.intersect(camera.ray(point).map_err(error)?).map_err(error)?;
  self.drag=Some(PointerDrag{pointer,target:self.target,constraint,anchor,pivot,press:point,started:false,signature:DisplayStamp::camera_signature(camera),identity:FrameStamp::from_frame(&model.session.snapshot())});self.sample=self.sample.wrapping_add(1);Ok(true)
 }
 pub fn update(&mut self,model:&mut CosinesModel,camera:&CameraMatrices,point:DVec2,pointer:u32)->std::result::Result<(),String>{
  let drag=self.drag.as_mut().ok_or("no pointer gesture")?;if drag.pointer!=pointer{return Err("foreign pointer".into());}
  if drag.signature!=DisplayStamp::camera_signature(camera){return Err("view changed during pointer gesture".into());}
  let frame=FrameStamp::from_frame(&model.session.snapshot());
  if frame.generation!=drag.identity.generation||frame.document!=drag.identity.document||frame.session!=drag.identity.session||frame.epoch!=drag.identity.epoch||frame.document_revision!=drag.identity.document_revision{return Err("pointer identity/document changed".into());}
  if !drag.started{
   if point.distance_squared(drag.press)<1e-8{return Ok(());}
   drag.pivot=model.binding.pivot(&model.session.snapshot(),drag.target).map_err(error)?;
   drag.constraint=match drag.constraint{DragConstraint::Plane{normal,..}=>DragConstraint::Plane{origin:drag.pivot,normal:if drag.target==TriangleTarget::Angle{model.position("axis-x")?.cross(model.position("axis-y")?)}else{normal}},DragConstraint::Axis{direction,..}=>DragConstraint::Axis{origin:drag.pivot,direction}};
   drag.anchor=drag.constraint.intersect(camera.ray(drag.press).map_err(error)?).map_err(error)?;
   let first=drag.constraint.intersect(camera.ray(point).map_err(error)?).map_err(error)?;
   if first.distance_squared(drag.anchor)<1e-20{return Ok(());}
  }
  let intersection=drag.constraint.intersect(camera.ray(point).map_err(error)?).map_err(error)?;
  let patch=model.binding.solve(&model.session.snapshot(),drag.target,drag.pivot+intersection-drag.anchor).map_err(error)?;
  let pending=!drag.started;
  if pending{model.begin_drag(drag.target)?;}
  let admitted=model.input.update(&mut model.session,patch);
  let changed=match admitted{
   Ok(changed)=>changed,
   Err(failure)=>{if pending{model.input.abandon(&model.session);}return Err(error(failure));}
  };
  if pending&&!changed{model.input.abandon(&model.session);return Ok(());}
  // This owns an admitted write before compilation. A render failure remains compensable.
  drag.started=true;self.sample=self.sample.wrapping_add(1);model.compile()
 }
 pub fn end(&mut self,model:&mut CosinesModel,pointer:u32,cancel:bool)->std::result::Result<(),String>{
  let drag=self.drag.as_ref().ok_or("no pointer gesture")?;if drag.pointer!=pointer{return Err("foreign pointer".into());}
  let started=drag.started;
  let terminal=if started{model.end_drag(cancel)}else{Ok(())};
  // Pointer ownership ends even when compensation was refused. Its error remains visible.
  self.drag=None;self.sample=self.sample.wrapping_add(1);terminal
 }
 pub fn selected_name(&self)->String{match self.target{TriangleTarget::Vertex(0)=>"O",TriangleTarget::Vertex(1)=>"A",TriangleTarget::Vertex(2)=>"B",TriangleTarget::Group=>"body",TriangleTarget::Angle=>"theta",TriangleTarget::Side(0)=>"side.a",TriangleTarget::Side(1)=>"side.b",TriangleTarget::Side(2)=>"side.c",_=>"body"}.into()}
}
