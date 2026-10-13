use glam::DVec3;
use sim_render_trial::cosines::CosinesModel;
fn close(a:DVec3,b:DVec3){assert!((a-b).length()<1e-10,"{a:?} != {b:?}");}
#[test]fn independent_vertices_and_body_are_authoritative(){
 for name in ["O","A","B","body"]{
  let mut m=CosinesModel::new().unwrap();let old=[m.position("origin").unwrap(),m.position("A").unwrap(),m.position("B").unwrap()];
  let index=match name{"O"=>0,"A"=>1,"B"=>2,_=>3};
  let target=if index==3{(old[0]+old[1]+old[2])/3.0}else{old[index]}+DVec3::new(0.2,-0.1,0.4);
  m.move_vertex(name,target).unwrap();
  let new=[m.position("origin").unwrap(),m.position("A").unwrap(),m.position("B").unwrap()];
  for i in 0..3{close(new[i],if index==3{old[i]+DVec3::new(0.2,-0.1,0.4)}else if i==index{target}else{old[i]});}
  assert!(((new[1]-new[2]).length_squared()-m.scalar("c2").unwrap()).abs()<1e-10);
  close(m.position("axis-x").unwrap().cross(m.position("axis-y").unwrap()).normalize(),m.position("axis-x").unwrap().cross(m.position("axis-y").unwrap()));
 }
}
#[test]fn inverse_collinear_and_singular_are_explicit(){
 for r in [DVec3::new(3.0,0.0,0.0),DVec3::new(-3.0,0.0,0.0)]{
  let p=sim_render::interaction::inverse_triangle(DVec3::ZERO,r,DVec3::X*4.0,DVec3::Y).unwrap();
  assert!((p.u.dot(p.v)).abs()<1e-14);assert!((p.u*p.theta.cos()*p.a+p.v*p.theta.sin()*p.a-r).length()<1e-12);
 }
 assert!(sim_render::interaction::inverse_triangle(DVec3::ZERO,DVec3::ZERO,DVec3::X,DVec3::Y).is_err());
 let mut m=CosinesModel::new().unwrap();let rev=m.session.revision();let old=m.position("A").unwrap();
 assert!(m.move_vertex("A",DVec3::ZERO).is_err());assert_eq!(m.session.revision(),rev);close(old,m.position("A").unwrap());
}
#[test]fn cursor_zoom_preserves_anchor_and_camera_never_changes_math(){
 use sim_render::{camera::Camera,interaction::{DragConstraint,Navigation}};
 let m=CosinesModel::new().unwrap();let revision=m.session.revision();
 let c=Camera::default();let css=[800.0,480.0];let pointer=glam::DVec2::new(650.0,120.0);
 let plane=DragConstraint::Plane{origin:DVec3::ZERO,normal:DVec3::Z};
 let before=plane.intersect(c.matrices(css).unwrap().ray(pointer).unwrap()).unwrap();
 let next=c.zoom_at(pointer,0.3,css).unwrap();
 close(before,plane.intersect(next.matrices(css).unwrap().ray(pointer).unwrap()).unwrap());
 for kind in [Navigation::Orbit,Navigation::Pan,Navigation::Dolly]{c.navigate(kind,pointer,pointer+glam::DVec2::new(40.0,30.0),css,false).unwrap();}
 assert_eq!(m.session.revision(),revision);
}

#[test]fn group_preserves_parameter_bits_and_drag_cancel(){
 let mut m=CosinesModel::new().unwrap();let before=m.session.snapshot();let points=[m.position("origin").unwrap(),m.position("A").unwrap(),m.position("B").unwrap()];
 let pivot=(points[0]+points[1]+points[2])/3.0;
 m.begin_drag(sim_render::bindings::TriangleTarget::Group).unwrap();m.drag(sim_render::bindings::TriangleTarget::Group,pivot+DVec3::new(0.3,0.7,0.2)).unwrap();
 for name in ["a","b","theta","u_raw","v_raw"]{let id=sim_model::ValueId::new(name).unwrap();assert_eq!(m.session.snapshot().truth().get(&id),before.truth().get(&id));assert!(!m.session.live_overrides().contains_key(&sim_model::ParameterId::new(name).unwrap()));}
 m.end_drag(true).unwrap();for (name,point)in ["origin","A","B"].into_iter().zip(points){close(m.position(name).unwrap(),point);}assert!(m.session.live_overrides().is_empty());
}
#[test]fn angle_side_and_vertex_binding_use_atomic_core(){
 use sim_render::bindings::TriangleTarget;
 for target in [TriangleTarget::Angle,TriangleTarget::Side(0),TriangleTarget::Side(1),TriangleTarget::Side(2)]{
  let mut m=CosinesModel::new().unwrap();let old=[m.position("origin").unwrap(),m.position("A").unwrap(),m.position("B").unwrap()];
  let pivot=m.binding.pivot(&m.session.snapshot(),target).unwrap();
  m.begin_drag(target).unwrap();let revision=m.session.revision();m.drag(target,pivot+DVec3::new(0.1,0.1,0.0)).unwrap();assert_eq!(m.session.revision(),revision+1);
  let new=[m.position("origin").unwrap(),m.position("A").unwrap(),m.position("B").unwrap()];
  if let TriangleTarget::Side(side)=target{let fixed=[2,1,0][side];close(new[fixed],old[fixed]);}
  assert!(((new[1]-new[2]).length_squared()-m.scalar("c2").unwrap()).abs()<1e-10);m.end_drag(false).unwrap();m.undo_input(false).unwrap();
  for i in 0..3{close([m.position("origin").unwrap(),m.position("A").unwrap(),m.position("B").unwrap()][i],old[i]);}
 }
}
