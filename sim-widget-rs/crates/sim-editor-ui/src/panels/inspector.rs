//! Attribute Editor adapted from Warpbro WorldGridHooks. Exact canonical f64 stays in the runtime.
use std::collections::{BTreeMap,HashSet};
use sim_model::{ParameterId,Value};
use crate::{language::Language,intents::*,projection::{EditorProjection,PropertyProjection},accessibility::{record,Role}};
#[derive(Clone,Debug)]pub struct Draft{pub text:String,pub source:Value,pub dirty:bool,pub decimal:crate::language::DecimalSeparator,pub stamp:crate::projection::ProjectionStamp,pub stale:bool,pub error:Option<String>,pub widget:egui::Id,pub pending:bool,pub submitted:Option<Value>}
#[derive(Default)]pub struct Inspector{pub grid:egui_attr_grid::AttrGridState,pub search:String,pub drafts:BTreeMap<ParameterId,Draft>,pub metadata:String,pub custom_name:String,pub custom_value:f64}
impl Inspector{
 pub fn show(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,selection:&[sim_model::ObjectId],out:&mut UiOutput){
  ui.horizontal(|ui|{ui.label(l.text("Search properties"));ui.text_edit_singleline(&mut self.search);});
  if selection.is_empty(){ui.label(l.text("Select an object to edit its properties."));}
  else{for id in selection{if let Some(object)=p.document.objects.iter().find(|x|&x.id==id){ui.heading(&object.label);}}}
  let query=self.search.to_lowercase();let props:Vec<_>=p.properties().filter(|x|x.parameter.id.as_str().to_lowercase().contains(&query)).collect();
  let mut fields:Vec<_>=props.iter().enumerate().map(|(i,x)|{let mut field=egui_attr_grid::AttrField::new(x.parameter.id.as_str(),egui_attr_grid::AttrValue::Label(format_value(l,x.value)));field.order=i as f32;field.hint=Some(format!("{:?} · {:?}",x.parameter.port.shape,x.parameter.port.unit));field}).collect();
  egui::ScrollArea::vertical().show(ui,|ui|{
   egui::CollapsingHeader::new(l.text("Properties")).default_open(true).show(ui,|ui|{
    let mut hooks=RuntimeHooks{locale:l,projection:p,capabilities:caps,props:&props,drafts:&mut self.drafts,out};
    let config=egui_attr_grid::grid_config(Default::default());let _=egui_attr_grid::render_grid_with_config(ui,&mut fields,&mut self.grid,&HashSet::new(),&config,&mut hooks);
   });
   egui::CollapsingHeader::new(l.text("Derived values")).show(ui,|ui|{for(id,value)in p.frame.truth(){if !p.document.parameters.iter().any(|x|&x.value_id==id){ui.horizontal(|ui|{ui.label(id.as_str());ui.monospace(format_value(l,value));});}}});
   egui::CollapsingHeader::new(l.text("Custom numeric attribute")).show(ui,|ui|{ui.horizontal(|ui|{ui.label(l.text("Attribute name"));ui.text_edit_singleline(&mut self.custom_name);});ui.add(egui::DragValue::new(&mut self.custom_value));if ui.button(l.text("Add attribute")).clicked(){out.author(p,caps,AuthorAction::AddAttribute,selection.to_vec(),None,serde_json::json!({"name":self.custom_name,"value":self.custom_value}));}});
   egui::CollapsingHeader::new(l.text("Metadata")).show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.metadata).code_editor().desired_rows(7));if ui.button(l.text("Apply metadata")).clicked(){out.author(p,caps,AuthorAction::SetMetadata,selection.to_vec(),None,serde_json::json!({"text":self.metadata}));}});
  });
 }
 /// Close an unwritten editor focus when it disappeared or moved to another widget.
 pub fn retire_focus(&mut self,ctx:&egui::Context,p:&EditorProjection<'_>,out:&mut UiOutput){
  let focus=ctx.memory(|m|m.focused());
  for(id,d)in &mut self.drafts{if d.pending&&focus!=Some(d.widget){d.pending=false;out.gesture(p,d.widget.value(),InputSource::Keyboard,GesturePhase::Cancelled,vec![(id.clone(),None)]);}}
 }
 /// A proposed draft becomes clean only after the host publishes matching authoritative truth.
 pub fn acknowledge(&mut self,feedback:&crate::transport::GestureFeedback,published:Option<&EditorProjection<'_>>,l:Language<'_>){
  for(id,d)in &mut self.drafts{
   if d.widget.value()!=feedback.event.gesture{continue;}
   if matches!(feedback.event.phase,GesturePhase::Committed|GesturePhase::Cancelled){d.pending=false;}
   match &feedback.result{
    Err(error)=>{d.error=Some(error.clone());d.submitted=None;if d.dirty{d.stale|=error=="draft.stale";}},
    Ok(_) if feedback.event.phase==GesturePhase::Changed=>{
     let Some(p)=published else{d.error=Some("draft.unpublished".into());continue;};
     let accepted=p.properties().find(|x|&x.parameter.id==id);
     if let Some(prop)=accepted{
      let candidate=feedback.event.entries.iter().find(|(key,_)|key==id).and_then(|(_,v)|v.as_ref());
      if candidate==Some(prop.value){d.source=prop.value.clone();d.dirty=false;d.stale=false;d.submitted=None;d.error=None;d.decimal=l.decimal();d.stamp=p.stamp();if let Value::Scalar(v)=prop.value{d.text=l.number(v.get(),prop.parameter.metadata.precision.unwrap_or(8)as usize);}}
      else{d.error=Some("draft.unpublished".into());}
     }else{d.error=Some("draft.stale".into());}
    },
    _=>{}
   }
  }
 }
}
struct RuntimeHooks<'a,'b>{locale:Language<'b>,projection:&'b EditorProjection<'a>,capabilities:&'b PlatformCapabilities,props:&'b[PropertyProjection<'a>],drafts:&'b mut BTreeMap<ParameterId,Draft>,out:&'b mut UiOutput}
impl RuntimeHooks<'_,'_>{
 fn property(&self,key:&str)->Option<PropertyProjection<'_>>{self.props.iter().copied().find(|x|x.parameter.id.as_str()==key)}
 fn proposed(&mut self,id:ParameterId,value:Value,gesture:u64,phase:GesturePhase){self.out.gesture(self.projection,gesture,InputSource::WidgetPointer,phase,vec![(id,Some(value))]);}
}
impl egui_attr_grid::AttrGridHooks for RuntimeHooks<'_,'_>{
 fn disabled(&self,f:&egui_attr_grid::AttrField)->Option<String>{self.property(&f.key).and_then(|p|(p.parameter.readonly||p.parameter.metadata.locked||(matches!(self.projection.role,crate::projection::UiRole::Reader)&&!p.parameter.metadata.learner_exposed)).then(||self.locale.text("Locked").into_owned()))}
 fn anim(&self,f:&egui_attr_grid::AttrField)->Option<egui_attr_grid::AnimState>{self.property(&f.key).map(|p|{let track=self.projection.document.tracks.iter().find(|t|t.parameter==p.parameter.id);egui_attr_grid::AnimState{animated:track.is_some(),keyed:track.is_some_and(|t|t.keys.iter().any(|k|k.time==self.projection.time()))}})}
 fn anim_intent(&mut self,f:&egui_attr_grid::AttrField,intent:egui_attr_grid::AnimIntent){if let Some(p)=self.property(&f.key){let id=p.parameter.id.clone();self.out.author(self.projection,self.capabilities,AuthorAction::Key,vec![],Some(id),serde_json::json!({"intent":format!("{intent:?}"),"time":self.projection.time()}));}}
 fn context_menu(&mut self,ui:&mut egui::Ui,f:&egui_attr_grid::AttrField){if let Some(p)=self.property(&f.key){let id=p.parameter.id.clone();let default=p.parameter.default.clone();for label in["Reset to guide","Reset to default","Edit expression","Value curves","Key"]{if ui.button(self.locale.text(label)).clicked(){match label{"Reset to guide"=>self.out.one_shot(self.projection,ui.id().value(),vec![(id.clone(),None)]),"Reset to default"=>self.out.one_shot(self.projection,ui.id().value(),vec![(id.clone(),Some(default.clone()))]),_=>self.out.author(self.projection,self.capabilities,match label{"Key"=>AuthorAction::Key,"Value curves"=>AuthorAction::Curves,_=>AuthorAction::EditExpression},vec![],Some(id.clone()),serde_json::json!({}))}ui.close();}}}}
 fn editor(&mut self,ui:&mut egui::Ui,f:&mut egui_attr_grid::AttrField,_mixed:bool,_layout:&egui_attr_grid::ValueEditorLayout,_extra:&egui_attr_grid::EditorCtx)->Option<bool>{
  let prop=self.property(&f.key)?;let parameter=prop.parameter.clone();let value=prop.value.clone();let id=parameter.id.clone();let l=self.locale;
  let response_id=ui.id().with(("canonical",&f.key));
  match &value{
   Value::Scalar(v)=>{ui.horizontal(|ui|{
    let canonical=v.get();
    if let Some(range)=&parameter.range{
     let mut x=canonical;let rail=ui.add(egui_slider::SlimSlider::new(&mut x,range.min.get()..=range.max.get()).value_field(false).width((ui.available_width()-110.).max(20.)).defer_reset());
     let gesture=ui.id().with(("rail",&f.key)).value();
     if rail.drag_started(){self.proposed(id.clone(),value.clone(),gesture,GesturePhase::BeginPending);}
     if rail.changed(){if let Ok(v)=Value::scalar(x){if rail.dragged(){self.proposed(id.clone(),v,gesture,GesturePhase::Changed);}else{self.out.one_shot(self.projection,gesture,vec![(id.clone(),Some(v))]);}}}
     if rail.drag_stopped(){self.out.gesture(self.projection,gesture,InputSource::WidgetPointer,GesturePhase::Committed,vec![(id.clone(),None)]);}
     if rail.dragged()&&ui.input(|i|i.key_pressed(egui::Key::Escape)){self.out.gesture(self.projection,gesture,InputSource::WidgetPointer,GesturePhase::Cancelled,vec![(id.clone(),None)]);}
    }
    let d=self.drafts.entry(id.clone()).or_insert_with(||Draft{text:l.number(canonical,parameter.metadata.precision.unwrap_or(8)as usize),source:value.clone(),dirty:false,decimal:l.decimal(),stamp:self.projection.stamp(),stale:false,error:None,widget:response_id,pending:false,submitted:None});
    if !d.dirty&&d.decimal!=l.decimal(){d.text=l.number(canonical,parameter.metadata.precision.unwrap_or(8)as usize);d.decimal=l.decimal();}
    if d.source!=value{if d.dirty{d.stale=true;}else{d.source=value.clone();d.text=l.number(canonical,parameter.metadata.precision.unwrap_or(8)as usize);}}
    let had_focus=ui.memory(|m|m.has_focus(response_id));let response=ui.add(egui::TextEdit::singleline(&mut d.text).id(response_id).desired_width(90.));
    if response.gained_focus()&&!d.pending{if !d.dirty{d.text=l.canonical_edit(canonical);d.decimal=l.decimal();}d.stamp=self.projection.stamp();d.pending=true;self.out.gesture(self.projection,response_id.value(),InputSource::Keyboard,GesturePhase::BeginPending,vec![(id.clone(),Some(value.clone()))]);}
    if response.changed(){d.dirty=true;d.error=None;if !d.pending&&!d.stale{d.pending=true;d.stamp=self.projection.stamp();self.out.gesture(self.projection,response_id.value(),InputSource::Keyboard,GesturePhase::BeginPending,vec![(id.clone(),Some(value.clone()))]);}}
    let cancel=(had_focus||response.has_focus())&&ui.input(|i|i.key_pressed(egui::Key::Escape));
    let commit=!cancel&&d.dirty&&(response.lost_focus()||response.has_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter)));
    record(self.out,format!("parameter:{}",id.as_str()),id.as_str().into(),Role::Field,&response,Some(d.text.clone()));
    if cancel{d.source=value.clone();d.text=l.number(canonical,parameter.metadata.precision.unwrap_or(8)as usize);d.dirty=false;d.stale=false;d.error=None;d.submitted=None;d.pending=false;response.surrender_focus();self.out.gesture(self.projection,response_id.value(),InputSource::Keyboard,GesturePhase::Cancelled,vec![(id.clone(),None)]);}
    let proposed=if commit{if d.stale{d.error=Some("draft.stale".into());None}else{match parse_draft(d.decimal,&d.text).and_then(|x|Value::scalar(x).map_err(|e|e.to_string())).and_then(|v|parameter.validate_value(&v).map(|()|v).map_err(|e|e.to_string())){Ok(v)=>{d.submitted=Some(v.clone());d.error=None;Some(v)},Err(_)=>{d.error=Some("draft.invalid".into());None}}}}else{None};
    if d.dirty{ui.small(format!("{} · {}: {}",l.text("Draft"),l.text("Canonical value"),l.number(canonical,parameter.metadata.precision.unwrap_or(8)as usize)));}
    if let Some(error)=&d.error{ui.label(l.text(error));}
    if !d.dirty&&d.pending&&(response.lost_focus()||response.has_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter))){d.pending=false;self.out.gesture(self.projection,response_id.value(),InputSource::Keyboard,GesturePhase::Cancelled,vec![(id.clone(),None)]);}
    if let Some(v)=proposed{d.pending=false;self.out.gesture(self.projection,response_id.value(),InputSource::Keyboard,GesturePhase::Changed,vec![(id.clone(),Some(v))]);self.out.gesture(self.projection,response_id.value(),InputSource::Keyboard,GesturePhase::Committed,vec![(id.clone(),None)]);}
   });},
   Value::Bool(v)=>{let mut x=*v;let r=ui.checkbox(&mut x,"");if r.changed(){self.out.one_shot(self.projection,response_id.value(),vec![(id,Some(Value::Bool(x)))]);}},
   Value::Integer(v)=>{let mut x=*v;let r=ui.add(egui::DragValue::new(&mut x));if r.drag_started(){self.proposed(id.clone(),value.clone(),response_id.value(),GesturePhase::BeginPending);}if r.changed(){if !r.dragged(){self.out.one_shot(self.projection,response_id.value(),vec![(id.clone(),Some(Value::Integer(x)))]);}else{self.proposed(id.clone(),Value::Integer(x),response_id.value(),GesturePhase::Changed);}}if r.drag_stopped(){self.proposed(id.clone(),Value::Integer(x),response_id.value(),GesturePhase::Committed);}},
   Value::Enum(v)=>{let mut x=v.clone();egui::ComboBox::from_id_salt(response_id).selected_text(&x).show_ui(ui,|ui|{for choice in &parameter.metadata.enum_choices{ui.selectable_value(&mut x,choice.clone(),choice);}});if x!=*v{self.out.one_shot(self.projection,response_id.value(),vec![(id,Some(Value::Enum(x)))]);}},
   Value::Vector(v)=>{let mut xs:Vec<f64>=v.iter().map(|x|x.get()).collect();let mut changed=false;let mut started=false;let mut stopped=false;let mut dragging=false;
    ui.horizontal(|ui|{for x in &mut xs{let r=ui.add(egui::DragValue::new(x).speed(parameter.metadata.step.map_or(0.1,|x|x.get())));changed|=r.changed();started|=r.drag_started();stopped|=r.drag_stopped();dragging|=r.dragged();}});
    if started{self.proposed(id.clone(),value.clone(),response_id.value(),GesturePhase::BeginPending);}
    if changed{if let Ok(v)=Value::vector(xs){if dragging{self.proposed(id.clone(),v,response_id.value(),GesturePhase::Changed);}else{self.out.one_shot(self.projection,response_id.value(),vec![(id.clone(),Some(v))]);}}}
    if stopped{self.out.gesture(self.projection,response_id.value(),InputSource::WidgetPointer,GesturePhase::Committed,vec![(id.clone(),None)]);}
    if dragging&&ui.input(|i|i.key_pressed(egui::Key::Escape)){self.out.gesture(self.projection,response_id.value(),InputSource::WidgetPointer,GesturePhase::Cancelled,vec![(id.clone(),None)]);}
   },
   Value::File(_)=>{ui.label(format_value(l,&value));if ui.button(l.text("Open…")).clicked(){self.out.service(self.projection,self.capabilities,ServiceKind::Import,"parameter-file",serde_json::json!({"parameter":id}));}},
   _=>{ui.label(format_value(l,&value));}
  }Some(false)
 }
}
fn parse_draft(decimal:crate::language::DecimalSeparator,text:&str)->Result<f64,String>{let s=if decimal==crate::language::DecimalSeparator::Comma{text.trim().replace(',',".")}else{text.trim().into()};let x=s.parse::<f64>().map_err(|_|"draft.invalid".to_owned())?;if x.is_finite(){Ok(x)}else{Err("draft.invalid".into())}}
pub fn format_value(l:Language<'_>,v:&Value)->String{match v{Value::Scalar(x)=>l.number(x.get(),8),Value::Integer(x)=>x.to_string(),Value::Bool(x)=>x.to_string(),Value::Enum(x)=>x.clone(),Value::Vector(xs)=>format!("[{}]",xs.iter().map(|x|l.number(x.get(),8)).collect::<Vec<_>>().join("; ")),_=>serde_json::to_string(v).unwrap_or_default()}}
