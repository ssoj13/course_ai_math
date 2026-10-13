//! Borrow-only typed views over the otio-core node tree.
//!
//! Each view wraps `(&Document, &Node)` and decodes fields lazily via [`crate::codec`].
//! Shared `Item`/`Composition` behavior is provided by the [`NodeView`] /
//! [`ItemView`] traits (default methods) to avoid duplication. Views are
//! read-only; they never copy data, so they cannot drift from what serializes.

use box_rs::RustBox;
use otio_core::{canonical_schema, Document, Field, Node, NodeId};
use otio_time::{RationalTime, TimeRange};
use serde_json::Value;

use crate::codec::{self, Color};

// --- field helpers ---------------------------------------------------------

/// Borrow a plain-value field (not a structural child link).
fn val<'a>(node: &'a Node, key: &str) -> Option<&'a Value> {
    match node.field(key) {
        Some(Field::Value(v)) => Some(v),
        _ => None,
    }
}

/// Borrow a string field.
fn str_of<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    val(node, key)?.as_str()
}

// --- shared traits ---------------------------------------------------------

/// Common behavior for any serializable-object view.
pub trait NodeView<'d> {
    /// The owning document.
    fn doc(&self) -> &'d Document;
    /// The wrapped node.
    fn node(&self) -> &'d Node;
    /// The wrapped node's id (use it to mutate via `otio_schema::edit`).
    fn id(&self) -> NodeId;

    /// The object's `name`, if set.
    fn name(&self) -> Option<&'d str> {
        str_of(self.node(), "name")
    }

    /// The object's `metadata` as a (general-purpose) box-rs container snapshot.
    fn metadata(&self) -> RustBox {
        self.node().metadata()
    }
}

/// Common behavior for `Item` subclasses (Clip / Gap / Track / Stack).
pub trait ItemView<'d>: NodeView<'d> {
    /// Whether the item is enabled (defaults to `true` when absent).
    fn enabled(&self) -> bool {
        val(self.node(), "enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    /// The item's `source_range`, if trimmed.
    fn source_range(&self) -> Option<TimeRange> {
        codec::time_range(val(self.node(), "source_range")?)
    }

    /// The item's display color, if set.
    fn color(&self) -> Option<Color> {
        codec::color(val(self.node(), "color")?)
    }

    /// The item's effects.
    fn effects(&self) -> Vec<Effect<'d>> {
        self.node()
            .children("effects")
            .iter()
            .filter_map(|&id| Effect::new(self.doc(), id))
            .collect()
    }

    /// The item's markers.
    fn markers(&self) -> Vec<Marker<'d>> {
        self.node()
            .children("markers")
            .iter()
            .filter_map(|&id| Marker::new(self.doc(), id))
            .collect()
    }
}

/// Generate a view struct + its checked constructor + [`NodeView`] impl.
macro_rules! view_struct {
    ($(#[$doc:meta])* $name:ident, $schema:literal) => {
        $(#[$doc])*
        #[derive(Clone, Copy)]
        pub struct $name<'d> {
            doc: &'d Document,
            node: &'d Node,
            id: NodeId,
        }

        impl<'d> $name<'d> {
            /// Wrap node `id` as this view, or `None` if its schema doesn't match.
            pub fn new(doc: &'d Document, id: NodeId) -> Option<Self> {
                let node = doc.node(id)?;
                // Compare canonicalized so legacy aliases match (e.g. Sequence -> Track).
                (canonical_schema(node.schema_name()) == $schema).then_some(Self { doc, node, id })
            }
        }

        impl<'d> NodeView<'d> for $name<'d> {
            fn doc(&self) -> &'d Document {
                self.doc
            }
            fn node(&self) -> &'d Node {
                self.node
            }
            fn id(&self) -> NodeId {
                self.id
            }
        }
    };
}

view_struct!(
    /// A `Timeline` — the document root: a name, a root `Stack`, and an optional
    /// global start time.
    Timeline,
    "Timeline"
);
view_struct!(
    /// A `Stack` — composables layered in time (the timeline's `tracks`, or a
    /// nested stack).
    Stack,
    "Stack"
);
view_struct!(
    /// A `Track` — an ordered sequence of clips/gaps/transitions/stacks.
    Track,
    "Track"
);
view_struct!(
    /// A `Clip` — references media over a source range.
    Clip,
    "Clip"
);
view_struct!(
    /// A `Gap` — empty space on a track.
    Gap,
    "Gap"
);
view_struct!(
    /// A `Transition` between two neighboring items.
    Transition,
    "Transition"
);
view_struct!(
    /// A `Marker` annotating a time range on an item.
    Marker,
    "Marker"
);
impl<'d> ItemView<'d> for Clip<'d> {}
impl<'d> ItemView<'d> for Gap<'d> {}
impl<'d> ItemView<'d> for Track<'d> {}
impl<'d> ItemView<'d> for Stack<'d> {}

