use glam::{DMat4,DVec2,DVec3};
use sim_model::{GenerationId,DocumentId,SessionId,ObjectId,ViewId};
use sim_render::{camera::{Camera,Projection},frame::FrameStamp,picking::*};
use std::sync::Arc;
fn id(s:&str)->SemanticId{SemanticId::new(s).unwrap()}
fn stamp(camera:&sim_render::camera::CameraMatrices)->DisplayStamp{DisplayStamp{frame:FrameStamp{generation:GenerationId::new("generation").unwrap(),document:DocumentId::new("document").unwrap(),session:SessionId::new("session").unwrap(),epoch:0,document_revision:0,revision:0},scene_revision:0,topology_revision:0,view:ViewId::new("main").unwrap(),layout_generation:0,device_generation:0,sample:0,view_signature:DisplayStamp::camera_signature(camera)}}
fn asset(shape:Shape,component:Component,priority:u8)->Arc<GeometryAsset>{GeometryAsset::new(vec![Primitive{shape,component,semantic:id("value"),priority}],0).unwrap()}
fn object(name:&str,geometry:Arc<GeometryAsset>,world:DMat4)->PickObject{PickObject{object:ObjectId::new(name).unwrap(),instance:0,geometry,world,visible:true,pickable:true,locked:false,opacity:Opacity::Opaque,layer:Layer::Scene}}
fn query<'a>(s:&'a DisplayStamp,c:&'a sim_render::camera::CameraMatrices,p:DVec2)->PickQuery<'a>{PickQuery{stamp:s,camera:c,pointer:p,clip:[0.0,0.0,c.css()[0],c.css()[1]],touch:false,filter:PickFilter::default(),offset:0,limit:64}}
#[test]fn r1_segment_crosses_near_plane(){
 let camera=Camera{eye:[0.0;3],target:[0.0,0.0,-1.0],near:1.0,far:100.0,projection:Projection::Perspective{vertical_fov:1.0},..Camera::default()}.matrices([800.0,480.0]).unwrap();
 let geometry=asset(Shape::Segment{a:DVec3::new(-0.2,0.0,-0.5),b:DVec3::new(1.0,0.0,-5.0),radius_css:6.0},Component::Segment(0),2);
 let s=stamp(&camera);let index=PickIndex::new(vec![object("edge",geometry,DMat4::IDENTITY)],s.clone()).unwrap();
 let pointer=camera.project(DVec3::new(0.2,0.0,-2.0)).unwrap().truncate();
 let hits=index.query(&query(&s,&camera,pointer)).unwrap();assert_eq!(hits.hits.len(),1);assert!(hits.hits[0].world.z<=-1.0);
}
#[test]fn r2_occlusion_before_priority_and_explicit_overlay(){
 let camera=Camera::default().matrices([800.0,480.0]).unwrap();let s=stamp(&camera);
 let triangle=asset(Shape::Triangle{a:DVec3::new(-2.0,-2.0,1.0),b:DVec3::new(2.0,-2.0,1.0),c:DVec3::new(0.0,2.0,1.0)},Component::Triangle(0),1);
 let point=asset(Shape::Point{position:DVec3::ZERO,radius_css:4.0},Component::Handle(id("A")),3);
 let mut hidden=object("hidden",point,DMat4::IDENTITY);let front=object("front",triangle,DMat4::IDENTITY);
 let index=PickIndex::new(vec![front.clone(),hidden.clone()],s.clone()).unwrap();let mut q=query(&s,&camera,DVec2::new(400.0,240.0));
 assert_eq!(index.query(&q).unwrap().hits[0].object,ObjectId::new("front").unwrap());q.filter.xray=true;
 assert_eq!(index.query(&q).unwrap().hits[0].object,ObjectId::new("hidden").unwrap());
 hidden.layer=Layer::GizmoOverlay;let index=PickIndex::new(vec![front,hidden],s.clone()).unwrap();q.filter.xray=false;
 assert_eq!(index.query(&q).unwrap().hits[0].object,ObjectId::new("hidden").unwrap());q.filter.gizmo_overlay=false;
 assert_eq!(index.query(&q).unwrap().hits[0].object,ObjectId::new("front").unwrap());
}
#[test]fn r3_coincident_stack_and_lock_filter(){
 let camera=Camera::default().matrices([800.0,480.0]).unwrap();let s=stamp(&camera);
 let point=|n:&str|asset(Shape::Point{position:DVec3::ZERO,radius_css:4.0},Component::Handle(id(n)),3);
 let a=object("A",point("A"),DMat4::IDENTITY);let mut b=object("B",point("B"),DMat4::IDENTITY);b.locked=true;
 let index=PickIndex::new(vec![b,a],s.clone()).unwrap();let mut q=query(&s,&camera,DVec2::new(400.0,240.0));
 let hits=index.query(&q).unwrap();assert_eq!(hits.total,2);assert_eq!(hits.hits[0].object,ObjectId::new("A").unwrap());assert_eq!(hits.hits[1].object,ObjectId::new("B").unwrap());
 q.offset=1;q.limit=1;assert_eq!(index.query(&q).unwrap().hits[0].object,ObjectId::new("B").unwrap());
 q.offset=0;q.filter.include_locked=false;assert_eq!(index.query(&q).unwrap().total,1);
}
#[test]fn transformed_instance_world_distance_barycentric_and_refit(){
 let camera=Camera::default().matrices([800.0,480.0]).unwrap();let mut s=stamp(&camera);
 let geometry=asset(Shape::Triangle{a:DVec3::new(-1.0,-1.0,0.0),b:DVec3::new(1.0,-1.0,0.0),c:DVec3::new(0.0,1.0,0.0)},Component::Triangle(0),1);
 let world=DMat4::from_translation(DVec3::new(1.0,0.0,2.0))*DMat4::from_scale(DVec3::new(-2.0,0.5,3.0));
 let mut index=PickIndex::new(vec![object("mesh",geometry,world)],s.clone()).unwrap();
 let p=camera.project(DVec3::new(1.0,0.0,2.0)).unwrap().truncate();let hits=index.query(&query(&s,&camera,p)).unwrap();
 assert!(hits.hits[0].world.distance(DVec3::new(1.0,0.0,2.0))<1e-8);assert!((hits.hits[0].barycentric.unwrap().iter().sum::<f64>()-1.0).abs()<1e-12);
 s.layout_generation+=1;assert!(index.query(&query(&s,&camera,p)).is_err());
 let moved=DMat4::from_translation(DVec3::new(-1.0,0.0,0.0));index.refit(&[(ObjectId::new("mesh").unwrap(),0,moved)],s.clone()).unwrap();
 let p=camera.project(DVec3::new(-1.0,0.0,0.0)).unwrap().truncate();assert_eq!(index.query(&query(&s,&camera,p)).unwrap().total,1);assert_eq!(index.refits,1);
}
#[test]fn css_slop_broadphase_both_projections_and_viewport_sizes(){
 for projection in [Projection::Orthographic{vertical_span:8.0},Projection::Perspective{vertical_fov:0.8}]{
  for css in [[800.0,480.0],[1600.0,960.0]]{
   let camera=Camera{projection,..Camera::default()}.matrices(css).unwrap();let s=stamp(&camera);
   let geometry=asset(Shape::Point{position:DVec3::ZERO,radius_css:4.0},Component::Handle(id("A")),3);
   let index=PickIndex::new(vec![object("A",geometry,DMat4::IDENTITY)],s.clone()).unwrap();let center=camera.project(DVec3::ZERO).unwrap().truncate();
   assert_eq!(index.query(&query(&s,&camera,center+DVec2::new(13.0,0.0))).unwrap().total,1);
   assert_eq!(index.query(&query(&s,&camera,center+DVec2::new(15.0,0.0))).unwrap().total,0);
  }
 }
}
#[test]fn spatial_index_large_mesh_and_alpha_mask(){
 let mut primitives=Vec::new();for y in 0..100{for x in 0..100{let a=DVec3::new(x as f64,y as f64,0.0);primitives.push(Primitive{component:Component::Triangle((y*100+x)as u32),semantic:id("mesh"),shape:Shape::Triangle{a,b:a+DVec3::X,c:a+DVec3::Y},priority:1});}}
 let geometry=GeometryAsset::new(primitives,0).unwrap();let mut mesh=object("mesh",geometry,DMat4::IDENTITY);mesh.opacity=Opacity::Mask{alpha:0.0,cutoff:0.5};
 let camera=Camera{target:[50.0,50.0,0.0],eye:[50.0,50.0,10.0],..Camera::default()}.matrices([800.0,480.0]).unwrap();
 let s=stamp(&camera);let index=PickIndex::new(vec![mesh],s.clone()).unwrap();let page=index.query(&query(&s,&camera,DVec2::new(400.0,240.0))).unwrap();
 assert_eq!(page.total,0);assert!(page.tested_primitives<100,"tested {}",page.tested_primitives);
}

