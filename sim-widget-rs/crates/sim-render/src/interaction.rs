//! Reusable, device-independent pointer geometry and camera navigation.
use crate::{Result, error::invalid, camera::{Camera,Projection,Ray}};
use glam::{DVec2,DVec3,DQuat};
#[derive(Clone,Copy,Debug,PartialEq)]
pub enum Navigation { Orbit, Pan, Dolly }
#[derive(Clone,Copy,Debug,PartialEq)]
pub enum DragConstraint { Plane { origin:DVec3, normal:DVec3 }, Axis { origin:DVec3, direction:DVec3 } }
impl DragConstraint {
 pub fn intersect(self,ray:Ray)->Result<DVec3>{
  match self {
   Self::Plane{origin,normal}=>{
    if !origin.is_finite()||!normal.is_finite()||normal.length_squared()<1e-20{return Err(invalid("drag.plane","invalid plane"));}
    let denominator=normal.dot(ray.direction);
    if denominator.abs()<1e-10{return Err(invalid("drag.plane","view ray parallel to plane"));}
    let t=normal.dot(origin-ray.origin)/denominator;
    if !t.is_finite()||t<0.0{return Err(invalid("drag.plane","plane behind view ray"));}
    Ok(ray.origin+ray.direction*t)
   }
   Self::Axis{origin,direction}=>{
    if !origin.is_finite()||!direction.is_finite()||direction.length_squared()<1e-20{return Err(invalid("drag.axis","invalid axis"));}
    let d=direction.normalize(); let k=d.dot(ray.direction);let denominator=1.0-k*k;
    if denominator<1e-10{return Err(invalid("drag.axis","axis parallel to view ray"));}
    let offset=ray.origin-origin;
    Ok(origin+d*((offset.dot(d)-k*offset.dot(ray.direction))/denominator))
   }
  }
 }
}
impl Camera {
 /// Navigation is pure view state; the host persists the result without a math command.
 pub fn navigate(&self,kind:Navigation,from:DVec2,to:DVec2,css:[f64;2],precision:bool)->Result<Self>{
  if !from.is_finite()||!to.is_finite(){return Err(invalid("navigation","nonfinite pointer"));}
  let matrices=self.matrices(css)?;let eye=DVec3::from_array(self.eye);let pivot=DVec3::from_array(self.target);
  let delta=(to-from)*if precision{0.1}else{1.0};let mut result=*self;
  match kind{
   Navigation::Orbit=>{
    let right=matrices.view().inverse().x_axis.truncate().normalize();
    let up=DVec3::from_array(self.up).normalize();
    let rotation=DQuat::from_axis_angle(right,-delta.y*0.006)*DQuat::from_axis_angle(up,-delta.x*0.006);
    result.eye=(pivot+rotation*(eye-pivot)).to_array();
    result.up=(rotation*up).to_array();
   }
   Navigation::Pan=>{
    let plane=DragConstraint::Plane{origin:pivot,normal:matrices.toward_eye()};
    let movement=plane.intersect(matrices.ray(from)?)?-plane.intersect(matrices.ray(from+delta)?)?;
    result.eye=(eye+movement).to_array();result.target=(pivot+movement).to_array();
   }
   Navigation::Dolly=>{result=self.zoom_at(from,delta.y*0.01,css)?;}
  }
  result.matrices(css)?;Ok(result)
 }
 /// Exponential dolly keeps the world point beneath the cursor on the pivot plane fixed.
 pub fn zoom_at(&self,cursor:DVec2,log_scale:f64,css:[f64;2])->Result<Self>{
  if !log_scale.is_finite()||log_scale.abs()>20.0{return Err(invalid("navigation.zoom","invalid dolly delta"));}
  let m=self.matrices(css)?;let pivot=DVec3::from_array(self.target);
  let plane=DragConstraint::Plane{origin:pivot,normal:m.toward_eye()};
  let anchor=plane.intersect(m.ray(cursor)?)?;let scale=log_scale.exp();
  let mut result=*self;
  match self.projection{
   Projection::Orthographic{vertical_span}=>{let span=vertical_span*scale;if !(0.001..1e8).contains(&span){return Err(invalid("navigation.zoom","span outside range"));}result.projection=Projection::Orthographic{vertical_span:span};}
   Projection::Perspective{..}=>{let distance=(DVec3::from_array(self.eye)-pivot).length()*scale;if distance<self.near*2.0||distance>self.far*0.8{return Err(invalid("navigation.zoom","distance outside clip range"));}result.eye=(pivot+m.toward_eye()*distance).to_array();}
  }
  let after=result.matrices(css)?;let displacement=anchor-plane.intersect(after.ray(cursor)?)?;
  result.eye=(DVec3::from_array(result.eye)+displacement).to_array();result.target=(pivot+displacement).to_array();
  result.matrices(css)?;Ok(result)
 }
}
/// Canonical inverse of O+a*(u*cos(theta)+v*sin(theta)), O+b*u.
/// At collinearity the prior valid plane determines v.
#[derive(Clone,Copy,Debug)]
pub struct TrianglePose { pub origin:DVec3,pub a:f64,pub b:f64,pub theta:f64,pub u:DVec3,pub v:DVec3 }
pub fn inverse_triangle(origin:DVec3,a_world:DVec3,b_world:DVec3,prior_v:DVec3)->Result<TrianglePose>{
 if [origin,a_world,b_world,prior_v].iter().any(|v|!v.is_finite()){return Err(invalid("triangle.inverse","nonfinite input"));}
 let r=a_world-origin;let s=b_world-origin;let a=r.length();let b=s.length();
 if a<0.1||b<0.1||a>8.0||b>8.0{return Err(invalid("triangle.length","length outside [0.1,8]"));}
 let u=s/b;let x=r.dot(u);let w=r-u*x;let perpendicular=w.length();
 let theta=perpendicular.atan2(x);
 let v=if perpendicular>1e-10*a {w/perpendicular}else{
  let projected=prior_v-u*prior_v.dot(u);
  if projected.length_squared()>1e-16{projected.normalize()}else{
   let fallback=if u.x.abs()<=u.y.abs()&&u.x.abs()<=u.z.abs(){DVec3::X}else if u.y.abs()<=u.z.abs(){DVec3::Y}else{DVec3::Z};
   (fallback-u*fallback.dot(u)).normalize()
  }
 };
 Ok(TrianglePose{origin,a,b,theta,u,v})
}
