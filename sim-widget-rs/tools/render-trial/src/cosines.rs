//! Coherent acceptance fixture: all numerical outputs and vertex coordinates are runtime values.
use sim_model::*;
use sim_runtime::{Session,SessionConfig,ViewState,CameraState};
use sim_math::OperatorRegistry;
use sim_render::{frame::SceneCompiler,camera::{Camera,Projection},bounds::Bounds};
use glam::DVec3;
use std::collections::BTreeMap;
use sim_render::{input::InputCoordinator,bindings::{TriangleBinding,TriangleTarget}};
pub struct CosinesModel {pub session:Session,pub scene:SceneSpec,pub compiler:SceneCompiler,pub view:ViewState,pub camera:Camera,pub perspective:bool,pub locale:String,pub input:InputCoordinator,pub binding:TriangleBinding}
impl CosinesModel{
    pub fn new()->Result<Self>{
        let document=Document::decode(include_bytes!("../../../fixtures/render/cosines-runtime.json"),DecodeLimits::default())?;
        let session=Session::new(document,OperatorRegistry::standard()?,SessionConfig::default())?;
        let object=ObjectId::new("triangle")?;
        let property=|s:&str|PropertyId::new(s).map(ScenePosition::Property);
        let scene=SceneSpec{schema_version:SCENE_SCHEMA_VERSION,document:session.snapshot().document_id().clone(),document_revision:session.snapshot().document_revision(),revision:0,
            up_axis:SceneUpAxis::Y,metres_per_unit:Finite::new(1.0)?,objects:vec![RenderObjectSpec{
                object,parent:None,transform:SceneTransform::default(),primitive:ScenePrimitive::Polygon{positions:vec![property("O")?,property("B")?,property("A")?]},
                material:SceneMaterial::default(),visible:true,pickable:true,labels:Vec::new()}]};
        let mut model=Self{session,scene,compiler:SceneCompiler::default(),view:ViewState::new(ViewId::new("cosines-main")?),camera:Camera::default(),perspective:false,locale:"ru".into(),input:InputCoordinator::default(),binding:TriangleBinding{origin:ParameterId::new("origin")?,a:ParameterId::new("a")?,b:ParameterId::new("b")?,theta:ParameterId::new("theta")?,u_raw:ParameterId::new("u_raw")?,v_raw:ParameterId::new("v_raw")?,world:[ValueId::new("origin")?,ValueId::new("A")?,ValueId::new("B")?],axes:[ValueId::new("axis-x")?,ValueId::new("axis-y")?],angle:ValueId::new("theta")?,angle_radius:0.8}};
        model.compile().map_err(|e|Diagnostic::new(DiagnosticCode::Type,"render",e))?;Ok(model)
    }
    pub fn compile(&mut self)->std::result::Result<(),String>{
        let frame=self.session.snapshot();self.compiler.compile(&frame,&self.scene,None).map_err(|e|e.to_string())?;Ok(())
    }
    pub fn scalar(&self,name:&str)->std::result::Result<f64,String>{
        self.session.snapshot().truth().get(&ValueId::new(name).map_err(|e|e.to_string())?).ok_or_else(||format!("missingtruth{name}"))?.as_scalar().map_err(|e|e.to_string())
    }
    pub fn position(&self,name:&str)->std::result::Result<DVec3,String>{
        let frame=self.session.snapshot();match frame.truth().get(&ValueId::new(name).map_err(|e|e.to_string())?){
            Some(Value::Vector(v))if v.len()==3=>Ok(DVec3::new(v[0].get(),v[1].get(),v[2].get())),_=>Err(format!("missingvector{name}"))}
    }
    pub fn set(&mut self,name:&str,value:f64)->std::result::Result<(),String>{
        if !["a","b","theta"].contains(&name){return Err("unknownparameter".into());}
        let value=if name=="theta"{DisplayUnit::Degrees.to_canonical(Finite::new(value).map_err(|e|e.to_string())?,Unit::ANGLE).map_err(|e|e.to_string())?.get()}else{value};
        self.session.set_live_override(self.session.revision(),ParameterId::new(name).map_err(|e|e.to_string())?,Value::scalar(value).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        self.compile()
    }
    pub fn begin_drag(&mut self,target:TriangleTarget)->std::result::Result<(),String>{self.input.begin(&self.session,&self.binding.keys(target)).map_err(|e|e.to_string())}
    pub fn drag(&mut self,target:TriangleTarget,point:DVec3)->std::result::Result<(),String>{
        let patch=self.binding.solve(&self.session.snapshot(),target,point).map_err(|e|e.to_string())?;
        self.input.update(&mut self.session,patch).map_err(|e|e.to_string())?;self.compile()
    }
    pub fn end_drag(&mut self,cancel:bool)->std::result::Result<(),String>{
        let terminal=if cancel{self.input.cancel(&mut self.session)}else{self.input.commit(&mut self.session)};
        if let Err(error)=terminal{self.input.abandon(&self.session);return Err(error.to_string());}self.compile()
    }
    pub fn undo_input(&mut self,redo:bool)->std::result::Result<(),String>{
        if redo{self.input.redo(&mut self.session).map_err(|e|e.to_string())?;}else{self.input.undo(&mut self.session).map_err(|e|e.to_string())?;}self.compile()
    }
    pub fn move_vertex(&mut self,name:&str,target:DVec3)->std::result::Result<(),String>{
        let tool=match name{"O"=>TriangleTarget::Vertex(0),"A"=>TriangleTarget::Vertex(1),"B"=>TriangleTarget::Vertex(2),"body"=>TriangleTarget::Group,_=>return Err("unknown handle".into())};
        self.begin_drag(tool)?;
        if let Err(error)=self.drag(tool,target){let cancel=self.end_drag(true);return Err(match cancel{Ok(())=>error,Err(cancel)=>format!("{error}; cancel: {cancel}")});}
        self.end_drag(false)
    }
    pub fn seek(&mut self,fraction:f64)->std::result::Result<(),String>{
        if !fraction.is_finite()||!(0.0..=1.0).contains(&fraction){return Err("seekrange0..1".into());}
        self.session.return_to_guide(self.session.revision(),&[ParameterId::new("theta").map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
        self.session.seek(self.session.revision(),TimeTicks::new((fraction*254016000000.0).round() as i64)).map_err(|e|e.to_string())?;
        self.compile()
    }
    pub fn advance(&mut self,fraction:f64)->std::result::Result<(),String>{
        if !fraction.is_finite()||!(0.0..=1.0).contains(&fraction){return Err("seekrange0..1".into());}
        self.session.seek(self.session.revision(),TimeTicks::new((fraction*254016000000.0).round() as i64)).map_err(|e|e.to_string())?;self.compile()
    }
    pub fn persist_camera(&mut self)->std::result::Result<(),String>{
        let vector=|v:[f64;3]|Value::vector(v.to_vec()).map_err(|e|e.to_string());
        let scalar=|v:f64|Value::scalar(v).map_err(|e|e.to_string());
        self.view.set_camera(Some(CameraState{schema:"sim.camera.v1".into(),fields:BTreeMap::from([
            ("eye".into(),vector(self.camera.eye)?),("target".into(),vector(self.camera.target)?),("up".into(),vector(self.camera.up)?),
            ("near".into(),scalar(self.camera.near)?),("far".into(),scalar(self.camera.far)?),
            ("projection".into(),Value::Enum(if self.perspective{"perspective"}else{"orthographic"}.into())),
            ("projection_scale".into(),scalar(match self.camera.projection{Projection::Orthographic{vertical_span}=>vertical_span,Projection::Perspective{vertical_fov}=>vertical_fov})?)
        ])})).map_err(|e|e.to_string())
    }
    pub fn fit(&mut self,css:[f64;2],perspective:bool)->std::result::Result<(),String>{
        self.perspective=perspective;
        let bounds=Bounds::from_points([self.position("origin")?,self.position("A")?,self.position("B")?]).map_err(|e|e.to_string())?;
        let center=bounds.center();let u=self.position("axis-x")?;let v=self.position("axis-y")?;
        self.camera=if perspective{Camera{eye:[7.0,6.0,8.0],projection:Projection::Perspective{vertical_fov:0.7},..Camera::default()}}else{Camera{eye:(center+u.cross(v)*10.0).to_array(),target:center.to_array(),up:v.to_array(),..Camera::default()}};
        self.camera=self.camera.fit(bounds,css,48.0).map_err(|e|e.to_string())?;
        self.persist_camera()?;
        Ok(())
    }
}
