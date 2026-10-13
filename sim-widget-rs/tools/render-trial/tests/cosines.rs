use sim_render_trial::cosines::CosinesModel;
#[test]
fn same_runtime_values_produce_geometry_and_formula(){
 let mut model=CosinesModel::new().unwrap();
 for (time,expected)in[(0.0,13.0),(0.5,25.0),(1.0,37.0)]{
  model.seek(time).unwrap();let a=model.position("A").unwrap();let b=model.position("B").unwrap();
  assert!((model.scalar("c2").unwrap()-expected).abs()<1e-12);
  assert!(((a-b).length_squared()-model.scalar("c2").unwrap()).abs()<1e-12);
 }
 model.set("a",4.5).unwrap();model.set("theta",75.0).unwrap();
 let a=model.position("A").unwrap();let b=model.position("B").unwrap();
 assert!(((a-b).length_squared()-model.scalar("c2").unwrap()).abs()<1e-12);
 let revision=model.session.revision();model.fit([800.0,480.0],true).unwrap();assert_eq!(model.session.revision(),revision);
}
