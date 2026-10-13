//! Typed inverse bindings consume immutable runtime coordinates and return atomic input patches.
use crate::{input::{InputError,InputResult,OverridePatch},interaction::inverse_triangle};
use glam::DVec3;
use sim_model::{ParameterId,ValueId,Value};
use sim_runtime::FrameSnapshot;
#[derive(Clone,Debug)]pub struct TriangleBinding{
 pub origin:ParameterId,pub a:ParameterId,pub b:ParameterId,pub theta:ParameterId,pub u_raw:ParameterId,pub v_raw:ParameterId,
 pub world:[ValueId;3],pub axes:[ValueId;2],pub angle:ValueId,pub angle_radius:f64,
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum TriangleTarget{Vertex(usize),Group,Side(usize),Angle}
pub fn vector(frame:&FrameSnapshot,id:&ValueId)->InputResult<DVec3>{match frame.truth().get(id){Some(Value::Vector(v))if v.len()==3=>Ok(DVec3::new(v[0].get(),v[1].get(),v[2].get())),_=>Err(InputError::Invalid("binding requires vec3 value"))}}
fn value(v:DVec3)->InputResult<Value>{Value::vector(v.to_array().to_vec()).map_err(InputError::Runtime)}
fn scalar(v:f64)->InputResult<Value>{Value::scalar(v).map_err(InputError::Runtime)}
impl TriangleBinding{
 pub fn keys(&self,target:TriangleTarget)->Vec<ParameterId>{match target{TriangleTarget::Group=>vec![self.origin.clone()],TriangleTarget::Angle=>vec![self.theta.clone()],_=>vec![self.origin.clone(),self.a.clone(),self.b.clone(),self.theta.clone(),self.u_raw.clone(),self.v_raw.clone()]}}
 pub fn points(&self,frame:&FrameSnapshot)->InputResult<[DVec3;3]>{Ok([vector(frame,&self.world[0])?,vector(frame,&self.world[1])?,vector(frame,&self.world[2])?])}
 pub fn pivot(&self,frame:&FrameSnapshot,target:TriangleTarget)->InputResult<DVec3>{let p=self.points(frame)?;Ok(match target{TriangleTarget::Vertex(index)if index<3=>p[index],TriangleTarget::Group=>(p[0]+p[1]+p[2])/3.0,TriangleTarget::Side(index)if index<3=>{let pairs=[(0,1),(0,2),(1,2)];let(a,b)=pairs[index];(p[a]+p[b])*0.5},TriangleTarget::Angle=>{let theta=frame.truth().get(&self.angle).ok_or(InputError::Invalid("angle value missing"))?.as_scalar().map_err(InputError::Runtime)?;if !self.angle_radius.is_finite()||self.angle_radius<=0.0{return Err(InputError::Invalid("angle handle radius"));}p[0]+(vector(frame,&self.axes[0])?*theta.cos()+vector(frame,&self.axes[1])?*theta.sin())*self.angle_radius},_=>return Err(InputError::Invalid("binding target"))})}
 /// Target is a world-space constrained point. Group translation preserves raw generators and scalar bits.
 pub fn solve(&self,frame:&FrameSnapshot,target:TriangleTarget,position:DVec3)->InputResult<OverridePatch>{
  if !position.is_finite(){return Err(InputError::Invalid("nonfinite binding point"));}
  let mut p=self.points(frame)?;
  match target{
   TriangleTarget::Group=>{let delta=position-(p[0]+p[1]+p[2])/3.0;return Ok(vec![(self.origin.clone(),Some(value(p[0]+delta)?))]);}
   TriangleTarget::Angle=>{
    let direction=position-p[0];let u=vector(frame,&self.axes[0])?;let v=vector(frame,&self.axes[1])?;
    if direction.length_squared()<1e-20{return Err(InputError::Invalid("angle direction is zero"));}
    let theta=direction.dot(v).atan2(direction.dot(u));
    if !(0.0..=std::f64::consts::PI).contains(&theta){return Err(InputError::Invalid("angle outside 0..pi"));}
    return Ok(vec![(self.theta.clone(),Some(scalar(theta)?))]);
   }
   TriangleTarget::Vertex(index)if index<3=>p[index]=position,
   TriangleTarget::Side(index)if index<3=>{let pairs=[(0,1),(0,2),(1,2)];let(a,b)=pairs[index];let delta=position-(p[a]+p[b])*0.5;p[a]+=delta;p[b]+=delta;},
   _=>return Err(InputError::Invalid("binding target")),
  }
  let pose=inverse_triangle(p[0],p[1],p[2],vector(frame,&self.axes[1])?).map_err(|_|InputError::Invalid("inverse pose outside profile or singular"))?;
  Ok(vec![(self.origin.clone(),Some(value(pose.origin)?)),(self.a.clone(),Some(scalar(pose.a)?)),(self.b.clone(),Some(scalar(pose.b)?)),(self.theta.clone(),Some(scalar(pose.theta)?)),(self.u_raw.clone(),Some(value(pose.u)?)),(self.v_raw.clone(),Some(value(pose.v)?))])
 }
}
/// Generic direct vector binding shares the same coordinator and constraint solver as triangle tools.
#[derive(Clone,Debug)]pub struct VectorBinding{pub parameter:ParameterId}
impl VectorBinding{pub fn solve(&self,position:DVec3)->InputResult<OverridePatch>{Ok(vec![(self.parameter.clone(),Some(value(position)?))])}}
