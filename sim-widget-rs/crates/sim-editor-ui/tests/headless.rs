use sim_editor_ui::{*,projection::{RuntimeUiState,UiRole},language::{CatalogRegistry,LocaleId,Catalog,DecimalSeparator},intents::*,transport::{InputTransport,InputOwner}};
use sim_model::*;
use sim_runtime::{Session,SessionConfig,CommandKind};
use sim_render::input::InputCoordinator;
fn session()->Session{Session::new(Document::decode(include_bytes!("../../../fixtures/render/cosines-runtime.json"),DecodeLimits::default()).unwrap(),sim_math::OperatorRegistry::standard().unwrap(),SessionConfig::default()).unwrap()}
fn projection<'a>(s:&'a Session,f:&'a sim_runtime::FrameSnapshot,input:&InputCoordinator)->EditorProjection<'a>{EditorProjection::from_runtime(f,s.document().document(),RuntimeUiState{role:UiRole::Reader,playing:s.is_playing(),playback_intent:s.playback_intent(),can_undo:input.can_undo(),can_redo:input.can_redo()}).unwrap()}
fn raw(events:Vec<egui::Event>)->egui::RawInput{egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(1600.,1100.))),events,..Default::default()}}
fn draw(ctx:&egui::Context,editor:&mut EditorUi,panel:Panel,p:&EditorProjection<'_>,registry:&CatalogRegistry,events:Vec<egui::Event>)->(UiOutput,usize){let mut out=UiOutput::default();let mut full=ctx.run_ui(raw(events),|root|{egui::CentralPanel::default().show(root,|ui|{editor.draw_panel(ui,panel,p,&Default::default(),registry.language(&editor.view.locale).unwrap(),&mut out);});});let n=full.shapes.len();full.textures_delta.clear();(out,n)}
struct Owner(InputCoordinator);
impl InputOwner for Owner{
 fn begin(&mut self,s:&Session,role:UiRole,keys:&[ParameterId])->std::result::Result<(),String>{assert!(matches!(role,UiRole::Reader));self.0.begin(s,keys).map_err(|e|e.to_string())}
 fn update(&mut self,s:&mut Session,entries:Vec<(ParameterId,Option<Value>)>)->std::result::Result<bool,String>{self.0.update(s,entries).map_err(|e|e.to_string())}
 fn commit(&mut self,s:&mut Session)->std::result::Result<bool,String>{self.0.commit(s).map_err(|e|e.to_string())}
 fn cancel(&mut self,s:&mut Session)->std::result::Result<bool,String>{self.0.cancel(s).map_err(|e|e.to_string())}
 fn abandon(&mut self,s:&Session){self.0.abandon(s);}
}
fn gesture(s:&Session,phase:GesturePhase,value:Option<Value>)->GestureIntent{GestureIntent{role:UiRole::Reader,source:InputSource::Keyboard,stamp:sim_editor_ui::projection::ProjectionStamp::capture(&s.snapshot()),gesture:42,pointer:2,phase,entries:vec![(ParameterId::new("a").unwrap(),value)]}}
#[test]fn all_eleven_panels_and_six_settings_render_actual_egui(){
 let s=session();let frame=s.snapshot();let p=projection(&s,&frame,&Default::default());let registry=CatalogRegistry::default();let ctx=egui::Context::default();let mut editor=EditorUi::default();
 for panel in Panel::ALL{let mut count=0;for _ in 0..3{let(out,n)=draw(&ctx,&mut editor,panel,&p,&registry,vec![]);assert_eq!(out.panels_drawn,vec![panel]);count=n;}assert!(count>5,"{panel:?} shapes={count}");}
 for page in 0..6{editor.preferences.panel.selected=page;let(_,n)=draw(&ctx,&mut editor,Panel::Settings,&p,&registry,vec![]);assert!(n>5);}
}
#[test]fn whole_docked_shell_and_floating_layout_roundtrip(){
 let s=session();let frame=s.snapshot();let p=projection(&s,&frame,&Default::default());let ctx=egui::Context::default();let registry=CatalogRegistry::default();let mut editor=EditorUi::default();let mut seen=0;
 for _ in 0..3{let mut f=ctx.run_ui(raw(vec![]),|root|{let out=editor.show(root,&p,&Default::default(),&registry);seen=out.panels_drawn.len();});assert!(!f.shapes.is_empty());f.textures_delta.clear();}assert!(seen>=5);
 sim_editor_ui::dock::open_panel(&mut editor.dock,Panel::MaterialLibrary);let blob=sim_editor_ui::dock::layout_blob(&editor.dock,&ctx).unwrap();let restored=sim_editor_ui::dock::restore_layout(&blob).unwrap();assert!(sim_editor_ui::dock::valid_layout(&restored));assert!(restored.find_tab(&Panel::MaterialLibrary).is_some());
}
#[test]fn third_catalog_registration_and_numeric_policy_are_executable(){
 let mut r=CatalogRegistry::default();let id=LocaleId::new("third").unwrap();let mut messages=r.english_messages();for value in messages.values_mut(){*value=format!("第三 {value}");}r.register(id.clone(),Catalog{name:"Third".into(),messages:messages.clone(),decimal:DecimalSeparator::Comma}).unwrap();assert!(r.language(&id).unwrap().text("Play").starts_with("第三"));assert_eq!(r.language(&id).unwrap().parse("3,125").unwrap(),3.125);
 messages.remove("Play");assert!(r.register(LocaleId::new("broken").unwrap(),Catalog{name:"Broken".into(),messages,decimal:DecimalSeparator::Dot}).is_err());
 let s=session();let frame=s.snapshot();let p=projection(&s,&frame,&Default::default());let mut editor=EditorUi::default();editor.view.locale=id;let ctx=egui::Context::default();let(_,n)=draw(&ctx,&mut editor,Panel::Inspector,&p,&r,vec![]);assert!(n>5);
}
#[test]fn runtime_controls_preserve_canonical_thirteen_twentyfive_thirtyseven(){
 let mut s=session();for(ticks,expected)in[(0,13.),(127008000000,25.),(254016000000,37.)]{let f=s.snapshot();let mut out=UiOutput::default();let p=projection(&s,&f,&Default::default());out.existing(&p,CommandKind::Seek{time:TimeTicks::new(ticks)});for i in out.intents{if let UiIntent::Existing{command,..}=i{s.dispatch(s.snapshot().command(command)).unwrap();}}assert!((s.snapshot().truth()[&ValueId::new("c2").unwrap()].as_scalar().unwrap()-expected).abs()<1e-9);}
}
#[test]fn one_shot_gestures_use_one_authoritative_input_history_and_exact_guide(){
 let mut s=session();let mut owner=Owner(Default::default());let mut bridge=InputTransport::default();for phase in[GesturePhase::BeginPending,GesturePhase::Changed,GesturePhase::Committed]{let event=gesture(&s,phase,Some(Value::scalar(4.).unwrap()));bridge.handle(&mut s,&mut owner,&event).unwrap();}
 assert!(owner.0.can_undo());assert!(!owner.0.active());assert!(!bridge.active());owner.0.undo(&mut s).unwrap();assert!(!s.live_overrides().contains_key(&ParameterId::new("a").unwrap()));owner.0.redo(&mut s).unwrap();assert_eq!(s.live_overrides()[&ParameterId::new("a").unwrap()].as_scalar().unwrap(),4.);
}
#[test]fn pending_rejection_rebases_playing_time_and_cancel_restores_absence_intent(){
 let mut s=session();s.dispatch(s.snapshot().command(CommandKind::SetPlaying{playing:true})).unwrap();let mut owner=Owner(Default::default());let mut bridge=InputTransport::default();let e=gesture(&s,GesturePhase::BeginPending,Some(Value::scalar(3.).unwrap()));bridge.handle(&mut s,&mut owner,&e).unwrap();
 let e=gesture(&s,GesturePhase::Changed,Some(Value::scalar(-1.).unwrap()));assert!(bridge.handle(&mut s,&mut owner,&e).is_err());assert!(!owner.0.active());assert!(s.is_playing());
 s.seek(s.revision(),TimeTicks::new(1000)).unwrap();let e=gesture(&s,GesturePhase::Changed,Some(Value::scalar(4.).unwrap()));assert!(bridge.handle(&mut s,&mut owner,&e).unwrap());assert!(!s.is_playing());let e=gesture(&s,GesturePhase::Cancelled,None);bridge.handle(&mut s,&mut owner,&e).unwrap();assert!(s.is_playing());assert!(s.playback_intent());assert_eq!(s.snapshot().time(),TimeTicks::new(1000));assert!(!s.live_overrides().contains_key(&ParameterId::new("a").unwrap()));
}
#[test]fn pending_touched_aba_and_terminal_conflict_release_without_writing_external_truth(){
 let mut s=session();let mut owner=Owner(Default::default());let mut bridge=InputTransport::default();let e=gesture(&s,GesturePhase::BeginPending,Some(Value::scalar(3.).unwrap()));bridge.handle(&mut s,&mut owner,&e).unwrap();
 for x in[4.,3.]{s.dispatch(s.snapshot().command(CommandKind::SetLiveOverride{parameter:ParameterId::new("a").unwrap(),value:Value::scalar(x).unwrap()})).unwrap();}
 let e=gesture(&s,GesturePhase::Changed,Some(Value::scalar(5.).unwrap()));assert!(bridge.handle(&mut s,&mut owner,&e).is_err());bridge.cancel(&mut s,&mut owner).unwrap();assert!(!bridge.active());
 let e=gesture(&s,GesturePhase::BeginPending,Some(Value::scalar(3.).unwrap()));bridge.handle(&mut s,&mut owner,&e).unwrap();let e=gesture(&s,GesturePhase::Changed,Some(Value::scalar(5.).unwrap()));bridge.handle(&mut s,&mut owner,&e).unwrap();s.dispatch(s.snapshot().command(CommandKind::SetLiveOverride{parameter:ParameterId::new("a").unwrap(),value:Value::scalar(3.7).unwrap()})).unwrap();let before=s.revision();let e=gesture(&s,GesturePhase::Cancelled,None);assert!(bridge.handle(&mut s,&mut owner,&e).is_err());assert!(!bridge.active());assert!(!owner.0.active());assert_eq!(s.revision(),before);assert_eq!(s.live_overrides()[&ParameterId::new("a").unwrap()].as_scalar().unwrap(),3.7);
}
