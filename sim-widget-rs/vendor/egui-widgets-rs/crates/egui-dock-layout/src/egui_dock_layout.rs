//! Named layouts for [`egui_dock`]: safe JSON persistence plus a typed save/restore toolbar.
//!
//! # The trap this crate exists for
//!
//! Persisting a dock layout looks like a one-liner:
//!
//! ```ignore
//! let blob = serde_json::to_string(&dock_state)?;          // save
//! if let Ok(state) = serde_json::from_str(&blob) { .. }     // restore
//! ```
//!
//! [`DockState`] carries the layout rects it computed on the last frame, and a node that has
//! never been drawn carries `NaN` ones. `serde_json` writes `NaN` as `null` and then refuses
//! to read it back as `f32`, so **a layout captured before its window was first drawn can
//! never be loaded again**. Because hosts apply layouts with `if let Ok(..)`, nothing is
//! reported — the user simply finds their arrangement reset on the next start.
//!
//! [`to_blob`] removes that class of bug by clearing the rects before serialising. Nothing is
//! lost: rects are layout *output*, recomputed on the next frame, while the parts that encode
//! the user's intent — the tree, the tabs, split fractions, collapsed flags — are kept.
//!
//! # The two halves
//!
//! | Need | Use |
//! |---|---|
//! | save / load one layout | [`to_blob`], [`from_blob`] |
//! | a set of *named* layouts with a toolbar | [`DockLayoutBar`] over [`LayoutStore`] |
//!
//! # Relationship to `egui-layout-manager`
//!
//! The toolbar UX is not reimplemented here: [`DockLayoutBar`] wraps
//! [`egui_layout_manager::LayoutManager`], which owns that widget and is deliberately
//! dock-agnostic — it stores opaque `String` blobs and leaves serialisation to the host, which
//! is exactly where the trap above bites. Reach for this crate when your layout **is** a
//! [`DockState`]; reach for that one for a custom layout type.
//!
//! ```no_run
//! # use egui_dock::DockState;
//! # use egui_dock_layout::{DockLayoutBar, LayoutStore};
//! # fn demo(ui: &mut egui::Ui, store: &mut LayoutStore, bar: &mut DockLayoutBar,
//! #         state: &mut DockState<String>) -> Result<(), egui_dock_layout::Error> {
//! // The bar takes the live state and hands back one to apply - no blobs in host code.
//! if let Some(restored) = bar.show(ui, store, state)? {
//!     *state = restored;
//! }
//! # Ok(())
//! # }
//! ```

use egui_dock::{DockState, Node};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use egui_layout_manager::{LayoutGlyphs, LayoutStore};

