//! Portable colour settings retain donor controls; platform config/display work is requested.
use crate::{language::Language,intents::*,projection::EditorProjection};
#[derive(Clone,Debug,serde::Serialize,serde::Deserialize)]
pub struct ColorPreferences{pub config:String,pub output:String,pub exposure:f64,pub gamma:f64,pub sdr_white:f64,pub hdr_peak:f64,pub channel:String}
impl Default for ColorPreferences{fn default()->Self{Self{config:String::new(),output:"sRGB".into(),exposure:0.,gamma:1.,sdr_white:100.,hdr_peak:1000.,channel:"RGB".into()}}}
impl ColorPreferences{pub fn show(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,out:&mut UiOutput){
 egui_prefs2::section_header(ui,&l.text("Colour configuration"));
 ui.horizontal(|ui|{ui.label(&self.config);if ui.button(l.text("Choose config…")).clicked(){out.service(p,caps,ServiceKind::ColorConfig,"choose",serde_json::json!({}));}});
 ui.horizontal(|ui|{ui.label(l.text("Output transform"));egui::ComboBox::from_id_salt("color-output").selected_text(&self.output).show_ui(ui,|ui|{for x in["sRGB","Linear","Rec.709","Display P3","ACES"]{ui.selectable_value(&mut self.output,x.into(),x);}});});
 egui::Grid::new("colour-values").num_columns(2).show(ui,|ui|{
 for(label,v,lo,hi)in[("Exposure",&mut self.exposure,-20.,20.),("Gamma",&mut self.gamma,0.01,5.),("SDR reference white",&mut self.sdr_white,1.,1000.),("HDR peak",&mut self.hdr_peak,80.,10000.)]{ui.label(l.text(label));if ui.add(egui::DragValue::new(v).range(lo..=hi)).changed(){out.service(p,caps,ServiceKind::Display,"color",serde_json::json!({"field":label,"value":*v}));}ui.end_row();}});
 ui.menu_button(l.text("Monitor presets"),|ui|{for(name,white,peak)in[("sRGB",100.,100.),("Display P3",100.,600.),("HDR10",203.,1000.),("HDR10+",203.,2000.)]{if ui.button(name).clicked(){self.sdr_white=white;self.hdr_peak=peak;out.service(p,caps,ServiceKind::Display,"monitor-preset",serde_json::json!({"name":name,"white":white,"peak":peak}));ui.close();}}});
 ui.label(l.text(if caps.hdr{"HDR peak"}else{"Display output"}));
}}
