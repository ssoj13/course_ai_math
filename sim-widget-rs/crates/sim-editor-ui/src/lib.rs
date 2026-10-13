//! Portable professional editor shell adapted from the frozen Warpbro UI.
//! The shell reads an immutable runtime projection and emits typed intents.
//! It cannot create a GPU device, submit work, evaluate mathematics or own domain undo.
pub mod accessibility;
pub mod camera_slots;
pub mod catalog;
pub mod chrome;
pub mod color_ui;
pub mod dock;
pub mod hotkeys;
pub mod intents;
pub mod language;
pub mod panels;
pub mod preferences;
pub mod projection;
pub mod shell;
pub mod transport;
pub use dock::Panel;
pub use intents::{UiIntent,UiOutput,PlatformCapabilities,Capability,ServiceRequest};
pub use projection::EditorProjection;
pub use shell::{EditorUi,EditorViewState};
