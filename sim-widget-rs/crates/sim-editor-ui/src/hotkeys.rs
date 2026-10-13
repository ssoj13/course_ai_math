//! Scoped shell shortcuts; editor/IME focus never becomes a viewport edit.
use crate::{intents::{UiOutput,UiIntent,ViewIntent},projection::EditorProjection};
#[derive(Clone,Copy,Debug)]pub enum Scope{Viewport,Timeline,Gallery,Bookmarks,Materials,AttributeEditor,Settings,Export,Outliner,MaterialLibrary,Takes}
pub fn register(ui:&egui::Ui,scope:Scope,rect:egui::Rect){ui.ctx().data_mut(|d|d.insert_temp(ui.id().with("scope"),(scope,rect)));}
pub fn gather(ctx:&egui::Context,p:&EditorProjection<'_>,out:&mut UiOutput){
 if ctx.egui_wants_keyboard_input(){return;}
 let key=ctx.input_mut(|i|{
 if i.consume_key(egui::Modifiers::COMMAND|egui::Modifiers::SHIFT,egui::Key::Z){Some(true)}
 else if i.consume_key(egui::Modifiers::COMMAND,egui::Key::Z){Some(false)}else{None}});
 if let Some(redo)=key{if if redo{p.can_redo}else{p.can_undo}{out.intents.push(UiIntent::View(ViewIntent::UndoInput{redo}));}}
}
