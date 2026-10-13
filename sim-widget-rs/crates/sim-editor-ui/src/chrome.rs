//! The complete Warpbro menu/viewport/status shell, with IO and domain ownership injected.
use crate::{dock::{self,Panel},language::{Language,CatalogRegistry},intents::*,projection::EditorProjection,shell::EditorUi};
pub fn top_bar(editor:&mut EditorUi,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,registry:&CatalogRegistry,out:&mut UiOutput){
 egui::MenuBar::new().ui(ui,|ui|{
 ui.menu_button(l.text("File"),|ui|{
  for(label,kind,op)in[("New",ServiceKind::Template,"new"),("Open…",ServiceKind::Open,"open"),("Save",ServiceKind::Save,"save"),("Save As…",ServiceKind::SaveAs,"save-as")]{if ui.button(l.text(label)).clicked(){out.service(p,caps,kind,op,serde_json::json!({}));ui.close();}}
  ui.menu_button(l.text("Templates"),|ui|{for(label,op)in[("Save","save-template"),("Open…","open-template"),("Refresh preview","refresh-templates")]{if ui.button(l.text(label)).clicked(){out.service(p,caps,ServiceKind::Template,op,serde_json::json!({}));}}for entry in p.catalog{if ui.button(&entry.name).on_hover_text(&entry.description).clicked(){out.service(p,caps,ServiceKind::Template,"open",serde_json::json!({"id":entry.id}));ui.close();}}});
  ui.separator();if ui.button(l.text("Save bookmark")).clicked(){out.service(p,caps,ServiceKind::Bookmark,"save",serde_json::json!({}));ui.close();}
  ui.menu_button(l.text("Snapshot"),|ui|{for format in["PNG","OpenEXR","HDR PNG"]{if ui.button(format).clicked(){out.service(p,caps,ServiceKind::Export,"viewport-capture",serde_json::json!({"format":format}));ui.close();}}if ui.button(l.text("Copy PNG")).clicked(){out.service(p,caps,ServiceKind::Clipboard,"viewport-png",serde_json::json!({}));}});
  ui.menu_button(l.text("Window capture"),|ui|{for operation in["Save PNG","Copy PNG"]{if ui.button(l.text(operation)).clicked(){out.service(p,caps,if operation=="Copy PNG"{ServiceKind::Clipboard}else{ServiceKind::Export},"window-capture",serde_json::json!({"operation":operation}));ui.close();}}});
 });
 ui.menu_button(l.text("Edit"),|ui|{
  for(label,redo,enabled)in[("Undo",false,p.can_undo),("Redo",true,p.can_redo)]{if ui.add_enabled(enabled,egui::Button::new(l.text(label))).clicked(){out.intents.push(UiIntent::View(ViewIntent::UndoInput{redo}));ui.close();}}
  ui.separator();if ui.button(l.text("Reset scene")).clicked(){out.service(p,caps,ServiceKind::Template,"reset",serde_json::json!({}));ui.close();}if ui.button(l.text("Copy scene JSON")).clicked(){out.service(p,caps,ServiceKind::Clipboard,"document-json",serde_json::json!({}));ui.close();}
  if ui.button(l.text("Settings")).clicked(){editor.open.push(Panel::Settings);ui.close();}
 });
 ui.menu_button(l.text("View"),|ui|{for(label,tool)in[("Pause rendering","render-pause"),("Fly","fly"),("Restore camera","restore-camera"),("Hide UI","hide-ui")]{if ui.button(l.text(label)).clicked(){out.intents.push(UiIntent::View(ViewIntent::Tool(tool.into())));ui.close();}}if ui.button(l.text("Render & Viewport")).clicked(){editor.preferences.panel.selected=5;editor.open.push(Panel::Settings);ui.close();}});
 ui.menu_button(l.text("Render"),|ui|{if ui.button(l.text("Render / Encode")).clicked(){editor.open.push(Panel::Export);ui.close();}});
 ui.menu_button(l.text("Window"),|ui|{for panel in Panel::ALL{if ui.selectable_label(editor.dock.find_tab(&panel).is_some(),l.text(panel.title())).clicked(){editor.open.push(panel);ui.close();}}ui.separator();if ui.button(l.text("Reset layout")).clicked(){editor.dock=dock::default_layout();editor.open.clear();editor.layouts.cache.invalidate();ui.close();}if ui.button(l.text("Fonts")).clicked(){editor.preferences.panel.selected=3;editor.open.push(Panel::Settings);ui.close();}});
 ui.menu_button(l.text("Help"),|ui|{if ui.button(l.text("Help")).clicked(){out.service(p,caps,ServiceKind::Help,"manual",serde_json::json!({}));ui.close();}});
 ui.menu_button(l.text("Language"),|ui|{for(id,name)in registry.entries(){if ui.selectable_label(editor.view.locale==*id,name).clicked(){editor.view.locale=id.clone();out.intents.push(UiIntent::View(ViewIntent::Locale(id.clone())));ui.close();}}});
 ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
  if let Err(e)=editor.layouts.cache.refresh(&editor.dock,ui.ctx()){out.errors.push(e.to_string());}
  let mut cache=std::mem::take(&mut editor.layouts.cache);let mut restored=false;let blob=cache.blob.as_deref().unwrap_or_default();
  if let Some(egui_layout_manager::LayoutAction::Restore(saved))=editor.layouts.manager.show(ui,&mut editor.layouts.store,blob){match dock::restore_layout(&saved){Ok(state)=>{editor.dock=state;editor.open.clear();restored=true;},Err(e)=>out.errors.push(e)}}
  ui.menu_button(l.text("Save layout"),|ui|{ui.text_edit_singleline(&mut editor.layout_name);if ui.add_enabled(!editor.layout_name.trim().is_empty(),egui::Button::new(l.text("Save"))).clicked(){editor.layouts.store.save(&editor.layout_name,blob.to_owned());ui.close();}});
 if restored{cache.invalidate();}editor.layouts.cache=cache;
 });
 });
}
pub fn viewport(editor:&mut EditorUi,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,out:&mut UiOutput){
 let size=ui.available_size().max(egui::vec2(1.,1.));let(rect,response)=ui.allocate_exact_size(size,egui::Sense::click_and_drag());
 if let Some(texture)=p.viewport_texture{ui.painter().image(texture,rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.,1.)),egui::Color32::WHITE);}else{ui.painter().rect_filled(rect,0.,ui.visuals().extreme_bg_color);ui.painter().text(rect.center(),egui::Align2::CENTER_CENTER,l.text("No viewport texture published"),egui::FontId::proportional(11.),ui.visuals().weak_text_color());}
 out.viewports.push(ViewportRequest{hovered:response.contains_pointer(),view:editor.view.view_id.clone(),rect,stamp:p.stamp()});crate::accessibility::record(out,"scene:viewport".into(),l.text("Viewport").into_owned(),crate::accessibility::Role::Viewport,&response,None);
 let mut toolbar=editor.toolbar;let bar=egui_viewport_toolbar::ViewportToolbar{edge:egui_viewport_toolbar::ToolbarEdge::Top,..Default::default()}.show(ui,rect,&mut toolbar,|ui|{
  ui.horizontal(|ui|{for tool in["Select tool","Move","Rotate","Scale","Universal","Box","Shear","Corner pin","Arcball"]{if ui.selectable_label(editor.view.tool==tool,l.text(tool)).clicked(){editor.view.tool=tool.into();out.intents.push(UiIntent::View(ViewIntent::Tool(tool.into())));}}
   ui.separator();egui::ComboBox::from_id_salt("transform-space").selected_text(l.text(&editor.view.space)).show_ui(ui,|ui|{for space in["Local","Global","Parent space","Custom space"]{if ui.selectable_value(&mut editor.view.space,space.into(),l.text(space)).clicked(){out.intents.push(UiIntent::View(ViewIntent::Space(space.into())));}}});
   ui.menu_button(l.text("Visible"),|ui|{for kind in["Point","Vector","Mesh","Group","Camera","Environment","Light","Extras","Grid"]{ui.horizontal(|ui|{ui.label(l.text(kind));let state=editor.view.filters.entry(kind.into()).or_insert((true,true));if ui.checkbox(&mut state.0,l.text("Visible")).changed()||ui.checkbox(&mut state.1,l.text("Selectable")).changed(){out.intents.push(UiIntent::View(ViewIntent::Filter{kind:kind.into(),visible:state.0,selectable:state.1}));}});}});
   ui.separator();if ui.selectable_label(editor.view.perspective,l.text(if editor.view.perspective{"Perspective"}else{"Orthographic"})).clicked(){editor.view.perspective=!editor.view.perspective;out.intents.push(UiIntent::View(ViewIntent::Projection{view:editor.view.view_id.clone(),perspective:editor.view.perspective}));}
   if ui.button(l.text("Fit all")).clicked(){out.intents.push(UiIntent::View(ViewIntent::Fit(editor.view.view_id.clone())));}
   if ui.button(l.text("Snapshot")).clicked(){out.service(p,caps,ServiceKind::Export,"viewport-png",serde_json::json!({}));}
  });
 });editor.toolbar=toolbar;editor.view.chrome_rect=Some(bar.rect);
}
pub fn status(editor:&mut EditorUi,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,out:&UiOutput){
 egui_statusbar::StatusBar::default().locked(!editor.view.status_resizable).show_with(ui,&mut editor.status_layout,&[180.,200.,100.],|i,ui|{match i{0=>{ui.label(l.text(if p.playing{"Moving"}else{"Idle"}));ui.label(p.time().to_string());},1=>{ui.label(l.text("Navigation: hold Space or Alt; left orbit, middle pan, right dolly."));},_=>{if let Some(error)=out.errors.last(){ui.colored_label(ui.visuals().warn_fg_color,l.text(error));}else{ui.label(if p.status.is_empty(){l.text("Ready")}else{l.text(p.status)});}}}});
}
