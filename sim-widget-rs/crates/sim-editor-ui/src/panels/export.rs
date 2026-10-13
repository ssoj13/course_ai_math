//! Render / Encode form adapted from Warpbro export.rs UI. Services own actual jobs and resource IO.
use crate::{language::Language,intents::*,projection::EditorProjection};
pub struct Export{pub name:String,pub width:u32,pub height:u32,pub first:i64,pub last:i64,pub fps:f64,pub format:String,pub encoding:String,pub video:String,pub encoder:String,pub peak:f64,pub quality:u32,pub denoise:bool,pub output_display:String,pub output_view:String}
impl Default for Export{fn default()->Self{Self{name:"render".into(),width:1920,height:1080,first:0,last:0,fps:24.,format:"PNG".into(),encoding:"SDR 8-bit".into(),video:"Off".into(),encoder:"Kvazaar".into(),peak:1000.,quality:18,denoise:false,output_display:String::new(),output_view:String::new()}}}
impl Export{pub fn show(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,out:&mut UiOutput){
 crate::panels::render_profiles::settings(ui,l,p,caps,out);ui.separator();
 egui::Grid::new("render-encode").num_columns(2).show(ui,|ui|{
 ui.label(l.text("Output name"));ui.text_edit_singleline(&mut self.name);ui.end_row();
 ui.label(l.text("Resolution"));ui.horizontal(|ui|{ui.add(egui::DragValue::new(&mut self.width).range(1..=16384));ui.label("×");ui.add(egui::DragValue::new(&mut self.height).range(1..=16384));});ui.end_row();
 ui.label(l.text("Samples"));ui.label(l.text("Profile values"));ui.end_row();
 ui.label(l.text("Denoise"));ui.checkbox(&mut self.denoise,l.text("Current frame"));ui.end_row();
 ui.label(l.text("Frame range"));ui.horizontal_wrapped(|ui|{ui.add(egui::DragValue::new(&mut self.first));ui.label("…");ui.add(egui::DragValue::new(&mut self.last));if ui.button(l.text("Current frame")).clicked(){self.first=p.time().to_rational_floor(1,24).unwrap_or(0);self.last=self.first;}if ui.button(l.text("Timeline")).clicked(){self.first=p.document.tracks.iter().flat_map(|t|&t.keys).map(|k|k.time.to_rational_floor(1,24).unwrap_or(0)).min().unwrap_or(0);self.last=p.document.tracks.iter().flat_map(|t|&t.keys).map(|k|k.time.to_rational_floor(1,24).unwrap_or(0)).max().unwrap_or(0);}});ui.end_row();
 ui.label(l.text("Format"));ui.horizontal(|ui|{for format in["PNG","OpenEXR","HEVC"]{ui.selectable_value(&mut self.format,format.into(),format);}});ui.end_row();
 ui.label(l.text("Output transform"));ui.horizontal(|ui|{ui.text_edit_singleline(&mut self.output_display);ui.text_edit_singleline(&mut self.output_view);});ui.end_row();
 if self.format=="PNG"{ui.label(l.text("Format"));combo(ui,"png-encoding",&mut self.encoding,&["SDR 8-bit","SDR 16-bit","PQ 16-bit","HLG 16-bit"]);ui.end_row();if self.encoding.contains("PQ")||self.encoding.contains("HLG"){ui.label(l.text("HDR peak"));ui.add(egui::DragValue::new(&mut self.peak).range(80.0..=10000.));ui.end_row();}ui.label(l.text("Encoder"));combo(ui,"png-video",&mut self.video,&["Off","ProRes","H.264","HEVC"]);ui.end_row();}
 if self.format=="HEVC"||self.video!="Off"{ui.label(l.text("Encoder"));combo(ui,"video-encoder",&mut self.encoder,&["Kvazaar","Vulkan","FFmpeg"]);ui.end_row();ui.label(l.text("FPS"));ui.horizontal(|ui|{ui.add(egui::DragValue::new(&mut self.fps).range(1.0..=240.));egui::ComboBox::from_id_salt("fps-presets").selected_text(l.text("FPS")).show_ui(ui,|ui|{for rate in[23.976,24.,25.,29.97,30.,50.,59.94,60.,120.]{ui.selectable_value(&mut self.fps,rate,rate.to_string());}});});ui.end_row();ui.label(l.text("Quality"));ui.add(egui_slider::SlimSlider::new(&mut self.quality,0..=51).fill(true));ui.end_row();}
 });
 let valid=!self.name.trim().is_empty()&&self.first<=self.last&&self.fps.is_finite()&&self.fps>0.;
 let r=ui.add_enabled(valid,egui::Button::new(l.text("Start render")));if r.clicked(){out.service(p,caps,ServiceKind::Render,"start",serde_json::json!({"name":self.name,"width":self.width,"height":self.height,"first":self.first,"last":self.last,"fps":self.fps,"format":self.format,"encoding":self.encoding,"video":self.video,"encoder":self.encoder,"peak":self.peak,"quality":self.quality,"denoise":self.denoise,"display":self.output_display,"view":self.output_view}));}
 if ui.button(l.text("Cancel job")).clicked(){out.service(p,caps,ServiceKind::CancelJob,"cancel",serde_json::json!({}));}
}}
fn combo(ui:&mut egui::Ui,id:&str,value:&mut String,choices:&[&str]){egui::ComboBox::from_id_salt(id).selected_text(value.as_str()).show_ui(ui,|ui|{for x in choices{ui.selectable_value(value,(*x).into(),*x);}});}
