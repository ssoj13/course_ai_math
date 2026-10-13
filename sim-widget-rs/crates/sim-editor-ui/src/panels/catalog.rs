//! Gallery/bookmark browser adapted from Warpbro browser: filters, category cards, selection and context actions.
use crate::{dock::Panel,language::Language,intents::*,projection::{EditorProjection,CatalogEntry}};
#[derive(Default)]pub struct Browser{pub search:String,pub category:String,pub selected:Option<String>,pub card_width:f32,pub name:String}
impl Browser{
 pub fn show(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,panel:Panel,out:&mut UiOutput){
  let items=if panel==Panel::Bookmarks{p.bookmarks}else{p.catalog};
  ui.horizontal_wrapped(|ui|{ui.label(l.text("Search"));ui.text_edit_singleline(&mut self.search);egui::ComboBox::from_id_salt(("category",panel)).selected_text(if self.category.is_empty(){l.text("All").into_owned()}else{self.category.clone()}).show_ui(ui,|ui|{ui.selectable_value(&mut self.category,String::new(),l.text("All"));let mut categories:Vec<_>=items.iter().map(|x|x.category.as_str()).collect();categories.sort();categories.dedup();for category in categories{ui.selectable_value(&mut self.category,category.into(),category);}});
   if panel==Panel::Bookmarks{if ui.button(l.text("Save bookmark")).clicked(){out.service(p,caps,ServiceKind::Bookmark,"save",serde_json::json!({"name":self.name}));}ui.text_edit_singleline(&mut self.name);}
   else{for(label,kind)in[("New",ServiceKind::Template),("Open…",ServiceKind::Open),("Import…",ServiceKind::Import)]{if ui.button(l.text(label)).clicked(){out.service(p,caps,kind,"browser",serde_json::json!({}));}}}
   if self.card_width==0.{self.card_width=144.;}ui.add(egui::Slider::new(&mut self.card_width,96.0..=260.).text(l.text("Zoom")));
  });
  ui.separator();let needle=self.search.to_lowercase();let visible:Vec<_>=items.iter().filter(|x|(self.category.is_empty()||self.category==x.category)&&x.name.to_lowercase().contains(&needle)).collect();
  if visible.is_empty(){ui.label(l.text(if panel==Panel::Bookmarks{"No bookmarks"}else{"Empty"}));}
  egui::ScrollArea::vertical().show(ui,|ui|{ui.horizontal_wrapped(|ui|{for entry in visible{card(ui,l,entry,self.card_width,self.selected.as_deref()==Some(&entry.id),|ui|{let response=ui.selectable_label(self.selected.as_deref()==Some(&entry.id),&entry.name).on_hover_text(&entry.description);if response.clicked(){self.selected=Some(entry.id.clone());}if response.double_clicked(){out.service(p,caps,ServiceKind::Open,"catalog-entry",serde_json::json!({"id":entry.id}));}
   response.context_menu(|ui|{for(label,kind,op)in[("Open…",ServiceKind::Open,"open"),("Rename",ServiceKind::Bookmark,"rename"),("Duplicate",ServiceKind::Bookmark,"duplicate"),("Delete",ServiceKind::DeleteBookmark,"delete"),("Refresh preview",ServiceKind::Thumbnail,"refresh"),("Export…",ServiceKind::Export,"export")]{if ui.button(l.text(label)).clicked(){out.service(p,caps,kind,op,serde_json::json!({"id":entry.id}));ui.close();}}});
  });}});});
 }
}
pub fn card(ui:&mut egui::Ui,l:Language<'_>,entry:&CatalogEntry,width:f32,selected:bool,body:impl FnOnce(&mut egui::Ui)){
 let fill=if selected{ui.visuals().selection.bg_fill}else{ui.visuals().faint_bg_color};
 egui::Frame::new().fill(fill).inner_margin(6.).show(ui,|ui|{ui.set_width(width);let size=egui::vec2(width,width*0.65);if let Some(texture)=entry.texture{ui.add(egui::Image::new((texture,size)));}else{let(rect,_)=ui.allocate_exact_size(size,egui::Sense::hover());ui.painter().rect_filled(rect,2.,ui.visuals().extreme_bg_color);ui.painter().text(rect.center(),egui::Align2::CENTER_CENTER,l.text("Refresh preview"),egui::FontId::proportional(11.),ui.visuals().weak_text_color());}if let Some(error)=&entry.error{ui.label(l.text(error));}body(ui);});
}
