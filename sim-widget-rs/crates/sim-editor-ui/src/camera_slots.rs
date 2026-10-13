//! Camera-slot presentation adapted from Warpbro slot controls; host ViewState owns poses.
use crate::{language::Language,intents::{UiOutput,UiIntent,ViewIntent}};
#[derive(Default)] pub struct CameraSlots{pub selected:Option<usize>,pub occupied:[bool;10]}
impl CameraSlots{pub fn show(&mut self,ui:&mut egui::Ui,locale:Language<'_>,swap:bool,out:&mut UiOutput){
 ui.horizontal_wrapped(|ui|{ui.label(locale.text("Camera slots"));for i in 0..10{let response=ui.selectable_label(self.selected==Some(i),i.to_string()).on_hover_text(locale.text(if self.occupied[i]{"Recall"}else{"Store"}));
 if response.clicked(){out.intents.push(UiIntent::View(ViewIntent::CameraSlot{index:i,store:swap}));}
 if response.secondary_clicked(){out.intents.push(UiIntent::View(ViewIntent::CameraSlot{index:i,store:!swap}));}
 response.context_menu(|ui|{for(label,store)in[("Store",true),("Recall",false)]{if ui.button(locale.text(label)).clicked(){out.intents.push(UiIntent::View(ViewIntent::CameraSlot{index:i,store}));ui.close();}}});
 }});}
}
