//! Neutral f64 transform/presentation boundary. The caller owns picking, document admission and history.
use glam::{DMat3,DMat4,DQuat,DVec2,DVec3,DVec4};
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd,Hash)]pub struct GizmoId(pub u64);
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd,Hash)]pub struct TargetId(pub u64);
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd,Hash)]pub struct PointerId(pub u64);
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd,Hash)]pub struct GestureId(pub u64);
#[repr(u8)]#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd,Hash)]pub enum Axis{X=0,Y=1,Z=2}
impl Axis{pub fn vector(self)->DVec3{match self{Self::X=>DVec3::X,Self::Y=>DVec3::Y,Self::Z=>DVec3::Z}}}
#[repr(u8)]#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd,Hash)]pub enum Plane{XY=0,XZ=1,YZ=2}
#[repr(u8)]#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd,Hash)]pub enum Box2Grip{NE=0,NW=1,SW=2,SE=3,N=4,E=5,S=6,W=7}
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd,Hash)]pub enum HandleKind{
 TranslateAxis(Axis),TranslatePlane(Plane),TranslateScreen,RotateAxis(Axis),RotateView,Arcball,
 ScaleAxis(Axis),ScalePlane(Plane),ScaleUniform,Box2Grip(Box2Grip),Box3Grip{sx:i8,sy:i8,sz:i8},
 Shear{source:Axis,affected:Axis,plane:Plane},CornerPin{corner:u8}
}
impl HandleKind{
 /// Version-one stable encoding. Independent of Rust TypeId, hash seeds, build and iteration order.
 pub fn stable_code(self)->Result<u32,Error>{Ok(match self{
 Self::TranslateAxis(a)=>0x100+a as u32,Self::TranslatePlane(p)=>0x110+p as u32,Self::TranslateScreen=>0x120,
 Self::RotateAxis(a)=>0x200+a as u32,Self::RotateView=>0x210,Self::Arcball=>0x220,
 Self::ScaleAxis(a)=>0x300+a as u32,Self::ScalePlane(p)=>0x310+p as u32,Self::ScaleUniform=>0x320,
 Self::Box2Grip(h)=>0x400+h as u32,Self::Box3Grip{sx,sy,sz}=>{
 if ![-1,0,1].contains(&sx)||![-1,0,1].contains(&sy)||![-1,0,1].contains(&sz)||(sx,sy,sz)==(0,0,0){return Err(Error::Handle);}
 0x500+((sx+1)as u32)*9+((sy+1)as u32)*3+(sz+1)as u32},
 Self::Shear{source,affected,plane}=>{if source==affected||!plane.contains(source)||!plane.contains(affected){return Err(Error::Handle);}0x600+(plane as u32)*16+(source as u32)*4+affected as u32},
 Self::CornerPin{corner}=>{if corner>3{return Err(Error::Handle);}0x700+corner as u32}
 })}
}
impl Plane{pub fn contains(self,a:Axis)->bool{match self{Self::XY=>a!=Axis::Z,Self::XZ=>a!=Axis::Y,Self::YZ=>a!=Axis::X}}pub fn axes(self)->(Axis,Axis){match self{Self::XY=>(Axis::X,Axis::Y),Self::XZ=>(Axis::X,Axis::Z),Self::YZ=>(Axis::Y,Axis::Z)}}}
#[derive(Clone,Copy,Debug,Eq,PartialEq,Ord,PartialOrd,Hash)]pub struct HandleId{pub gizmo:GizmoId,pub kind:HandleKind}
#[derive(Clone,Debug,Eq,PartialEq)]pub enum Error{Camera,Pointer,Handle,Target,Basis,Constraint,Capability,Budget,Stale,Busy,ForeignPointer,Order,ReceiptConflict,Terminal,Overflow}
impl std::fmt::Display for Error{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{write!(f,"gizmo.{:?}",self)}}impl std::error::Error for Error{}
#[derive(Clone,Copy,Debug,PartialEq)]pub enum Handedness{Right,Left}
#[derive(Clone,Copy,Debug,PartialEq)]pub enum ProjectionKind{Perspective,Orthographic}
#[derive(Clone,Copy,Debug,PartialEq)]pub struct ClipDepth{near:f64,far:f64,min:f64,max:f64}
impl ClipDepth{
 pub const REVERSED_ZERO_TO_ONE:Self=Self{near:1.,far:0.,min:0.,max:1.};
 pub const ZERO_TO_ONE:Self=Self{near:0.,far:1.,min:0.,max:1.};
 pub const OPEN_GL:Self=Self{near:-1.,far:1.,min:-1.,max:1.};
 pub fn new(near:f64,far:f64,min:f64,max:f64)->Result<Self,Error>{if ![near,far,min,max].iter().all(|x|x.is_finite())||min>=max||near==far||near<min||near>max||far<min||far>max{return Err(Error::Camera);}Ok(Self{near,far,min,max})}
 pub fn near(self)->f64{self.near}pub fn far(self)->f64{self.far}pub fn range(self)->[f64;2]{[self.min,self.max]}
}
#[derive(Clone,Debug,PartialEq)]pub struct CameraPacket64{view:DMat4,projection:DMat4,vp:DMat4,inverse:DMat4,css:[f64;2],handedness:Handedness,kind:ProjectionKind,depth:ClipDepth}
impl CameraPacket64{
 pub fn new(view:[f64;16],projection:[f64;16],css:[f64;2],handedness:Handedness,kind:ProjectionKind,depth:ClipDepth)->Result<Self,Error>{
 let view=DMat4::from_cols_array(&view);let projection=DMat4::from_cols_array(&projection);
 if !view.is_finite()||!projection.is_finite()||!css.iter().all(|x|x.is_finite()&&*x>0.&&*x<=65536.)||view.determinant().abs()<1e-15||projection.determinant().abs()<1e-15{return Err(Error::Camera);}
 let vp=projection*view;let inverse=vp.inverse();if !inverse.is_finite(){return Err(Error::Camera);}
 Ok(Self{view,projection,vp,inverse,css,handedness,kind,depth})
 }
 pub fn view(&self)->[f64;16]{self.view.to_cols_array()}pub fn projection(&self)->[f64;16]{self.projection.to_cols_array()}pub fn inverse(&self)->[f64;16]{self.inverse.to_cols_array()}pub fn viewport_css(&self)->[f64;2]{self.css}pub fn depth(&self)->ClipDepth{self.depth}pub fn handedness(&self)->Handedness{self.handedness}pub fn kind(&self)->ProjectionKind{self.kind}
 pub fn project(&self,world:[f64;3])->Option<[f64;3]>{let c=self.vp*DVec3::from_array(world).extend(1.);if !self.clip_contains(c){return None;}let n=c.truncate()/c.w;Some([(n.x+1.)*self.css[0]*0.5,(1.-n.y)*self.css[1]*0.5,n.z])}
 fn clip_contains(&self,p:DVec4)->bool{p.is_finite()&&p.w>0.&&p.x.abs()<=p.w&&p.y.abs()<=p.w&&p.z>=self.depth.min*p.w&&p.z<=self.depth.max*p.w}
 pub fn ray(&self,local_css:[f64;2])->Result<WorldRay64,Error>{
 if !local_css.iter().all(|x|x.is_finite()){return Err(Error::Pointer);}
 let n=DVec2::new(local_css[0]/self.css[0]*2.-1.,1.-local_css[1]/self.css[1]*2.);
 let a=self.inverse*DVec4::new(n.x,n.y,self.depth.near,1.);let b=self.inverse*DVec4::new(n.x,n.y,self.depth.far,1.);
 if !a.is_finite()||!b.is_finite()||a.w.abs()<1e-15{return Err(Error::Camera);}
 let origin=a.truncate()/a.w;let d=if b.w.abs()<1e-15{b.truncate()}else{b.truncate()/b.w-origin};
 WorldRay64::new(origin.to_array(),d.to_array())
 }
 pub fn check_pointer(&self,p:&PointerSample64)->Result<(),Error>{let expected=self.ray(p.local_css)?;let a=DVec3::from_array(expected.origin);let b=DVec3::from_array(p.ray.origin);if (a-b).length()>1e-9*(1.+a.length())||DVec3::from_array(expected.direction).dot(DVec3::from_array(p.ray.direction))<1.-1e-10{return Err(Error::Pointer);}Ok(())}
 pub fn clip_segment(&self,a:[f64;3],b:[f64;3])->Option<[[f64;3];2]>{
 let ca=self.vp*DVec3::from_array(a).extend(1.);let cb=self.vp*DVec3::from_array(b).extend(1.);if !ca.is_finite()||!cb.is_finite(){return None;}
 let values=|c:DVec4|[c.w+c.x,c.w-c.x,c.w+c.y,c.w-c.y,c.z-self.depth.min*c.w,self.depth.max*c.w-c.z,c.w-1e-12];
 let aa=values(ca);let bb=values(cb);let(mut lo,mut hi)=(0f64,1f64);for i in 0..7{if aa[i]<0.&&bb[i]<0.{return None;}if aa[i]<0.{lo=lo.max(aa[i]/(aa[i]-bb[i]));}if bb[i]<0.{hi=hi.min(aa[i]/(aa[i]-bb[i]));}}if lo>hi{return None;}let a=DVec3::from_array(a);let b=DVec3::from_array(b);Some([a.lerp(b,lo).to_array(),a.lerp(b,hi).to_array()])
 }
 pub(crate) fn scale_at(&self,p:DVec3)->Result<f64,Error>{let c=self.vp*p.extend(1.);if !c.is_finite()||c.w<=1e-12{return Err(Error::Camera);}let scale=(2.*c.w/self.projection.x_axis.x/self.css[0]).abs();if !scale.is_finite()||scale<=0.{Err(Error::Camera)}else{Ok(scale)}}
}
#[derive(Clone,Copy,Debug,PartialEq)]pub struct WorldRay64{origin:[f64;3],direction:[f64;3]}
impl WorldRay64{pub fn new(origin:[f64;3],direction:[f64;3])->Result<Self,Error>{let o=DVec3::from_array(origin);let d=DVec3::from_array(direction);if !o.is_finite()||!d.is_finite()||d.length_squared()<1e-24{return Err(Error::Pointer);}Ok(Self{origin,direction:d.normalize().to_array()})}pub fn origin(self)->[f64;3]{self.origin}pub fn direction(self)->[f64;3]{self.direction}}
#[derive(Clone,Copy,Debug,Default,PartialEq)]pub struct Modifiers64{pub precision:bool,pub snapping:bool,pub aspect_lock:bool}
#[derive(Clone,Copy,Debug,PartialEq)]pub enum PointerKind{Mouse,Touch,Pen,Keyboard}
#[derive(Clone,Copy,Debug,PartialEq)]pub struct PointerSample64{pub pointer:PointerId,pub local_css:[f64;2],pub ray:WorldRay64,pub modifiers:Modifiers64,pub kind:PointerKind}
#[derive(Clone,Copy,Debug,PartialEq)]pub enum PivotMode{Median,Active,IndividualOrigins,CustomWorld([f64;3])}
#[derive(Clone,Copy,Debug,PartialEq)]pub enum Orientation64{Global,Local(TargetId),Parent(TargetId),CustomOrthonormal([f64;9])}
#[derive(Clone,Copy,Debug,PartialEq)]pub struct OperationFrame64{pub pivot:PivotMode,pub orientation:Orientation64,pub active:TargetId}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]pub enum TargetCapability{Affine,Trs,PlanarWarp}
#[derive(Clone,Debug,PartialEq)]pub struct TargetFrame64{pub id:TargetId,pub parent:Option<TargetId>,pub world_affine:[f64;16],pub parent_affine:[f64;16],pub half_extent:[f64;3],pub shear:[f64;2],pub corners:Option<[[f64;2];4]>,pub capability:TargetCapability}
fn affine(a:[f64;16])->Result<DMat4,Error>{let m=DMat4::from_cols_array(&a);if !m.is_finite()||m.x_axis.w.abs()>1e-12||m.y_axis.w.abs()>1e-12||m.z_axis.w.abs()>1e-12||(m.w_axis.w-1.).abs()>1e-12||m.determinant().abs()<1e-15{return Err(Error::Target);}Ok(m)}
impl TargetFrame64{pub fn validate(&self)->Result<(),Error>{affine(self.world_affine)?;affine(self.parent_affine)?;if !self.half_extent.iter().all(|x|x.is_finite()&&*x>0.)||!self.shear.iter().all(|x|x.is_finite())||self.corners.as_ref().is_some_and(|c|c.iter().flatten().any(|x|!x.is_finite())){return Err(Error::Target);}Ok(())}}
pub(crate) fn rotation_frame(m:DMat4)->Result<DMat3,Error>{
 let mut r=DMat3::from_cols(m.x_axis.truncate(),m.y_axis.truncate(),m.z_axis.truncate());if !r.is_finite()||r.determinant().abs()<1e-15{return Err(Error::Basis);}
 // Polar rotation preserves shear without pretending the affine was TRS. Reflect X deterministically.
 if r.determinant()<0.{r.x_axis=-r.x_axis;}for _ in 0..32{let next=(r+r.inverse().transpose())*0.5;let difference=next.to_cols_array().into_iter().zip(r.to_cols_array()).map(|(a,b)|(a-b).abs()).fold(0f64,f64::max);r=next;if difference<1e-13{break;}}
 validate_basis(r)?;Ok(r)
}
pub(crate) fn validate_basis(b:DMat3)->Result<(),Error>{if !b.is_finite()||(b.transpose()*b-DMat3::IDENTITY).to_cols_array().iter().any(|x|x.abs()>1e-10)||(b.determinant()-1.).abs()>1e-10{Err(Error::Basis)}else{Ok(())}}
pub(crate) fn operation_frame(frame:OperationFrame64,targets:&[TargetFrame64])->Result<(DVec3,DMat3),Error>{
 let active=targets.iter().find(|x|x.id==frame.active).ok_or(Error::Target)?;
 let pivot=match frame.pivot{PivotMode::Median=>targets.iter().map(|x|DMat4::from_cols_array(&x.world_affine).w_axis.truncate()).sum::<DVec3>()/targets.len()as f64,PivotMode::Active|PivotMode::IndividualOrigins=>DMat4::from_cols_array(&active.world_affine).w_axis.truncate(),PivotMode::CustomWorld(p)=>DVec3::from_array(p)};
 let basis=match frame.orientation{Orientation64::Global=>DMat3::IDENTITY,Orientation64::Local(id)=>rotation_frame(affine(targets.iter().find(|x|x.id==id).ok_or(Error::Target)?.world_affine)?)?,Orientation64::Parent(id)=>rotation_frame(affine(targets.iter().find(|x|x.id==id).ok_or(Error::Target)?.parent_affine)?)?,Orientation64::CustomOrthonormal(b)=>DMat3::from_cols_array(&b)};
 if !pivot.is_finite(){return Err(Error::Basis);}validate_basis(basis)?;Ok((pivot,basis))
}
