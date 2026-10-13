//! Bounded live-input transactions. This module is downstream of Session and never owns truth.
//! Geometry/picking remains separate; hosts supply typed parameter patches from binding solvers.
use sim_model::{ParameterId,Value,TimeTicks,Diagnostic};
use sim_runtime::{Session,CommandKind,OverridePlaybackPolicy,EventKind,SessionEvent};
use crate::frame::FrameStamp;
use std::collections::{VecDeque};
use thiserror::Error;
#[derive(Debug,Error)]
pub enum InputError{
 #[error("another gesture is active")]Busy,
 #[error("input conflict: {0}")]Conflict(&'static str),
 #[error("input budget exceeded")]Budget,
 #[error("invalid input: {0}")]Invalid(&'static str),
 #[error("{0}")]Runtime(Diagnostic),
}
pub type InputResult<T>=std::result::Result<T,InputError>;
pub type OverridePatch=Vec<(ParameterId,Option<Value>)>;
#[derive(Clone,Copy,Debug)]
pub struct InputLimits{pub max_keys:usize,pub max_gesture_bytes:usize,pub max_history_entries:usize,pub max_history_bytes:usize}
impl Default for InputLimits{fn default()->Self{Self{max_keys:64,max_gesture_bytes:64*1024,max_history_entries:64,max_history_bytes:1024*1024}}}
#[derive(Clone,Debug)]
struct Guard{stamp:FrameStamp,time:TimeTicks,playing:bool,intent:bool}
impl Guard{
 fn capture(session:&Session)->Self{let frame=session.snapshot();Self{stamp:FrameStamp::from_frame(&frame),time:frame.time(),playing:session.is_playing(),intent:session.playback_intent()}}
 fn check(&self,session:&Session)->InputResult<()>{
  let current=Self::capture(session);
  if current.stamp.generation!=self.stamp.generation||current.stamp.session!=self.stamp.session||current.stamp.document!=self.stamp.document||current.stamp.epoch!=self.stamp.epoch||current.stamp.document_revision!=self.stamp.document_revision{return Err(InputError::Conflict("identity/document/permission changed"));}
  if current.time!=self.time{return Err(InputError::Conflict("logical time changed"));}
  if (current.playing,current.intent)!=(self.playing,self.intent){return Err(InputError::Conflict("playback changed"));}
  Ok(())
 }
}
#[derive(Clone,Debug)]
struct Gesture{before:OverridePatch,after:OverridePatch,start:Guard,owned:Guard,changed:bool,bytes:usize,conflict:Option<&'static str>}
#[derive(Clone,Debug)]
struct HistoryEntry{before:OverridePatch,after:OverridePatch,guard:Guard,bytes:usize,conflict:Option<&'static str>}
/// Undo history is learner input history. Author transaction history remains in Session.
pub struct InputCoordinator{limits:InputLimits,active:Option<Gesture>,undo:VecDeque<HistoryEntry>,redo:VecDeque<HistoryEntry>,history_bytes:usize,observed:Option<AuditCursor>}

#[derive(Clone,Copy)]struct FixedId{bytes:[u8;128],len:u8}
impl FixedId{fn new(value:&str)->Self{let mut bytes=[0;128];bytes[..value.len()].copy_from_slice(value.as_bytes());Self{bytes,len:value.len()as u8}}fn matches(&self,value:&str)->bool{&self.bytes[..usize::from(self.len)]==value.as_bytes()}}
/// Fixed-size cursor owns no event envelopes, values, documents or ID allocations.
#[derive(Clone,Copy)]struct AuditCursor{generation:FixedId,document:FixedId,session:FixedId,epoch:u64,document_revision:u64,revision:u64,sequence:Option<u64>,playing:bool,intent:bool}
impl AuditCursor{
 fn capture(session:&Session)->Self{let frame=session.snapshot();Self{generation:FixedId::new(frame.generation().as_str()),document:FixedId::new(frame.document_id().as_str()),session:FixedId::new(frame.session_id().as_str()),epoch:frame.epoch(),document_revision:frame.document_revision(),revision:frame.revision(),sequence:session.events().back().filter(|event|event.revision==frame.revision()).map(|event|event.sequence),playing:session.is_playing(),intent:session.playback_intent()}}
 fn same_owner(&self,other:&Self)->bool{self.generation.bytes==other.generation.bytes&&self.generation.len==other.generation.len&&self.document.bytes==other.document.bytes&&self.document.len==other.document.len&&self.session.bytes==other.session.bytes&&self.session.len==other.session.len&&self.epoch==other.epoch&&self.document_revision==other.document_revision}
 fn event_owner(&self,event:&SessionEvent)->bool{self.session.matches(event.session.as_str())&&self.document.matches(event.document.as_str())&&self.epoch==event.epoch&&self.document_revision==event.document_revision}
}
fn touches(kind:&EventKind,patch:&OverridePatch)->bool{
 let contains=|id:&ParameterId|patch.iter().any(|(key,_)|key==id);
 match kind{
  EventKind::LiveOverride{parameter,..}=>contains(parameter),
  EventKind::LiveOverrides{values}|EventKind::ResetDefaults{values}=>values.keys().any(contains),
  EventKind::ReturnToGuide{parameters}=>parameters.iter().any(contains),
  EventKind::PatchLiveOverrides{entries,..}=>entries.iter().any(|(id,_)|contains(id)),
  _=>false,
 }
}

fn patch_bytes(patch:&OverridePatch)->InputResult<usize>{
 let mut bytes=patch.capacity().checked_mul(std::mem::size_of::<(ParameterId,Option<Value>)>()).ok_or(InputError::Budget)?;
 for(id,value)in patch{
  bytes=bytes.checked_add(id.as_str().len()).ok_or(InputError::Budget)?;
  if let Some(value)=value{bytes=bytes.checked_add(value.heap_bytes().map_err(InputError::Runtime)?).ok_or(InputError::Budget)?;}
 }Ok(bytes)
}
fn guard_heap(guard:&Guard)->usize{guard.stamp.generation.as_str().len()+guard.stamp.document.as_str().len()+guard.stamp.session.as_str().len()}
fn history_payload(before:&OverridePatch,after:&OverridePatch,guard:&Guard)->InputResult<usize>{patch_bytes(before)?.checked_add(patch_bytes(after)?).and_then(|n|n.checked_add(guard_heap(guard))).ok_or(InputError::Budget)}
fn snapshot_patch(session:&Session,keys:impl IntoIterator<Item=ParameterId>)->OverridePatch{keys.into_iter().map(|id|{let value=session.live_overrides().get(&id).cloned();(id,value)}).collect()}
fn matches_patch(session:&Session,patch:&OverridePatch)->bool{patch.iter().all(|(id,value)|session.live_overrides().get(id)==value.as_ref())}
fn dispatch(session:&mut Session,patch:OverridePatch,playback:OverridePlaybackPolicy)->InputResult<bool>{
 let revision=session.revision();session.dispatch(session.snapshot().command(CommandKind::PatchLiveOverrides{entries:patch,playback})).map_err(InputError::Runtime)?;Ok(session.revision()!=revision)
}
impl InputCoordinator{
 pub fn new(limits:InputLimits)->InputResult<Self>{
  if limits.max_keys==0||limits.max_keys>4096||limits.max_gesture_bytes<512||limits.max_history_entries==0||limits.max_history_entries>4096||limits.max_history_bytes<limits.max_gesture_bytes{return Err(InputError::Invalid("limits"));}
  let base=limits.max_history_entries.checked_mul(2*std::mem::size_of::<HistoryEntry>()).and_then(|n|n.checked_add(std::mem::size_of::<Self>())).ok_or(InputError::Budget)?;
  if base.checked_add(limits.max_gesture_bytes).ok_or(InputError::Budget)?>limits.max_history_bytes{return Err(InputError::Budget);}
  Ok(Self{limits,active:None,undo:VecDeque::with_capacity(limits.max_history_entries),redo:VecDeque::with_capacity(limits.max_history_entries),history_bytes:base,observed:None})
 }