// --- Timeline --------------------------------------------------------------

impl<'d> Timeline<'d> {
    /// Wrap the document root as a `Timeline`, if it is one.
    pub fn root(doc: &'d Document) -> Option<Self> {
        Self::new(doc, doc.root_id()?)
    }

    /// The root `Stack` (`tracks`).
    pub fn tracks(&self) -> Option<Stack<'d>> {
        Stack::new(self.doc, self.node.child("tracks")?)
    }

    /// The optional global start time.
    pub fn global_start_time(&self) -> Option<RationalTime> {
        codec::rational_time(val(self.node, "global_start_time")?)
    }
}

// --- Stack / Track children ------------------------------------------------

impl<'d> Stack<'d> {
    /// The stacked child items.
    pub fn items(&self) -> Vec<AnyItem<'d>> {
        self.node
            .children("children")
            .iter()
            .map(|&id| AnyItem::new(self.doc, id))
            .collect()
    }
}

impl<'d> Track<'d> {
    /// The track kind (`"Video"`/`"Audio"`/custom).
    ///
    /// Returns `None` if the field is absent. OTIO treats `kind` as required, so a
    /// missing value signals malformed input — we surface that rather than mask it
    /// behind a fabricated `"Video"`.
    pub fn kind(&self) -> Option<&'d str> {
        str_of(self.node, "kind")
    }

    /// The ordered child items.
    pub fn items(&self) -> Vec<AnyItem<'d>> {
        self.node
            .children("children")
            .iter()
            .map(|&id| AnyItem::new(self.doc, id))
            .collect()
    }
}

// --- Clip ------------------------------------------------------------------

impl<'d> Clip<'d> {
    /// The clip's active media reference, if any.
    ///
    /// Handles both schema versions: Clip.1 stores a single `media_reference`
    /// child; Clip.2 stores a `media_references` map and an
    /// `active_media_reference_key` that selects the active entry.
    pub fn media_reference(&self) -> Option<MediaRef<'d>> {
        if let Some(id) = self.node.child("media_reference") {
            return MediaRef::new(self.doc, id);
        }
        // OTIO defaults the active key to "DEFAULT_MEDIA" when the field is absent
        // (clip.h: `default_media_key`). A valid Clip.2 may omit
        // `active_media_reference_key` and rely on this default — falling back here
        // avoids dropping its media reference.
        let key = self.active_media_reference_key().unwrap_or("DEFAULT_MEDIA");
        let id = self.node.child_in_map("media_references", key)?;
        MediaRef::new(self.doc, id)
    }

    /// The active media reference key (Clip.2), if present.
    pub fn active_media_reference_key(&self) -> Option<&'d str> {
        str_of(self.node, "active_media_reference_key")
    }
}

// --- Transition ------------------------------------------------------------

impl<'d> Transition<'d> {
    /// The transition type (`"SMPTE_Dissolve"`/`"Custom_Transition"`/custom).
    pub fn transition_type(&self) -> Option<&'d str> {
        str_of(self.node, "transition_type")
    }

    /// Time the transition extends into the preceding item.
    pub fn in_offset(&self) -> Option<RationalTime> {
        codec::rational_time(val(self.node, "in_offset")?)
    }

    /// Time the transition extends into the following item.
    pub fn out_offset(&self) -> Option<RationalTime> {
        codec::rational_time(val(self.node, "out_offset")?)
    }
}

// --- Marker ----------------------------------------------------------------

impl<'d> NodeViewMarkerExt<'d> for Marker<'d> {}

/// Marker-specific accessors (kept as an ext trait so the macro stays generic).
pub trait NodeViewMarkerExt<'d>: NodeView<'d> {
    /// The annotated time range.
    fn marked_range(&self) -> Option<TimeRange> {
        codec::time_range(val(self.node(), "marked_range")?)
    }

    /// The marker color (e.g. `"RED"`).
    fn marker_color(&self) -> Option<&'d str> {
        str_of(self.node(), "color")
    }

    /// The marker comment.
    fn comment(&self) -> Option<&'d str> {
        str_of(self.node(), "comment")
    }
}

// --- Effect ----------------------------------------------------------------

/// An `Effect` (base class or any subclass: built-in `TimeEffect`/`LinearTimeWarp`/
/// `FreezeFrame`, or a third-party/plugin effect).
///
/// Hand-written (not via the `view_struct!` macro) because it accepts the whole
/// effect subclass family, not a single schema name.
#[derive(Clone, Copy)]
pub struct Effect<'d> {
    doc: &'d Document,
    node: &'d Node,
    id: NodeId,
}

