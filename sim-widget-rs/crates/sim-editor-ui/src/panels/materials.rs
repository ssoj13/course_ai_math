//! Material work area and category library retain independent create, assign and preview actions.
use crate::{language::Language,intents::*,projection::EditorProjection};
#[derive(Default)]pub struct Materials{pub filter:String,pub selected:Option<String>,pub width:f32}
impl Materials{pub fn show(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,library:bool,selection:&[sim_model::ObjectId],out:&mut UiOutput){
 let entries=if library{p.material_library}else{p.materials};if self.width==0.{self.width=144.;}
 ui.horizontal_wrapped(|ui|{ui.label(l.text("Search"));ui.text_edit_singleline(&mut self.filter);
 if !library{if ui.button(l.text("+ New material")).clicked(){out.author(p,caps,AuthorAction::Create,vec![],None,serde_json::json!({"kind":"material"}));}if ui.button(l.text("Library…")).clicked(){out.service(p,caps,ServiceKind::Template,"material-library",serde_json::json!({}));}}
 ui.menu_button(l.text("Create from preset"),|ui|{let mut categories:Vec<_>=p.material_library.iter().map(|x|x.category.as_str()).collect();categories.sort();categories.dedup();for category in categories{ui.menu_button(category,|ui|{for preset in p.material_library.iter().filter(|x|x.category==category){if ui.button(&preset.name).clicked(){out.author(p,caps,AuthorAction::Create,vec![],None,serde_json::json!({"kind":"material","preset":preset.id,"assign":false}));ui.close();}}});}if p.material_library.is_empty(){ui.label(l.text("Empty"));}});
 });
 ui.separator();if entries.is_empty(){ui.label(l.text("No material nodes"));}
 let needle=self.filter.to_lowercase();let mut categories:Vec<_>=entries.iter().map(|x|x.category.as_str()).collect();categories.sort();categories.dedup();
 egui::ScrollArea::vertical().show(ui,|ui|{for category in categories{egui::CollapsingHeader::new(category).default_open(true).show(ui,|ui|{ui.horizontal_wrapped(|ui|{for entry in entries.iter().filter(|x|x.category==category&&x.name.to_lowercase().contains(&needle)){
  crate::panels::catalog::card(ui,l,entry,self.width,self.selected.as_deref()==Some(&entry.id),|ui|{
   let r=ui.selectable_label(self.selected.as_deref()==Some(&entry.id),&entry.name);if r.clicked(){self.selected=Some(entry.id.clone());out.author(p,caps,AuthorAction::AssignMaterial,vec![],None,serde_json::json!({"edit":entry.id}));}
   r.context_menu(|ui|{for(label,action)in[("Assign to selected",AuthorAction::AssignMaterial),("Assign to all",AuthorAction::AssignMaterial),("Duplicate",AuthorAction::Duplicate),("Delete",AuthorAction::Delete)]{if ui.button(l.text(label)).clicked(){let objects=if label=="Assign to all"{p.document.objects.iter().map(|o|o.id.clone()).collect()}else{selection.to_vec()};out.author(p,caps,action,objects,None,serde_json::json!({"material":entry.id}));ui.close();}}
    if ui.button(l.text("Refresh preview")).clicked(){out.service(p,caps,ServiceKind::Thumbnail,"material",serde_json::json!({"id":entry.id}));ui.close();}
    ui.menu_button(l.text("Apply preset to this material"),|ui|{for preset in p.material_library{if ui.button(&preset.name).clicked(){out.author(p,caps,AuthorAction::ApplyPreset,vec![],None,serde_json::json!({"material":entry.id,"preset":preset.id}));ui.close();}}});
   });
   if library&&ui.button(l.text("Add to work area")).clicked(){out.author(p,caps,AuthorAction::Create,vec![],None,serde_json::json!({"kind":"material","preset":entry.id,"assign":false}));}
  });
 } });});}});
}}
