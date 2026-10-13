use glam::{DVec2,DVec3};
use sim_render_trial::{cosines::CosinesModel,cosines_input::CosinesInput};
use sim_render::{bindings::TriangleTarget,camera::CameraMatrices,interaction::DragConstraint};
const CSS:[f64;2]=[800.0,480.0];
fn pixel(camera:&CameraMatrices,p:DVec3)->DVec2{camera.project(p).unwrap().truncate()}
fn truth(m:&CosinesModel){let p=m.binding.points(&m.session.snapshot()).unwrap();assert!(((p[1]-p[2]).length_squared()-m.scalar("c2").unwrap()).abs()<1e-10);}
#[test]fn vertices_shared_pointer_both_projections_cancel_and_commit(){
 for perspective in [false,true]{for(index,name)in ["O","A","B"].into_iter().enumerate(){
  let mut m=CosinesModel::new().unwrap();m.fit(CSS,perspective).unwrap();let c=m.camera.matrices(CSS).unwrap();let mut i=CosinesInput::default();
  let before=m.binding.points(&m.session.snapshot()).unwrap();let p=pixel(&c,before[index]);let rev=m.session.revision();
  assert!(i.begin(&mut m,&c,p,false,7).unwrap());assert_eq!(m.session.revision(),rev);assert_eq!(i.selected_name(),name);
  i.update(&mut m,&c,p+DVec2::new(9.0,-6.0),7).unwrap();truth(&m);
  let after=m.binding.points(&m.session.snapshot()).unwrap();for fixed in 0..3{if fixed!=index{assert!((after[fixed]-before[fixed]).length()<1e-10);}}
  i.end(&mut m,7,true).unwrap();for k in 0..3{assert!((m.binding.points(&m.session.snapshot()).unwrap()[k]-before[k]).length()<1e-10);}assert!(m.session.live_overrides().is_empty());
  i.begin(&mut m,&c,p,false,8).unwrap();i.update(&mut m,&c,p+DVec2::new(7.0,-4.0),8).unwrap();i.end(&mut m,8,false).unwrap();m.undo_input(false).unwrap();assert!(m.session.live_overrides().is_empty());
 }}
}
#[test]fn pending_press_rebases_after_live_time_and_restores_exact_intent(){
 for cancel in [false,true]{
  let mut m=CosinesModel::new().unwrap();m.fit(CSS,false).unwrap();let c=m.camera.matrices(CSS).unwrap();let mut i=CosinesInput::default();
  m.session.set_playing(m.session.revision(),true).unwrap();let p=pixel(&c,m.position("A").unwrap());let rev=m.session.revision();
  i.begin(&mut m,&c,p,false,4).unwrap();assert!(m.session.is_playing());assert_eq!(m.session.revision(),rev);
  m.advance(0.4).unwrap();let before=m.position("A").unwrap();let time=m.session.snapshot().time();let other=m.position("B").unwrap();
  i.update(&mut m,&c,p+DVec2::new(8.0,-5.0),4).unwrap();assert!(!m.session.is_playing());assert_eq!(m.session.snapshot().time(),time);truth(&m);assert!((m.position("B").unwrap()-other).length()<1e-10);
  i.end(&mut m,4,cancel).unwrap();assert!(m.session.is_playing());assert!(m.session.playback_intent());assert_eq!(m.session.snapshot().time(),time);
  if cancel{assert!(m.session.live_overrides().is_empty());assert!((m.position("A").unwrap()-before).length()<1e-10);}
 }
 let mut m=CosinesModel::new().unwrap();m.fit(CSS,false).unwrap();m.session.set_playing(m.session.revision(),true).unwrap();let c=m.camera.matrices(CSS).unwrap();let mut i=CosinesInput::default();let p=pixel(&c,m.position("A").unwrap());let rev=m.session.revision();i.begin(&mut m,&c,p,false,9).unwrap();i.end(&mut m,9,false).unwrap();assert_eq!(m.session.revision(),rev);assert!(m.session.is_playing());
}
#[test]fn coincident_handles_are_reachable_by_same_resolver_cycle(){
 for perspective in [false,true]{
  let mut m=CosinesModel::new().unwrap();m.set("a",4.0).unwrap();m.set("theta",0.0).unwrap();m.fit(CSS,perspective).unwrap();let c=m.camera.matrices(CSS).unwrap();let p=pixel(&c,m.position("A").unwrap());let mut i=CosinesInput::default();i.hover(&m,&c,p,false).unwrap();
  let names=i.candidate_names();assert!(names.iter().any(|s|s=="A"));assert!(names.iter().any(|s|s=="B"));
  for expected in ["A","B"]{for _ in 0..names.len(){if i.selection.chosen().is_some_and(|h|h.semantic.as_str()==expected){break;}i.cycle(1);}assert!(i.begin(&mut m,&c,p,false,1).unwrap());assert_eq!(i.selected_name(),expected);i.end(&mut m,1,false).unwrap();}
 }
}
#[test]fn out_of_xy_roundtrip_is_plane_aligned_without_math_revision(){
 let mut m=CosinesModel::new().unwrap();m.fit(CSS,true).unwrap();let c=m.camera.matrices(CSS).unwrap();let mut i=CosinesInput::default();i.constraint="z".into();let p=pixel(&c,m.position("A").unwrap());
 i.begin(&mut m,&c,p,false,1).unwrap();i.update(&mut m,&c,p+DVec2::new(0.0,-12.0),1).unwrap();i.end(&mut m,1,false).unwrap();assert!(m.position("A").unwrap().z.abs()>1e-4);truth(&m);
 let world=m.binding.points(&m.session.snapshot()).unwrap();let revision=m.session.revision();m.fit(CSS,false).unwrap();assert_eq!(revision,m.session.revision());assert_eq!(world,m.binding.points(&m.session.snapshot()).unwrap());
 let c2=m.camera.matrices(CSS).unwrap();let n=m.position("axis-x").unwrap().cross(m.position("axis-y").unwrap());assert!(n.dot(c2.toward_eye()).abs()>1.0-1e-12);
 i.constraint="screen".into();let press=pixel(&c2,world[2]);i.begin(&mut m,&c2,press,false,2).unwrap();i.update(&mut m,&c2,press+DVec2::new(10.0,5.0),2).unwrap();i.end(&mut m,2,false).unwrap();truth(&m);let p=m.binding.points(&m.session.snapshot()).unwrap();let revision=m.session.revision();m.fit(CSS,true).unwrap();assert_eq!(revision,m.session.revision());assert_eq!(p,m.binding.points(&m.session.snapshot()).unwrap());
}
#[test]fn radial_pivot_and_camera_conflict_are_explicit(){
 let mut m=CosinesModel::new().unwrap();m.fit(CSS,false).unwrap();let pivot=m.binding.pivot(&m.session.snapshot(),TriangleTarget::Angle).unwrap();let expected=m.position("origin").unwrap()+(m.position("axis-x").unwrap()*m.scalar("theta").unwrap().cos()+m.position("axis-y").unwrap()*m.scalar("theta").unwrap().sin())*0.8;assert!((pivot-expected).length()<1e-12);
 let c=m.camera.matrices(CSS).unwrap();let mut i=CosinesInput::default();let p=pixel(&c,m.position("A").unwrap());i.begin(&mut m,&c,p,false,1).unwrap();let rev=m.session.revision();m.fit(CSS,true).unwrap();let changed=m.camera.matrices(CSS).unwrap();assert!(i.update(&mut m,&changed,p+DVec2::ONE,1).is_err());assert_eq!(m.session.revision(),rev);i.end(&mut m,1,true).unwrap();
 assert!(DragConstraint::Axis{origin:DVec3::ZERO,direction:DVec3::Z}.intersect(c.ray(DVec2::new(400.0,240.0)).unwrap()).is_err());
}