 fn mark_external(&mut self,event:Option<&SessionEvent>,global:Option<&'static str>){
  for entry in self.undo.iter_mut().chain(self.redo.iter_mut()){
   let reason=global.or_else(||event.filter(|event|touches(&event.kind,&entry.before)).map(|_|"external touched-key transition"));
   if entry.conflict.is_none(){entry.conflict=reason;}
  }
  if let Some(gesture)=&mut self.active{
   let reason=global.or_else(||event.filter(|event|touches(&event.kind,&gesture.before)).map(|_|"external touched-key transition"));
   if gesture.conflict.is_none(){gesture.conflict=reason;}
  }
 }
 /// Only unowned intervals are classified. Successful coordinator commands advance this cursor explicitly.
 fn observe(&mut self,session:&Session){
  let current=AuditCursor::capture(session);let Some(mut previous)=self.observed else{self.observed=Some(current);return;};
  if !previous.same_owner(&current){self.mark_external(None,Some("external identity/document/permission transition"));self.observed=Some(current);return;}
  if current.revision==previous.revision{return;}
  if current.revision<previous.revision{self.mark_external(None,Some("nonmonotonic audit revision"));self.observed=Some(current);return;}
  let events=session.events();let start=events.partition_point(|event|event.revision<=previous.revision);
  let mut seen_revision=previous.revision;let mut seen_sequence=previous.sequence;
  for event in events.range(start..){
   if !previous.event_owner(event)||Some(event.revision)!=seen_revision.checked_add(1)||seen_sequence.is_some_and(|sequence|Some(event.sequence)!=sequence.checked_add(1)){
    self.mark_external(None,Some("audit interval unavailable"));self.observed=Some(current);return;
   }
   seen_revision=event.revision;seen_sequence=Some(event.sequence);
   let global=match &event.kind{
    EventKind::Seek{..}=>Some("external logical-time transition"),
    EventKind::Playback{playing}=>{previous.playing=*playing;previous.intent=*playing;Some("external playback transition")},
    EventKind::AuthorRevision{..}|EventKind::TransactionBegin{..}|EventKind::TransactionUpdate{..}|EventKind::TransactionDocumentUpdate{..}|EventKind::TransactionCancel{..}=>Some("external author transition"),
    EventKind::LiveOverride{..}|EventKind::LiveOverrides{..}=>{let changed=previous.playing||previous.intent;previous.playing=false;previous.intent=false;changed.then_some("external playback transition")},
    EventKind::PatchLiveOverrides{playback,..}=>{
     let target=match playback{OverridePlaybackPolicy::Pause=>(false,false),OverridePlaybackPolicy::Preserve=>(previous.playing,previous.intent),OverridePlaybackPolicy::SetIntent(intent)=>(*intent,*intent)};
     let changed=target!=(previous.playing,previous.intent);previous.playing=target.0;previous.intent=target.1;changed.then_some("external playback transition")
    }
    _=>None,
   };
   self.mark_external(Some(event),global);
  }
  if seen_revision!=current.revision||previous.playing!=current.playing||previous.intent!=current.intent{self.mark_external(None,Some("audit interval unavailable"));}
  self.observed=Some(current);
 }
 fn owned(&mut self,session:&Session){self.observed=Some(AuditCursor::capture(session));}

