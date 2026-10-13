//! Warpbro outliner UI adapter; donor hierarchy widget proposes typed semantic operations.
use std::collections::BTreeMap;
use sim_model::ObjectId;
use crate::{language::Language,intents::*,projection::EditorProjection};
#[derive(Default)]pub struct Outliner{ids:BTreeMap<ObjectId,u64>,next:u64}
impl Outliner{
 fn identity(&mut self,id:&ObjectId)->u64{if let Some(x)=self.ids.get(id){return *x;}self.next=self.next.checked_add(1).expect("bounded document object count");self.ids.insert(id.clone(),self.next);self.next}
 pub fn show(&mut self,ui:&mut egui::Ui,l:Language<'_>,p:&EditorProjection<'_>,caps:&PlatformCapabilities,selection:&mut Vec<ObjectId>,out:&mut UiOutput){
  ui.horizontal_wrapped(|ui|{ui.menu_button(l.text("Add object"),|ui|{for kind in["Group","Camera","Environment","Light","Point","Vector","Mesh"]{if ui.button(l.text(kind)).clicked(){out.author(p,caps,AuthorAction::Create,selection.clone(),None,serde_json::json!({"kind":kind}));ui.close();}}});for(label,action)in[("Duplicate",AuthorAction::Duplicate),("Delete",AuthorAction::Delete),("Isolate",AuthorAction::Isolate)]{if ui.add_enabled(!selection.is_empty(),egui::Button::new(l.text(label))).clicked(){out.author(p,caps,action,selection.clone(),None,serde_json::json!({}));}}});
  let roots=p.document.objects.iter().map(|x|egui_outliner::TreeNode::new(self.identity(&x.id),&x.label)).collect::<Vec<_>>();
  let mut model=egui_outliner::OutlinerModel::with_roots(roots);model.selection=selection.iter().filter_map(|x|self.ids.get(x).copied()).collect();
  let config=egui_outliner::OutlinerConfig{show_filter:true,editable_labels:true,draggable:true,visibility_on_right:true,context_items:["Rename","Duplicate","Delete","Visible","Selectable","Locked","Isolate","Reveal"].iter().map(|x|egui_outliner::ContextItem::new(*x,l.text(x).into_owned())).collect(),..Default::default()};
  for action in egui_outliner::show(ui,&model,&config){
   let resolve=|n:u64|self.ids.iter().find(|(_,v)|**v==n).map(|(id,_)|id.clone());
   match action{
    egui_outliner::OutlinerAction::Select{id,additive,range}=>{if let Some(id)=resolve(id){if !additive&&!range{selection.clear();}if !selection.contains(&id){selection.push(id.clone());}out.intents.push(UiIntent::View(ViewIntent::Select{objects:selection.clone(),additive,range}));}},
    egui_outliner::OutlinerAction::SelectMany{ids}=>{*selection=ids.into_iter().filter_map(resolve).collect();out.intents.push(UiIntent::View(ViewIntent::Select{objects:selection.clone(),additive:false,range:false}));},
    egui_outliner::OutlinerAction::ClearSelection=>{selection.clear();out.intents.push(UiIntent::View(ViewIntent::Select{objects:vec![],additive:false,range:false}));},
    egui_outliner::OutlinerAction::Activate{id}=>{if let Some(id)=resolve(id){out.intents.push(UiIntent::View(ViewIntent::Reveal(id)));}},
    egui_outliner::OutlinerAction::Rename{id,name}=>{if let Some(id)=resolve(id){out.author(p,caps,AuthorAction::Rename,vec![id],None,serde_json::json!({"name":name}));}},
    egui_outliner::OutlinerAction::Move{id,new_parent,index}=>{if let Some(id)=resolve(id){out.author(p,caps,AuthorAction::Reparent,vec![id],None,serde_json::json!({"parent":new_parent.and_then(resolve),"index":index}));}},
    egui_outliner::OutlinerAction::ToggleVisible{id,visible}=>{if let Some(id)=resolve(id){out.author(p,caps,AuthorAction::Visibility,vec![id],None,serde_json::json!({"visible":visible}));}},
    egui_outliner::OutlinerAction::Context{id,action}=>{if let Some(id)=resolve(id){if action=="Reveal"{out.intents.push(UiIntent::View(ViewIntent::Reveal(id)));}else{let a=match action.as_str(){"Rename"=>AuthorAction::Rename,"Duplicate"=>AuthorAction::Duplicate,"Delete"=>AuthorAction::Delete,"Visible"=>AuthorAction::Visibility,"Selectable"=>AuthorAction::Selectability,"Locked"=>AuthorAction::Lock,_=>AuthorAction::Isolate};out.author(p,caps,a,vec![id],None,serde_json::json!({}));}}},
    _=>{}
   }
  }
 }
}
