use sim_widget_rs::{math::*,model::*,runtime::*};
use std::collections::{BTreeMap,VecDeque};
fn base()->Document {Document::decode(include_bytes!("../cosines.json"),DecodeLimits::default()).unwrap().document().clone()}
fn p(s:&str)->ParameterId {ParameterId::new(s).unwrap()}
fn v(s:&str)->ValueId {ValueId::new(s).unwrap()}
fn scalar(n:f64)->Value {Value::scalar(n).unwrap()}
fn session(d:Document,config:SessionConfig)->Session {Session::new(d.validate(config.decode_limits).unwrap(),OperatorRegistry::standard().unwrap(),config).unwrap()}
fn n(s:&Session,id:&str)->f64 {s.snapshot().truth()[&v(id)].as_scalar().unwrap()}
fn command(s:&mut Session,kind:CommandKind)->Result<DispatchResult> {s.dispatch(s.prepare_command(kind))}
#[derive(Clone,Debug,PartialEq)]
struct Envelope {document:Document,frame:FrameSnapshot,state:SessionCheckpoint,events:VecDeque<SessionEvent>,history:usize,undo:bool,redo:bool,playing:bool}
fn capture(s:&Session)->Envelope {Envelope{document:s.document().document().clone(),frame:s.snapshot(),state:s.checkpoint(),events:s.events().clone(),history:s.history_len(),undo:s.can_undo(),redo:s.can_redo(),playing:s.is_playing()}}