#[test]fn admission_rejection_shared_assets_and_atomic_refit(){
 let c=Camera::default().matrices([800.0,480.0]).unwrap();let s=stamp(&c);
 let p=Primitive{component:Component::Point(0),semantic:id("point"),shape:Shape::Point{position:DVec3::ZERO,radius_css:4.0},priority:2};
 assert!(GeometryAsset::with_budget(vec![p.clone()],0,PickBudget{resident_bytes:1,..PickBudget::default()}).is_err());
 assert!(GeometryAsset::with_budget(vec![p.clone()],0,PickBudget{temporary_bytes:1,..PickBudget::default()}).is_err());
 let g=GeometryAsset::new(vec![p],0).unwrap();
 let single=PickIndex::new(vec![object("one",g.clone(),DMat4::IDENTITY)],s.clone()).unwrap();
 let mut pair=PickIndex::new(vec![object("one",g.clone(),DMat4::IDENTITY),object("two",g.clone(),DMat4::IDENTITY)],s.clone()).unwrap();
 assert!(pair.resident_bytes<single.resident_bytes*2);
 assert!(PickIndex::with_budget(vec![object("one",g,DMat4::IDENTITY)],s.clone(),PickBudget{resident_bytes:1,..PickBudget::default()}).is_err());
 let before=pair.query(&query(&s,&c,DVec2::new(400.0,240.0))).unwrap();
 assert!(pair.refit(&[(ObjectId::new("one").unwrap(),0,DMat4::ZERO)],s.clone()).is_err());
 assert_eq!(pair.query(&query(&s,&c,DVec2::new(400.0,240.0))).unwrap().hits,before.hits);assert_eq!(pair.refits,0);
}
#[test]fn touch_clip_backface_and_camera_invalidation(){
 let c=Camera::default().matrices([800.0,480.0]).unwrap();let s=stamp(&c);
 let geometry=asset(Shape::Point{position:DVec3::ZERO,radius_css:4.0},Component::Handle(id("A")),3);
 let index=PickIndex::new(vec![object("A",geometry,DMat4::IDENTITY)],s.clone()).unwrap();
 let mut q=query(&s,&c,DVec2::new(420.0,240.0));assert_eq!(index.query(&q).unwrap().total,0);q.touch=true;assert_eq!(index.query(&q).unwrap().total,1);
 q.clip=[0.0,0.0,400.0,480.0];assert_eq!(index.query(&q).unwrap().total,0);
 let changed=Camera{eye:[1.0,0.0,10.0],..Camera::default()}.matrices([800.0,480.0]).unwrap();
 assert!(index.query(&query(&s,&changed,DVec2::new(400.0,240.0))).is_err());
 let geometry=asset(Shape::Triangle{a:DVec3::new(-1.0,-1.0,0.0),b:DVec3::new(1.0,-1.0,0.0),c:DVec3::new(0.0,1.0,0.0)},Component::Triangle(0),1);
 let index=PickIndex::new(vec![object("mirror",geometry,DMat4::from_scale(DVec3::new(-2.0,1.0,1.0)))],s.clone()).unwrap();
 let mut q=query(&s,&c,DVec2::new(400.0,240.0));q.filter.backfaces=Backfaces::CullBack;
 assert_eq!(index.query(&q).unwrap().total,1);q.filter.winding=Winding::World;assert_eq!(index.query(&q).unwrap().total,0);
 q.filter.backfaces=Backfaces::CullFront;assert_eq!(index.query(&q).unwrap().total,1);
}