impl<'d> NodeView<'d> for Effect<'d> {
    fn doc(&self) -> &'d Document {
        self.doc
    }
    fn node(&self) -> &'d Node {
        self.node
    }
    fn id(&self) -> NodeId {
        self.id
    }
}

impl<'d> Effect<'d> {
    /// Wrap node `id` as an effect view.
    ///
    /// Accepts any node, not a fixed schema allowlist: effect base-class membership
    /// is guaranteed by the structural slot this is built from (an item's `effects`
    /// field holds only Effect subclasses, per the OTIO schema). A hardcoded
    /// allowlist silently dropped third-party/plugin effect subclasses, since this
    /// crate has no type registry to consult. Returns `None` only for a dangling id.
    /// Typed getters (`effect_name`/`time_scalar`) stay permissive, so an unknown
    /// subclass is preserved as a raw passthrough view rather than discarded.
    pub fn new(doc: &'d Document, id: NodeId) -> Option<Self> {
        let node = doc.node(id)?;
        Some(Self { doc, node, id })
    }

    /// The concrete schema name (e.g. `"LinearTimeWarp"` or a plugin effect).
    pub fn kind(&self) -> &'d str {
        self.node.schema_name()
    }

    /// The effect name (e.g. `"Crop"`).
    pub fn effect_name(&self) -> Option<&'d str> {
        str_of(self.node, "effect_name")
    }

    /// The time scalar (`LinearTimeWarp`/`FreezeFrame`), if present.
    pub fn time_scalar(&self) -> Option<f64> {
        val(self.node, "time_scalar")?.as_f64()
    }
}

// --- MediaRef --------------------------------------------------------------

/// A media reference view (External / Generator / ImageSequence / Missing /
/// unknown). Typed getters return `Some` only for the relevant schema.
///
/// Holds `&Node` + its id (no `&Document`): a media reference is a leaf, with no
/// child nodes to follow.
#[derive(Clone, Copy)]
pub struct MediaRef<'d> {
    node: &'d Node,
    id: NodeId,
}

impl<'d> MediaRef<'d> {
    /// Wrap node `id` as a media reference view.
    ///
    /// Accepts any node, not a fixed schema allowlist: media-reference base-class
    /// membership is guaranteed by the structural slot this is built from (a clip's
    /// `media_reference` child / `media_references` map holds only MediaReference
    /// subclasses, per the OTIO schema). A hardcoded allowlist silently dropped
    /// third-party/plugin reference subclasses, since this crate has no type
    /// registry to consult. Returns `None` only for a dangling id. `kind()` reports
    /// the true schema name, so an unknown subclass is preserved as a raw
    /// passthrough view rather than discarded.
    pub fn new(doc: &'d Document, id: NodeId) -> Option<Self> {
        let node = doc.node(id)?;
        Some(Self { node, id })
    }

    /// This media reference's node id.
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The concrete schema name (e.g. `"ExternalReference"`).
    pub fn kind(&self) -> &'d str {
        self.node.schema_name()
    }

    /// The available range of the referenced media, if known.
    pub fn available_range(&self) -> Option<TimeRange> {
        codec::time_range(val(self.node, "available_range")?)
    }

    /// `ExternalReference` target URL.
    pub fn target_url(&self) -> Option<&'d str> {
        str_of(self.node, "target_url")
    }

    /// `GeneratorReference` kind (e.g. `"SMPTEBars"`).
    pub fn generator_kind(&self) -> Option<&'d str> {
        str_of(self.node, "generator_kind")
    }

    /// `GeneratorReference` parameters as a box-rs container, if present.
    pub fn parameters(&self) -> Option<RustBox> {
        let v = val(self.node, "parameters")?;
        Some(RustBox::from_json_value(v.clone(), box_rs::BoxConfig::default()))
    }

    /// `ImageSequenceReference` target URL base.
    pub fn target_url_base(&self) -> Option<&'d str> {
        str_of(self.node, "target_url_base")
    }

    /// `ImageSequenceReference` start frame.
    pub fn start_frame(&self) -> Option<i64> {
        val(self.node, "start_frame")?.as_i64()
    }

    /// `ImageSequenceReference` filename prefix (before the frame number).
    pub fn name_prefix(&self) -> Option<&'d str> {
        str_of(self.node, "name_prefix")
    }

    /// `ImageSequenceReference` filename suffix (after the frame number).
    pub fn name_suffix(&self) -> Option<&'d str> {
        str_of(self.node, "name_suffix")
    }

    /// `ImageSequenceReference` frame step.
    pub fn frame_step(&self) -> Option<i64> {
        val(self.node, "frame_step")?.as_i64()
    }

    /// `ImageSequenceReference` zero-padding width for the frame number.
    pub fn frame_zero_padding(&self) -> Option<i64> {
        val(self.node, "frame_zero_padding")?.as_i64()
    }

    /// `ImageSequenceReference` frame rate.
    pub fn rate(&self) -> Option<f64> {
        val(self.node, "rate")?.as_f64()
    }
}

