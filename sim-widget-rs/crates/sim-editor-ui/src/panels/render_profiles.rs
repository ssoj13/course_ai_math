//! Warpbro render/quality/output catalog UI; independent-copy and live-profile semantics are explicit intents.
use crate::{language::Language,intents::*,projection::EditorProjection};
#[derive(Clone)]struct NameDialog{kind:String,role:String,source:Option<String>,target:String,name:String}
pub fn settings(ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,out:&mut UiOutput){
 egui::Grid::new("profile-bindings").num_columns(2).show(ui,|ui|{for target in["Moving","Idle","Manual","Output"]{ui.label(l.text(target));egui::ComboBox::from_id_salt(("binding",target)).selected_text(l.text("Assign profile")).show_ui(ui,|ui|{for profile in p.profiles{if ui.button(&profile.name).clicked(){out.author(p,caps,AuthorAction::AssignProfile,vec![],None,serde_json::json!({"profile":profile.id,"target":target,"copy":false}));ui.close();}}});ui.end_row();}});
 for(kind,title)in[("render","Render"),("quality","Quality"),("output","Output")]{
  egui::CollapsingHeader::new(l.text(title)).default_open(true).show(ui,|ui|{
   ui.horizontal_wrapped(|ui|{for role in["profile","template"]{let label=if role=="profile"{"New"}else{"Save"};if ui.button(l.text(label)).on_hover_text(l.text("Assign profile")).clicked(){ui.data_mut(|d|d.insert_temp(ui.id().with("name"),NameDialog{kind:kind.into(),role:role.into(),source:None,target:"Output".into(),name:String::new()}));}}});
   if p.profiles.is_empty(){ui.label(l.text("No profiles"));}
   for profile in p.profiles{
    let r=ui.button(&profile.name);
    if r.clicked(){out.author(p,caps,AuthorAction::AssignProfile,vec![],None,serde_json::json!({"kind":kind,"profile":profile.id,"copy":false}));}
    r.context_menu(|ui|{for(label,op)in[("Select / Edit","edit"),("Rename","rename"),("Duplicate","copy"),("Delete","delete"),("Save","save-template")]{if ui.button(l.text(label)).clicked(){out.author(p,caps,match op{"delete"=>AuthorAction::Delete,"rename"=>AuthorAction::Rename,_=>AuthorAction::AssignProfile},vec![],None,serde_json::json!({"kind":kind,"profile":profile.id,"operation":op}));ui.close();}}});
    egui::CollapsingHeader::new(l.text("Profile values")).id_salt((kind,&profile.id)).show(ui,|ui|{
     for(name,value)in &profile.fields{ui.horizontal(|ui|{ui.label(name);match value{sim_model::Value::Scalar(x)=>{let mut candidate=x.get();if ui.add(egui::DragValue::new(&mut candidate)).changed(){out.author(p,caps,AuthorAction::AssignProfile,vec![],None,serde_json::json!({"profile":profile.id,"field":name,"value":candidate}));}},sim_model::Value::Bool(x)=>{let mut candidate=*x;if ui.checkbox(&mut candidate,"").changed(){out.author(p,caps,AuthorAction::AssignProfile,vec![],None,serde_json::json!({"profile":profile.id,"field":name,"value":candidate}));}},_=>{ui.label(crate::panels::inspector::format_value(l,value));}}});}
    });
   }
   let id=ui.id().with("name");if let Some(mut draft)=ui.data(|d|d.get_temp::<NameDialog>(id)){let mut closed=false;egui::Window::new(l.text("Name")).id(id.with("window")).collapsible(false).show(ui.ctx(),|ui|{ui.text_edit_singleline(&mut draft.name);ui.horizontal(|ui|{if ui.add_enabled(!draft.name.trim().is_empty(),egui::Button::new(l.text("Save"))).clicked(){out.author(p,caps,AuthorAction::Create,vec![],None,serde_json::json!({"kind":draft.kind,"role":draft.role,"source":draft.source,"target":draft.target,"name":draft.name}));closed=true;}if ui.button(l.text("Close")).clicked(){closed=true;}});});ui.data_mut(|d|{if closed{d.remove::<NameDialog>(id);}else{d.insert_temp(id,draft);}});}
  });
 }
}
