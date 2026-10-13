use sim_model::*;
use sim_runtime::{Session,SessionConfig};
use sim_math::OperatorRegistry;
use sim_render::input::*;
fn session()->Session{Session::new(Document::decode(include_bytes!("../../../fixtures/render/cosines-runtime.json"),DecodeLimits::default()).unwrap(),OperatorRegistry::standard().unwrap(),SessionConfig::default()).unwrap()}
fn key(n:&str)->ParameterId{ParameterId::new(n).unwrap()}
fn set(n:&str,v:f64)->OverridePatch{vec![(key(n),Some(Value::scalar(v).unwrap()))]}
#[test]fn exact_cancel_absence_source_and_monotonic_identity(){
 let mut s=session();let frame=s.snapshot();let before=s.snapshot().provenance().clone();let mut input=InputCoordinator::default();
 input.begin(&s,&[key("a"),key("b")]).unwrap();input.update(&mut s,vec![(key("a"),Some(Value::scalar(5.0).unwrap())),(key("b"),Some(Value::scalar(6.0).unwrap()))]).unwrap();
 let revision=s.revision();input.cancel(&mut s).unwrap();
 assert!(s.live_overrides().is_empty());assert_eq!(s.snapshot().provenance(),&before);assert!(s.revision()>revision);assert_eq!(s.snapshot().generation(),frame.generation());assert_eq!(input.history_len(),0);
}
#[test]fn one_drag_one_undo_redo_and_unrelated_edit_preserved(){
 let mut s=session();let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();
 input.update(&mut s,set("a",4.0)).unwrap();input.update(&mut s,set("a",5.0)).unwrap();
 s.set_live_override(s.revision(),key("b"),Value::scalar(7.0).unwrap()).unwrap();
 input.commit(&mut s).unwrap();assert_eq!(input.history_len(),1);
 input.undo(&mut s).unwrap();assert!(!s.live_overrides().contains_key(&key("a")));assert_eq!(s.live_overrides()[&key("b")],Value::scalar(7.0).unwrap());
 input.redo(&mut s).unwrap();assert_eq!(s.live_overrides()[&key("a")],Value::scalar(5.0).unwrap());assert_eq!(input.history_len(),1);
}
#[test]fn active_playback_pause_and_cancel_restores_intent(){
 let mut s=session();s.set_playing(s.revision(),true).unwrap();let mut input=InputCoordinator::default();
 input.begin(&s,&[key("a")]).unwrap();assert!(s.is_playing());input.update(&mut s,set("a",4.0)).unwrap();
 assert!(!s.is_playing());assert!(!s.playback_intent());input.cancel(&mut s).unwrap();assert!(s.is_playing());assert!(s.playback_intent());assert!(s.live_overrides().is_empty());
}
#[test]fn no_op_empty_gesture_does_not_pause_or_record(){
 let mut s=session();s.set_live_override(s.revision(),key("a"),Value::scalar(3.0).unwrap()).unwrap();s.set_playing(s.revision(),true).unwrap();
 let revision=s.revision();let events=s.events().len();let mut input=InputCoordinator::default();
 input.begin(&s,&[key("a")]).unwrap();assert!(!input.update(&mut s,set("a",3.0)).unwrap());assert!(!input.commit(&mut s).unwrap());
 assert_eq!(s.revision(),revision);assert_eq!(s.events().len(),events);assert!(s.is_playing());assert_eq!(input.history_len(),0);
}
#[test]fn touched_time_and_playback_conflicts_do_not_fake_cancel(){
 let mut s=session();let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();
 s.set_live_override(s.revision(),key("a"),Value::scalar(6.0).unwrap()).unwrap();let revision=s.revision();assert!(matches!(input.cancel(&mut s),Err(InputError::Conflict(_))));assert_eq!(s.revision(),revision);assert!(input.active());
 let mut s=session();let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();
 s.seek(s.revision(),TimeTicks::new(254016000000)).unwrap();assert!(matches!(input.cancel(&mut s),Err(InputError::Conflict(_))));
 let mut s=session();let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();s.set_playing(s.revision(),true).unwrap();
 assert!(matches!(input.cancel(&mut s),Err(InputError::Conflict(_))));
}
#[test]fn bounded_history_eviction_and_invalid_patch_atomic(){
 let mut s=session();let mut input=InputCoordinator::new(InputLimits{max_history_entries:2,..InputLimits::default()}).unwrap();
 for value in [4.0,5.0,6.0]{input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",value)).unwrap();input.commit(&mut s).unwrap();}
 assert_eq!(input.history_len(),2);assert!(input.history_bytes()<=InputLimits::default().max_history_bytes);
 input.undo(&mut s).unwrap();input.undo(&mut s).unwrap();assert_eq!(s.live_overrides()[&key("a")],Value::scalar(4.0).unwrap());assert!(!input.undo(&mut s).unwrap());
 input.begin(&s,&[key("a")]).unwrap();let revision=s.revision();assert!(input.update(&mut s,set("b",5.0)).is_err());assert!(input.update(&mut s,set("a",100.0)).is_err());assert_eq!(s.revision(),revision);input.cancel(&mut s).unwrap();
}
#[test]fn mixed_some_none_restores_preexisting_value(){
 let mut s=session();s.set_live_override(s.revision(),key("a"),Value::scalar(4.5).unwrap()).unwrap();
 let mut input=InputCoordinator::default();input.begin(&s,&[key("a"),key("b")]).unwrap();
 input.update(&mut s,vec![(key("a"),None),(key("b"),Some(Value::scalar(6.0).unwrap()))]).unwrap();input.cancel(&mut s).unwrap();
 assert_eq!(s.live_overrides()[&key("a")],Value::scalar(4.5).unwrap());assert!(!s.live_overrides().contains_key(&key("b")));
}

