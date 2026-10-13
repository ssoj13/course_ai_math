//! Typed frame event bus: widgets propose, host authorities admit and execute.
use std::collections::BTreeMap;
use sim_model::{ObjectId,ParameterId,Value,ViewId};
use sim_runtime::CommandKind;
use crate::projection::ProjectionStamp;
/// Shell-local view changes remain separate from document math and domain undo.
#[derive(Clone,Debug)]
pub enum ViewIntent{Filter{kind:String,visible:bool,selectable:bool},Select{objects:Vec<ObjectId>,additive:bool,range:bool},Reveal(ObjectId),Fit(ViewId),Projection{view:ViewId,perspective:bool},Tool(String),Space(String),Locale(crate::language::LocaleId),CameraSlot{index:usize,store:bool},UndoInput{redo:bool}}
/// Author operations whose runtime implementation is deliberately injected by the host.
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub enum AuthorAction{Create,Rename,Duplicate,Delete,Reparent,Visibility,Selectability,Lock,Isolate,SetMetadata,AddAttribute,Key,AutoKey,Interpolation,MoveKeys,DeleteKeys,MoveClip,Trim,Slide,WorkArea,Simplify,Smooth,EulerFilter,RecordTake,BindTake,UnbindTake,SplitTake,DeriveTake,BakeTake,AssignMaterial,ApplyPreset,AssignProfile,EditExpression,Graph,Curves}
/// Platform and resource services never run native IO inside the portable shell.
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub enum ServiceKind{Open,Save,SaveAs,Template,Bookmark,DeleteBookmark,Import,Export,Clipboard,Thumbnail,ColorConfig,Environment,Render,Encode,CancelJob,Display,Fonts,TakePackage,Help}
/// Capability state is explicit; unavailable actions emit a request with the scheduled gate.
#[derive(Clone,Debug)]
pub enum Capability{Available,RequiresUserGesture,Unavailable{reason_key:String,gate:String}}
#[derive(Clone,Debug,Default)]
pub struct PlatformCapabilities{pub services:BTreeMap<ServiceKind,Capability>,pub author:BTreeMap<AuthorAction,Capability>,pub hdr:bool,pub filterable_float32:bool,pub coarse_pointer:bool}
impl PlatformCapabilities{
 pub fn service(&self,kind:&ServiceKind)->Capability{self.services.get(kind).cloned().unwrap_or_else(||Capability::Unavailable{reason_key:"capability.pending".into(),gate:"WP-4 platform/resource service".into()})}
 pub fn author(&self,kind:&AuthorAction)->Capability{self.author.get(kind).cloned().unwrap_or_else(||Capability::Unavailable{reason_key:"capability.pending".into(),gate:"WP-3 authoritative author commands".into()})}
}
/// Logical URI and typed payload, stamped against the publication that requested it.
#[derive(Clone,Debug)]
pub struct ServiceRequest{pub id:u64,pub stamp:ProjectionStamp,pub kind:ServiceKind,pub operation:String,pub payload:serde_json::Value,pub capability:Capability}
#[derive(Clone,Debug)]
pub struct AuthorRequest{pub stamp:ProjectionStamp,pub action:AuthorAction,pub objects:Vec<ObjectId>,pub parameter:Option<ParameterId>,pub payload:serde_json::Value,pub capability:Capability}
#[derive(Clone,Copy,Debug)]pub enum InputSource{WidgetPointer,Keyboard}
/// A parameter edit describes gesture lifecycle; the existing InputCoordinator remains owner.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum GesturePhase{BeginPending,Changed,Committed,Cancelled}
#[derive(Clone,Debug)]
pub struct GestureIntent{pub role:crate::projection::UiRole,pub source:InputSource,pub stamp:ProjectionStamp,pub gesture:u64,pub pointer:u32,pub phase:GesturePhase,pub entries:Vec<(ParameterId,Option<Value>)>}
#[derive(Clone,Debug)]
pub enum UiIntent{Existing{stamp:ProjectionStamp,command:CommandKind},View(ViewIntent),Gesture(GestureIntent),Author(AuthorRequest),Service(ServiceRequest)}
/// One scene view target requested from the injected shared GPU host.
#[derive(Clone,Debug)]
pub struct ViewportRequest{pub hovered:bool,pub view:ViewId,pub rect:egui::Rect,pub stamp:ProjectionStamp}
#[derive(Default)]
pub struct UiOutput{pub intents:Vec<UiIntent>,pub viewports:Vec<ViewportRequest>,pub semantics:Vec<crate::accessibility::SemanticControl>,pub panels_drawn:Vec<crate::dock::Panel>,pub errors:Vec<String>}
impl UiOutput{
 pub fn gesture(&mut self,p:&crate::projection::EditorProjection<'_>,gesture:u64,source:InputSource,phase:GesturePhase,entries:Vec<(ParameterId,Option<Value>)>){self.intents.push(UiIntent::Gesture(GestureIntent{role:p.role,source,stamp:p.stamp(),gesture,pointer:match source{InputSource::WidgetPointer=>1,InputSource::Keyboard=>2},phase,entries}));}
 pub fn one_shot(&mut self,p:&crate::projection::EditorProjection<'_>,gesture:u64,entries:Vec<(ParameterId,Option<Value>)>){for phase in[GesturePhase::BeginPending,GesturePhase::Changed,GesturePhase::Committed]{self.gesture(p,gesture,InputSource::Keyboard,phase,entries.clone());}}

 pub fn service(&mut self,p:&crate::projection::EditorProjection<'_>,caps:&PlatformCapabilities,kind:ServiceKind,operation:impl Into<String>,payload:serde_json::Value){let capability=caps.service(&kind);if let Capability::Unavailable{reason_key,..}=&capability{self.errors.push(reason_key.clone());}self.intents.push(UiIntent::Service(ServiceRequest{id:self.intents.len() as u64,stamp:p.stamp(),kind,operation:operation.into(),payload,capability}));}
 pub fn author(&mut self,p:&crate::projection::EditorProjection<'_>,caps:&PlatformCapabilities,action:AuthorAction,objects:Vec<ObjectId>,parameter:Option<ParameterId>,payload:serde_json::Value){let capability=caps.author(&action);if let Capability::Unavailable{reason_key,..}=&capability{self.errors.push(reason_key.clone());}self.intents.push(UiIntent::Author(AuthorRequest{stamp:p.stamp(),action,objects,parameter,payload,capability}));}

 pub fn existing(&mut self,projection:&crate::projection::EditorProjection<'_>,command:CommandKind){self.intents.push(UiIntent::Existing{stamp:projection.stamp(),command});}
}