 /// Release only transient gesture ownership. This never dispatches or restores Session state.
 /// Hosts call this after a terminal commit/compensation failure and report that original failure.
 /// Unowned events are observed first, so historical conflicts and the audit cursor remain authoritative.
 pub fn abandon(&mut self,session:&Session)->bool{self.observe(session);self.active.take().is_some()}
 pub fn active(&self)->bool{self.active.is_some()}
 pub fn can_undo(&self)->bool{!self.undo.is_empty()&&!self.active()}
 pub fn can_redo(&self)->bool{!self.redo.is_empty()&&!self.active()}
 pub fn history_bytes(&self)->usize{self.history_bytes}
 pub fn history_len(&self)->usize{self.undo.len()}
 pub fn begin(&mut self,session:&Session,keys:&[ParameterId])->InputResult<()>{
  if self.active(){return Err(InputError::Busy);}
  if keys.is_empty()||keys.len()>self.limits.max_keys{return Err(InputError::Budget);}
  self.observe(session);
  let document=session.document();let permissions=document.document().learner_permissions;
  if !permissions.overrides||!permissions.return_to_guide||session.is_playing()&&!permissions.playback{return Err(InputError::Invalid("gesture requires set/clear and owned playback compensation permission"));}
  for key in keys{let parameter=document.parameter(key).map_err(InputError::Runtime)?;if parameter.readonly||parameter.metadata.locked||!parameter.metadata.learner_exposed{return Err(InputError::Invalid("parameter is not an exposed editable input"));}}
  // Inspect values before cloning any user-sized payload.
  let frame=session.snapshot();let guard_bytes=frame.generation().as_str().len()+frame.document_id().as_str().len()+frame.session_id().as_str().len();
  let mut admission=keys.len().checked_mul(2*std::mem::size_of::<(ParameterId,Option<Value>)>()+std::mem::size_of::<&ParameterId>()).and_then(|n|n.checked_add(std::mem::size_of::<Gesture>()+2*guard_bytes)).ok_or(InputError::Budget)?;
  for key in keys{admission=admission.checked_add(2*key.as_str().len()).ok_or(InputError::Budget)?;}
  for key in keys{if let Some(value)=session.live_overrides().get(key){admission=admission.checked_add(value.heap_bytes().map_err(InputError::Runtime)?.checked_mul(2).ok_or(InputError::Budget)?).ok_or(InputError::Budget)?;}}
  if admission>self.limits.max_gesture_bytes{return Err(InputError::Budget);}
  let mut ordered:Vec<_>=keys.iter().collect();ordered.sort_unstable();if ordered.windows(2).any(|p|p[0]==p[1]){return Err(InputError::Invalid("duplicate gesture key"));}drop(ordered);
  let start=Guard::capture(session);
  let before=snapshot_patch(session,keys.iter().cloned());let bytes=history_payload(&before,&before,&start)?;
  if bytes.checked_add(std::mem::size_of::<Gesture>()+guard_heap(&start)).ok_or(InputError::Budget)?>self.limits.max_gesture_bytes{return Err(InputError::Budget);}
  self.active=Some(Gesture{after:before.clone(),before,start:start.clone(),owned:start,changed:false,bytes,conflict:None});Ok(())
 }
 /// Failure leaves the active gesture and Session untouched. A lock/conflict can therefore be reported honestly.
 pub fn update(&mut self,session:&mut Session,entries:OverridePatch)->InputResult<bool>{
  self.observe(session);
  let gesture=self.active.as_ref().ok_or(InputError::Invalid("no active gesture"))?;
  if let Some(reason)=gesture.conflict{return Err(InputError::Conflict(reason));}
  gesture.owned.check(session)?;if !matches_patch(session,&gesture.after){return Err(InputError::Conflict("touched override changed"));}
  if entries.len()>self.limits.max_keys||entries.capacity()>self.limits.max_keys{return Err(InputError::Budget);}
  let after_bytes=patch_bytes(&entries)?;if after_bytes.checked_add(patch_bytes(&gesture.before)?).ok_or(InputError::Budget)?>self.limits.max_gesture_bytes{return Err(InputError::Budget);}
  // All patch/data staging is admitted before cloning. Borrowed sorted IDs replace temporary tree nodes.
  let staging=patch_bytes(&gesture.before)?.checked_add(patch_bytes(&gesture.after)?).and_then(|n|n.checked_add(after_bytes)).and_then(|n|n.checked_add(patch_bytes(&gesture.after).ok()?)).and_then(|n|n.checked_add(std::mem::size_of::<Gesture>()+3*guard_heap(&gesture.owned)+entries.len()*std::mem::size_of::<&ParameterId>())).ok_or(InputError::Budget)?;
  if staging>self.limits.max_gesture_bytes{return Err(InputError::Budget);}
  let mut keys:Vec<_>=entries.iter().map(|(id,_)|id).collect();keys.sort_unstable();if keys.windows(2).any(|pair|pair[0]==pair[1]){return Err(InputError::Invalid("duplicate gesture key"));}drop(keys);
  let mut after=gesture.after.clone();
  for(id,value)in &entries{let target=after.iter_mut().find(|(key,_)|key==id).ok_or(InputError::Invalid("undeclared gesture key"))?;target.1=value.clone();}
  let bytes=history_payload(&gesture.before,&after,&gesture.owned)?;
  if bytes.checked_add(std::mem::size_of::<Gesture>()+guard_heap(&gesture.owned)).ok_or(InputError::Budget)?>self.limits.max_gesture_bytes{return Err(InputError::Budget);}
  if matches_patch(session,&after){return Ok(false);}
  let playback=if !gesture.changed&&gesture.start.playing{OverridePlaybackPolicy::Pause}else{OverridePlaybackPolicy::Preserve};
  let changed=dispatch(session,entries,playback)?;self.owned(session);
  let gesture=self.active.as_mut().expect("active preserved");gesture.after=after;gesture.owned=Guard::capture(session);gesture.changed|=changed;gesture.bytes=bytes;Ok(changed)
 }
 pub fn cancel(&mut self,session:&mut Session)->InputResult<bool>{
  self.observe(session);
  let gesture=self.active.as_ref().ok_or(InputError::Invalid("no active gesture"))?;
  if let Some(reason)=gesture.conflict{return Err(InputError::Conflict(reason));}
  gesture.owned.check(session)?;if !matches_patch(session,&gesture.after){return Err(InputError::Conflict("touched override changed"));}
  if !gesture.changed{self.active=None;return Ok(false);}
  let playback=if gesture.start.playing{OverridePlaybackPolicy::SetIntent(true)}else{OverridePlaybackPolicy::Preserve};
  let changed=dispatch(session,gesture.before.clone(),playback)?;self.owned(session);
  self.active=None;Ok(changed)
 }
 pub fn commit(&mut self,session:&mut Session)->InputResult<bool>{
  self.observe(session);
  let gesture=self.active.as_ref().ok_or(InputError::Invalid("no active gesture"))?;
  if let Some(reason)=gesture.conflict{return Err(InputError::Conflict(reason));}
  gesture.owned.check(session)?;if !matches_patch(session,&gesture.after){return Err(InputError::Conflict("touched override changed"));}
  if !gesture.changed{self.active=None;return Ok(false);}
  if gesture.start.playing{dispatch(session,Vec::new(),OverridePlaybackPolicy::SetIntent(true))?;}self.owned(session);
  let gesture=self.active.take().expect("active preserved");
  // Compensation back to the exact original presence/value is a no-history gesture.
  if gesture.before==gesture.after{return Ok(false);}
  for entry in self.redo.drain(..){self.history_bytes-=entry.bytes;}
  while self.undo.len()>=self.limits.max_history_entries||self.history_bytes.checked_add(gesture.bytes).ok_or(InputError::Budget)?>self.limits.max_history_bytes{
   let old=self.undo.pop_front().ok_or(InputError::Budget)?;self.history_bytes-=old.bytes;
  }
  self.history_bytes+=gesture.bytes;
  self.undo.push_back(HistoryEntry{before:gesture.before,after:gesture.after,guard:Guard::capture(session),bytes:gesture.bytes,conflict:None});Ok(true)
 }
 pub fn undo(&mut self,session:&mut Session)->InputResult<bool>{self.apply_history(session,false)}
 pub fn redo(&mut self,session:&mut Session)->InputResult<bool>{self.apply_history(session,true)}
 fn apply_history(&mut self,session:&mut Session,redo:bool)->InputResult<bool>{
  if self.active(){return Err(InputError::Busy);}
  self.observe(session);
  let from=if redo{&self.redo}else{&self.undo};let Some(entry)=from.back()else{return Ok(false);};
  if let Some(reason)=entry.conflict{return Err(InputError::Conflict(reason));}
  entry.guard.check(session)?;
  if !matches_patch(session,if redo{&entry.before}else{&entry.after}){return Err(InputError::Conflict("history touched override changed"));}
  let changed=dispatch(session,if redo{entry.after.clone()}else{entry.before.clone()},OverridePlaybackPolicy::Preserve)?;self.owned(session);
  let mut entry=if redo{self.redo.pop_back()}else{self.undo.pop_back()}.expect("entry preserved");
  entry.guard=Guard::capture(session);
  if redo{self.undo.push_back(entry);}else{self.redo.push_back(entry);}
  Ok(changed)
 }
}
impl Default for InputCoordinator{fn default()->Self{Self::new(InputLimits::default()).expect("constant limits")}}