#[test]fn query_budget_and_transparent_filter(){
 let c=Camera::default().matrices([800.0,480.0]).unwrap();let s=stamp(&c);let geometry=asset(Shape::Point{position:DVec3::ZERO,radius_css:4.0},Component::Point(0),2);
 let mut object=object("transparent",geometry,DMat4::IDENTITY);object.opacity=Opacity::Blend{alpha:0.2};
 let index=PickIndex::new(vec![object.clone()],s.clone()).unwrap();let mut q=query(&s,&c,DVec2::new(400.0,240.0));
 assert_eq!(index.query(&q).unwrap().total,1);q.filter.transparency=Transparency::Nearest{minimum_alpha:0.5};assert_eq!(index.query(&q).unwrap().total,0);
 q.filter.transparency=Transparency::Through;assert_eq!(index.query(&q).unwrap().total,0);
 let bounded=PickIndex::with_budget(vec![object],s.clone(),PickBudget{query_bytes:1,..PickBudget::default()}).unwrap();
 assert!(bounded.query(&q).is_err());
}
#[test]fn constraint_parallel_and_zero_rejected(){
 use sim_render::{camera::Ray,interaction::DragConstraint};
 let ray=Ray{origin:DVec3::Z,direction:-DVec3::Z};
 assert!(DragConstraint::Plane{origin:DVec3::ZERO,normal:DVec3::X}.intersect(ray).is_err());
 assert!(DragConstraint::Axis{origin:DVec3::ZERO,direction:DVec3::Z}.intersect(ray).is_err());
 assert!(DragConstraint::Axis{origin:DVec3::ZERO,direction:DVec3::ZERO}.intersect(ray).is_err());
 assert_eq!(DragConstraint::Plane{origin:DVec3::ZERO,normal:DVec3::Z}.intersect(ray).unwrap(),DVec3::ZERO);
}

