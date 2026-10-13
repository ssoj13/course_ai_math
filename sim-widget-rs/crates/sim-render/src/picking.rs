//! Shared stamped geometric picking. BLAS assets are reused by transformed instances.
use crate::{Result,RenderError,error::invalid,bounds::Bounds,camera::{CameraMatrices,Ray},frame::FrameStamp};
use glam::{DMat4,DVec2,DVec3};
use sim_model::{ObjectId,ViewId};
use std::{sync::Arc};
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub struct SemanticId(String);
impl SemanticId{pub fn new(s:impl Into<String>)->Result<Self>{let s=s.into();if s.is_empty()||s.len()>256{return Err(invalid("pick.semantic","invalid stable ID"));}Ok(Self(s))}pub fn as_str(&self)->&str{&self.0}}
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub enum Component{Point(u32),Segment(u32),Triangle(u32),ArrowHead(u32),Handle(SemanticId)}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct DisplayStamp{pub frame:FrameStamp,pub scene_revision:u64,pub topology_revision:u64,pub view:ViewId,pub layout_generation:u64,pub device_generation:u64,pub sample:u64,pub view_signature:[u64;18]}
impl DisplayStamp{pub fn camera_signature(camera:&CameraMatrices)->[u64;18]{let mut result=[0;18];for (to,from)in result[..16].iter_mut().zip(camera.view_projection().to_cols_array()){*to=from.to_bits();}result[16]=camera.css()[0].to_bits();result[17]=camera.css()[1].to_bits();result}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Layer{Scene,GizmoOverlay}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Backfaces{Both,CullBack,CullFront}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Winding{Authored,World}
#[derive(Clone,Copy,Debug,PartialEq)]pub enum Opacity{Opaque,Mask{alpha:f64,cutoff:f64},Blend{alpha:f64}}
#[derive(Clone,Copy,Debug,PartialEq)]pub enum Transparency{Nearest{minimum_alpha:f64},Through}
#[derive(Clone,Copy,Debug,PartialEq)]
pub enum Shape{Point{position:DVec3,radius_css:f64},Segment{a:DVec3,b:DVec3,radius_css:f64},Triangle{a:DVec3,b:DVec3,c:DVec3},ArrowHead{a:DVec3,b:DVec3,length_css:f64}}
#[derive(Clone,Debug)]
pub struct Primitive{pub component:Component,pub semantic:SemanticId,pub shape:Shape,pub priority:u8}
#[derive(Clone,Debug)]
struct Node{bounds:Bounds,left:Option<usize>,right:Option<usize>,start:usize,count:usize}
#[derive(Clone,Debug)]
struct Tree{nodes:Vec<Node>,order:Vec<usize>}
fn build_tree(bounds:&[Bounds])->Tree{
 fn split(tree:&mut Tree,bounds:&[Bounds],start:usize,count:usize)->usize{
  let mut total=bounds[tree.order[start]];for i in start+1..start+count{total=total.union(bounds[tree.order[i]]);}
  let at=tree.nodes.len();tree.nodes.push(Node{bounds:total,left:None,right:None,start,count});
  if count>8{let size=total.max-total.min;let axis=if size.x>=size.y&&size.x>=size.z{0}else if size.y>=size.z{1}else{2};
   tree.order[start..start+count].sort_unstable_by(|a,b|bounds[*a].center()[axis].total_cmp(&bounds[*b].center()[axis]).then(a.cmp(b)));
   let n=count/2;let left=split(tree,bounds,start,n);let right=split(tree,bounds,start+n,count-n);
   tree.nodes[at].left=Some(left);tree.nodes[at].right=Some(right);tree.nodes[at].count=0;
  }at
 }
 let mut tree=Tree{nodes:Vec::with_capacity(bounds.len()*2),order:(0..bounds.len()).collect()};if !bounds.is_empty(){split(&mut tree,bounds,0,bounds.len());}tree
}

/// Admission ceilings include owned heap capacity and conservative allocator/node overhead.
/// The caller still owns its incoming vectors; rejection never allocates a BVH.
#[derive(Clone,Copy,Debug)]pub struct PickBudget{pub resident_bytes:usize,pub temporary_bytes:usize,pub query_bytes:usize}
impl Default for PickBudget{fn default()->Self{Self{resident_bytes:64*1024*1024,temporary_bytes:32*1024*1024,query_bytes:16*1024*1024}}}
fn bytes(parts:&[(usize,usize)])->Result<usize>{parts.iter().try_fold(0usize,|sum,(count,width)|count.checked_mul(*width).and_then(|n|sum.checked_add(n)).ok_or_else(||RenderError::Budget("pick byte overflow".into())))}
fn admit(actual:usize,limit:usize)->Result<()>{if actual>limit{Err(RenderError::Budget(format!("pick allocation {actual} exceeds {limit}")))}else{Ok(())}}
fn tree_bytes(count:usize)->Result<usize>{bytes(&[(count,2*std::mem::size_of::<Node>()+std::mem::size_of::<usize>())])}
#[repr(C)]struct ArcAllocation<T>{strong:usize,weak:usize,value:T}
fn primitive_heap(p:&Primitive)->usize{p.semantic.0.capacity()+match &p.component{Component::Handle(id)=>id.0.capacity(),_=>0}}

#[derive(Clone,Debug)]pub struct GeometryAsset{primitives:Vec<Primitive>,tree:Tree,bounds:Bounds,max_radius:f64,has_handles:bool,max_hit_heap:usize,pub resident_bytes:usize,pub temporary_bytes:usize,pub revision:u64}
impl GeometryAsset{
 pub fn new(primitives:Vec<Primitive>,revision:u64)->Result<Arc<Self>>{Self::with_budget(primitives,revision,PickBudget::default())}
 pub fn with_budget(primitives:Vec<Primitive>,revision:u64,budget:PickBudget)->Result<Arc<Self>>{
  if primitives.is_empty()||primitives.len()>200_000||primitives.capacity()>200_000{return Err(RenderError::Budget("pick geometry count".into()));}
  let strings=primitives.iter().try_fold(0usize,|n,p|n.checked_add(primitive_heap(p)).ok_or_else(||RenderError::Budget("pick string bytes".into())))?;
  let resident_bytes=bytes(&[(primitives.capacity(),std::mem::size_of::<Primitive>()),(1,strings),(1,tree_bytes(primitives.len())?),(1,std::mem::size_of::<ArcAllocation<GeometryAsset>>())])?;
  admit(resident_bytes,budget.resident_bytes)?;
  // Uniqueness borrows component IDs; no cloned Strings or BTree nodes.
  let temporary=bytes(&[(primitives.len(),std::mem::size_of::<Bounds>().max(std::mem::size_of::<&Component>()))])?;
  admit(temporary,budget.temporary_bytes)?;
  let mut keys:Vec<_>=primitives.iter().map(|p|&p.component).collect();
  keys.sort_unstable();if keys.windows(2).any(|pair|pair[0]==pair[1]){return Err(invalid("pick.component","duplicate component in asset"));}
  drop(keys);
  let mut bounds=Vec::with_capacity(primitives.len());let mut radius:f64=0.0;
  for p in &primitives{
   let(points,count,r)=match p.shape{
    Shape::Point{position,radius_css}=>([position;3],1,radius_css),
    Shape::Segment{a,b,radius_css}=>([a,b,b],2,radius_css),
    Shape::Triangle{a,b,c}=>([a,b,c],3,0.0),
    Shape::ArrowHead{a,b,length_css}=>([a,b,b],2,length_css)};
   if !r.is_finite()||r<0.0||r>128.0||r==0.0&&!matches!(p.shape,Shape::Triangle{..}){return Err(invalid("pick.radius","CSS radius outside0..128"));}
   radius=radius.max(r);bounds.push(Bounds::from_points(points[..count].iter().copied())?);
  }
  let tree=build_tree(&bounds);let total=tree.nodes[0].bounds;drop(bounds);
  let has_handles=primitives.iter().any(|p|matches!(p.component,Component::Handle(_)));
  let max_hit_heap=primitives.iter().map(|p|p.semantic.0.len()+match &p.component{Component::Handle(id)=>id.0.len(),_=>0}).max().unwrap_or(0);
  Ok(Arc::new(Self{primitives,tree,bounds:total,max_radius:radius,has_handles,max_hit_heap,resident_bytes,temporary_bytes:temporary,revision}))
 }
 pub fn primitive_count(&self)->usize{self.primitives.len()}
}
#[derive(Clone,Debug)]
pub struct PickObject{pub object:ObjectId,pub instance:u32,pub geometry:Arc<GeometryAsset>,pub world:DMat4,pub visible:bool,pub pickable:bool,pub locked:bool,pub opacity:Opacity,pub layer:Layer}
#[derive(Clone,Debug)]struct Instance{object:PickObject,inverse:DMat4,bounds:Bounds,inverse_radius:f64}
#[derive(Clone,Debug)]pub struct PickIndex{objects:Vec<Instance>,tree:Tree,pub stamp:DisplayStamp,pub refits:u64,pub builds:u64,pub resident_bytes:usize,index_bytes:usize,asset_bytes:usize,max_hit_heap:usize,pub temporary_bytes:usize,budget:PickBudget}
#[derive(Clone,Copy,Debug)]
pub struct PickFilter{pub handles:bool,pub primitives:bool,pub bodies:bool,pub include_locked:bool,pub xray:bool,pub gizmo_overlay:bool,pub backfaces:Backfaces,pub winding:Winding,pub transparency:Transparency}
impl Default for PickFilter{fn default()->Self{Self{handles:true,primitives:true,bodies:true,include_locked:true,xray:false,gizmo_overlay:true,backfaces:Backfaces::Both,winding:Winding::Authored,transparency:Transparency::Nearest{minimum_alpha:0.01}}}}
#[derive(Clone,Debug)]pub struct PickQuery<'a>{pub stamp:&'a DisplayStamp,pub camera:&'a CameraMatrices,pub pointer:DVec2,pub clip:[f64;4],pub touch:bool,pub filter:PickFilter,pub offset:usize,pub limit:usize}
#[derive(Clone,Debug,PartialEq)]
pub struct PickHit{pub object:ObjectId,pub instance:u32,pub component:Component,pub semantic:SemanticId,pub world:DVec3,pub local:DVec3,pub barycentric:Option<[f64;3]>,pub distance_css:f64,pub ray_distance:f64,pub depth:f64,pub priority:u8,pub layer:Layer,pub locked:bool,pub stamp:DisplayStamp}
#[derive(Clone,Debug)]pub struct CandidatePage{pub hits:Vec<PickHit>,pub total:usize,pub has_more:bool,pub tested_primitives:usize}
fn valid_opacity(opacity:Opacity)->bool{match opacity{Opacity::Opaque=>true,Opacity::Mask{alpha,cutoff}=>alpha.is_finite()&&cutoff.is_finite()&&(0.0..=1.0).contains(&alpha)&&(0.0..=1.0).contains(&cutoff),Opacity::Blend{alpha}=>alpha.is_finite()&&(0.0..=1.0).contains(&alpha)}}
impl PickIndex{
 pub fn new(objects:Vec<PickObject>,stamp:DisplayStamp)->Result<Self>{Self::with_budget(objects,stamp,PickBudget::default())}
 pub fn with_budget(objects:Vec<PickObject>,stamp:DisplayStamp,budget:PickBudget)->Result<Self>{
  if objects.len()>4096||objects.capacity()>4096{return Err(RenderError::Budget("pick instance count".into()));}
  let identity_bytes=objects.iter().try_fold(0usize,|sum,o|sum.checked_add(o.object.as_str().len()).ok_or_else(||RenderError::Budget("pick identities".into())))?;
  let stamp_bytes=stamp.frame.generation.as_str().len()+stamp.frame.document.as_str().len()+stamp.frame.session.as_str().len()+stamp.view.as_str().len();
  let index_bytes=bytes(&[(objects.len(),std::mem::size_of::<Instance>()),(1,identity_bytes),(1,tree_bytes(objects.len())?),(1,stamp_bytes),(1,std::mem::size_of::<PickIndex>())])?;
  // Input vector remains owned during construction; bounds and borrowed identity/asset vectors are admitted before allocating.
  let temporary=bytes(&[(objects.capacity(),std::mem::size_of::<PickObject>()),(objects.len(),std::mem::size_of::<Bounds>()+2*std::mem::size_of::<usize>())])?;
  admit(temporary,budget.temporary_bytes)?;admit(index_bytes,budget.resident_bytes)?;
  let mut resident_bytes=index_bytes;
  let mut assets:Vec<_>=objects.iter().map(|o|&o.geometry).collect();
  assets.sort_unstable_by_key(|asset|Arc::as_ptr(asset)as usize);assets.dedup_by_key(|asset|Arc::as_ptr(asset)as usize);
  for asset in &assets{resident_bytes=resident_bytes.checked_add(asset.resident_bytes).ok_or_else(||RenderError::Budget("pick asset aggregate".into()))?;}
  admit(resident_bytes,budget.resident_bytes)?;let asset_bytes=resident_bytes-index_bytes;drop(assets);
  let mut keys:Vec<_>=objects.iter().map(|o|(&o.object,o.instance)).collect();keys.sort_unstable();
  if keys.windows(2).any(|pair|pair[0]==pair[1]){return Err(invalid("pick.instance","duplicate qualified instance"));}
  drop(keys);
  let max_hit_heap=objects.iter().map(|object|object.object.as_str().len()+object.geometry.max_hit_heap).max().unwrap_or(0);
  let mut instances=Vec::with_capacity(objects.len());let mut bounds=Vec::with_capacity(objects.len());
  for object in objects{
   if !object.world.is_finite()||object.world.w_axis.w!=1.0||object.world.x_axis.w!=0.0||object.world.y_axis.w!=0.0||object.world.z_axis.w!=0.0||object.world.determinant()==0.0||!valid_opacity(object.opacity){return Err(invalid("pick.object","invalid transform/opacity"));}
   let inverse=object.world.inverse();if !inverse.is_finite(){return Err(invalid("pick.inverse","singular inverse"));}
   let box_world=object.geometry.bounds.transformed(object.world)?;
   let inverse_radius=(inverse.x_axis.truncate().abs()+inverse.y_axis.truncate().abs()+inverse.z_axis.truncate().abs()).max_element();
   bounds.push(box_world);instances.push(Instance{object,inverse,bounds:box_world,inverse_radius});
  }
  Ok(Self{objects:instances,tree:build_tree(&bounds),stamp,refits:0,builds:1,resident_bytes,index_bytes,asset_bytes,max_hit_heap,temporary_bytes:temporary,budget})
 }
 fn index_admission_for_stamp(&self,stamp:&DisplayStamp)->Result<usize>{
  let identity_bytes=self.objects.iter().try_fold(0usize,|sum,object|sum.checked_add(object.object.object.as_str().len()).ok_or_else(||RenderError::Budget("pick identity overflow".into())))?;
  bytes(&[(self.objects.capacity(),std::mem::size_of::<Instance>()),(self.tree.nodes.capacity(),std::mem::size_of::<Node>()),(self.tree.order.capacity(),std::mem::size_of::<usize>()),(1,identity_bytes),(1,stamp.frame.generation.as_str().len()+stamp.frame.document.as_str().len()+stamp.frame.session.as_str().len()+stamp.view.as_str().len()),(1,std::mem::size_of::<PickIndex>())])
 }
 /// Refit preserves shared BLAS. Replacements are atomic and require exact qualified identities.
 pub fn refit(&mut self,updates:&[(ObjectId,u32,DMat4)],stamp:DisplayStamp)->Result<()>{
  let new_index_bytes=self.index_admission_for_stamp(&stamp)?;
  let new_resident=self.asset_bytes.checked_add(new_index_bytes).ok_or_else(||RenderError::Budget("pick refit resident overflow".into()))?;
  admit(new_resident,self.budget.resident_bytes)?;
  admit(self.refit_admission_bytes(updates.len(),&stamp)?,self.budget.temporary_bytes)?;
  if updates.len()>self.objects.len(){return Err(RenderError::Budget("pick refit updates".into()));}
  let mut keys:Vec<_>=updates.iter().map(|(id,instance,_)|(id,*instance)).collect();keys.sort_unstable();if keys.windows(2).any(|pair|pair[0]==pair[1]){return Err(invalid("pick.refit","duplicate update"));}drop(keys);
  let mut candidate=self.clone();
  for(id,instance,world)in updates{
   let object=candidate.objects.iter_mut().find(|o|&o.object.object==id&&o.object.instance==*instance).ok_or_else(||invalid("pick.refit","missing instance"))?;
   if !world.is_finite()||world.w_axis.w!=1.0||world.x_axis.w!=0.0||world.y_axis.w!=0.0||world.z_axis.w!=0.0||world.determinant()==0.0{return Err(invalid("pick.refit","invalid transform"));}
   let inverse=world.inverse();if !inverse.is_finite(){return Err(invalid("pick.refit","invalid inverse"));}
   object.object.world=*world;object.inverse=inverse;object.bounds=object.object.geometry.bounds.transformed(*world)?;
   object.inverse_radius=(inverse.x_axis.truncate().abs()+inverse.y_axis.truncate().abs()+inverse.z_axis.truncate().abs()).max_element();
  }
  fn visit(tree:&mut Tree,objects:&[Instance],at:usize)->Bounds{
   let node=tree.nodes[at].clone();let bounds=if let(Some(l),Some(r))=(node.left,node.right){visit(tree,objects,l).union(visit(tree,objects,r))}else{
    let mut b=objects[tree.order[node.start]].bounds;for i in node.start+1..node.start+node.count{b=b.union(objects[tree.order[i]].bounds);}b};
   tree.nodes[at].bounds=bounds;bounds
  }
  if !candidate.tree.nodes.is_empty(){visit(&mut candidate.tree,&candidate.objects,0);}
  candidate.stamp=stamp;
  candidate.index_bytes=candidate.index_admission_for_stamp(&candidate.stamp)?;
  candidate.resident_bytes=candidate.asset_bytes.checked_add(candidate.index_bytes).ok_or_else(||RenderError::Budget("pick refit retained overflow".into()))?;
  candidate.refits=candidate.refits.checked_add(1).ok_or_else(||RenderError::Budget("pick refit counter overflow".into()))?;*self=candidate;Ok(())
 }
 pub fn refit_admission_bytes(&self,updates:usize,stamp:&DisplayStamp)->Result<usize>{bytes(&[(1,self.index_admission_for_stamp(&self.stamp)?),(updates,std::mem::size_of::<(&ObjectId,u32)>()),(1,stamp.frame.generation.as_str().len()+stamp.frame.document.as_str().len()+stamp.frame.session.as_str().len()+stamp.view.as_str().len())])}
 pub fn query_admission_bytes(&self,query:&PickQuery<'_>)->Result<usize>{
  let retained=query.offset.checked_add(query.limit).and_then(|n|n.checked_add(1)).ok_or_else(||RenderError::Budget("pick query overflow".into()))?;
  let stamp_heap=query.stamp.frame.generation.as_str().len()+query.stamp.frame.document.as_str().len()+query.stamp.frame.session.as_str().len()+query.stamp.view.as_str().len();
  bytes(&[(retained,std::mem::size_of::<PickHit>()+self.max_hit_heap+stamp_heap),(256,std::mem::size_of::<usize>())])
 }
 pub fn query(&self,query:&PickQuery<'_>)->Result<CandidatePage>{
  if query.stamp!=&self.stamp||query.stamp.view_signature!=DisplayStamp::camera_signature(query.camera){return Err(RenderError::Stale("pick display stamp".into()));}
  if !query.pointer.is_finite()||query.clip.iter().any(|v|!v.is_finite())||query.clip[2]<=0.0||query.clip[3]<=0.0||query.clip[0]<0.0||query.clip[1]<0.0||query.clip[0]+query.clip[2]>query.camera.css()[0]||query.clip[1]+query.clip[3]>query.camera.css()[1]||query.limit==0||query.limit>256||query.offset>4096||matches!(query.filter.transparency,Transparency::Nearest{minimum_alpha}if !minimum_alpha.is_finite()||!(0.0..=1.0).contains(&minimum_alpha)){return Err(invalid("pick.query","invalid coordinates/page"));}
  let retained=query.offset+query.limit+1;
  admit(self.query_admission_bytes(query)?,self.budget.query_bytes)?;
  let mut page=CandidatePage{hits:Vec::new(),total:0,has_more:false,tested_primitives:0};
  if !inside(query.pointer,query.clip){return Ok(page);}
  let ray=query.camera.ray(query.pointer)?;let mut stack=Vec::with_capacity(64);if !self.tree.nodes.is_empty(){stack.push(0);}
  let mut candidates=Vec::with_capacity(retained);
  while let Some(at)=stack.pop(){let node=&self.tree.nodes[at];
   let radius=world_radius(query.camera,node.bounds,128.0_f64.max(if query.touch{22.0}else{14.0}));
   if !intersects(node.bounds,ray,radius){continue;}
   if let(Some(l),Some(r))=(node.left,node.right){stack.push(r);stack.push(l);continue;}
   for i in node.start..node.start+node.count{let object_index=self.tree.order[i];let object=&self.objects[object_index];
    if !object.object.visible||!object.object.pickable||object.object.locked&&!query.filter.include_locked||object.object.layer==Layer::GizmoOverlay&&!query.filter.gizmo_overlay{continue;}
    let local_ray=Ray{origin:object.inverse.transform_point3(ray.origin),direction:object.inverse.transform_vector3(ray.direction)};
    let radius=world_radius(query.camera,object.bounds,object.object.geometry.max_radius.max(if object.object.geometry.has_handles{if query.touch{22.0}else{14.0}}else{0.0}))*object.inverse_radius;
    let mut local_stack=Vec::with_capacity(64);local_stack.push(0);
    while let Some(n)=local_stack.pop(){let node=&object.object.geometry.tree.nodes[n];if !intersects(node.bounds,local_ray,radius){continue;}
     if let(Some(l),Some(r))=(node.left,node.right){local_stack.push(r);local_stack.push(l);continue;}
     for j in node.start..node.start+node.count{let p=&object.object.geometry.primitives[object.object.geometry.tree.order[j]];
      let enabled=match &p.component{Component::Handle(_)=>query.filter.handles,Component::Triangle(_)=>query.filter.bodies,_=>query.filter.primitives};
      if !enabled{continue;}page.tested_primitives+=1;
      if let Some(hit)=narrow(object,p,query,ray,local_ray)?{
       if !opacity_visible(object.object.opacity,query.filter.transparency){continue;}
       if !query.filter.xray&&object.object.layer!=Layer::GizmoOverlay{
        let screen=query.camera.project(hit.world).ok_or_else(||invalid("pick.hit","clipped candidate"))?.truncate();
        let occlusion_ray=query.camera.ray(screen)?;
        if self.occluded(occlusion_ray,hit.world,query.camera,&query.filter)?{continue;}
       }
       page.total+=1;candidates.push(hit);
       candidates.sort_unstable_by(compare_hits);if candidates.len()>query.offset+query.limit{candidates.pop();}
      }
     }
    }
   }
  }
  page.has_more=page.total>query.offset+query.limit;
  let skip=query.offset.min(candidates.len());candidates.drain(..skip);page.hits=candidates;Ok(page)
 }
 fn occluded(&self,ray:Ray,world:DVec3,camera:&CameraMatrices,filter:&PickFilter)->Result<bool>{
  let target=camera.project(world).ok_or_else(||invalid("pick.depth","clipped target"))?.z;
  let mut stack=Vec::with_capacity(64);stack.push(0);if self.tree.nodes.is_empty(){return Ok(false);}
  while let Some(at)=stack.pop(){let node=&self.tree.nodes[at];if !intersects(node.bounds,ray,0.0){continue;}
   if let(Some(l),Some(r))=(node.left,node.right){stack.push(l);stack.push(r);continue;}
   for i in node.start..node.start+node.count{let object=&self.objects[self.tree.order[i]];
    if !object.object.visible||object.object.layer!=Layer::Scene||!matches!(object.object.opacity,Opacity::Opaque|Opacity::Mask{..})||!opacity_visible(object.object.opacity,filter.transparency){continue;}
    let local=Ray{origin:object.inverse.transform_point3(ray.origin),direction:object.inverse.transform_vector3(ray.direction)};let mut nodes=Vec::with_capacity(64);nodes.push(0);
    while let Some(n)=nodes.pop(){let node=&object.object.geometry.tree.nodes[n];if !intersects(node.bounds,local,0.0){continue;}
     if let(Some(l),Some(r))=(node.left,node.right){nodes.push(l);nodes.push(r);continue;}
     for j in node.start..node.start+node.count{
      let p=&object.object.geometry.primitives[object.object.geometry.tree.order[j]];
      if let Shape::Triangle{a,b,c}=p.shape{if let Some((t,_))=triangle(local,a,b,c,filter.backfaces,filter.winding,object.object.world.determinant()<0.0){
       if let Some(depth)=camera.project(ray.origin+ray.direction*t){let epsilon=1e-7+32.0*f32::EPSILON as f64*target.abs().max(depth.z.abs());if depth.z>target+epsilon{return Ok(true);}}
      }}
     }
    }
   }
  }Ok(false)
 }
}
fn compare_hits(a:&PickHit,b:&PickHit)->std::cmp::Ordering{b.priority.cmp(&a.priority).then_with(||a.distance_css.total_cmp(&b.distance_css)).then_with(||a.ray_distance.total_cmp(&b.ray_distance)).then(a.object.cmp(&b.object)).then(a.instance.cmp(&b.instance)).then(a.component.cmp(&b.component)).then(a.semantic.cmp(&b.semantic))}
fn opacity_visible(value:Opacity,policy:Transparency)->bool{match value{Opacity::Opaque=>true,Opacity::Mask{alpha,cutoff}=>alpha>=cutoff,Opacity::Blend{alpha}=>match policy{Transparency::Through=>false,Transparency::Nearest{minimum_alpha}=>alpha>=minimum_alpha}}}
fn inside(p:DVec2,r:[f64;4])->bool{p.x>=r[0]&&p.y>=r[1]&&p.x<=r[0]+r[2]&&p.y<=r[1]+r[3]}
fn world_radius(camera:&CameraMatrices,bounds:Bounds,radius:f64)->f64{
 let scale=if camera.projection().w_axis.w.abs()<1e-10{bounds.corners().iter().map(|p|-camera.view().transform_point3(*p).z).fold(0.0,f64::max)}else{1.0};
 2.0*radius*scale/(camera.css()[1]*camera.projection().y_axis.y.abs())
}
fn intersects(bounds:Bounds,ray:Ray,expand:f64)->bool{
 let lo=bounds.min-DVec3::splat(expand);let hi=bounds.max+DVec3::splat(expand);let mut enter:f64=0.0;let mut exit=f64::INFINITY;
 for axis in 0..3{let d=ray.direction[axis];if d.abs()<1e-20{if ray.origin[axis]<lo[axis]||ray.origin[axis]>hi[axis]{return false;}}else{let a=(lo[axis]-ray.origin[axis])/d;let b=(hi[axis]-ray.origin[axis])/d;enter=enter.max(a.min(b));exit=exit.min(a.max(b));if enter>exit{return false;}}}true
}
/// Homogeneous Liang-Barsky clipping preserves actual world endpoints and interpolation.
pub fn clip_segment(camera:&CameraMatrices,a:DVec3,b:DVec3,radius:f64,clip:[f64;4])->Option<(DVec3,DVec3)>{
 let ca=camera.view_projection()*a.extend(1.0);let cb=camera.view_projection()*b.extend(1.0);
 let left=2.0*(clip[0]-radius)/camera.css()[0]-1.0;let right=2.0*(clip[0]+clip[2]+radius)/camera.css()[0]-1.0;
 let bottom=1.0-2.0*(clip[1]+clip[3]+radius)/camera.css()[1];let top=1.0-2.0*(clip[1]-radius)/camera.css()[1];
 let planes=|c:glam::DVec4|[c.x-left*c.w,right*c.w-c.x,c.y-bottom*c.w,top*c.w-c.y,c.z,c.w-c.z,c.w-1e-12];
 let x=planes(ca);let y=planes(cb);let(mut enter,mut exit)=(0.0_f64,1.0_f64);
 for k in 0..7{if x[k]<0.0&&y[k]<0.0{return None;}if x[k]<0.0{enter=enter.max(x[k]/(x[k]-y[k]));}else if y[k]<0.0{exit=exit.min(x[k]/(x[k]-y[k]));}}
 (enter<=exit).then_some((a.lerp(b,enter),a.lerp(b,exit)))
}
fn triangle(ray:Ray,a:DVec3,b:DVec3,c:DVec3,backfaces:Backfaces,winding:Winding,negative:bool)->Option<(f64,[f64;3])>{
 let e1=b-a;let e2=c-a;let p=ray.direction.cross(e2);let det=e1.dot(p);
 let facing=det*if winding==Winding::World&&negative{-1.0}else{1.0};
 if det.abs()<1e-14||backfaces==Backfaces::CullBack&&facing<=0.0||backfaces==Backfaces::CullFront&&facing>=0.0{return None;}
 let inv=1.0/det;let offset=ray.origin-a;let u=offset.dot(p)*inv;let q=offset.cross(e1);let v=ray.direction.dot(q)*inv;let t=e2.dot(q)*inv;
 (u>=-1e-12&&v>=-1e-12&&u+v<=1.0+1e-12&&t>=0.0&&t.is_finite()).then_some((t,[1.0-u-v,u,v]))
}
fn narrow(object:&Instance,p:&Primitive,q:&PickQuery<'_>,ray:Ray,local_ray:Ray)->Result<Option<PickHit>>{
 let world=|p:DVec3|object.object.world.transform_point3(p);
 let tolerance=if q.touch{22.0}else{14.0};
 let candidate=match p.shape{
  Shape::Triangle{a,b,c}=>triangle(local_ray,a,b,c,q.filter.backfaces,q.filter.winding,object.object.world.determinant()<0.0).map(|(t,bary)|(ray.origin+ray.direction*t,Some(bary),0.0)),
  Shape::Point{position,radius_css}=>q.camera.project(world(position)).and_then(|projected|{
   let distance=projected.truncate().distance(q.pointer);let radius=if matches!(p.component,Component::Handle(_)){radius_css.max(tolerance)}else{radius_css};
   (distance<=radius).then_some((world(position),None,distance))
  }),
  Shape::Segment{a,b,radius_css}=>{
   let radius=if matches!(p.component,Component::Handle(_)){radius_css.max(tolerance)}else{radius_css};
   clip_segment(q.camera,world(a),world(b),radius,q.clip).and_then(|(a,b)|{
    let pa=q.camera.project(a)?;let pb=q.camera.project(b)?;let delta=pb.truncate()-pa.truncate();
    let t=if delta.length_squared()>1e-20{((q.pointer-pa.truncate()).dot(delta)/delta.length_squared()).clamp(0.0,1.0)}else{0.0};
    let distance=(pa.truncate()+delta*t).distance(q.pointer);
    let wa=(q.camera.view_projection()*a.extend(1.0)).w;let wb=(q.camera.view_projection()*b.extend(1.0)).w;
    let k=(t/wb)/((1.0-t)/wa+t/wb);
    (distance<=radius).then_some((a.lerp(b,k),None,distance))
   })
  },
  Shape::ArrowHead{a,b,length_css}=>{
   clip_segment(q.camera,world(a),world(b),length_css,q.clip).and_then(|(a,b)|{
    let pa=q.camera.project(a)?;let pb=q.camera.project(b)?;let delta=pb.truncate()-pa.truncate();if delta.length_squared()<1e-20{return None;}
    let direction=delta.normalize();let perp=DVec2::new(-direction.y,direction.x);let tip=pb.truncate();let left=tip-direction*length_css+perp*length_css*0.5;let right=tip-direction*length_css-perp*length_css*0.5;
    let cross=|x:DVec2,y:DVec2|x.x*y.y-x.y*y.x;let area=cross(left-tip,right-tip);let u=cross(q.pointer-tip,right-tip)/area;let v=cross(left-tip,q.pointer-tip)/area;
    if u<0.0||v<0.0||u+v>1.0{return None;}
    q.camera.unproject(q.pointer,pb.z).ok().map(|world|(world,Some([1.0-u-v,u,v]),0.0))
   })
  }
 };
 let Some((position,barycentric,distance_css))=candidate else{return Ok(None);};
 let Some(projected)=q.camera.project(position)else{return Ok(None);};
 let ray_distance=(position-ray.origin).dot(ray.direction);if ray_distance<0.0{return Ok(None);}
 Ok(Some(PickHit{object:object.object.object.clone(),instance:object.object.instance,component:p.component.clone(),semantic:p.semantic.clone(),world:position,local:object.inverse.transform_point3(position),barycentric,distance_css,ray_distance,depth:projected.z,priority:p.priority,layer:object.object.layer,locked:object.object.locked,stamp:q.stamp.clone()}))
}