#[test]fn imported_paused_intent_true_preserved(){
 let mut original=session();original.set_playing(original.revision(),true).unwrap();let checkpoint=original.checkpoint();
 let document=Document::decode(include_bytes!("../../../fixtures/render/cosines-runtime.json"),DecodeLimits::default()).unwrap();
 let mut s=Session::from_checkpoint(document,OperatorRegistry::standard().unwrap(),SessionConfig::default(),checkpoint.session.clone(),checkpoint.epoch,checkpoint).unwrap();
 assert!(!s.is_playing());assert!(s.playback_intent());let mut input=InputCoordinator::default();
 input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();assert!(!s.is_playing());assert!(s.playback_intent());
 input.cancel(&mut s).unwrap();assert!(!s.is_playing());assert!(s.playback_intent());assert!(s.live_overrides().is_empty());
 input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",5.0)).unwrap();input.commit(&mut s).unwrap();input.undo(&mut s).unwrap();
 assert!(!s.is_playing());assert!(s.playback_intent());
}
#[test]fn compensation_permission_admission_and_midgesture_permission_conflict(){
 use sim_runtime::{CommandKind,DocumentEdit};
 let mut s=session();let mut permissions=s.document().document().learner_permissions;permissions.return_to_guide=false;
 s.dispatch(s.snapshot().command(CommandKind::EditDocument{expected_document_revision:s.snapshot().document_revision(),edits:vec![DocumentEdit::SetPermissions{permissions}]})).unwrap();
 let revision=s.revision();let mut input=InputCoordinator::default();assert!(input.begin(&s,&[key("a")]).is_err());assert_eq!(s.revision(),revision);
 let mut s=session();let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();
 let mut permissions=s.document().document().learner_permissions;permissions.return_to_guide=false;
 s.dispatch(s.snapshot().command(CommandKind::EditDocument{expected_document_revision:s.snapshot().document_revision(),edits:vec![DocumentEdit::SetPermissions{permissions}]})).unwrap();
 let revision=s.revision();assert!(matches!(input.cancel(&mut s),Err(InputError::Conflict(_))));assert_eq!(s.revision(),revision);assert!(input.active());assert_eq!(s.live_overrides()[&key("a")],Value::scalar(4.0).unwrap());
}

fn configured_session(capacity:usize)->Session{Session::new(Document::decode(include_bytes!("../../../fixtures/render/cosines-runtime.json"),DecodeLimits::default()).unwrap(),OperatorRegistry::standard().unwrap(),SessionConfig{event_capacity:capacity,..SessionConfig::default()}).unwrap()}
#[test]fn ordered_observer_retention_zero_one_and_owned_commands(){
 for capacity in [0,1]{
  let mut s=configured_session(capacity);let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();
  for value in [4.0,5.0,6.0]{input.update(&mut s,set("a",value)).unwrap();}
  s.set_live_override(s.revision(),key("b"),Value::scalar(5.0).unwrap()).unwrap();s.set_live_override(s.revision(),key("b"),Value::scalar(6.0).unwrap()).unwrap();
  let before=s.checkpoint();assert!(matches!(input.update(&mut s,set("a",7.0)),Err(InputError::Conflict(_))));assert_eq!(s.checkpoint(),before);
 }
 let mut s=configured_session(1);let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();
 s.set_live_override(s.revision(),key("b"),Value::scalar(6.0).unwrap()).unwrap();input.update(&mut s,set("a",5.0)).unwrap();input.cancel(&mut s).unwrap();
 assert!(!s.live_overrides().contains_key(&key("a")));assert_eq!(s.live_overrides()[&key("b")],Value::scalar(6.0).unwrap());
}
#[test]fn redo_detects_touched_and_time_aba(){
 for time in [false,true]{
  let mut s=session();let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();input.commit(&mut s).unwrap();input.undo(&mut s).unwrap();
  if time{s.seek(s.revision(),TimeTicks::new(127008000000)).unwrap();s.seek(s.revision(),TimeTicks::ZERO).unwrap();}
  else{s.set_live_override(s.revision(),key("a"),Value::scalar(3.0).unwrap()).unwrap();s.return_to_guide(s.revision(),&[key("a")]).unwrap();}
  let before=s.checkpoint();assert!(matches!(input.redo(&mut s),Err(InputError::Conflict(_))));assert_eq!(s.checkpoint(),before);
 }
}
#[test]fn external_preserve_patch_on_unrelated_key_remains_allowed(){
 use sim_runtime::{CommandKind,OverridePlaybackPolicy};
 let mut s=session();let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();
 s.dispatch(s.snapshot().command(CommandKind::PatchLiveOverrides{entries:set("b",5.0),playback:OverridePlaybackPolicy::Preserve})).unwrap();
 input.update(&mut s,set("a",6.0)).unwrap();input.commit(&mut s).unwrap();input.undo(&mut s).unwrap();
 assert!(!s.live_overrides().contains_key(&key("a")));assert_eq!(s.live_overrides()[&key("b")],Value::scalar(5.0).unwrap());
}

