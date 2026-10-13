//! View-scoped event routing and selection. Hover, press and cycling call one stamped resolver.
use crate::{picking::{PickIndex,PickQuery,PickHit,DisplayStamp,Component},interaction::Navigation,Result,error::invalid};
use glam::DVec2;
use sim_model::{ObjectId,ViewId};
#[derive(Clone,Copy,Debug,PartialEq)]pub struct ViewportRect{pub left:f64,pub top:f64,pub width:f64,pub height:f64,pub dpr:f64}
impl ViewportRect{
 pub fn validate(self)->Result<Self>{if [self.left,self.top,self.width,self.height,self.dpr].iter().any(|v|!v.is_finite())||self.width<=0.0||self.height<=0.0||self.dpr<=0.0||self.dpr>16.0{return Err(invalid("input.viewport","invalid rect/DPR"));}Ok(self)}
 pub fn client_to_css(self,point:DVec2)->Result<DVec2>{self.validate()?;if !point.is_finite(){return Err(invalid("input.pointer","nonfinite client coordinates"));}Ok(point-DVec2::new(self.left,self.top))}
 pub fn physical_to_css(self,point:DVec2)->Result<DVec2>{self.validate()?;if !point.is_finite(){return Err(invalid("input.pointer","nonfinite physical coordinates"));}Ok(point/self.dpr)}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum PointerKind{Mouse,Pen,Touch}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum PointerButton{Primary,Middle,Secondary}
#[derive(Clone,Copy,Debug,Default)]pub struct Modifiers{pub space:bool,pub alt:bool,pub shift:bool}
pub fn camera_gesture(button:PointerButton,modifiers:Modifiers)->Option<Navigation>{
 if !modifiers.space&&!modifiers.alt{return None;}
 Some(match button{PointerButton::Primary=>Navigation::Orbit,PointerButton::Middle=>Navigation::Pan,PointerButton::Secondary=>Navigation::Dolly})
}
#[derive(Clone,Debug,PartialEq,Eq)]pub struct PickIdentity{pub view:ViewId,pub object:ObjectId,pub instance:u32,pub component:Component}
impl PickIdentity{pub fn from_hit(hit:&PickHit)->Self{Self{view:hit.stamp.view.clone(),object:hit.object.clone(),instance:hit.instance,component:hit.component.clone()}}}
#[derive(Default)]pub struct SelectionState{pub selected:Option<PickIdentity>,pub hovered:Option<PickIdentity>,candidates:Vec<PickHit>,cursor:usize,stamp:Option<DisplayStamp>}
impl SelectionState{
 /// A page is bounded by PickQuery. The host may expose additional pages without a second hit-test implementation.
 pub fn resolve<'a>(&'a mut self,index:&PickIndex,query:&PickQuery<'_>)->Result<Option<&'a PickHit>>{
  let page=index.query(query)?;
  let old=self.candidates.get(self.cursor).map(PickIdentity::from_hit);
  self.candidates=page.hits;
  self.cursor=old.and_then(|old|self.candidates.iter().position(|h|PickIdentity::from_hit(h)==old)).unwrap_or(0);
  self.stamp=Some(query.stamp.clone());
  self.hovered=self.candidates.get(self.cursor).map(PickIdentity::from_hit);
  Ok(self.candidates.get(self.cursor))
 }
 pub fn cycle(&mut self,step:i32)->Option<&PickHit>{if self.candidates.is_empty(){return None;}self.cursor=(self.cursor as i64+i64::from(step)).rem_euclid(self.candidates.len()as i64)as usize;self.hovered=self.candidates.get(self.cursor).map(PickIdentity::from_hit);self.candidates.get(self.cursor)}
 pub fn choose(&mut self)->Option<PickHit>{let hit=self.candidates.get(self.cursor).cloned();self.selected=hit.as_ref().map(PickIdentity::from_hit);hit}
 pub fn candidates(&self)->&[PickHit]{&self.candidates}
 pub fn chosen(&self)->Option<&PickHit>{self.candidates.get(self.cursor)}
 pub fn clear_hover(&mut self){self.hovered=None;self.candidates.clear();self.stamp=None;self.cursor=0;}
}