#[test]fn terminal_conflict_releases_both_layers_without_restoring_external_truth(){
 for cancel in [false,true]{
  let mut m=CosinesModel::new().unwrap();m.fit(CSS,false).unwrap();let c=m.camera.matrices(CSS).unwrap();let mut i=CosinesInput::default();
  let p=pixel(&c,m.position("A").unwrap());i.begin(&mut m,&c,p,false,7).unwrap();i.update(&mut m,&c,p+DVec2::new(9.0,-6.0),7).unwrap();
  m.set("a",3.7).unwrap();let external=m.session.checkpoint();let override_snapshot=m.session.live_overrides().clone();let vertices=m.binding.points(&m.session.snapshot()).unwrap();let error=i.end(&mut m,7,cancel).unwrap_err();assert!(error.contains("external touched-key transition"),"{error}");
  assert!(i.drag.is_none());assert!(!m.input.active());assert_eq!(m.session.checkpoint(),external);assert_eq!(m.binding.points(&m.session.snapshot()).unwrap(),vertices);assert_eq!(m.input.history_len(),0);
  let next=pixel(&c,vertices[0]);assert!(i.begin(&mut m,&c,next,false,8).unwrap());assert_eq!(i.selected_name(),"O");i.update(&mut m,&c,next+DVec2::new(6.0,-3.0),8).unwrap();i.end(&mut m,8,true).unwrap();assert!(!m.input.active());assert!(i.drag.is_none());assert_eq!(m.session.live_overrides(),&override_snapshot);assert_eq!(m.binding.points(&m.session.snapshot()).unwrap(),vertices);
 }
}

