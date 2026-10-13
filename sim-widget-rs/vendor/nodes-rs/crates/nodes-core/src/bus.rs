//! GDP-style named data bus scoped to a [`crate::graph_container::Graph`].
//!
//! `BusRead` / `BusWrite` builtin nodes read and write named **slots** holding a
//! [`BoxValue`]. Cook order defers bus reads until after the main graph (see
//! [`crate::cook`]).
//!
//! # Two lanes
//! - **slots** (`HashMap<String, BoxValue>`) — serializable data, the original bus.
//! - **handles** (`HashMap<String, HandleSlot>`) — TRANSIENT, runtime-only
//!   `Arc<dyn Any>` references to heavy in-memory objects a data-flow graph must
//!   reference but never serialize (a database connection, a reconstruction, a GPU
//!   buffer). Handles are NEVER persisted: [`DataBus::to_json_map`] /
//!   [`DataBus::load_json_map`] ignore them, so a saved graph round-trips only the
//!   `BoxValue` data and the handles are re-populated at cook time. The serializable
//!   side of the graph references a handle by its string key (the `…​.ref` pattern),
//!   keeping the checkpoint pure. This lets pure Rust compute graphs (e.g. an SfM /
//!   MVS pipeline) flow heavy objects through the bus without forcing them through
//!   `BoxValue` — `box-rs` stays a pure serializable value model.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value as Json;

use crate::box_model::BoxValue;
use crate::error::CoreResult;

/// A transient runtime handle stored in the [`DataBus`] handle lane: an opaque
/// `Arc<dyn Any + Send + Sync>` plus the concrete `type_name` it was inserted as
/// (used for `Debug` and to make a mismatched downcast fail loud rather than
/// silently wrong). Cloning is an `Arc` refcount bump. NEVER serialized.
#[derive(Clone)]
pub struct HandleSlot {
    value: Arc<dyn Any + Send + Sync>,
    type_name: &'static str,
}

impl HandleSlot {
    /// The concrete type name the handle was inserted as.
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }
}

impl std::fmt::Debug for HandleSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn Any` is not `Debug`; print the recorded type name only.
        write!(f, "HandleSlot({})", self.type_name)
    }
}

/// Named slots shared by all nodes in one graph (Houdini-style bus). Two lanes:
/// serializable `BoxValue` **slots** and transient `Arc<dyn Any>` **handles** (see
/// the module docs). No `PartialEq`/`Debug` derive on the bus, so the opaque handle
/// lane composes cleanly (only `Clone`, which `Arc` provides).
#[derive(Clone, Default)]
pub struct DataBus {
    slots: HashMap<String, BoxValue>,
    /// Transient runtime handles (heavy `Arc` objects), never serialized. See [`HandleSlot`].
    handles: HashMap<String, HandleSlot>,
}

impl DataBus {
    pub fn new() -> Self {
        Self::default()
    }

    // ── serializable BoxValue slots (the original bus) ───────────────────────

    pub fn contains(&self, key: &str) -> bool {
        self.slots.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.slots.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&BoxValue> {
        self.slots.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, value: BoxValue) {
        self.slots.insert(key.into(), value);
    }

    pub fn remove(&mut self, key: &str) -> Option<BoxValue> {
        self.slots.remove(key)
    }

    /// Clear BOTH lanes (slots + handles).
    pub fn clear(&mut self) {
        self.slots.clear();
        self.handles.clear();
    }

    // ── transient handle lane (heavy Arc objects, never serialized) ──────────

    /// Store a heavy runtime handle under `key` (transient; ignored by
    /// serialization). Records `T`'s type name for debug + a checked downcast.
    pub fn set_handle<T: Any + Send + Sync>(&mut self, key: impl Into<String>, value: Arc<T>) {
        self.handles.insert(
            key.into(),
            HandleSlot {
                value,
                type_name: std::any::type_name::<T>(),
            },
        );
    }

    /// Fetch a handle by `key`, downcast to `Arc<T>`. Returns `None` if the slot is
    /// absent OR the stored concrete type is not `T` — a wrong-type read fails loud
    /// (never a silently-wrong value).
    pub fn get_handle<T: Any + Send + Sync>(&self, key: &str) -> Option<Arc<T>> {
        self.handles.get(key)?.value.clone().downcast::<T>().ok()
    }

    /// Whether a handle slot exists under `key`.
    pub fn contains_handle(&self, key: &str) -> bool {
        self.handles.contains_key(key)
    }

    /// Remove a handle slot; returns whether one was present.
    pub fn remove_handle(&mut self, key: &str) -> bool {
        self.handles.remove(key).is_some()
    }

    /// The `type_name` recorded for a handle slot (introspection/debug).
    pub fn handle_type_name(&self, key: &str) -> Option<&'static str> {
        self.handles.get(key).map(HandleSlot::type_name)
    }

    /// Keys of the handle lane.
    pub fn handle_keys(&self) -> impl Iterator<Item = &str> {
        self.handles.keys().map(String::as_str)
    }

    // ── serialization (slots ONLY; handles are transient) ────────────────────

    /// Serialize the `BoxValue` slots to tagged JSON (matching [`BoxValue`] tags).
    /// The transient handle lane is intentionally omitted (never persisted).
    pub fn to_json_map(&self) -> HashMap<String, Json> {
        self.slots
            .iter()
            .map(|(k, v)| (k.clone(), v.to_json_value()))
            .collect()
    }

    /// Replace all `BoxValue` slots from a JSON map (tagged `BoxValue` JSON). Does
    /// NOT touch the handle lane (handles are re-populated at cook time, not loaded).
    pub fn load_json_map(&mut self, map: HashMap<String, Json>) -> CoreResult<()> {
        self.slots.clear();
        for (key, json) in map {
            self.slots.insert(key, BoxValue::from_json_value(json));
        }
        Ok(())
    }
}

#[cfg(test)]
mod handle_tests {
    use super::*;

    #[test]
    fn handle_roundtrip_and_typed_downcast() {
        let mut bus = DataBus::new();
        let obj = Arc::new(vec![1u32, 2, 3]);
        bus.set_handle("recon", obj.clone());
        assert!(bus.contains_handle("recon"));
        let got = bus
            .get_handle::<Vec<u32>>("recon")
            .expect("present + right type");
        assert_eq!(*got, vec![1, 2, 3]);
        assert!(Arc::ptr_eq(&got, &obj));
    }

    #[test]
    fn wrong_type_downcast_is_none_not_panic() {
        let mut bus = DataBus::new();
        bus.set_handle("x", Arc::new(42u64));
        assert!(bus.get_handle::<String>("x").is_none()); // fail-loud, no silent wrong value
        assert!(bus.get_handle::<u64>("x").is_some());
    }

    #[test]
    fn handles_are_not_serialized() {
        let mut bus = DataBus::new();
        bus.set("data", BoxValue::I64(7));
        bus.set_handle("heavy", Arc::new(99u8));
        let json = bus.to_json_map();
        assert!(json.contains_key("data"));
        assert!(!json.contains_key("heavy")); // transient — omitted from serialization
    }
}
