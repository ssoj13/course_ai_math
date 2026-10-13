//! One portable egui shell. Domain truth, history, GPU and platform jobs stay with the caller.
use std::collections::BTreeMap;
use egui_dock::{DockArea,DockState,TabViewer};
use crate::{dock::{self,Panel,Layouts},language::{CatalogRegistry,LocaleId,Language},intents::*,projection::EditorProjection,panels};
pub struct EditorViewState{pub locale:LocaleId,pub selection:Vec<sim_model::ObjectId>,pub view_id:sim_model::ViewId,pub tool:String,pub space:String,pub perspective:bool,pub filters:BTreeMap<String,(bool,bool)>,pub chrome_rect:Option<egui::Rect>,pub status_resizable:bool}
impl Default for EditorViewState{fn default()->Self{Self{locale:LocaleId::new("en").unwrap(),selection:vec![],view_id:sim_model::ViewId::new("editor:main").unwrap(),tool:"Move".into(),space:"Global".into(),perspective:false,filters:Default::default(),chrome_rect:None,status_resizable:false}}}
pub struct EditorUi{
 pub view:EditorViewState,pub dock:DockState<Panel>,pub layouts:Layouts,pub open:Vec<Panel>,pub layout_name:String,pub preferences:crate::preferences::Preferences,
 pub inspector:panels::inspector::Inspector,pub timeline:panels::timeline::Timeline,pub outliner:panels::outliner::Outliner,pub browser:panels::catalog::Browser,pub materials:panels::materials::Materials,pub takes:panels::takes::Takes,pub export:panels::export::Export,
 pub toolbar:egui_viewport_toolbar::ToolbarState,pub status_layout:egui_statusbar::StatusBarLayout,pub camera_slots:crate::camera_slots::CameraSlots,
}
impl Default for EditorUi{fn default()->Self{Self{view:Default::default(),dock:dock::default_layout(),layouts:Default::default(),open:vec![],layout_name:String::new(),preferences:Default::default(),inspector:Default::default(),timeline:Default::default(),outliner:Default::default(),browser:Default::default(),materials:Default::default(),takes:Default::default(),export:Default::default(),toolbar:Default::default(),status_layout:Default::default(),camera_slots:Default::default()}}}
impl EditorUi{
 pub fn show(&mut self,root:&mut egui::Ui,p:&EditorProjection<'_>,caps:&PlatformCapabilities,registry:&CatalogRegistry)->UiOutput{
  let mut out=UiOutput::default();let l=match registry.language(&self.view.locale){Ok(x)=>x,Err(e)=>{out.errors.push(e);return out;}};
  self.preferences.apply_fonts(root.ctx());root.ctx().all_styles_mut(|s|{s.spacing.interact_size.y=if caps.coarse_pointer{44.}else{26.};s.spacing.item_spacing=egui::vec2(8.,8.);});
  egui::Panel::top("editor-top").exact_size(36.).show(root,|ui|crate::chrome::top_bar(self,ui,l,p,caps,registry,&mut out));
  egui::Panel::bottom("editor-status").show(root,|ui|crate::chrome::status(self,ui,l,p,&out));
  crate::hotkeys::gather(root.ctx(),p,&mut out);
  egui::CentralPanel::default().show(root,|ui|{
   for panel in self.open.drain(..){dock::open_panel(&mut self.dock,panel);}
   let mut state=std::mem::replace(&mut self.dock,DockState::new(vec![]));
   DockArea::new(&mut state).id(egui::Id::new("sim-editor-workspace")).style(egui_dock::Style::from_egui(ui.style().as_ref())).show_inside(ui,&mut Viewer{editor:self,projection:p,caps,language:l,out:&mut out});
   for panel in self.open.drain(..){dock::open_panel(&mut state,panel);}self.dock=state;
  });self.inspector.retire_focus(root.ctx(),p,&mut out);out
 }
 pub fn draw_panel(&mut self,ui:&mut egui::Ui,panel:Panel,p:&EditorProjection<'_>,caps:&PlatformCapabilities,l:Language<'_>,out:&mut UiOutput){
  if !out.panels_drawn.contains(&panel){out.panels_drawn.push(panel);}
  ui.push_id(panel,|ui|{match panel{
   Panel::Viewport=>{crate::chrome::viewport(self,ui,l,p,caps,out);self.camera_slots.show(ui,l,self.preferences.controls.swap_slots,out);},
   Panel::Inspector=>self.inspector.show(ui,l,p,caps,&self.view.selection,out),
   Panel::Timeline=>self.timeline.show(ui,l,p,caps,out),
   Panel::Outliner=>self.outliner.show(ui,l,p,caps,&mut self.view.selection,out),
   Panel::Gallery|Panel::Bookmarks=>self.browser.show(ui,l,p,caps,panel,out),
   Panel::Materials|Panel::MaterialLibrary=>self.materials.show(ui,l,p,caps,panel==Panel::MaterialLibrary,&self.view.selection,out),
   Panel::Takes=>self.takes.show(ui,l,p,caps,&self.view.selection,out),
   Panel::Settings=>self.preferences.show(ui,l,p,caps,out),
   Panel::Export=>self.export.show(ui,l,p,caps,out),
  }});
 }
}
struct Viewer<'a,'p>{editor:&'a mut EditorUi,projection:&'a EditorProjection<'p>,caps:&'a PlatformCapabilities,language:Language<'a>,out:&'a mut UiOutput}
impl TabViewer for Viewer<'_,'_>{
 type Tab=Panel;
 fn id(&mut self,tab:&mut Panel)->egui::Id{egui::Id::new(("sim-panel",*tab))}
 fn title(&mut self,tab:&mut Panel)->egui::WidgetText{self.language.text(tab.title()).into_owned().into()}
 fn ui(&mut self,ui:&mut egui::Ui,tab:&mut Panel){self.editor.draw_panel(ui,*tab,self.projection,self.caps,self.language,self.out);}
 fn is_closeable(&self,tab:&Panel)->bool{*tab!=Panel::Viewport}
 fn scroll_bars(&self,_:&Panel)->[bool;2]{[false,false]}
 fn context_menu(&mut self,ui:&mut egui::Ui,_:&mut Panel,_:egui_dock::NodePath){for panel in Panel::ALL{if ui.button(self.language.text(panel.title())).clicked(){self.editor.open.push(panel);ui.close();}}}
}
