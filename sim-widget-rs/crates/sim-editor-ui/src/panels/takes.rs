//! Warpbro Takes panel: filter, multi/range selection, pin, bind, rename, clip edits and bake requests.
use std::collections::BTreeSet;
use crate::{language::Language,intents::*,projection::EditorProjection};
pub struct Takes{pub filter:String,pub pinned_only:bool,pub selection:BTreeSet<String>,pub anchor:Option<String>,pub rename:Option<(String,String)>,pub trim:(f64,f64),pub trim_of:Option<String>,pub speed:f64,pub effect_amount:f64,pub bake_transform:bool,pub bake_focus:bool,pub bake_zoom:bool,pub bake_aperture:bool,pub tolerance:f64}
impl Default for Takes{fn default()->Self{Self{filter:String::new(),pinned_only:false,selection:Default::default(),anchor:None,rename:None,trim:(0.,0.),trim_of:None,speed:1.,effect_amount:0.01,bake_transform:true,bake_focus:true,bake_zoom:true,bake_aperture:true,tolerance:0.0001}}}
impl Takes{
 pub fn show(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,objects:&[sim_model::ObjectId],out:&mut UiOutput){
  self.selection.retain(|id|p.takes.iter().any(|t|&t.id==id));let selected:Vec<_>=self.selection.iter().cloned().collect();let any=!selected.is_empty();
  ui.horizontal_wrapped(|ui|{ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(l.text("Search")).desired_width(110.));ui.checkbox(&mut self.pinned_only,l.text("Pinned only"));
   for(label,action,op)in[("Record",AuthorAction::RecordTake,"record"),("Capture camera",AuthorAction::RecordTake,"camera"),("Capture objects",AuthorAction::RecordTake,"pose"),("Pin",AuthorAction::DeriveTake,"pin"),("Duplicate",AuthorAction::Duplicate,"duplicate"),("Delete",AuthorAction::Delete,"delete")]{if ui.add_enabled(any||matches!(action,AuthorAction::RecordTake),egui::Button::new(l.text(label))).clicked(){out.author(p,caps,action,objects.to_vec(),None,serde_json::json!({"takes":selected,"operation":op}));}}
   for(label,kind,op)in[("Copy",ServiceKind::Clipboard,"copy-takes"),("Paste",ServiceKind::Clipboard,"paste-takes"),("Export take…",ServiceKind::TakePackage,"export"),("Import take…",ServiceKind::TakePackage,"import")]{if ui.add_enabled(any||matches!(kind,ServiceKind::Clipboard)||op=="import",egui::Button::new(l.text(label))).clicked(){out.service(p,caps,kind,op,serde_json::json!({"takes":selected}));}}
  });
  ui.separator();let needle=self.filter.to_lowercase();let visible:Vec<_>=p.takes.iter().rev().filter(|t|(!self.pinned_only||t.pinned)&&t.name.to_lowercase().contains(&needle)).collect();
  if p.takes.is_empty(){ui.label(l.text("No takes recorded"));}
  egui::ScrollArea::vertical().max_height((ui.available_height()-220.).max(80.)).show(ui,|ui|{
   for take in &visible{ui.horizontal(|ui|{
    ui.label(&take.kind);if ui.selectable_label(take.pinned,l.text("Pin")).clicked(){out.author(p,caps,AuthorAction::DeriveTake,vec![],None,serde_json::json!({"take":take.id,"pin":!take.pinned}));}
    let bound=!take.targets.is_empty();if ui.selectable_label(bound,l.text(if bound{"Unbind"}else{"Bind"})).clicked(){out.author(p,caps,if bound{AuthorAction::UnbindTake}else{AuthorAction::BindTake},objects.to_vec(),None,serde_json::json!({"take":take.id}));}
    if ui.button(l.text("Play")).clicked(){out.author(p,caps,AuthorAction::BindTake,objects.to_vec(),None,serde_json::json!({"take":take.id,"play":true}));}
    let editing=self.rename.as_ref().is_some_and(|(id,_)|id==&take.id);
    if editing{if let Some((id,name))=&mut self.rename{let r=ui.add(egui::TextEdit::singleline(name).desired_width(130.));if r.lost_focus()||r.has_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter)){out.author(p,caps,AuthorAction::Rename,vec![],None,serde_json::json!({"take":id,"name":name}));self.rename=None;}else if r.has_focus()&&ui.input(|i|i.key_pressed(egui::Key::Escape)){self.rename=None;}}}
    else{let r=ui.selectable_label(self.selection.contains(&take.id),&take.name);if r.double_clicked(){self.rename=Some((take.id.clone(),take.name.clone()));}else if r.clicked(){let m=ui.input(|i|i.modifiers);if m.shift{if let Some(anchor)=&self.anchor{if let(Some(a),Some(z))=(visible.iter().position(|t|&t.id==anchor),visible.iter().position(|t|t.id==take.id)){self.selection.extend(visible[a.min(z)..=a.max(z)].iter().map(|t|t.id.clone()));}}}else{if !m.command{self.selection.clear();}if !self.selection.insert(take.id.clone()){self.selection.remove(&take.id);}self.anchor=Some(take.id.clone());}}}
    ui.weak(format!("{} · {} · {}",take.start,take.end,take.sample_count));
   });}
  });
  ui.separator();
  if self.selection.len()==1{if let Some(take)=p.takes.iter().find(|t|self.selection.contains(&t.id)){
   let duration=(take.end.ticks()-take.start.ticks())as f64/sim_model::TICKS_PER_SECOND as f64;if self.trim_of.as_deref()!=Some(&take.id){self.trim=(0.,duration);self.trim_of=Some(take.id.clone());}
   egui::Grid::new("take-edit").num_columns(3).show(ui,|ui|{
    ui.label(l.text("Start"));let mut start=take.start.ticks();if ui.add(egui::DragValue::new(&mut start)).changed(){out.author(p,caps,AuthorAction::DeriveTake,vec![],None,serde_json::json!({"take":take.id,"start":sim_model::TimeTicks::new(start)}));}
    let mut timing=take.timing.clone();egui::ComboBox::from_id_salt("take-timing").selected_text(l.text(&timing)).show_ui(ui,|ui|{for mode in["Keep speed","Fit work area"]{ui.selectable_value(&mut timing,mode.into(),l.text(mode));}});if timing!=take.timing{out.author(p,caps,AuthorAction::DeriveTake,vec![],None,serde_json::json!({"take":take.id,"timing":timing}));}ui.end_row();
    ui.label(l.text("Trim"));ui.horizontal(|ui|{ui.add(egui::DragValue::new(&mut self.trim.0).range(0.0..=duration));ui.add(egui::DragValue::new(&mut self.trim.1).range(0.0..=duration));});if ui.button(l.text("Trim")).clicked(){out.author(p,caps,AuthorAction::DeriveTake,vec![],None,serde_json::json!({"take":take.id,"trim_seconds":self.trim}));}ui.end_row();
    ui.label(l.text("Speed"));ui.add(egui::DragValue::new(&mut self.speed).range(0.05..=20.));if ui.button(l.text("Derive")).clicked(){out.author(p,caps,AuthorAction::DeriveTake,vec![],None,serde_json::json!({"take":take.id,"speed":self.speed}));}ui.end_row();
    ui.label(l.text("Effects"));ui.add(egui::DragValue::new(&mut self.effect_amount).range(0.0..=1.));ui.menu_button(l.text("Apply"),|ui|{for effect in["Simplify","Smooth","Euler filter"]{if ui.button(l.text(effect)).clicked(){out.author(p,caps,AuthorAction::DeriveTake,vec![],None,serde_json::json!({"take":take.id,"effect":effect,"amount":self.effect_amount}));ui.close();}}});ui.end_row();
    ui.label(l.text("Split"));ui.label("");if ui.button(l.text("Split")).clicked(){out.author(p,caps,AuthorAction::SplitTake,vec![],None,serde_json::json!({"take":take.id,"time":p.time()}));}ui.end_row();
   });
  }}else{ui.weak(l.text("Select one take to edit it."));}
  ui.horizontal_wrapped(|ui|{ui.label(l.text("Bake channels"));ui.checkbox(&mut self.bake_transform,l.text("Transform"));ui.checkbox(&mut self.bake_focus,l.text("Focus"));ui.checkbox(&mut self.bake_zoom,l.text("Zoom"));ui.checkbox(&mut self.bake_aperture,"f-number");ui.add(egui::DragValue::new(&mut self.tolerance).range(0.0..=1.).speed(0.0001));});
  if ui.add_enabled(self.selection.len()==1,egui::Button::new(l.text("Bake channels"))).clicked(){out.author(p,caps,AuthorAction::BakeTake,objects.to_vec(),None,serde_json::json!({"takes":self.selection,"transform":self.bake_transform,"focus":self.bake_focus,"zoom":self.bake_zoom,"aperture":self.bake_aperture,"tolerance":self.tolerance}));}
 }
}