/// Why a layout could not be saved or loaded.
///
/// Distinguishing the two directions matters for the caller: a serialise failure is a bug in
/// the `Tab` type, while a deserialise failure is normal wear — a stored layout written by an
/// older build whose tab set has since changed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The layout could not be turned into JSON.
    #[error("could not serialise the dock layout: {0}")]
    Serialise(#[source] serde_json::Error),
    /// The stored JSON could not be turned back into a layout.
    #[error("could not read the stored dock layout: {0}")]
    Deserialise(#[source] serde_json::Error),
}

/// Serialises `state` into a blob that is guaranteed to load back through [`from_blob`].
///
/// The layout rects are cleared first — see the module docs for why keeping them makes a blob
/// unloadable. `state` itself is not modified.
pub fn to_blob<Tab>(state: &DockState<Tab>) -> Result<String, Error>
where
    Tab: Serialize + Clone,
{
    let mut portable = state.clone();
    clear_rects(&mut portable);
    serde_json::to_string(&portable).map_err(Error::Serialise)
}

/// Reads a blob produced by [`to_blob`].
///
/// Returns an error instead of falling back to a default layout, so a host can tell the
/// difference between "nothing saved" and "saved but unreadable" and say so.
pub fn from_blob<Tab>(blob: &str) -> Result<DockState<Tab>, Error>
where
    Tab: DeserializeOwned,
{
    serde_json::from_str(blob).map_err(Error::Deserialise)
}

/// Clears every cached layout rect in `state`.
///
/// Rects are recomputed on the next frame, so this is lossless for anything the user chose.
/// Exposed for hosts that keep their own serialisation but still need the fix.
pub fn clear_rects<Tab>(state: &mut DockState<Tab>) {
    // `Rect::ZERO` rather than `Rect::NOTHING`: the latter is built from infinities, which
    // serialise to `null` for exactly the same reason NaN does.
    let zero = egui::Rect::ZERO;
    for surface in state.iter_surfaces_mut() {
        let Some(tree) = surface.node_tree_mut() else {
            continue;
        };
        for node in tree.iter_mut() {
            match node {
                Node::Leaf(leaf) => {
                    leaf.rect = zero;
                    leaf.viewport = zero;
                }
                Node::Vertical(split) | Node::Horizontal(split) => split.rect = zero,
                Node::Empty => {}
            }
        }
    }
}

/// The named-layout toolbar, typed for [`DockState`].
///
/// Same widget as [`egui_layout_manager::LayoutManager`] — a combo of saved names plus
/// save / delete / rename — except the host passes the live [`DockState`] in and gets a
/// [`DockState`] back. That is the point: the serialisation the module docs warn about happens
/// in here, once, instead of at every call site.
#[derive(Debug, Clone, Default)]
pub struct DockLayoutBar {
    inner: egui_layout_manager::LayoutManager,
}

impl DockLayoutBar {
    /// New toolbar with the shared phosphor glyphs and a 120pt combo.
    pub fn new() -> Self {
        Self {
            inner: egui_layout_manager::LayoutManager::new(),
        }
    }

    /// Override the combo-box width, in points.
    pub fn with_combo_width(mut self, width: f32) -> Self {
        self.inner = self.inner.with_combo_width(width);
        self
    }

    /// Override the toolbar glyphs.
    pub fn with_glyphs(mut self, glyphs: LayoutGlyphs) -> Self {
        self.inner = self.inner.with_glyphs(glyphs);
        self
    }

    /// Draws the toolbar and returns the layout to apply, if the user picked one.
    ///
    /// `current` is filed away verbatim when the user saves. Save, delete and rename only
    /// mutate `store`, so the caller has nothing to do for those.
    ///
    /// # Errors
    ///
    /// [`Error::Serialise`] if `current` cannot be written (a bug in `Tab`), or
    /// [`Error::Deserialise`] if the layout the user selected cannot be read — typically one
    /// saved by an older build with a different tab set. The toolbar is drawn either way, so
    /// a stale entry can still be deleted or replaced.
    pub fn show<Tab>(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut LayoutStore,
        current: &DockState<Tab>,
    ) -> Result<Option<DockState<Tab>>, Error>
    where
        Tab: Serialize + DeserializeOwned + Clone,
    {
        let blob = to_blob(current)?;
        match self.inner.show(ui, store, &blob) {
            Some(egui_layout_manager::LayoutAction::Restore(restored)) => {
                from_blob(&restored).map(Some)
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_dock::{DockState, NodeIndex};

    /// Two tabs on the left, one on the right, with the right leaf collapsed — enough shape
    /// to prove the parts a user actually arranged survive a round trip.
    fn sample() -> DockState<String> {
        let mut state = DockState::new(vec!["cpu".to_string(), "gpu".to_string()]);
        let surface = state.main_surface_mut();
        let [_, right] = surface.split_right(NodeIndex::root(), 0.25, vec!["disks".to_string()]);
        surface[right].set_collapsed(true);
        state
    }

    /// The regression this crate was written for: a state that has never been drawn carries
    /// NaN rects, and the naive `serde_json` round trip cannot read them back.
    #[test]
    fn a_never_drawn_layout_round_trips() {
        let state = sample();

        let naive = serde_json::to_string(&state).expect("serialising always works");
        assert!(
            serde_json::from_str::<DockState<String>>(&naive).is_err(),
            "if this passes, egui_dock stopped storing NaN rects and to_blob can be simplified"
        );

        let blob = to_blob(&state).expect("to_blob");
        from_blob::<String>(&blob).expect("a blob from to_blob must always load");
    }

    #[test]
    fn the_users_arrangement_survives() {
        let blob = to_blob(&sample()).expect("to_blob");
        let restored: DockState<String> = from_blob(&blob).expect("from_blob");

        let tabs: Vec<&String> = restored.iter_all_tabs().map(|(_, tab)| tab).collect();
        assert_eq!(
            tabs,
            vec!["cpu", "gpu", "disks"],
            "tabs or their order changed"
        );

        let path = restored
            .find_tab(&"disks".to_string())
            .expect("the collapsed leaf is still there");
        assert!(
            restored[path.surface][path.node].is_collapsed(),
            "collapsed state is part of the arrangement and must be kept"
        );
    }

    #[test]
    fn to_blob_leaves_the_caller_state_alone() {
        let state = sample();
        let before = format!("{state:?}");
        let _ = to_blob(&state).expect("to_blob");
        assert_eq!(
            format!("{state:?}"),
            before,
            "to_blob must not mutate its input"
        );
    }

    #[test]
    fn clearing_rects_makes_them_finite() {
        let mut state = sample();
        clear_rects(&mut state);
        for surface in state.iter_surfaces() {
            let Some(tree) = surface.node_tree() else {
                continue;
            };
            for node in tree.iter() {
                let rects = match node {
                    Node::Leaf(leaf) => vec![leaf.rect, leaf.viewport],
                    Node::Vertical(split) | Node::Horizontal(split) => vec![split.rect],
                    Node::Empty => vec![],
                };
                for rect in rects {
                    assert!(
                        rect.min.x.is_finite()
                            && rect.min.y.is_finite()
                            && rect.max.x.is_finite()
                            && rect.max.y.is_finite(),
                        "a non-finite rect is what serialises to null"
                    );
                }
            }
        }
    }

    #[test]
    fn unreadable_blob_reports_instead_of_defaulting() {
        let err = from_blob::<String>("{not json").expect_err("must not silently succeed");
        assert!(matches!(err, Error::Deserialise(_)));
    }

    /// A layout written when the tab type was different must surface as an error rather than
    /// being silently swallowed — that silence is how arrangements vanish.
    #[test]
    fn a_blob_from_another_tab_type_is_an_error() {
        #[derive(Clone, serde::Serialize, serde::Deserialize)]
        struct Other {
            field: u8,
        }

        let blob = to_blob(&DockState::new(vec![Other { field: 1 }])).expect("to_blob");
        assert!(matches!(
            from_blob::<String>(&blob),
            Err(Error::Deserialise(_))
        ));
    }

    #[test]
    fn store_round_trips_named_layouts() {
        let mut store = LayoutStore::new();
        store.save("Wide".to_string(), to_blob(&sample()).expect("to_blob"));

        let json = serde_json::to_string(&store).expect("the store is plain serde data");
        let reloaded: LayoutStore = serde_json::from_str(&json).expect("store reload");
        let blob = reloaded.get("Wide").expect("named layout is still there");
        from_blob::<String>(blob).expect("and it still loads");
    }
}
