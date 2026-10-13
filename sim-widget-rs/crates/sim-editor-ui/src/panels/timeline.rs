//! Warpbro timeline transport, layers, keys, clips and effect menus over the runtime's exact tracks.
use std::collections::BTreeSet;
use sim_model::{ParameterId,TimeTicks,TICKS_PER_SECOND};
use crate::{language::Language,intents::*,projection::EditorProjection,accessibility::{record,Role}};
#[derive(Default)]pub struct Timeline{pub view:egui_track_timeline::TimelineView,pub config:egui_track_timeline::TimelineConfig,pub fps:i64,pub auto_key:bool,pub looped:bool,pub hide_shy:bool,pub expanded:bool,pub selection_playback:bool,pub preview_cache:bool,pub selected_keys:BTreeSet<(ParameterId,TimeTicks)>}
impl Timeline{
 pub fn show(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,out:&mut UiOutput){
  if self.fps==0{self.fps=24;}let fps=self.fps;
  ui.horizontal_wrapped(|ui|{
   for(label,forward)in[("Start",false),("End",true)]{if ui.button(l.text(label)).clicked(){let time=if forward{p.document.tracks.iter().flat_map(|t|t.keys.iter()).map(|k|k.time).max().unwrap_or(TimeTicks::ZERO)}else{p.document.tracks.iter().flat_map(|t|t.keys.iter()).map(|k|k.time).min().unwrap_or(TimeTicks::ZERO)};out.existing(p,sim_runtime::CommandKind::Seek{time});}}
   let label=if p.playing{"Pause"}else{"Play"};let r=ui.button(l.text(label));record(out,"transport:play".into(),l.text(label).into_owned(),Role::Button,&r,None);if r.clicked(){out.existing(p,sim_runtime::CommandKind::SetPlaying{playing:!p.playing});}
   for(label,next)in[("Previous key",false),("Next key",true)]{if ui.button(l.text(label)).clicked(){let now=p.time();let candidates=p.document.tracks.iter().flat_map(|t|t.keys.iter()).map(|k|k.time);let time=if next{candidates.filter(|t|*t>now).min()}else{candidates.filter(|t|*t<now).max()};if let Some(time)=time{out.existing(p,sim_runtime::CommandKind::Seek{time});}}}
   let mut frame=p.time().to_rational_floor(1,fps).unwrap_or(0);ui.label(l.text("Frame"));let r=ui.add(egui::DragValue::new(&mut frame));if r.changed(){match TimeTicks::from_rational(frame,1,fps){Ok(time)=>out.existing(p,sim_runtime::CommandKind::Seek{time}),Err(_)=>out.errors.push("draft.invalid".into())}}
   ui.label(l.text("FPS"));ui.add(egui::DragValue::new(&mut self.fps).range(1..=240));
   if ui.checkbox(&mut self.auto_key,l.text("Auto Key")).changed(){out.author(p,caps,AuthorAction::AutoKey,vec![],None,serde_json::json!({"enabled":self.auto_key}));}
   ui.checkbox(&mut self.looped,l.text("Loop"));ui.checkbox(&mut self.hide_shy,l.text("Hide shy"));ui.checkbox(&mut self.expanded,l.text("Expand components"));
   ui.checkbox(&mut self.selection_playback,l.text("Selection playback"));ui.checkbox(&mut self.preview_cache,l.text("Preview cache"));ui.checkbox(&mut self.config.snap_keys_to_frames,l.text("Snap"));
   ui.add(egui::Slider::new(&mut self.view.zoom,0.05..=20.).text(l.text("Zoom")));
   ui.menu_button(l.text("Interpolation"),|ui|{for name in["Linear","Smooth","Hold"]{if ui.button(l.text(name)).clicked(){out.author(p,caps,AuthorAction::Interpolation,vec![],None,serde_json::json!({"mode":name,"keys":self.selected_keys.iter().collect::<Vec<_>>()}));ui.close();}}});
   ui.menu_button(l.text("Effects"),|ui|{for(label,kind)in[("Simplify",AuthorAction::Simplify),("Smooth",AuthorAction::Smooth),("Euler filter",AuthorAction::EulerFilter),("Value curves",AuthorAction::Curves),("Speed curves",AuthorAction::Curves)]{if ui.button(l.text(label)).clicked(){out.author(p,caps,kind,vec![],None,serde_json::json!({"effect":label,"keys":self.selected_keys.iter().collect::<Vec<_>>()}));ui.close();}}});
  });
  let mut addresses=Vec::new();let tracks=p.document.tracks.iter().map(|t|{
   let keys=t.keys.iter().map(|k|{let frame=k.time.ticks()as f64/TICKS_PER_SECOND as f64*fps as f64;addresses.push((t.parameter.clone(),k.time,frame.to_bits()));egui_track_timeline::Keyframe::new(frame,k.value.get()).selected(self.selected_keys.contains(&(t.parameter.clone(),k.time)))}).collect();
   egui_track_timeline::Track::new(t.parameter.as_str(),vec![]).with_lanes(vec![egui_track_timeline::PropLane::new(t.parameter.as_str(),keys)]).expanded(true)
  }).collect();
  let mut model=egui_track_timeline::TimelineModel::new(fps as f32,tracks);model.playhead=p.time().to_rational_floor(1,fps).unwrap_or(0);
  let widget=egui_track_timeline::TrackTimeline::new(self.config);
  let response=egui::ScrollArea::both().show(ui,|ui|widget.show(ui,&mut self.view,&model)).inner;
  let resolve=|keys:Vec<egui_track_timeline::KeyPos>|->Result<Vec<(ParameterId,TimeTicks)>,String>{keys.into_iter().map(|key|{let t=p.document.tracks.get(key.track).ok_or("draft.stale")?;let found:Vec<_>=addresses.iter().filter(|(id,_,bits)|id==&t.parameter&&*bits==key.frame.to_bits()).collect();if key.lane!=0||found.len()!=1{return Err("draft.stale".into());}Ok((found[0].0.clone(),found[0].1))}).collect()};
  for action in response.actions{use egui_track_timeline::TimelineAction as A;match action{
   A::Seek{frame}=>match TimeTicks::from_rational(frame,1,fps){Ok(time)=>out.existing(p,sim_runtime::CommandKind::Seek{time}),Err(_)=>out.errors.push("draft.invalid".into())},
   A::SelectKeys{keys,add}=>match resolve(keys){Ok(keys)=>{if !add{self.selected_keys.clear();}self.selected_keys.extend(keys);},Err(e)=>out.errors.push(e)},
   A::ClearKeySelection=>self.selected_keys.clear(),
   A::MoveKeys{keys,delta}=>match resolve(keys){Ok(keys)=>out.author(p,caps,AuthorAction::MoveKeys,vec![],None,serde_json::json!({"keys":keys,"delta_frames":delta,"fps":fps})),Err(e)=>out.errors.push(e)},
   A::DeleteKeys{keys}=>match resolve(keys){Ok(keys)=>out.author(p,caps,AuthorAction::DeleteKeys,vec![],None,serde_json::json!({"keys":keys})),Err(e)=>out.errors.push(e)},
   A::AddKey{track,lane,frame}=>{if let Some(t)=p.document.tracks.get(track){out.author(p,caps,AuthorAction::Key,vec![],Some(t.parameter.clone()),serde_json::json!({"frame":frame,"fps":fps,"lane":lane}));}},
   other=>{let(kind,payload)=match other{A::SetWorkArea{start,end}=>(AuthorAction::WorkArea,serde_json::json!({"start_frame":start,"end_exclusive_frame":end,"fps":fps})),A::MoveClip{id,new_start,new_track}=>(AuthorAction::MoveClip,serde_json::json!({"id":id,"start_frame":new_start,"track":new_track,"fps":fps})),A::TrimStart{id,delta}|A::TrimEnd{id,delta}=>(AuthorAction::Trim,serde_json::json!({"id":id,"delta_frames":delta,"fps":fps})),A::Slide{id,delta}=>(AuthorAction::Slide,serde_json::json!({"id":id,"delta_frames":delta,"fps":fps})),_=>continue};out.author(p,caps,kind,vec![],None,payload);}
  }}
 }
}