#[test]fn original_underreported_resident_ceiling_rejected(){
 let primitive=Primitive{component:Component::Point(0),semantic:id("value"),shape:Shape::Point{position:DVec3::ZERO,radius_css:4.0},priority:2};
 assert!(GeometryAsset::with_budget(vec![primitive],0,PickBudget{resident_bytes:341,temporary_bytes:181,..PickBudget::default()}).is_err());
}

#[test]fn nonselectable_front_body_still_occludes_and_hidden_does_not(){
 for projection in [Projection::Orthographic{vertical_span:8.0},Projection::Perspective{vertical_fov:0.8}]{
  let c=Camera{projection,..Camera::default()}.matrices([800.0,480.0]).unwrap();let s=stamp(&c);
  let front_geometry=asset(Shape::Triangle{a:DVec3::new(-2.0,-2.0,1.0),b:DVec3::new(2.0,-2.0,1.0),c:DVec3::new(0.0,2.0,1.0)},Component::Triangle(0),1);
  let rear=object("rear",asset(Shape::Point{position:DVec3::ZERO,radius_css:4.0},Component::Handle(id("rear")),3),DMat4::IDENTITY);
  for visible in [true,false]{
   let mut front=object("front",front_geometry.clone(),DMat4::IDENTITY);front.visible=visible;front.pickable=false;front.locked=true;
   let index=PickIndex::new(vec![front,rear.clone()],s.clone()).unwrap();let mut q=query(&s,&c,DVec2::new(400.0,240.0));q.filter.bodies=false;q.filter.include_locked=false;
   assert_eq!(index.query(&q).unwrap().total,if visible{0}else{1});q.filter.xray=true;assert_eq!(index.query(&q).unwrap().total,1);
  }
 }
}

#[test]fn refit_metadata_growth_shrink_repetition_and_atomic_budget(){
 let camera=Camera::default().matrices([800.0,480.0]).unwrap();let old=stamp(&camera);
 let short=PickIndex::new(vec![],old.clone()).unwrap();let limit=short.resident_bytes;
 let mut tight=PickIndex::with_budget(vec![],old.clone(),PickBudget{resident_bytes:limit,..PickBudget::default()}).unwrap();
 let mut long=old.clone();long.frame.generation=GenerationId::new("g".repeat(128)).unwrap();long.frame.document=DocumentId::new("d".repeat(128)).unwrap();long.frame.session=SessionId::new("s".repeat(128)).unwrap();long.view=ViewId::new("v".repeat(128)).unwrap();
 assert!(tight.refit(&[],long.clone()).is_err());assert_eq!(tight.stamp,old);assert_eq!(tight.resident_bytes,limit);assert_eq!(tight.refits,0);
 let mut roomy=PickIndex::new(vec![],old.clone()).unwrap();
 for _ in 0..3{roomy.refit(&[],long.clone()).unwrap();assert_eq!(roomy.resident_bytes,limit+512-29);let long_admission=roomy.refit_admission_bytes(0,&old).unwrap();
  roomy.refit(&[],old.clone()).unwrap();assert_eq!(roomy.resident_bytes,limit);assert!(long_admission>roomy.refit_admission_bytes(0,&old).unwrap());}
 let mut temporary=PickIndex::with_budget(vec![],old.clone(),PickBudget{temporary_bytes:1,..PickBudget::default()}).unwrap();
 assert!(temporary.refit(&[],long).is_err());assert_eq!(temporary.stamp,old);assert_eq!(temporary.refits,0);
}
