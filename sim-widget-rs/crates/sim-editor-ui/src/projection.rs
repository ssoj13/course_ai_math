//! Borrowed projection of the accepted runtime; no duplicated evaluator or mutable Session.
use sim_model::{Document,Parameter,Value,ParameterId,ObjectId,TimeTicks,GenerationId,DocumentId,SessionId};
use sim_runtime::FrameSnapshot;
/// Publication identity carried by every emitted command.
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct ProjectionStamp { pub generation:GenerationId,pub document:DocumentId,pub session:SessionId,pub epoch:u64,pub document_revision:u64,pub revision:u64 }
impl ProjectionStamp {
 pub fn capture(frame:&FrameSnapshot)->Self{Self{generation:frame.generation().clone(),document:frame.document_id().clone(),session:frame.session_id().clone(),epoch:frame.epoch(),document_revision:frame.document_revision(),revision:frame.revision()}}
}
/// Exact property metadata and canonical value borrowed for one UI publication.
#[derive(Clone,Copy)]
pub struct PropertyProjection<'a>{pub parameter:&'a Parameter,pub value:&'a Value}
/// Host-provided catalogue entry. Thumbnails are real host texture IDs or pending service work.
#[derive(Clone,Debug)]
pub struct CatalogEntry{pub id:String,pub name:String,pub category:String,pub description:String,pub texture:Option<egui::TextureId>,pub error:Option<String>}
/// A real recorded take, projected by its authoritative owner.
#[derive(Clone,Debug)]
pub struct TakeProjection{pub id:String,pub name:String,pub pinned:bool,pub kind:String,pub timing:String,pub sample_count:usize,pub start:TimeTicks,pub end:TimeTicks,pub speed:f64,pub targets:Vec<ObjectId>}
/// A profile projection carries actual schema fields, not invented rendered samples.
#[derive(Clone,Debug)]
pub struct ProfileProjection{pub id:String,pub name:String,pub fields:Vec<(String,Value)>}
/// Runtime/controller state supplied explicitly; the shell cannot infer playback or history.
#[derive(Clone,Copy,Debug)]
pub enum UiRole{Reader,Author}
#[derive(Clone,Copy,Debug)]
pub struct RuntimeUiState{pub role:UiRole,pub playing:bool,pub playback_intent:bool,pub can_undo:bool,pub can_redo:bool}
/// Immutable document and sampled publication. Available controls map to existing commands.
pub struct EditorProjection<'a>{
 pub frame:&'a FrameSnapshot,pub document:&'a Document,pub role:UiRole,pub playing:bool,pub playback_intent:bool,pub can_undo:bool,pub can_redo:bool,
 pub catalog:&'a [CatalogEntry],pub bookmarks:&'a [CatalogEntry],pub materials:&'a [CatalogEntry],pub material_library:&'a [CatalogEntry],pub takes:&'a [TakeProjection],pub profiles:&'a [ProfileProjection],
 pub viewport_texture:Option<egui::TextureId>,pub status:&'a str,
}
impl<'a> EditorProjection<'a>{
 pub fn from_runtime(frame:&'a FrameSnapshot,document:&'a Document,state:RuntimeUiState)->Result<Self,String>{
  if frame.document_id()!=&document.id||frame.document_revision()!=document.revision{return Err("projection.mismatch".into());}
  Ok(Self{frame,document,role:state.role,playing:state.playing,playback_intent:state.playback_intent,can_undo:state.can_undo,can_redo:state.can_redo,catalog:&[],bookmarks:&[],materials:&[],material_library:&[],takes:&[],profiles:&[],viewport_texture:None,status:""})
 }
 pub fn stamp(&self)->ProjectionStamp{ProjectionStamp::capture(self.frame)}
 pub fn properties(&self)->impl Iterator<Item=PropertyProjection<'a>>+'_{
  self.document.parameters.iter().filter_map(move|parameter|self.frame.truth().get(&parameter.value_id).map(|value|PropertyProjection{parameter,value}))
 }
 pub fn property(&self,id:&ParameterId)->Option<PropertyProjection<'a>>{self.document.parameters.iter().find(|p|&p.id==id).and_then(|parameter|self.frame.truth().get(&parameter.value_id).map(|value|PropertyProjection{parameter,value}))}
 pub fn time(&self)->TimeTicks{self.frame.time()}
}
