//! Semantic controls are exported alongside actual egui responses for platform adapters.
#[derive(Clone,Debug)]
pub enum Role{Button,Field,Slider,Tree,Output,Viewport,Menu,Choice}
#[derive(Clone,Debug)]
pub struct SemanticControl{pub id:String,pub label:String,pub role:Role,pub rect:egui::Rect,pub enabled:bool,pub value:Option<String>}
pub fn record(out:&mut crate::UiOutput,id:String,label:String,role:Role,response:&egui::Response,value:Option<String>){out.semantics.push(SemanticControl{id:id.into(),label:label.into(),role,rect:response.rect,enabled:response.enabled(),value});}