#[test]fn rejected_first_proposal_rebases_then_pauses_and_cancels_exactly(){
 for perspective in [false,true]{
  let mut m=CosinesModel::new().unwrap();m.fit(CSS,perspective).unwrap();m.session.set_playing(m.session.revision(),true).unwrap();
  let c=m.camera.matrices(CSS).unwrap();let p=pixel(&c,m.position("A").unwrap());let mut i=CosinesInput::default();i.begin(&mut m,&c,p,false,5).unwrap();
  let before=m.session.checkpoint();assert!(i.update(&mut m,&c,p+DVec2::new(10000.0,0.0),5).is_err());assert_eq!(m.session.checkpoint(),before);assert!(!m.input.active());assert!(!i.drag.as_ref().unwrap().started);
  m.advance(0.4).unwrap();let time=m.session.snapshot().time();let overrides=m.session.live_overrides().clone();let vertices=m.binding.points(&m.session.snapshot()).unwrap();
  i.update(&mut m,&c,p+DVec2::new(8.0,-4.0),5).unwrap();assert!(m.input.active());assert!(i.drag.as_ref().unwrap().started);assert!(!m.session.is_playing());assert!(!m.session.playback_intent());assert_eq!(m.session.snapshot().time(),time);
  i.end(&mut m,5,true).unwrap();assert!(m.session.is_playing());assert!(m.session.playback_intent());assert_eq!(m.session.snapshot().time(),time);assert_eq!(m.session.live_overrides(),&overrides);assert_eq!(m.binding.points(&m.session.snapshot()).unwrap(),vertices);
 }
}
#[test]fn pending_noop_does_not_pause_or_capture_coordinator(){
 let mut m=CosinesModel::new().unwrap();m.set("theta",0.0).unwrap();m.fit(CSS,false).unwrap();m.session.set_playing(m.session.revision(),true).unwrap();
 let c=m.camera.matrices(CSS).unwrap();let pivot=m.binding.pivot(&m.session.snapshot(),TriangleTarget::Angle).unwrap();let press=pixel(&c,pivot);let mut i=CosinesInput::default();i.begin(&mut m,&c,press,false,6).unwrap();assert_eq!(i.target,TriangleTarget::Angle);
 // Radial motion changes the pointer but yields the exact same angle override.
 let before=m.session.checkpoint();i.update(&mut m,&c,press+DVec2::new(8.0,0.0),6).unwrap();assert_eq!(m.session.checkpoint(),before);assert!(!m.input.active());assert!(!i.drag.as_ref().unwrap().started);
 m.advance(0.2).unwrap();let before=m.session.checkpoint();i.update(&mut m,&c,press,6).unwrap();assert_eq!(m.session.checkpoint(),before);assert!(!m.input.active());i.end(&mut m,6,true).unwrap();assert!(m.session.is_playing());assert!(m.session.playback_intent());
}
#[test]fn postwrite_compile_failure_keeps_compensable_started_gesture(){
 let mut m=CosinesModel::new().unwrap();m.fit(CSS,false).unwrap();m.session.set_playing(m.session.revision(),true).unwrap();
 let c=m.camera.matrices(CSS).unwrap();let press=pixel(&c,m.position("A").unwrap());let mut i=CosinesInput::default();i.begin(&mut m,&c,press,false,2).unwrap();let original=m.scene.objects[0].parent.clone();
 m.scene.objects[0].parent=Some(sim_model::ObjectId::new("missing-parent").unwrap());let revision=m.session.revision();
 assert!(i.update(&mut m,&c,press+DVec2::new(8.0,-4.0),2).is_err());assert!(m.session.revision()>revision);assert!(!m.session.is_playing());assert!(m.input.active());assert!(i.drag.as_ref().unwrap().started);assert!(!m.session.live_overrides().is_empty());
 m.scene.objects[0].parent=original;i.end(&mut m,2,true).unwrap();assert!(m.session.live_overrides().is_empty());assert!(m.session.is_playing());assert!(m.session.playback_intent());assert!(!m.input.active());
}

#[test]fn rejected_runtime_patch_releases_new_ownership_only(){
 let mut m=CosinesModel::new().unwrap();m.set("theta",0.0).unwrap();m.fit(CSS,false).unwrap();m.session.set_playing(m.session.revision(),true).unwrap();let c=m.camera.matrices(CSS).unwrap();let p=pixel(&c,m.binding.pivot(&m.session.snapshot(),TriangleTarget::Angle).unwrap());let mut i=CosinesInput::default();i.begin(&mut m,&c,p,false,3).unwrap();assert_eq!(i.target,TriangleTarget::Angle);
 // A valid scalar proposal bound to an incompatible range exercises actual Session rejection.
 let theta=m.binding.theta.clone();m.binding.theta=m.binding.a.clone();let before=m.session.checkpoint();let failure=i.update(&mut m,&c,p+DVec2::new(8.0,0.0),3).unwrap_err();assert!(!failure.contains("inverse"));assert_eq!(m.session.checkpoint(),before);assert!(!m.input.active());assert!(!i.drag.as_ref().unwrap().started);
 m.binding.theta=theta;m.advance(0.4).unwrap();i.update(&mut m,&c,p+DVec2::new(0.0,-8.0),3).unwrap();assert!(!m.session.is_playing());assert!(i.drag.as_ref().unwrap().started);i.end(&mut m,3,true).unwrap();assert!(m.session.is_playing());assert!(m.session.playback_intent());
}
