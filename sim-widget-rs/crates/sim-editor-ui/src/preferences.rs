//! The six original settings categories, split from native App into portable view preferences.
use crate::{language::Language,color_ui::ColorPreferences,intents::*,projection::EditorProjection};
#[derive(Clone,Debug)]pub struct Controls{
 pub look_sensitivity:f64,pub fly_speed:f64,pub translate_decay:f64,pub rotate_decay:f64,pub inertial_look:bool,pub look_smoothing:f64,pub flip_degrees:f64,pub fast:f64,pub slow:f64,pub swap_slots:bool,pub gizmo_size:f64,pub stroke:f64,pub align_axis:bool,pub align_seconds:f64,
 pub field_height:f64,pub numeric_width:f64,pub icon_size:f64,pub row_spacing:f64,pub component_spacing:f64,
}
impl Default for Controls{fn default()->Self{Self{look_sensitivity:0.5,fly_speed:1.,translate_decay:0.96,rotate_decay:1.,inertial_look:true,look_smoothing:0.23,flip_degrees:48.6,fast:4.,slow:0.1,swap_slots:false,gizmo_size:120.,stroke:3.,align_axis:false,align_seconds:0.5,field_height:26.,numeric_width:90.,icon_size:16.,row_spacing:8.,component_spacing:4.}}}
#[derive(Clone,Debug,PartialEq)]pub struct Fonts{pub face:String,pub body:f32,pub small:f32,pub heading:f32,pub monospace:f32,pub zoom:f32}
impl Default for Fonts{fn default()->Self{Self{face:"Default".into(),body:11.,small:11.,heading:13.5,monospace:11.,zoom:1.}}}
pub struct Preferences{pub panel:egui_prefs2::PrefsPanelState,pub controls:Controls,pub fonts:Fonts,pub color:ColorPreferences,pub fps:f64,pub hdr:bool,pub new_key:String,pub applied:Option<Fonts>}
impl Default for Preferences{fn default()->Self{Self{panel:Default::default(),controls:Default::default(),fonts:Default::default(),color:Default::default(),fps:60.,hdr:false,new_key:"Linear".into(),applied:None}}}
impl Preferences{
 pub fn show(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,out:&mut UiOutput){
  let labels=["Display","Color","Controls","Fonts","Animation","Render & Viewport"].map(|x|l.text(x).into_owned());let cats:Vec<_>=labels.iter().map(|x|egui_prefs2::Category::new("",x)).collect();
  let mut panel=std::mem::take(&mut self.panel);
  egui_prefs2::draw(ui,&mut panel,&cats,|ui,idx|{match idx{
   0=>{egui_prefs2::section_header(ui,&l.text("Display output"));ui.horizontal(|ui|{ui.label(l.text("GUI FPS"));ui.add(egui::DragValue::new(&mut self.fps).range(15.0..=240.));});
    if ui.add_enabled(caps.hdr,egui::Checkbox::new(&mut self.hdr,l.text("HDR peak"))).changed(){out.service(p,caps,ServiceKind::Display,"hdr",serde_json::json!({"enabled":self.hdr}));}
    self.color.show(ui,l,p,caps,out);},
   1=>self.color.show(ui,l,p,caps,out),
   2=>self.controls_ui(ui,l),
   3=>self.fonts_ui(ui,l,p,caps,out),
   4=>{egui_prefs2::section_header(ui,&l.text("Animation"));ui.horizontal(|ui|{ui.label(l.text("New key type"));for x in["Linear","Smooth"]{ui.selectable_value(&mut self.new_key,x.into(),l.text(x));}});},
   5=>crate::panels::render_profiles::settings(ui,l,p,caps,out),
   _=>unreachable!()
  }ui.separator();if ui.button(l.text("Reset")).clicked(){match idx{0=>{self.fps=60.;self.hdr=false;},1=>self.color=Default::default(),2=>self.controls=Default::default(),3=>self.fonts=Default::default(),4=>self.new_key="Linear".into(),_=>out.author(p,caps,AuthorAction::AssignProfile,vec![],None,serde_json::json!({"reset":true}))}}},None::<fn()>);self.panel=panel;
 }
 fn controls_ui(&mut self,ui:&mut egui::Ui,l:Language<'_>){
  let c=&mut self.controls;egui_prefs2::section_header(ui,&l.text("Camera"));
  egui::Grid::new("camera-preferences").num_columns(3).show(ui,|ui|{
   for(label,v,lo,hi,default)in[("Mouse sensitivity",&mut c.look_sensitivity,0.1,5.,0.5),("Flight speed ×",&mut c.fly_speed,0.02,50.,1.),("Translate decay, s",&mut c.translate_decay,0.02,5.,0.96),("Rotate decay, s",&mut c.rotate_decay,0.02,5.,1.),("Look smoothing, s",&mut c.look_smoothing,0.,0.5,0.23),("Horizon flip, °",&mut c.flip_degrees,20.,85.,48.6),("Shift fast ×",&mut c.fast,1.,20.,4.),("Alt slow ×",&mut c.slow,0.01,1.,0.1)]{row(ui,l,label,v,lo,hi,default);}
   ui.label(l.text("Smooth look"));ui.checkbox(&mut c.inertial_look,"");ui.end_row();
  });
  egui_prefs2::section_header(ui,&l.text("Flight alignment"));ui.checkbox(&mut c.align_axis,l.text("To an axis"));row(ui,l,"Ease time, s",&mut c.align_seconds,0.05,5.,0.5);
  egui_prefs2::section_header(ui,&l.text("Manipulator"));row(ui,l,"Size, pt",&mut c.gizmo_size,40.,400.,120.);row(ui,l,"Line width, pt",&mut c.stroke,0.5,10.,3.);ui.checkbox(&mut c.swap_slots,l.text("Swap slot buttons"));
  egui_prefs2::section_header(ui,&l.text("Attribute controls"));
  egui::Grid::new("attribute-metrics").show(ui,|ui|{for(label,v,lo,hi,default)in[("Field height",&mut c.field_height,24.,44.,26.),("Numeric field width",&mut c.numeric_width,40.,240.,90.),("Icon size",&mut c.icon_size,10.,32.,16.),("Row spacing",&mut c.row_spacing,0.,24.,8.),("Component spacing",&mut c.component_spacing,0.,16.,4.)]{row(ui,l,label,v,lo,hi,default);}});
 }
 fn fonts_ui(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,out:&mut UiOutput){
  let definitions=egui::FontDefinitions::default();
  egui::ComboBox::from_id_salt("ui-font").selected_text(&self.fonts.face).show_ui(ui,|ui|{ui.selectable_value(&mut self.fonts.face,"Default".into(),l.text("Reset"));for name in definitions.font_data.keys(){ui.selectable_value(&mut self.fonts.face,name.clone(),name);}});
  egui::Grid::new("font-sizes").show(ui,|ui|{for(label,value)in[("Body font",&mut self.fonts.body),("UI size",&mut self.fonts.small),("Fonts",&mut self.fonts.heading),("Monospace font",&mut self.fonts.monospace)]{ui.label(l.text(label));ui.add(egui::DragValue::new(value).range(8.0..=36.));ui.end_row();}ui.label(l.text("UI scale"));ui.add(egui::Slider::new(&mut self.fonts.zoom,0.75..=2.));});
  ui.heading(l.text("Fonts"));ui.label(l.text("Canonical value"));ui.small(l.text("Status"));ui.monospace("0123456789 · α θ");
  if ui.button(l.text("Import…")).clicked(){out.service(p,caps,ServiceKind::Fonts,"load",serde_json::json!({}));}
 }
 pub fn apply_fonts(&mut self,ctx:&egui::Context){if self.applied.as_ref()==Some(&self.fonts){return;}let mut definitions=egui::FontDefinitions::default();if definitions.font_data.contains_key(&self.fonts.face){definitions.families.entry(egui::FontFamily::Proportional).or_default().insert(0,self.fonts.face.clone());}egui_widgets_config::add_icon_font(&mut definitions);ctx.set_fonts(definitions);ctx.set_zoom_factor(self.fonts.zoom);ctx.all_styles_mut(|style|{for(kind,size,family)in[(egui::TextStyle::Body,self.fonts.body,egui::FontFamily::Proportional),(egui::TextStyle::Button,self.fonts.body,egui::FontFamily::Proportional),(egui::TextStyle::Small,self.fonts.small,egui::FontFamily::Proportional),(egui::TextStyle::Heading,self.fonts.heading,egui::FontFamily::Proportional),(egui::TextStyle::Monospace,self.fonts.monospace,egui::FontFamily::Monospace)]{style.text_styles.insert(kind,egui::FontId::new(size,family));}});self.applied=Some(self.fonts.clone());}
}
fn row(ui:&mut egui::Ui,l:Language<'_>,label:&str,value:&mut f64,lo:f64,hi:f64,default:f64){ui.label(l.text(label));ui.add(egui::DragValue::new(value).range(lo..=hi));if ui.small_button("↺").on_hover_text(l.text("Reset")).clicked(){*value=default;}ui.end_row();}
