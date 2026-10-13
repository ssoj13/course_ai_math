//! UI gesture transport with borrowed Session admission. The injected owner retains its sole history.
use sim_model::{ParameterId,Value};
use sim_runtime::{Session,EventKind};
use crate::{intents::GestureIntent,intents::GesturePhase,projection::{ProjectionStamp,UiRole}};
pub trait InputOwner{
 fn begin(&mut self,session:&Session,role:UiRole,keys:&[ParameterId])->Result<(),String>;
 fn update(&mut self,session:&mut Session,entries:Vec<(ParameterId,Option<Value>)>)->Result<bool,String>;
 fn commit(&mut self,session:&mut Session)->Result<bool,String>;
 fn cancel(&mut self,session:&mut Session)->Result<bool,String>;
 fn abandon(&mut self,session:&Session);
}
#[derive(Clone,Debug)]pub struct GestureFeedback{pub event:GestureIntent,pub result:Result<bool,String>}
struct Pending{id:u64,role:UiRole,keys:Vec<ParameterId>,stamp:ProjectionStamp,revision:u64,started:bool}
#[derive(Default)]pub struct InputTransport{pending:Option<Pending>}
impl InputTransport{
 pub fn active(&self)->bool{self.pending.is_some()}
 pub fn active_gesture(&self)->Option<u64>{self.pending.as_ref().map(|p|p.id)}
 /// A frame transfers the previous owner before a newly focused field, regardless of widget order.
 /// Only this exclusive, owned batch can rebase a new Begin over writes from an earlier gesture.
 pub fn handle_frame(&mut self,session:&mut Session,owner:&mut impl InputOwner,mut events:Vec<GestureIntent>)->Result<Vec<GestureFeedback>,String>{
  if events.len()>256{return Err("input.budget".into());}
  let initial=ProjectionStamp::capture(&session.snapshot());let active=self.active_gesture();
  if let Some(id)=active{let mut front=0;for at in 0..events.len(){if events[at].gesture==id{events[front..=at].rotate_right(1);front+=1;}}}
  let mut feedback=Vec::with_capacity(events.len());let mut retired=false;
  for mut event in events{
   if retired&&event.phase==GesturePhase::BeginPending&&event.stamp==initial{event.stamp=ProjectionStamp::capture(&session.snapshot());}
   let result=self.handle(session,owner,&event);
   if active==Some(event.gesture)&&matches!(event.phase,GesturePhase::Committed|GesturePhase::Cancelled)&&!self.active(){retired=true;}
   feedback.push(GestureFeedback{event,result});
  }Ok(feedback)
 }
 /// End physical ownership after blur, hide, capture loss or a terminal admission failure.
 pub fn cancel(&mut self,session:&mut Session,owner:&mut impl InputOwner)->Result<bool,String>{
  let Some(pending)=self.pending.take()else{return Ok(false);};
  if !pending.started{return Ok(false);}let result=owner.cancel(session);if result.is_err(){owner.abandon(session);}result
 }
 pub fn handle(&mut self,session:&mut Session,owner:&mut impl InputOwner,event:&GestureIntent)->Result<bool,String>{
  match event.phase{
   GesturePhase::BeginPending=>{
    if self.pending.is_some(){return Err("input.busy".into());}
    if event.entries.is_empty()||event.entries.len()>64{return Err("input.budget".into());}
    let mut bytes=0usize;for(index,(id,value))in event.entries.iter().enumerate(){if event.entries[..index].iter().any(|(old,_)|old==id){return Err("input.duplicate".into());}bytes=bytes.checked_add(std::mem::size_of::<ParameterId>()+id.as_str().len()+value.as_ref().map_or(Ok(0),Value::heap_bytes).map_err(|e|e.to_string())?).ok_or("input.budget")?;if bytes>65536{return Err("input.budget".into());}}
    let frame=session.snapshot();let current=ProjectionStamp::capture(&frame);
    if !same_owner(&current,&event.stamp)||current.revision!=event.stamp.revision{return Err("draft.stale".into());}
    self.pending=Some(Pending{id:event.gesture,role:event.role,keys:event.entries.iter().map(|(id,_)|id.clone()).collect(),stamp:current,revision:session.revision(),started:false});Ok(false)
   },
   GesturePhase::Changed=>{
    admit(&event.entries)?;let pending=self.pending.as_mut().ok_or("input.missing")?;if pending.id!=event.gesture{return Err("input.busy".into());}
    if event.entries.iter().any(|(key,_)|!pending.keys.contains(key)){return Err("input.target".into());}
    if !pending.started{
     check_pending(pending,session)?;owner.begin(session,pending.role,&pending.keys)?;
     let before=session.revision();let result=owner.update(session,event.entries.clone());
     match result{Ok(true)=>{pending.started=true;Ok(true)},Ok(false)=>{owner.abandon(session);Ok(false)},Err(e)=>{if session.revision()!=before{pending.started=true;}else{owner.abandon(session);}Err(e)}}
    }else{owner.update(session,event.entries.clone())}
   },
   GesturePhase::Committed|GesturePhase::Cancelled=>{
    if self.pending.as_ref().is_some_and(|p|p.id!=event.gesture){return Err("input.busy".into());}
    let Some(pending)=self.pending.take()else{return Ok(false);};
    if !pending.started{return Ok(false);}let result=if event.phase==GesturePhase::Cancelled{owner.cancel(session)}else{owner.commit(session)};if result.is_err(){owner.abandon(session);}result
   }
  }
 }
}
fn admit(entries:&[(ParameterId,Option<Value>)])->Result<(),String>{if entries.is_empty()||entries.len()>64{return Err("input.budget".into());}let mut bytes=0usize;for(index,(id,value))in entries.iter().enumerate(){if entries[..index].iter().any(|(old,_)|old==id){return Err("input.duplicate".into());}bytes=bytes.checked_add(std::mem::size_of::<ParameterId>()+id.as_str().len()+value.as_ref().map_or(Ok(0),Value::heap_bytes).map_err(|e|e.to_string())?).ok_or("input.budget")?;if bytes>65536{return Err("input.budget".into());}}Ok(())}
fn same_owner(a:&ProjectionStamp,b:&ProjectionStamp)->bool{a.generation==b.generation&&a.document==b.document&&a.session==b.session&&a.epoch==b.epoch&&a.document_revision==b.document_revision}
fn check_pending(pending:&Pending,session:&Session)->Result<(),String>{
 let current=ProjectionStamp::capture(&session.snapshot());if !same_owner(&pending.stamp,&current){return Err("draft.stale".into());}
 let mut revision=pending.revision;
 for event in session.events().iter().filter(|e|e.revision>pending.revision){
  if event.revision!=revision.checked_add(1).ok_or("draft.stale")?{return Err("draft.stale".into());}revision=event.revision;
  let has=|id:&ParameterId|pending.keys.contains(id);
  let conflict=match &event.kind{
   EventKind::LiveOverride{parameter,..}=>has(parameter),EventKind::LiveOverrides{values}|EventKind::ResetDefaults{values}=>values.keys().any(has),
   EventKind::ReturnToGuide{parameters}=>parameters.iter().any(has),EventKind::PatchLiveOverrides{entries,..}=>entries.iter().any(|(id,_)|has(id)),
   EventKind::Seek{..}|EventKind::Playback{..}=>false,_=>true,
  };if conflict{return Err("draft.stale".into());}
 }if revision!=current.revision{return Err("draft.stale".into());}Ok(())
}