#[test]fn terminal_abandon_is_nonwriting_and_preserves_history_conflicts(){
 for context in ["key","time","playback","document","retention"]{
  let mut s=if context=="retention"{configured_session(1)}else{session()};let mut input=InputCoordinator::default();
  input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();input.commit(&mut s).unwrap();
  input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",5.0)).unwrap();
  match context{
   "key"=>{s.set_live_override(s.revision(),key("a"),Value::scalar(3.7).unwrap()).unwrap();},
   "time"=>{s.seek(s.revision(),TimeTicks::new(127008000000)).unwrap();},
   "playback"=>{s.set_playing(s.revision(),true).unwrap();},
   "document"=>{let mut permissions=s.document().document().learner_permissions;permissions.playback=false;s.dispatch(s.snapshot().command(sim_runtime::CommandKind::EditDocument{expected_document_revision:s.snapshot().document_revision(),edits:vec![sim_runtime::DocumentEdit::SetPermissions{permissions}]})).unwrap();},
   _=>{s.set_live_override(s.revision(),key("b"),Value::scalar(5.0).unwrap()).unwrap();s.set_live_override(s.revision(),key("b"),Value::scalar(6.0).unwrap()).unwrap();},
  }
  assert!(input.cancel(&mut s).is_err());let before=s.checkpoint();let override_snapshot=s.live_overrides().clone();let history=input.history_len();let bytes=input.history_bytes();
  assert!(input.abandon(&s));assert!(!input.active());assert_eq!(s.checkpoint(),before);assert_eq!(history,input.history_len());assert_eq!(bytes,input.history_bytes());assert!(!input.abandon(&s));
  assert!(matches!(input.undo(&mut s),Err(InputError::Conflict(_))));assert_eq!(s.checkpoint(),before);
  input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",6.0)).unwrap();input.commit(&mut s).unwrap();input.undo(&mut s).unwrap();assert_eq!(s.live_overrides()[&key("a")],override_snapshot[&key("a")]);
  assert!(matches!(input.undo(&mut s),Err(InputError::Conflict(_))));
 }
}
#[test]fn abandon_permission_denial_unlocks_without_bypassing_admission(){
 let mut s=session();let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();
 let mut permissions=s.document().document().learner_permissions;permissions.return_to_guide=false;
 s.dispatch(s.snapshot().command(sim_runtime::CommandKind::EditDocument{expected_document_revision:s.snapshot().document_revision(),edits:vec![sim_runtime::DocumentEdit::SetPermissions{permissions}]})).unwrap();
 assert!(input.cancel(&mut s).is_err());let before=s.checkpoint();assert!(input.abandon(&s));assert!(!input.active());assert_eq!(s.checkpoint(),before);
 assert!(matches!(input.begin(&s,&[key("a")]),Err(InputError::Invalid(_))));assert!(!input.active());assert_eq!(s.checkpoint(),before);
}

#[test]fn terminal_abandon_after_missing_audit_zero_one_accepts_fresh_owned_interval(){
 for capacity in [0,1]{
  let mut s=configured_session(capacity);let mut input=InputCoordinator::default();input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",4.0)).unwrap();
  s.set_live_override(s.revision(),key("b"),Value::scalar(5.0).unwrap()).unwrap();s.set_live_override(s.revision(),key("b"),Value::scalar(6.0).unwrap()).unwrap();
  assert!(matches!(input.commit(&mut s),Err(InputError::Conflict(_))));let before=s.checkpoint();assert!(input.abandon(&s));assert_eq!(s.checkpoint(),before);
  input.begin(&s,&[key("a")]).unwrap();input.update(&mut s,set("a",5.0)).unwrap();input.cancel(&mut s).unwrap();assert!(!input.active());assert_eq!(s.live_overrides()[&key("a")],Value::scalar(4.0).unwrap());assert_eq!(s.live_overrides()[&key("b")],Value::scalar(6.0).unwrap());
 }
}