// --- AnyItem ---------------------------------------------------------------

/// A track/stack child of unknown concrete type, resolved to a typed view.
pub enum AnyItem<'d> {
    /// A clip.
    Clip(Clip<'d>),
    /// A gap.
    Gap(Gap<'d>),
    /// A nested track.
    Track(Track<'d>),
    /// A nested stack.
    Stack(Stack<'d>),
    /// A transition.
    Transition(Transition<'d>),
    /// Any other schema (kept as a raw node id).
    Other(NodeId),
}

impl<'d> AnyItem<'d> {
    /// Resolve node `id` to its typed item view (canonicalizing legacy aliases).
    pub fn new(doc: &'d Document, id: NodeId) -> AnyItem<'d> {
        match doc.node(id).map(|n| canonical_schema(n.schema_name())) {
            Some("Clip") => Clip::new(doc, id).map(AnyItem::Clip),
            Some("Gap") => Gap::new(doc, id).map(AnyItem::Gap),
            Some("Track") => Track::new(doc, id).map(AnyItem::Track),
            Some("Stack") => Stack::new(doc, id).map(AnyItem::Stack),
            Some("Transition") => Transition::new(doc, id).map(AnyItem::Transition),
            _ => None,
        }
        .unwrap_or(AnyItem::Other(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn load(name: &str) -> Document {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../_ref/OpenTimelineIO/tests/sample_data")
            .join(name);
        Document::from_json_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn navigate_simple_cut() {
        let doc = load("simple_cut.otio");
        let tl = Timeline::root(&doc).unwrap();
        assert_eq!(tl.name(), Some("Figure 1 - Simple Cut List"));
        let stack = tl.tracks().unwrap();
        let items = stack.items();
        assert_eq!(items.len(), 1);
        let AnyItem::Track(track) = &items[0] else {
            panic!("expected a track");
        };
        assert_eq!(track.kind(), Some("Video"));
        let clips = track.items();
        assert_eq!(clips.len(), 4);

        let AnyItem::Clip(clip) = &clips[0] else {
            panic!("expected a clip");
        };
        assert_eq!(clip.name(), Some("Clip-001"));
        // First clip: source_range start=3, duration=3 @24.
        let sr = clip.source_range().unwrap();
        assert_eq!(sr.start_time(), RationalTime::new(3.0, 24.0));
        assert_eq!(sr.duration(), RationalTime::new(3.0, 24.0));
        // Media reference is an external file URL.
        let mr = clip.media_reference().unwrap();
        assert_eq!(mr.kind(), "ExternalReference");
        assert_eq!(mr.target_url(), Some("file:///folder/titles.mov"));

        // Last clip has no source_range (null) -> None, not a panic.
        let AnyItem::Clip(last) = &clips[3] else {
            panic!("expected a clip");
        };
        assert_eq!(last.source_range(), None);
    }

    /// First clip anywhere under a stack (recursing into nested tracks/stacks).
    fn first_clip<'d>(stack: &Stack<'d>) -> Option<Clip<'d>> {
        stack.items().into_iter().find_map(first_clip_in)
    }
    fn first_clip_in(item: AnyItem<'_>) -> Option<Clip<'_>> {
        match item {
            AnyItem::Clip(c) => Some(c),
            AnyItem::Track(t) => t.items().into_iter().find_map(first_clip_in),
            AnyItem::Stack(s) => s.items().into_iter().find_map(first_clip_in),
            _ => None,
        }
    }

    #[test]
    fn clip2_media_reference_via_map() {
        // premiere_example uses Clip.2: media_references map + active key.
        let doc = load("premiere_example.otio");
        let tl = Timeline::root(&doc).unwrap();
        let stack = tl.tracks().unwrap();
        let clip = first_clip(&stack).expect("a clip");
        let mr = clip.media_reference().expect("Clip.2 media reference resolves via map");
        assert!(
            matches!(
                mr.kind(),
                "ExternalReference"
                    | "MissingReference"
                    | "ImageSequenceReference"
                    | "GeneratorReference"
                    | "MediaReference"
            ),
            "unexpected media ref kind: {}",
            mr.kind()
        );
    }
}
