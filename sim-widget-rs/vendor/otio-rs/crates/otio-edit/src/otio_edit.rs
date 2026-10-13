//! otio-edit — editing / command / undo layer over the otio-core timeline model.
//!
//! This crate turns the otio-core **document model** into an **editing engine**
//! for an NLE timeline widget: high-level edit operations, a journal-of-
//! primitives undo/redo with stable node identity across undo, a per-track
//! layout cache, and a `revision()` change signal. The full design (and the
//! 3-agent review it came from) lives in `PLAN.md`.
//!
//! ## Shape
//! - [`Editor`] owns the [`Document`](otio_core::Document) privately and is the
//!   single mutation entry point. The widget reads `editor.document()`.
//! - [`Tx`] is the recording mutation wrapper; ops only mutate through it, so
//!   every change is journaled (no un-journaled path can corrupt undo).
//! - [`journal`] holds the reversible [`Prim`](journal::Prim)s and
//!   [`Transaction`](journal::Transaction)s.
//! - [`ops`] are the edit verbs (set_name/source_range, insert, ripple_delete, …).
//! - [`LayoutCache`] memoizes per-track `(start,duration)` layout for drawing.
//!
//! ## Identity contract
//! Undo never frees a node — it detaches and keeps the `NodeId` stable, and
//! [`Editor::gc`] pins every history-referenced id. So a handle the widget holds
//! stays valid across undo/redo. See PLAN.md "Generational-id stability".

pub mod editor;
pub mod error;
pub mod journal;
pub mod layout;
pub mod ops;
pub mod snap;
pub mod tx;

pub use editor::Editor;
pub use error::EditError;
pub use journal::{Prim, Transaction};
pub use layout::LayoutCache;
pub use snap::{snap, snap_targets};
pub use tx::Tx;

#[cfg(test)]
mod tests {
    use super::*;
    use otio_core::Document;
    use otio_schema::build;
    use otio_time::{RationalTime, TimeRange};
    use serde_json::Value;

    /// Build a one-track timeline with three clips, returning (doc, track id).
    fn timeline_with_clips() -> (Editor, otio_core::NodeId) {
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        for name in ["a", "b", "c"] {
            let mr = build::missing_reference(&mut doc);
            let sr = TimeRange::new(RationalTime::new(0.0, 24.0), RationalTime::new(10.0, 24.0));
            let clip = build::clip(&mut doc, name, Some(sr), mr);
            let _ = doc.append_child(track, clip);
        }
        (Editor::new(doc), track)
    }

    /// 24 fps time range helper.
    fn tr(start: f64, dur: f64) -> TimeRange {
        TimeRange::new(RationalTime::new(start, 24.0), RationalTime::new(dur, 24.0))
    }

    /// Build a one-track timeline whose clips carry an `ExternalReference` with an
    /// explicit `available_range` of `[0, avail)@24`, so trim/slip/razor have real
    /// media bounds to respect. Each clip's source_range is `(src_start, dur)@24`.
    /// Returns `(editor, track, [clip ids])`.
    fn timeline_with_media(
        clips: &[(f64, f64)],
        avail: f64,
    ) -> (Editor, otio_core::NodeId, Vec<otio_core::NodeId>) {
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let mut ids = Vec::new();
        for (i, &(s, d)) in clips.iter().enumerate() {
            let mr = build::external_reference(&mut doc, &format!("file://m{i}.mov"));
            if let Some(n) = doc.node_mut(mr) {
                n.set_value(
                    "available_range",
                    otio_schema::codec::time_range_value(tr(0.0, avail)),
                );
            }
            let clip = build::clip(&mut doc, &format!("c{i}"), Some(tr(s, d)), mr);
            let _ = doc.append_child(track, clip);
            ids.push(clip);
        }
        (Editor::new(doc), track, ids)
    }

    fn timeline_with_gap_and_media(
        gap_duration: f64,
        clips: &[(f64, f64)],
        avail: f64,
    ) -> (Editor, otio_core::NodeId, Vec<otio_core::NodeId>) {
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let gap = build::gap(&mut doc, tr(0.0, gap_duration));
        let _ = doc.append_child(track, gap);
        let mut ids = Vec::new();
        for (i, &(start, duration)) in clips.iter().enumerate() {
            let media = build::external_reference(&mut doc, &format!("file://g{i}.mov"));
            if let Some(node) = doc.node_mut(media) {
                node.set_value(
                    "available_range",
                    otio_schema::codec::time_range_value(tr(0.0, avail)),
                );
            }
            let clip = build::clip(&mut doc, &format!("g{i}"), Some(tr(start, duration)), media);
            let _ = doc.append_child(track, clip);
            ids.push(clip);
        }
        (Editor::new(doc), track, ids)
    }

    /// Strong gate for one op: snapshot pre-state, run `apply`, assert it changed
    /// the doc, then undo → byte-identical pre-state, redo → identical post-state.
    fn assert_undo_redo<F: FnOnce(&mut Editor)>(ed: &mut Editor, apply: F) {
        let before = json(ed);
        apply(ed);
        let after = json(ed);
        assert_ne!(before, after, "op did not change the document");
        assert!(ed.undo(), "nothing to undo");
        assert_eq!(json(ed), before, "undo not byte-identical to pre-op state");
        assert!(ed.redo(), "nothing to redo");
        assert_eq!(json(ed), after, "redo not identical to post-op state");
    }

    /// First child id of a track.
    fn child(ed: &Editor, track: otio_core::NodeId, i: usize) -> otio_core::NodeId {
        ed.document().node(track).unwrap().children("children")[i]
    }

    fn json(ed: &Editor) -> Value {
        serde_json::from_str(&ed.document().to_json_string().unwrap()).unwrap()
    }

    #[test]
    fn undo_redo_round_trips_to_identical_json() {
        let (mut ed, track) = timeline_with_clips();

        // Snapshot the JSON before each op; apply a varied script touching every
        // primitive kind (field set, child insert/remove).
        let mut states = vec![json(&ed)];
        let clip0 = ed.document().node(track).unwrap().children("children")[0];

        ops::set_name(&mut ed, clip0, "renamed").unwrap();
        states.push(json(&ed));

        ops::set_enabled(&mut ed, clip0, false).unwrap();
        states.push(json(&ed));

        let sr = TimeRange::new(RationalTime::new(5.0, 24.0), RationalTime::new(3.0, 24.0));
        ops::set_source_range(&mut ed, clip0, Some(sr)).unwrap();
        states.push(json(&ed));

        ops::ripple_delete(&mut ed, track, 1).unwrap(); // remove middle clip
        states.push(json(&ed));

        // Undo every step: each intermediate JSON must match exactly (this is the
        // strong gate — it catches field-reorder and identity bugs).
        for expected in states.iter().rev().skip(1) {
            assert!(ed.undo());
            assert_eq!(&json(&ed), expected, "undo did not restore prior JSON");
        }
        assert!(!ed.can_undo());

        // Redo forward: must reproduce each state in order.
        for expected in states.iter().skip(1) {
            assert!(ed.redo());
            assert_eq!(&json(&ed), expected, "redo did not reproduce next JSON");
        }
        assert!(!ed.can_redo());
    }

    #[test]
    fn field_order_preserved_across_remove_readd_undo() {
        // set_source_range to null then back exercises remove+reinsert of a
        // field; undo must restore the ORIGINAL field order (lossless).
        let (mut ed, track) = timeline_with_clips();
        let clip0 = ed.document().node(track).unwrap().children("children")[0];
        let before = json(&ed);

        ops::set_source_range(&mut ed, clip0, None).unwrap();
        ops::set_name(&mut ed, clip0, "x").unwrap();
        ed.undo();
        ed.undo();
        assert_eq!(json(&ed), before, "field order/content drifted after undo");
    }

    #[test]
    fn created_and_map_child_via_tx_round_trip() {
        // Build a clip + media reference entirely through Tx (exercises Created,
        // MapChildSet with before=None, ChildInserted, FieldSet). Undo must
        // restore the EXACT prior JSON — including dropping the `media_references`
        // field the edit created (the empty-ChildMap fix).
        let (mut ed, track) = timeline_with_clips();
        let before = json(&ed);
        ed.edit("add clip", |tx| {
            let clip = tx.create_node("Clip", 2);
            let mref = tx.create_node("MissingReference", 1);
            tx.set_value(clip, "name", serde_json::json!("new"))?;
            tx.set_child_in_map(clip, "media_references", "DEFAULT_MEDIA", mref)?;
            tx.append_child(track, clip)?;
            Ok(())
        })
        .unwrap();
        let after = json(&ed);
        assert_ne!(before, after);
        assert!(ed.undo());
        assert_eq!(
            json(&ed),
            before,
            "undo of create+map+append must restore exactly"
        );
        assert!(ed.redo());
        assert_eq!(json(&ed), after, "redo must reproduce the added clip");
    }

    #[test]
    fn deep_clone_subtree_round_trips_and_gc_pins_it() {
        // The clone path (Tx::deep_clone -> record_created_subtree) is the
        // identity contract's hardest case: a cloned clip WITH its media ref must
        // round-trip and be gc-pinned as a whole subtree.
        let (mut ed, track) = timeline_with_clips();
        let clip0 = ed.document().node(track).unwrap().children("children")[0];
        let before = json(&ed);

        ed.edit("duplicate clip", |tx| {
            let clone = tx.deep_clone(clip0)?;
            tx.append_child(track, clone)?;
            Ok(())
        })
        .unwrap();
        let after = json(&ed);

        assert_eq!(ed.gc(), 0, "clone subtree pinned by history");
        assert!(ed.undo());
        assert_eq!(json(&ed), before, "undo of clone+append restores exactly");
        assert_eq!(ed.gc(), 0, "detached clone still pinned for redo");
        assert!(ed.redo());
        assert_eq!(json(&ed), after, "redo restores the duplicated clip");

        // After undoing and clearing history, the orphan clone subtree (clip +
        // its media ref) is reclaimed.
        ed.undo();
        let freed = ed.clear_history();
        assert!(
            freed >= 2,
            "clone root + media ref should be reclaimed, got {freed}"
        );
    }

    #[test]
    fn edit_after_undo_clears_redo() {
        let (mut ed, track) = timeline_with_clips();
        let clip0 = ed.document().node(track).unwrap().children("children")[0];
        ops::set_name(&mut ed, clip0, "A").unwrap();
        ops::set_name(&mut ed, clip0, "B").unwrap();
        assert!(ed.undo()); // back to "A"
        assert!(ed.can_redo());
        ops::set_name(&mut ed, clip0, "C").unwrap(); // new edit invalidates redo
        assert!(
            !ed.can_redo(),
            "a new edit after undo must clear the redo stack"
        );
    }

    #[test]
    fn failed_edit_rolls_back_atomically() {
        let (mut ed, track) = timeline_with_clips();
        let clip0 = ed.document().node(track).unwrap().children("children")[0];
        let before = json(&ed);
        let res = ed.edit("bad", |tx| {
            tx.set_value(clip0, "name", serde_json::json!("changed"))?;
            Err(EditError::NodeNotFound) // fail after a partial mutation
        });
        assert!(res.is_err());
        assert_eq!(
            json(&ed),
            before,
            "a failed edit must roll back its partial work"
        );
        assert!(!ed.can_undo(), "a rolled-back edit leaves no undo entry");
    }

    #[test]
    fn fresh_editors_have_distinct_initial_revisions() {
        let (first, _) = timeline_with_clips();
        let (second, _) = timeline_with_clips();
        assert_ne!(first.revision(), second.revision());
        assert_ne!(first.revision() >> 20, second.revision() >> 20);
    }

    #[test]
    fn undo_redo_past_ends_are_noops() {
        let (mut ed, _track) = timeline_with_clips();
        let s = json(&ed);
        let initial_revision = ed.revision();
        assert!(!ed.undo());
        assert!(!ed.redo());
        assert_eq!(json(&ed), s);
        assert_eq!(
            ed.revision(),
            initial_revision,
            "no-op undo/redo must not bump the unique initial revision"
        );
    }

    #[test]
    fn gc_keeps_history_referenced_nodes_then_frees_after_clear() {
        let (mut ed, track) = timeline_with_clips();
        let removed = ed.document().node(track).unwrap().children("children")[1];

        ops::ripple_delete(&mut ed, track, 1).unwrap();
        // The removed clip is detached but pinned by the undo stack: gc must keep it.
        assert_eq!(ed.gc(), 0);
        assert!(ed.document().node(removed).is_some());
        assert!(ed.undo()); // can still bring it back
        assert!(
            ed.document()
                .node(track)
                .unwrap()
                .children("children")
                .len()
                == 3
        );

        // After clearing history nothing pins the orphan; it is reclaimed.
        ops::ripple_delete(&mut ed, track, 1).unwrap();
        let freed = ed.clear_history();
        assert!(freed >= 1, "clear_history should reclaim the orphaned clip");
        assert!(ed.document().node(removed).is_none());
    }

    // ---- v1 edit ops --------------------------------------------------------

    /// Read a child clip's source_range as `(start, dur)` at 24 fps.
    fn sr_of(ed: &Editor, clip: otio_core::NodeId) -> (f64, f64) {
        let r = otio_algo::trimmed_range(ed.document(), clip).unwrap();
        (r.start_time().value(), r.duration().value())
    }

    #[test]
    fn trim_in_moves_in_point_and_round_trips() {
        let (mut ed, _t, ids) = timeline_with_media(&[(2.0, 6.0)], 20.0);
        let clip = ids[0];
        assert_undo_redo(&mut ed, |ed| {
            ops::trim_in(ed, clip, RationalTime::new(2.0, 24.0)).unwrap();
        });
        // sr' = (s+2, d-2) = (4, 4).
        assert_eq!(sr_of(&ed, clip), (4.0, 4.0));
    }

    #[test]
    fn record_trim_in_positive_inserts_gap_and_preserves_following_placement() {
        let (mut ed, track, ids) = timeline_with_media(&[(2.0, 6.0), (0.0, 4.0)], 20.0);
        let clip = ids[0];
        let following = ids[1];
        assert_undo_redo(&mut ed, |ed| {
            ops::trim_in_record(ed, clip, RationalTime::new(2.0, 24.0)).unwrap();
        });

        assert_eq!(sr_of(&ed, clip), (4.0, 4.0));
        let children = ed.document().node(track).unwrap().children("children");
        assert_eq!(children.len(), 3);
        assert_eq!(
            otio_core::canonical_schema(ed.document().node(children[0]).unwrap().schema_name()),
            "Gap"
        );
        assert_eq!(sr_of(&ed, children[0]), (0.0, 2.0));
        let ranges = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        let clip_range = ranges.iter().find(|(id, _)| *id == clip).unwrap().1;
        let following_range = ranges.iter().find(|(id, _)| *id == following).unwrap().1;
        assert_eq!(clip_range.start_time().value(), 2.0);
        assert_eq!(clip_range.end_time_exclusive().value(), 6.0);
        assert_eq!(following_range.start_time().value(), 6.0);
    }

    #[test]
    fn record_trim_in_positive_grows_existing_gap() {
        let (mut ed, track, ids) = timeline_with_gap_and_media(2.0, &[(2.0, 6.0)], 20.0);
        let clip = ids[0];
        assert_undo_redo(&mut ed, |ed| {
            ops::trim_in_record(ed, clip, RationalTime::new(1.0, 24.0)).unwrap();
        });

        let gap = ed.document().node(track).unwrap().children("children")[0];
        assert_eq!(sr_of(&ed, gap), (0.0, 3.0));
        assert_eq!(sr_of(&ed, clip), (3.0, 5.0));
        let clip_range = otio_algo::range_of_all_children(ed.document(), track)
            .unwrap()
            .into_iter()
            .find(|(id, _)| *id == clip)
            .unwrap()
            .1;
        assert_eq!(clip_range.end_time_exclusive().value(), 8.0);
    }

    #[test]
    fn record_trim_in_negative_consumes_gap_and_round_trips() {
        let (mut ed, track, ids) =
            timeline_with_gap_and_media(2.0, &[(5.0, 6.0), (0.0, 4.0)], 20.0);
        let clip = ids[0];
        let following = ids[1];
        assert_undo_redo(&mut ed, |ed| {
            ops::trim_in_record(ed, clip, RationalTime::new(-2.0, 24.0)).unwrap();
        });

        assert_eq!(sr_of(&ed, clip), (3.0, 8.0));
        let children = ed.document().node(track).unwrap().children("children");
        assert_eq!(
            children,
            &[clip, following],
            "fully consumed Gap must be removed"
        );
        let ranges = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        let clip_range = ranges.iter().find(|(id, _)| *id == clip).unwrap().1;
        let following_range = ranges.iter().find(|(id, _)| *id == following).unwrap().1;
        assert_eq!(clip_range.start_time().value(), 0.0);
        assert_eq!(clip_range.end_time_exclusive().value(), 8.0);
        assert_eq!(following_range.start_time().value(), 8.0);
    }

    #[test]
    fn record_trim_in_negative_requires_gap_and_media_headroom() {
        let (mut no_gap, _track, ids) = timeline_with_media(&[(5.0, 6.0)], 20.0);
        let before = json(&no_gap);
        let error =
            ops::trim_in_record(&mut no_gap, ids[0], RationalTime::new(-1.0, 24.0)).unwrap_err();
        assert_eq!(
            error,
            EditError::Structural(
                "record trim in requires a preceding Gap large enough for the extension".into()
            )
        );
        assert_eq!(
            error.to_string(),
            "structural error: record trim in requires a preceding Gap large enough for the extension"
        );
        assert_eq!(json(&no_gap), before);
        assert!(!no_gap.can_undo());

        let (mut no_media, _track, ids) = timeline_with_gap_and_media(2.0, &[(0.0, 6.0)], 20.0);
        let before = json(&no_media);
        let error =
            ops::trim_in_record(&mut no_media, ids[0], RationalTime::new(-1.0, 24.0)).unwrap_err();
        assert_eq!(error, EditError::OutOfMediaBounds);
        assert_eq!(json(&no_media), before);
        assert!(!no_media.can_undo());
    }

    #[test]
    fn record_trim_in_negative_cannot_exceed_preceding_gap() {
        let (mut ed, _track, ids) = timeline_with_gap_and_media(1.0, &[(5.0, 6.0)], 20.0);
        let before = json(&ed);
        let error =
            ops::trim_in_record(&mut ed, ids[0], RationalTime::new(-2.0, 24.0)).unwrap_err();
        assert_eq!(
            error,
            EditError::Structural(
                "record trim in requires a preceding Gap large enough for the extension".into()
            )
        );
        assert_eq!(json(&ed), before);
        assert!(!ed.can_undo());
    }

    #[test]
    fn trim_out_extends_tail_and_round_trips() {
        let (mut ed, _t, ids) = timeline_with_media(&[(2.0, 6.0)], 20.0);
        let clip = ids[0];
        assert_undo_redo(&mut ed, |ed| {
            ops::trim_out(ed, clip, RationalTime::new(3.0, 24.0)).unwrap();
        });
        // sr' = (s, d+3) = (2, 9).
        assert_eq!(sr_of(&ed, clip), (2.0, 9.0));
    }

    #[test]
    fn slip_shifts_media_keeps_duration() {
        let (mut ed, _t, ids) = timeline_with_media(&[(5.0, 6.0)], 20.0);
        let clip = ids[0];
        assert_undo_redo(&mut ed, |ed| {
            ops::slip(ed, clip, RationalTime::new(-3.0, 24.0)).unwrap();
        });
        // sr' = (s-3, d) = (2, 6).
        assert_eq!(sr_of(&ed, clip), (2.0, 6.0));
    }

    #[test]
    fn trim_in_out_of_media_bounds_errors() {
        // s=0 already at avail.start; trim_in by +? keeps start in-bounds, but
        // trimming the head below avail.start must error. Use start at 0 and a
        // negative delta to push start < 0.
        let (mut ed, _t, ids) = timeline_with_media(&[(0.0, 6.0)], 20.0);
        let clip = ids[0];
        let before = json(&ed);
        let e = ops::trim_in(&mut ed, clip, RationalTime::new(-1.0, 24.0)).unwrap_err();
        assert_eq!(e, EditError::OutOfMediaBounds);
        assert_eq!(json(&ed), before, "failed trim_in must not mutate");
    }

    #[test]
    fn slip_out_of_media_bounds_errors() {
        let (mut ed, _t, ids) = timeline_with_media(&[(0.0, 6.0)], 20.0);
        let clip = ids[0];
        // Pulling start negative leaves the media range.
        let e = ops::slip(&mut ed, clip, RationalTime::new(-1.0, 24.0)).unwrap_err();
        assert_eq!(e, EditError::OutOfMediaBounds);
        // Pushing the tail past avail end (0+ (17+6) > 20) also errors.
        let e2 = ops::slip(&mut ed, clip, RationalTime::new(17.0, 24.0)).unwrap_err();
        assert_eq!(e2, EditError::OutOfMediaBounds);
    }

    #[test]
    fn trim_across_transition_errors() {
        // Build a track: transition, then a clip. trim_in on the clip (prev is a
        // transition) must refuse.
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let trans = build::transition(
            &mut doc,
            "x",
            "SMPTE_Dissolve",
            RationalTime::new(2.0, 24.0),
            RationalTime::new(2.0, 24.0),
        );
        let _ = doc.append_child(track, trans);
        let mr = build::external_reference(&mut doc, "file://m.mov");
        if let Some(n) = doc.node_mut(mr) {
            n.set_value(
                "available_range",
                otio_schema::codec::time_range_value(tr(0.0, 20.0)),
            );
        }
        let clip = build::clip(&mut doc, "c", Some(tr(2.0, 6.0)), mr);
        let _ = doc.append_child(track, clip);
        let mut ed = Editor::new(doc);

        let e = ops::trim_in(&mut ed, clip, RationalTime::new(1.0, 24.0)).unwrap_err();
        assert_eq!(e, EditError::AcrossTransition);
    }

    #[test]
    fn razor_splits_child_and_round_trips() {
        let (mut ed, track) = timeline_with_clips();
        // Clips are (0,10),(0,10),(0,10) laid out at 0,10,20. Cut at t=13 lands in
        // the middle clip (range [10,20)), off = 3.
        assert_undo_redo(&mut ed, |ed| {
            ops::razor(ed, track, RationalTime::new(13.0, 24.0)).unwrap();
        });
        let kids = ed.document().node(track).unwrap().children("children");
        assert_eq!(kids.len(), 4, "razor adds one child");
        // Left half (idx 1) = (0,3); right half (idx 2) = (3,7).
        assert_eq!(sr_of(&ed, kids[1]), (0.0, 3.0));
        assert_eq!(sr_of(&ed, kids[2]), (3.0, 7.0));
    }

    #[test]
    fn razor_on_existing_cut_errors() {
        let (mut ed, track) = timeline_with_clips();
        // t=10 is exactly the boundary between clip 0 and clip 1 → off==0 on the
        // clip it falls into → rejected.
        let e = ops::razor(&mut ed, track, RationalTime::new(10.0, 24.0)).unwrap_err();
        assert_eq!(e, EditError::OutOfMediaBounds);
    }

    #[test]
    fn razor_on_transition_errors() {
        // A transition centered on a cut; razor at the cut time must refuse.
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let a = {
            let mr = build::missing_reference(&mut doc);
            build::clip(&mut doc, "a", Some(tr(0.0, 10.0)), mr)
        };
        let _ = doc.append_child(track, a);
        let trans = build::transition(
            &mut doc,
            "x",
            "SMPTE_Dissolve",
            RationalTime::new(2.0, 24.0),
            RationalTime::new(2.0, 24.0),
        );
        let _ = doc.append_child(track, trans);
        let b = {
            let mr = build::missing_reference(&mut doc);
            build::clip(&mut doc, "b", Some(tr(0.0, 10.0)), mr)
        };
        let _ = doc.append_child(track, b);
        let mut ed = Editor::new(doc);
        // Transition range is [10-2, 2+2) = [8,12). Cut at t=9 lands on it.
        let e = ops::razor(&mut ed, track, RationalTime::new(9.0, 24.0)).unwrap_err();
        assert_eq!(e, EditError::AcrossTransition);
    }

    #[test]
    fn lift_leaves_a_gap_and_round_trips() {
        let (mut ed, track) = timeline_with_clips();
        assert_undo_redo(&mut ed, |ed| {
            ops::lift(ed, track, 1).unwrap();
        });
        let kids = ed.document().node(track).unwrap().children("children");
        assert_eq!(
            kids.len(),
            3,
            "lift keeps the child count (gap replaces clip)"
        );
        assert_eq!(
            ed.document().node(kids[1]).unwrap().schema_name(),
            "Gap",
            "middle child is now a Gap"
        );
        assert_eq!(
            sr_of(&ed, kids[1]),
            (0.0, 10.0),
            "gap matches removed duration"
        );
    }

    #[test]
    fn overwrite_inside_a_clip_splits_and_round_trips() {
        // One long clip [0,20); overwrite a 4-frame source clip at t=8 → the base
        // clip is head-trimmed to [0,8) and a tail [12,20) remains, with the new
        // clip between.
        let (mut ed, track, _ids) = timeline_with_media(&[(0.0, 20.0)], 20.0);
        // Create the source clip in the live editor via an edit (so it is a node
        // in the editable document), detached, then overwrite places it.
        let mut src_id = None;
        ed.edit("stage src", |tx| {
            let mr = tx.create_node("ExternalReference", 1);
            tx.set_value(
                mr,
                "available_range",
                otio_schema::codec::time_range_value(tr(0.0, 10.0)),
            )?;
            let clip = tx.create_node("Clip", 2);
            tx.set_value(clip, "name", serde_json::json!("src"))?;
            tx.set_value(
                clip,
                "source_range",
                otio_schema::codec::time_range_value(tr(0.0, 4.0)),
            )?;
            tx.set_child_in_map(clip, "media_references", "DEFAULT_MEDIA", mr)?;
            tx.set_value(
                clip,
                "active_media_reference_key",
                serde_json::json!("DEFAULT_MEDIA"),
            )?;
            src_id = Some(clip);
            Ok(())
        })
        .unwrap();
        let src_id = src_id.unwrap();

        let before = json(&ed);
        ops::overwrite(&mut ed, track, src_id, RationalTime::new(8.0, 24.0)).unwrap();
        let after = json(&ed);
        assert_ne!(before, after);
        let ranges = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        let starts: Vec<f64> = ranges.iter().map(|(_, r)| r.start_time().value()).collect();
        let durs: Vec<f64> = ranges.iter().map(|(_, r)| r.duration().value()).collect();
        // Expect: base head [0,8), src [8,12), base tail [12,20) → durations 8,4,8.
        assert_eq!(starts, vec![0.0, 8.0, 12.0]);
        assert_eq!(durs, vec![8.0, 4.0, 8.0]);

        assert!(ed.undo());
        assert_eq!(json(&ed), before, "overwrite undo must restore exactly");
        assert!(ed.redo());
        assert_eq!(json(&ed), after, "overwrite redo must reproduce exactly");
    }

    #[test]
    fn overwrite_past_track_end_pads_gap() {
        // Track ends at 10 (one clip [0,10)); overwrite a 4-frame clip at t=14 →
        // pad a gap [10,14) and place src [14,18).
        let (mut ed, track, _ids) = timeline_with_media(&[(0.0, 10.0)], 20.0);
        let mut src_id = None;
        ed.edit("stage src", |tx| {
            let mr = tx.create_node("ExternalReference", 1);
            tx.set_value(
                mr,
                "available_range",
                otio_schema::codec::time_range_value(tr(0.0, 10.0)),
            )?;
            let clip = tx.create_node("Clip", 2);
            tx.set_value(clip, "name", serde_json::json!("src"))?;
            tx.set_value(
                clip,
                "source_range",
                otio_schema::codec::time_range_value(tr(0.0, 4.0)),
            )?;
            tx.set_child_in_map(clip, "media_references", "DEFAULT_MEDIA", mr)?;
            tx.set_value(
                clip,
                "active_media_reference_key",
                serde_json::json!("DEFAULT_MEDIA"),
            )?;
            src_id = Some(clip);
            Ok(())
        })
        .unwrap();
        let src_id = src_id.unwrap();

        let before = json(&ed);
        ops::overwrite(&mut ed, track, src_id, RationalTime::new(14.0, 24.0)).unwrap();
        let after = json(&ed);
        let ranges = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        let starts: Vec<f64> = ranges.iter().map(|(_, r)| r.start_time().value()).collect();
        let durs: Vec<f64> = ranges.iter().map(|(_, r)| r.duration().value()).collect();
        // base [0,10), gap [10,14), src [14,18).
        assert_eq!(starts, vec![0.0, 10.0, 14.0]);
        assert_eq!(durs, vec![10.0, 4.0, 4.0]);
        assert_eq!(
            ed.document().node(ranges[1].0).unwrap().schema_name(),
            "Gap"
        );

        assert!(ed.undo());
        assert_eq!(json(&ed), before);
        assert!(ed.redo());
        assert_eq!(json(&ed), after);
    }

    #[test]
    fn ripple_delete_merges_adjacent_gaps() {
        // Track: gap(5), gap(5), clip(10). Ripple-delete the clip at index 2 does
        // not create adjacency; instead delete index 1 (a gap) is between two? Use
        // a layout gap, clip, gap then delete the clip → the two gaps become
        // adjacent and must merge into one gap of summed duration.
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let g1 = build::gap(&mut doc, tr(0.0, 5.0));
        let _ = doc.append_child(track, g1);
        let clip = {
            let mr = build::missing_reference(&mut doc);
            build::clip(&mut doc, "c", Some(tr(0.0, 10.0)), mr)
        };
        let _ = doc.append_child(track, clip);
        let g2 = build::gap(&mut doc, tr(0.0, 5.0));
        let _ = doc.append_child(track, g2);
        let mut ed = Editor::new(doc);

        let before = json(&ed);
        ops::ripple_delete(&mut ed, track, 1).unwrap(); // remove the clip
        let kids = ed.document().node(track).unwrap().children("children");
        assert_eq!(kids.len(), 1, "two adjacent gaps merged into one");
        assert_eq!(ed.document().node(kids[0]).unwrap().schema_name(), "Gap");
        assert_eq!(sr_of(&ed, kids[0]), (0.0, 10.0), "summed gap duration");
        let after = json(&ed);

        assert!(ed.undo());
        assert_eq!(json(&ed), before, "ripple+merge undo restores exactly");
        assert!(ed.redo());
        assert_eq!(json(&ed), after);
    }

    #[test]
    fn add_remove_reorder_track_round_trip() {
        let (mut ed, _track) = timeline_with_clips();
        let stack = ed
            .document()
            .node(ed.document().root_id().unwrap())
            .unwrap()
            .child("tracks")
            .unwrap();

        // add_track returns the new id and round-trips.
        let before = json(&ed);
        let new_track = ops::add_track(&mut ed, stack, "V2", "Video").unwrap();
        let after_add = json(&ed);
        assert_ne!(before, after_add);
        assert_eq!(
            ed.document()
                .node(stack)
                .unwrap()
                .children("children")
                .len(),
            2
        );
        assert!(ed.undo());
        assert_eq!(json(&ed), before, "add_track undo restores exactly");
        assert!(ed.redo());
        assert_eq!(json(&ed), after_add);
        let _ = new_track;

        // reorder_track: move track 1 -> 0.
        assert_undo_redo(&mut ed, |ed| {
            ops::reorder_track(ed, stack, 1, 0).unwrap();
        });

        // remove_track.
        assert_undo_redo(&mut ed, |ed| {
            ops::remove_track(ed, stack, 1).unwrap();
        });
    }

    #[test]
    fn add_remove_marker_round_trip() {
        let (mut ed, track) = timeline_with_clips();
        let clip0 = child(&ed, track, 0);

        // add_marker into the (initially empty `[]`) markers list, round-trip.
        let before = json(&ed);
        ops::add_marker(&mut ed, clip0, tr(1.0, 2.0), "RED").unwrap();
        let after = json(&ed);
        assert_ne!(before, after);
        assert_eq!(
            ed.document().node(clip0).unwrap().children("markers").len(),
            1,
            "marker pushed into the list"
        );
        assert!(ed.undo());
        assert_eq!(json(&ed), before, "add_marker undo restores empty []");
        assert!(ed.redo());
        assert_eq!(json(&ed), after);

        // remove_marker the one we re-added.
        assert_undo_redo(&mut ed, |ed| {
            ops::remove_marker(ed, clip0, 0).unwrap();
        });
    }

    #[test]
    fn add_marker_undo_restores_absence_when_field_was_absent() {
        // Regression: a clip with NO `markers` field (as parsed from a `.otio`
        // that omits it). add_marker creates the field; undo must restore ABSENCE,
        // not leave an empty `"markers": []` (the named-child analogue of the
        // empty-ChildMap bug).
        let mut doc = Document::new();
        let clip = doc.create_node("Clip", 2);
        doc.node_mut(clip)
            .unwrap()
            .set_value("name", serde_json::json!("c"));
        doc.set_root(clip);
        let mut ed = Editor::new(doc);

        let before = json(&ed);
        assert!(
            before.get("markers").is_none(),
            "fixture has no markers field"
        );
        ops::add_marker(&mut ed, clip, tr(0.0, 1.0), "RED").unwrap();
        assert!(json(&ed).get("markers").is_some(), "add created the field");
        assert!(ed.undo());
        assert_eq!(json(&ed), before, "undo must restore field absence, not []");
        assert!(ed.redo());
        assert!(json(&ed).get("markers").is_some());
    }

    /// Create a standalone (unattached) clip with `source_range` `(0, dur)@24`,
    /// for use as the source of an overwrite. It floats in the doc until inserted.
    fn standalone_clip(ed: &mut Editor, dur: f64) -> otio_core::NodeId {
        let mut id = None;
        ed.edit("mk src", |tx| {
            let c = tx.create_node("Clip", 2);
            tx.set_value(
                c,
                "source_range",
                otio_schema::codec::time_range_value(tr(0.0, dur)),
            )?;
            id = Some(c);
            Ok(())
        })
        .unwrap();
        // NOTE: leave the bookkeeping edit in history — it pins the floating clip
        // against gc until overwrite attaches it. (clear_history would gc the
        // unattached orphan and invalidate the id.)
        id.unwrap()
    }

    #[test]
    fn overwrite_spans_two_whole_clips() {
        // A[0,10) B[10,20) C[20,30); overwrite src(dur 20) at t=5 → window [5,25):
        // A head-trimmed to [0,5), B fully covered (removed), C tail-trimmed to
        // [25,30); src lands [5,25). Result starts [0,5,25], durs [5,20,5].
        let (mut ed, track, _ids) =
            timeline_with_media(&[(0.0, 10.0), (0.0, 10.0), (0.0, 10.0)], 30.0);
        let src = standalone_clip(&mut ed, 20.0);

        ops::overwrite(&mut ed, track, src, RationalTime::new(5.0, 24.0)).unwrap();

        let layout = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        let starts: Vec<f64> = layout.iter().map(|(_, r)| r.start_time().value()).collect();
        let durs: Vec<f64> = layout.iter().map(|(_, r)| r.duration().value()).collect();
        assert_eq!(
            starts,
            vec![0.0, 5.0, 25.0],
            "spanning overwrite layout starts"
        );
        assert_eq!(
            durs,
            vec![5.0, 20.0, 5.0],
            "spanning overwrite layout durations"
        );
        assert_eq!(layout.len(), 3, "B fully covered → removed");
        assert_eq!(layout[1].0, src, "src occupies the window");
    }

    #[test]
    fn reorder_track_forward_move_lands_after_shift() {
        // Stack [A,B,C]; reorder 0→2 must yield [B,C,A] (to is post-removal).
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        for name in ["A", "B", "C"] {
            let t = build::track(&mut doc, name, "Video");
            let _ = doc.append_child(stack, t);
        }
        let mut ed = Editor::new(doc);
        ops::reorder_track(&mut ed, stack, 0, 2).unwrap();
        let names: Vec<String> = ed
            .document()
            .node(stack)
            .unwrap()
            .children("children")
            .iter()
            .map(|&t| {
                ed.document()
                    .node(t)
                    .unwrap()
                    .field("name")
                    .and_then(|f| match f {
                        otio_core::Field::Value(v) => v.as_str().map(|s| s.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(
            names,
            vec!["B", "C", "A"],
            "forward move lands after the shift"
        );
        assert!(ed.undo());
        let back: Vec<String> = ed
            .document()
            .node(stack)
            .unwrap()
            .children("children")
            .iter()
            .map(|&t| {
                ed.document()
                    .node(t)
                    .unwrap()
                    .field("name")
                    .and_then(|f| match f {
                        otio_core::Field::Value(v) => v.as_str().map(|s| s.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(back, vec!["A", "B", "C"], "undo restores original order");
    }

    #[test]
    fn razor_rescales_offset_on_rate_mismatch() {
        // A 48 fps clip on a track; cut at a track-local time and confirm the
        // split source ranges sum to the original and the offset is rescaled.
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let mr = build::missing_reference(&mut doc);
        // source_range (0,96)@48 → 2.0s footprint.
        let sr = TimeRange::new(RationalTime::new(0.0, 48.0), RationalTime::new(96.0, 48.0));
        let clip = build::clip(&mut doc, "c", Some(sr), mr);
        let _ = doc.append_child(track, clip);
        let mut ed = Editor::new(doc);

        // Cut at 1.0s = 24 frames @24 track-time → maps to 48 source frames @48.
        ops::razor(&mut ed, track, RationalTime::new(24.0, 24.0)).unwrap();
        let kids = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .to_vec();
        assert_eq!(kids.len(), 2);
        let left = otio_algo::trimmed_range(ed.document(), kids[0]).unwrap();
        let right = otio_algo::trimmed_range(ed.document(), kids[1]).unwrap();
        assert_eq!(
            left.duration().value(),
            48.0,
            "left half = 48 source frames @48"
        );
        assert_eq!(left.duration().rate(), 48.0);
        assert_eq!(
            right.duration().value(),
            48.0,
            "right half = 48 source frames @48"
        );
        assert_eq!(
            right.start_time().value(),
            48.0,
            "right starts at source frame 48"
        );
    }

    #[test]
    fn merge_collapses_three_gap_run() {
        // Track [gap(3), clip(5), gap(4), gap(2)] → ripple the clip → the run
        // [gap(3), gap(4), gap(2)] collapses to a single gap of duration 9.
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let g0 = build::gap(&mut doc, tr(0.0, 3.0));
        let _ = doc.append_child(track, g0);
        let mr = build::missing_reference(&mut doc);
        let clip = build::clip(&mut doc, "c", Some(tr(0.0, 5.0)), mr);
        let _ = doc.append_child(track, clip);
        let g1 = build::gap(&mut doc, tr(0.0, 4.0));
        let _ = doc.append_child(track, g1);
        let g2 = build::gap(&mut doc, tr(0.0, 2.0));
        let _ = doc.append_child(track, g2);
        let mut ed = Editor::new(doc);

        ops::ripple_delete(&mut ed, track, 1).unwrap(); // remove the clip
        let kids = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .to_vec();
        assert_eq!(kids.len(), 1, "three adjacent gaps collapse to one");
        let r = otio_algo::trimmed_range(ed.document(), kids[0]).unwrap();
        assert_eq!(r.duration().value(), 9.0, "summed gap duration 3+4+2");
    }

    #[test]
    fn remove_marker_mid_list_keeps_neighbors() {
        let (mut ed, track) = timeline_with_clips();
        let clip0 = child(&ed, track, 0);
        ops::add_marker(&mut ed, clip0, tr(0.0, 1.0), "RED").unwrap();
        ops::add_marker(&mut ed, clip0, tr(1.0, 1.0), "GREEN").unwrap();
        ops::add_marker(&mut ed, clip0, tr(2.0, 1.0), "BLUE").unwrap();
        let before = json(&ed);

        ops::remove_marker(&mut ed, clip0, 1).unwrap(); // remove GREEN (middle)
        let colors: Vec<String> = ed
            .document()
            .node(clip0)
            .unwrap()
            .children("markers")
            .iter()
            .map(|&m| {
                ed.document()
                    .node(m)
                    .unwrap()
                    .field("color")
                    .and_then(|f| match f {
                        otio_core::Field::Value(v) => v.as_str().map(|s| s.to_string()),
                        _ => None,
                    })
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(
            colors,
            vec!["RED", "BLUE"],
            "mid-list remove keeps neighbors in order"
        );
        assert!(ed.undo());
        assert_eq!(
            json(&ed),
            before,
            "undo restores all three markers in order"
        );
    }

    #[test]
    fn lift_adjacent_to_gap_merges() {
        // Track [clip(10), gap(5), clip(10)]; lift index 0 → its gap(10) abuts the
        // existing gap(5); the invariant merges them into one gap(15).
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let mr0 = build::missing_reference(&mut doc);
        let c0 = build::clip(&mut doc, "a", Some(tr(0.0, 10.0)), mr0);
        let _ = doc.append_child(track, c0);
        let g = build::gap(&mut doc, tr(0.0, 5.0));
        let _ = doc.append_child(track, g);
        let mr1 = build::missing_reference(&mut doc);
        let c1 = build::clip(&mut doc, "b", Some(tr(0.0, 10.0)), mr1);
        let _ = doc.append_child(track, c1);
        let mut ed = Editor::new(doc);

        ops::lift(&mut ed, track, 0).unwrap();
        let kids = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .to_vec();
        // lift's gap(10) + existing gap(5) merge → [gap(15), clip].
        assert_eq!(kids.len(), 2, "lifted gap merges with the adjacent gap");
        let r0 = otio_algo::trimmed_range(ed.document(), kids[0]).unwrap();
        assert_eq!(r0.duration().value(), 15.0, "merged gap duration 10+5");
    }

    // ---- v2 edit ops --------------------------------------------------------

    #[test]
    fn roll_moves_edit_point_and_round_trips() {
        // Two abutting clips A[src 2,d6] B[src 2,d6] on a track, media avail [0,20).
        // roll A by +2: A → (2, 8), B → (4, 4). The shared edit point moves +2; the
        // pair's total footprint (12) is unchanged.
        let (mut ed, _t, ids) = timeline_with_media(&[(2.0, 6.0), (2.0, 6.0)], 20.0);
        let (a, b) = (ids[0], ids[1]);
        assert_undo_redo(&mut ed, |ed| {
            ops::roll(ed, a, RationalTime::new(2.0, 24.0)).unwrap();
        });
        assert_eq!(sr_of(&ed, a), (2.0, 8.0), "A grew its tail by +2");
        assert_eq!(sr_of(&ed, b), (4.0, 4.0), "B shrank its head by +2");
    }

    #[test]
    fn roll_past_media_bounds_errors() {
        // A[src 2,d6] (avail [0,20) → A end can reach 20), B[src 0,d6].
        // roll by +13: A new end = 2+6+13 = 21 > 20 → OutOfMediaBounds.
        let (mut ed, _t, ids) = timeline_with_media(&[(2.0, 6.0), (0.0, 6.0)], 20.0);
        let a = ids[0];
        let before = json(&ed);
        let e = ops::roll(&mut ed, a, RationalTime::new(13.0, 24.0)).unwrap_err();
        assert_eq!(e, EditError::OutOfMediaBounds);
        assert_eq!(json(&ed), before, "failed roll must not mutate");
        // Also B-side: roll by -1 pulls B start to 0-1 = -1 < avail.start(0) → error.
        let e2 = ops::roll(&mut ed, a, RationalTime::new(-1.0, 24.0)).unwrap_err();
        assert_eq!(e2, EditError::OutOfMediaBounds);
        assert_eq!(
            json(&ed),
            before,
            "failed B-side roll must not mutate either"
        );
    }

    #[test]
    fn roll_across_transition_errors() {
        // Track: A, transition, B. roll on A — the right sibling is the transition,
        // so roll must refuse with AcrossTransition.
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let a = {
            let mr = build::missing_reference(&mut doc);
            build::clip(&mut doc, "a", Some(tr(0.0, 10.0)), mr)
        };
        let _ = doc.append_child(track, a);
        let trans = build::transition(
            &mut doc,
            "x",
            "SMPTE_Dissolve",
            RationalTime::new(2.0, 24.0),
            RationalTime::new(2.0, 24.0),
        );
        let _ = doc.append_child(track, trans);
        let b = {
            let mr = build::missing_reference(&mut doc);
            build::clip(&mut doc, "b", Some(tr(0.0, 10.0)), mr)
        };
        let _ = doc.append_child(track, b);
        let mut ed = Editor::new(doc);

        let e = ops::roll(&mut ed, a, RationalTime::new(1.0, 24.0)).unwrap_err();
        assert_eq!(e, EditError::AcrossTransition);
    }

    #[test]
    fn slide_with_gap_neighbors_resizes_and_round_trips() {
        // Track [gap(5), clip(10), gap(8)]; slide the clip by +3 → prev gap grows to
        // 8, next gap shrinks to 5; the clip's own source_range is unchanged and its
        // footprint moves from [5,15) to [8,18).
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let g0 = build::gap(&mut doc, tr(0.0, 5.0));
        let _ = doc.append_child(track, g0);
        let mr = build::missing_reference(&mut doc);
        let clip = build::clip(&mut doc, "c", Some(tr(0.0, 10.0)), mr);
        let _ = doc.append_child(track, clip);
        let g1 = build::gap(&mut doc, tr(0.0, 8.0));
        let _ = doc.append_child(track, g1);
        let mut ed = Editor::new(doc);

        let clip_sr_before = sr_of(&ed, clip);
        assert_undo_redo(&mut ed, |ed| {
            ops::slide(ed, clip, RationalTime::new(3.0, 24.0)).unwrap();
        });
        let kids = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .to_vec();
        assert_eq!(kids.len(), 3, "slide keeps the child count");
        assert_eq!(sr_of(&ed, kids[0]), (0.0, 8.0), "prev gap grew by +3 → 8");
        assert_eq!(sr_of(&ed, kids[2]), (0.0, 5.0), "next gap shrank by 3 → 5");
        assert_eq!(
            sr_of(&ed, clip),
            clip_sr_before,
            "clip source_range unchanged"
        );
        let layout = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        assert_eq!(
            layout[1].1.start_time().value(),
            8.0,
            "clip moved to start 8"
        );
    }

    #[test]
    fn slide_gap_neighbor_clamp_errors() {
        // Track [gap(2), clip(10), gap(8)]; slide by -3 would shrink the prev gap to
        // -1 (<= 0) → OutOfMediaBounds, no mutation.
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let g0 = build::gap(&mut doc, tr(0.0, 2.0));
        let _ = doc.append_child(track, g0);
        let mr = build::missing_reference(&mut doc);
        let clip = build::clip(&mut doc, "c", Some(tr(0.0, 10.0)), mr);
        let _ = doc.append_child(track, clip);
        let g1 = build::gap(&mut doc, tr(0.0, 8.0));
        let _ = doc.append_child(track, g1);
        let mut ed = Editor::new(doc);

        let before = json(&ed);
        let e = ops::slide(&mut ed, clip, RationalTime::new(-3.0, 24.0)).unwrap_err();
        assert_eq!(e, EditError::OutOfMediaBounds);
        assert_eq!(json(&ed), before, "failed slide must roll back atomically");
    }

    #[test]
    fn move_clip_lands_at_new_index_and_leaves_gap() {
        // Track [A(10), B(10), C(10)] at [0,10,20). Move A (index 0) to index 2:
        // lift leaves a gap(10) at index 0 (so B,C stay at [10,20)), then A is
        // overwritten at C's start (20) — landing past the track end pads nothing
        // because 20 == track-end-before? track ends at 30. target=start of child
        // at index 2. After lift+merge the track is [gap, B, C]; child at index 2
        // is C with start 20 → A overwrites C.
        let (mut ed, track, ids) =
            timeline_with_media(&[(0.0, 10.0), (0.0, 10.0), (0.0, 10.0)], 10.0);
        let a = ids[0];
        let before = json(&ed);
        ops::move_clip(&mut ed, a, 2).unwrap();
        let after = json(&ed);
        assert_ne!(before, after);

        let layout = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        let starts: Vec<f64> = layout.iter().map(|(_, r)| r.start_time().value()).collect();
        // Source slot (index 0) is now a gap; A sits where C was (start 20).
        assert_eq!(
            ed.document().node(layout[0].0).unwrap().schema_name(),
            "Gap",
            "move leaves a gap at the source slot"
        );
        assert!(starts.contains(&20.0), "A landed at the index-2 start (20)");
        let a_layout = layout
            .iter()
            .find(|(id, _)| *id == a)
            .expect("A still present");
        assert_eq!(a_layout.1.start_time().value(), 20.0, "A moved to start 20");

        assert!(ed.undo());
        assert_eq!(json(&ed), before, "move_clip undo restores exactly");
        assert!(ed.redo());
        assert_eq!(json(&ed), after, "move_clip redo reproduces exactly");
    }

    #[test]
    fn move_by_leaves_prev_clip_untouched_and_opens_a_gap() {
        // The reported bug: with A[0,10) then B[10,20), dragging B away from A must
        // NOT stretch A's out-point. `move_by` repositions B via lift+overwrite,
        // opening a gap where it was; A is untouched. (Contrast `slide`, which
        // rolls A's tail so its out follows B — exactly what we do NOT want here.)
        let (mut ed, track, ids) = timeline_with_media(&[(0.0, 10.0), (0.0, 10.0)], 30.0);
        let (a, b) = (ids[0], ids[1]);
        let a_sr_before = sr_of(&ed, a);

        assert_undo_redo(&mut ed, |ed| {
            ops::move_by(ed, b, RationalTime::new(5.0, 24.0)).unwrap();
        });

        // A's source_range is UNCHANGED — its out-point did NOT follow B.
        assert_eq!(
            sr_of(&ed, a),
            a_sr_before,
            "prev clip A untouched by moving B"
        );
        let layout = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        assert_eq!(layout[0].1.start_time().value(), 0.0, "A still starts at 0");
        assert_eq!(
            layout[0].1.duration().value(),
            10.0,
            "A keeps its 10-frame footprint (out-point not stretched)"
        );
        let b_layout = layout.iter().find(|(id, _)| *id == b).expect("B present");
        assert_eq!(
            b_layout.1.start_time().value(),
            15.0,
            "B moved to start 15 (10 + 5)"
        );
        assert!(
            layout.iter().any(
                |(id, r)| ed.document().node(*id).unwrap().schema_name() == "Gap"
                    && r.start_time().value() == 10.0
            ),
            "a gap opened where B used to start"
        );
    }

    #[test]
    fn lift_clip_replaces_with_gap_and_keeps_layout() {
        // A[0,10) then B[10,20). lift_clip(B) replaces B with a same-duration gap
        // so A is untouched and the track footprint (end 20) is unchanged.
        let (mut ed, track, ids) = timeline_with_media(&[(0.0, 10.0), (0.0, 10.0)], 30.0);
        let (a, b) = (ids[0], ids[1]);
        let a_sr = sr_of(&ed, a);
        assert_undo_redo(&mut ed, |ed| {
            ops::lift_clip(ed, b).unwrap();
        });
        assert_eq!(sr_of(&ed, a), a_sr, "A untouched by deleting B");
        let kids = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .to_vec();
        assert_eq!(kids.len(), 2, "B became a gap, child count unchanged");
        assert_eq!(
            ed.document().node(kids[1]).unwrap().schema_name(),
            "Gap",
            "B's slot is now a gap"
        );
        let layout = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        assert_eq!(
            layout.last().unwrap().1.end_time_exclusive().value(),
            20.0,
            "track footprint unchanged (lift leaves a gap)"
        );
    }

    #[test]
    fn ripple_delete_clip_closes_the_gap() {
        // A[0,10) then B[10,20). ripple_delete_clip(B) removes B and closes up, so
        // A is untouched and the track shrinks to just A.
        let (mut ed, track, ids) = timeline_with_media(&[(0.0, 10.0), (0.0, 10.0)], 30.0);
        let (a, b) = (ids[0], ids[1]);
        let a_sr = sr_of(&ed, a);
        assert_undo_redo(&mut ed, |ed| {
            ops::ripple_delete_clip(ed, b).unwrap();
        });
        assert_eq!(sr_of(&ed, a), a_sr, "A untouched by ripple-deleting B");
        let kids = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .to_vec();
        assert_eq!(kids.len(), 1, "B removed, no gap left behind");
        let layout = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        assert_eq!(
            layout.last().unwrap().1.end_time_exclusive().value(),
            10.0,
            "track closed up to A only"
        );
    }

    #[test]
    fn ripple_delete_clip_on_a_gap_closes_it() {
        // A[0,10), B[10,20). Lift B -> [A, gap]; ripple-deleting that gap leaves
        // just [A] with the space closed (Premiere's "delete a selected gap").
        let (mut ed, track, ids) = timeline_with_media(&[(0.0, 10.0), (0.0, 10.0)], 30.0);
        ops::lift_clip(&mut ed, ids[1]).unwrap();
        let gap = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .get(1)
            .copied()
            .expect("gap at index 1 after lift");
        assert_eq!(ed.document().node(gap).unwrap().schema_name(), "Gap");
        assert_undo_redo(&mut ed, |ed| {
            ops::ripple_delete_clip(ed, gap).unwrap();
        });
        let kids = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .to_vec();
        assert_eq!(kids.len(), 1, "only A remains, the gap closed up");
        let layout = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        assert_eq!(
            layout.last().unwrap().1.end_time_exclusive().value(),
            10.0,
            "footprint closed to A only"
        );
    }

    #[test]
    fn three_point_sets_source_window_and_overwrites() {
        // Base track: one clip [0,20). Stage a source clip with media avail [0,30).
        // three_point(src_in=5, src_out=11, timeline_in=8): src source_range becomes
        // (5,6); it is overwritten at t=8 → base splits to head[0,8), src[8,14),
        // tail[14,20).
        let (mut ed, track, _ids) = timeline_with_media(&[(0.0, 20.0)], 20.0);
        let mut src_id = None;
        ed.edit("stage src", |tx| {
            let mr = tx.create_node("ExternalReference", 1);
            tx.set_value(
                mr,
                "available_range",
                otio_schema::codec::time_range_value(tr(0.0, 30.0)),
            )?;
            let clip = tx.create_node("Clip", 2);
            tx.set_value(clip, "name", serde_json::json!("src"))?;
            tx.set_value(
                clip,
                "source_range",
                otio_schema::codec::time_range_value(tr(0.0, 4.0)),
            )?;
            tx.set_child_in_map(clip, "media_references", "DEFAULT_MEDIA", mr)?;
            tx.set_value(
                clip,
                "active_media_reference_key",
                serde_json::json!("DEFAULT_MEDIA"),
            )?;
            src_id = Some(clip);
            Ok(())
        })
        .unwrap();
        let src = src_id.unwrap();

        let before = json(&ed);
        ops::three_point(
            &mut ed,
            track,
            src,
            RationalTime::new(5.0, 24.0),
            RationalTime::new(11.0, 24.0),
            RationalTime::new(8.0, 24.0),
        )
        .unwrap();
        let after = json(&ed);
        assert_ne!(before, after);

        // src source_range is the (src_in, src_out-src_in) window.
        assert_eq!(sr_of(&ed, src), (5.0, 6.0), "source window (5, 11-5)");
        let layout = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        let starts: Vec<f64> = layout.iter().map(|(_, r)| r.start_time().value()).collect();
        let durs: Vec<f64> = layout.iter().map(|(_, r)| r.duration().value()).collect();
        assert_eq!(starts, vec![0.0, 8.0, 14.0], "head/src/tail starts");
        assert_eq!(durs, vec![8.0, 6.0, 6.0], "head 8, src 6, tail 6");

        assert!(ed.undo());
        assert_eq!(json(&ed), before, "three_point undo restores exactly");
        assert!(ed.redo());
        assert_eq!(json(&ed), after);
    }

    #[test]
    fn three_point_inverted_window_errors() {
        let (mut ed, track, _ids) = timeline_with_media(&[(0.0, 20.0)], 20.0);
        let src = standalone_clip(&mut ed, 4.0);
        let before = json(&ed);
        let e = ops::three_point(
            &mut ed,
            track,
            src,
            RationalTime::new(11.0, 24.0),
            RationalTime::new(5.0, 24.0), // out < in
            RationalTime::new(8.0, 24.0),
        )
        .unwrap_err();
        assert_eq!(e, EditError::OutOfMediaBounds);
        assert_eq!(json(&ed), before, "inverted three_point must not mutate");
    }

    // ---- snap (pure helper) -------------------------------------------------

    #[test]
    fn snap_within_threshold_picks_nearest() {
        let targets = [
            RationalTime::new(0.0, 24.0),
            RationalTime::new(10.0, 24.0),
            RationalTime::new(20.0, 24.0),
        ];
        // candidate 9 is 1 from 10, 9 from 0 → snaps to 10 (within threshold 2).
        let out = snap::snap(
            &targets,
            RationalTime::new(9.0, 24.0),
            RationalTime::new(2.0, 24.0),
        );
        assert_eq!(
            out.value(),
            10.0,
            "snaps to the nearest edge within threshold"
        );
        assert_eq!(
            out.rate(),
            24.0,
            "returns the target itself (rate preserved)"
        );
    }

    #[test]
    fn snap_outside_threshold_returns_candidate() {
        let targets = [RationalTime::new(0.0, 24.0), RationalTime::new(20.0, 24.0)];
        // candidate 10 is 10 from each → outside threshold 2 → unchanged.
        let cand = RationalTime::new(10.0, 24.0);
        let out = snap::snap(&targets, cand, RationalTime::new(2.0, 24.0));
        assert_eq!(
            out.value(),
            10.0,
            "no target within threshold → candidate unchanged"
        );
    }

    #[test]
    fn snap_tie_breaks_to_first_target() {
        // candidate 10 equidistant (5) from 5 and 15; first (5) wins on the tie.
        let targets = [RationalTime::new(5.0, 24.0), RationalTime::new(15.0, 24.0)];
        let out = snap::snap(
            &targets,
            RationalTime::new(10.0, 24.0),
            RationalTime::new(6.0, 24.0),
        );
        assert_eq!(
            out.value(),
            5.0,
            "exact tie picks the first equally-near target"
        );
    }

    #[test]
    fn snap_targets_collects_edges_and_track_start() {
        // Track [clip(10), clip(10)] → edges at 0,10,10,20 plus track start 0.
        let (mut ed, track, _ids) = timeline_with_media(&[(0.0, 10.0), (0.0, 10.0)], 10.0);
        let _ = &mut ed;
        let targets = snap::snap_targets(ed.document(), track);
        let vals: Vec<f64> = targets.iter().map(|t| t.value()).collect();
        // track-start(0) + clip0 [0,10) + clip1 [10,20).
        assert_eq!(vals, vec![0.0, 0.0, 10.0, 10.0, 20.0]);
    }

    // ---- v2 review additions: lock the math-confirmed-correct edge paths -----

    /// Track-local start of each child (layout order).
    fn starts(ed: &Editor, track: otio_core::NodeId) -> Vec<f64> {
        otio_algo::range_of_all_children(ed.document(), track)
            .unwrap()
            .iter()
            .map(|(_, r)| r.start_time().value())
            .collect()
    }

    #[test]
    fn slide_with_two_clip_neighbors_rolls_both_edges() {
        // [A(0,10) B(0,10) C(0,10)] @ avail20. Slide B by +3: prev clip A rolls its
        // tail +3 (→ dur 13), next clip C rolls its head +3 (→ start 3, dur 7); B's
        // own source_range is unchanged. This is the riskiest slide branch (inline
        // roll math on both neighbours) and had no forward coverage.
        let (mut ed, track, ids) =
            timeline_with_media(&[(0.0, 10.0), (0.0, 10.0), (0.0, 10.0)], 20.0);
        let (a, b, c) = (ids[0], ids[1], ids[2]);
        let b_before = sr_of(&ed, b);
        assert_undo_redo(&mut ed, |ed| {
            ops::slide(ed, b, RationalTime::new(3.0, 24.0)).unwrap();
        });
        assert_eq!(sr_of(&ed, a), (0.0, 13.0), "prev clip A tail grew +3");
        assert_eq!(sr_of(&ed, c), (3.0, 7.0), "next clip C head shrank +3");
        assert_eq!(
            sr_of(&ed, b),
            b_before,
            "slid clip's source_range unchanged"
        );
        assert_eq!(
            starts(&ed, track),
            vec![0.0, 13.0, 23.0],
            "total length invariant"
        );
    }

    #[test]
    fn slide_at_track_start_inserts_lead_in_gap() {
        // [clip(0,10), gap(8)]; slide clip (index 0) +3: no prev neighbour → a
        // lead-in gap(3) is inserted; next gap shrinks 8→5. Clip sr unchanged.
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let mr = build::missing_reference(&mut doc);
        let clip = build::clip(&mut doc, "c", Some(tr(0.0, 10.0)), mr);
        let _ = doc.append_child(track, clip);
        let g = build::gap(&mut doc, tr(0.0, 8.0));
        let _ = doc.append_child(track, g);
        let mut ed = Editor::new(doc);

        let clip_before = sr_of(&ed, clip);
        ops::slide(&mut ed, clip, RationalTime::new(3.0, 24.0)).unwrap();
        let kids = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .to_vec();
        assert_eq!(kids.len(), 3, "a lead-in gap was inserted");
        assert!(is_gap(ed.document(), kids[0]), "front is a gap");
        assert_eq!(sr_of(&ed, kids[0]), (0.0, 3.0), "lead-in gap dur = delta");
        assert_eq!(sr_of(&ed, clip), clip_before, "clip source_range unchanged");
        assert_eq!(
            starts(&ed, track),
            vec![0.0, 3.0, 13.0],
            "clip pushed to start 3"
        );
        assert!(ed.undo());

        // delta<0 with no prev neighbour → refuse (no room before track start).
        let before = json(&ed);
        let e = ops::slide(&mut ed, clip, RationalTime::new(-1.0, 24.0)).unwrap_err();
        assert_eq!(e, EditError::OutOfMediaBounds);
        assert_eq!(json(&ed), before, "refused slide must not mutate");
    }

    /// Whether a node is a canonical `Gap` (test-local mirror of the ops helper).
    fn is_gap(doc: &Document, id: otio_core::NodeId) -> bool {
        doc.node(id)
            .map(|n| otio_core::canonical_schema(n.schema_name()) == "Gap")
            .unwrap_or(false)
    }

    #[test]
    fn move_clip_backward_preserves_layout() {
        // [A,B,C] @ [0,10,20]. Move C (index 2) back to index 0: target = start of
        // pre-lift child at index 0 (A, start 0); lift C leaves a tail gap; overwrite
        // C at 0 covers A. Result: C at 0, B intact, a gap fills C's old slot.
        let (mut ed, track, ids) =
            timeline_with_media(&[(0.0, 10.0), (0.0, 10.0), (0.0, 10.0)], 10.0);
        let (b, c) = (ids[1], ids[2]);
        ops::move_clip(&mut ed, c, 0).unwrap();
        let layout = otio_algo::range_of_all_children(ed.document(), track).unwrap();
        assert_eq!(
            starts(&ed, track),
            vec![0.0, 10.0, 20.0],
            "abutting layout preserved"
        );
        assert_eq!(layout[0].0, c, "C now occupies the head");
        assert_eq!(layout[1].0, b, "B survived intact");
        assert_eq!(sr_of(&ed, b), (0.0, 10.0), "B duration unchanged");
        // Round-trips.
        assert!(ed.undo());
        assert_eq!(starts(&ed, track), vec![0.0, 10.0, 20.0]);
        assert_eq!(
            otio_algo::range_of_all_children(ed.document(), track).unwrap()[0].0,
            ids[0],
            "undo restores A at the head"
        );
    }

    #[test]
    fn roll_with_gap_neighbor_resizes_only_dur() {
        // [clip(2,6) @ avail20, gap(0,6)]; roll the clip by +2 → clip (2,8), gap
        // shrinks 6→4. A gap has no media bounds, so only its duration changes.
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let mr = build::external_reference(&mut doc, "file://m.mov");
        if let Some(n) = doc.node_mut(mr) {
            n.set_value(
                "available_range",
                otio_schema::codec::time_range_value(tr(0.0, 20.0)),
            );
        }
        let clip = build::clip(&mut doc, "c", Some(tr(2.0, 6.0)), mr);
        let _ = doc.append_child(track, clip);
        let g = build::gap(&mut doc, tr(0.0, 6.0));
        let _ = doc.append_child(track, g);
        let mut ed = Editor::new(doc);

        assert_undo_redo(&mut ed, |ed| {
            ops::roll(ed, clip, RationalTime::new(2.0, 24.0)).unwrap();
        });
        assert_eq!(sr_of(&ed, clip), (2.0, 8.0), "clip tail grew +2");
        assert_eq!(
            sr_of(&ed, g),
            (0.0, 4.0),
            "gap absorbed the roll (dur only)"
        );
    }

    #[test]
    fn snap_mixed_rate_target_orders_by_seconds() {
        // The headline claim: distance is measured in SECONDS, so a 24fps candidate
        // snaps to the nearest edge even when it is at a different rate, and the
        // returned time keeps the TARGET's rate.
        let targets = [RationalTime::new(0.0, 24.0), RationalTime::new(48.0, 48.0)]; // 0s, 1.0s
        let cand = RationalTime::new(26.0, 24.0); // ≈1.083s
        let out = snap::snap(&targets, cand, RationalTime::new(4.0, 24.0)); // thr ≈0.167s
        assert_eq!(
            out.value(),
            48.0,
            "snapped to the 1.0s edge by seconds-distance"
        );
        assert_eq!(out.rate(), 48.0, "target rate preserved");
    }

    // ---- regression locks: ops.rs edit-fix guards --------------------------
    //
    // These pin the four bughunt fixes (see .bughunt/fix_edit.md). Each asserts
    // the EXACT post-fix behaviour AND that a rejected op leaves the document
    // byte-identical (no partial mutation), so a future refactor that re-breaks
    // a guard fails loudly instead of silently corrupting layout.

    /// Build a single-track timeline `[clip a, transition, clip b]` (no media
    /// bounds needed) and return `(editor, track)`. Mirrors the construction in
    /// `razor_on_transition_errors`.
    fn track_with_transition() -> (Editor, otio_core::NodeId) {
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let a = {
            let mr = build::missing_reference(&mut doc);
            build::clip(&mut doc, "a", Some(tr(0.0, 10.0)), mr)
        };
        let _ = doc.append_child(track, a);
        let trans = build::transition(
            &mut doc,
            "x",
            "SMPTE_Dissolve",
            RationalTime::new(2.0, 24.0),
            RationalTime::new(2.0, 24.0),
        );
        let _ = doc.append_child(track, trans);
        let b = {
            let mr = build::missing_reference(&mut doc);
            build::clip(&mut doc, "b", Some(tr(0.0, 10.0)), mr)
        };
        let _ = doc.append_child(track, b);
        (Editor::new(doc), track)
    }

    #[test]
    fn lift_on_transition_index_errors_and_does_not_mutate() {
        // MEDIUM-1: lifting the transition (index 1) must refuse with
        // AcrossTransition — NOT inject a same-"duration" gap that would shove
        // every downstream item right by in+out and tear the cross-fade apart.
        let (mut ed, track) = track_with_transition();
        // Sanity: index 1 really is the transition (so the guard, not an
        // out-of-range index, is what rejects the op).
        assert_eq!(
            ed.document()
                .node(track)
                .unwrap()
                .children("children")
                .len(),
            3
        );
        assert_eq!(
            otio_core::canonical_schema(
                ed.document()
                    .node(child(&ed, track, 1))
                    .unwrap()
                    .schema_name()
            ),
            "Transition"
        );
        let before = json(&ed);
        let e = ops::lift(&mut ed, track, 1).unwrap_err();
        assert_eq!(
            e,
            EditError::AcrossTransition,
            "lift must reject a transition"
        );
        assert_eq!(
            json(&ed),
            before,
            "rejected lift must leave the timeline unchanged"
        );
        assert!(!ed.can_undo(), "a rejected lift records no undo entry");
        // A non-transition index still lifts (the guard is targeted, not blanket).
        assert!(
            ops::lift(&mut ed, track, 0).is_ok(),
            "lifting a clip still works"
        );
    }

    #[test]
    fn overwrite_with_src_already_on_track_errors_and_does_not_mutate() {
        // MEDIUM-2: passing a clip that is ALREADY a child of the target track as
        // the overwrite source would double-attach the same NodeId (the window
        // scan rewrites/clones it, then re-inserts it) → corrupt tree. The guard
        // rejects it with a Structural error and rolls back.
        let (mut ed, track, ids) = timeline_with_media(&[(0.0, 10.0), (0.0, 10.0)], 20.0);
        let before = json(&ed);
        let e = ops::overwrite(&mut ed, track, ids[0], RationalTime::new(5.0, 24.0)).unwrap_err();
        assert!(
            matches!(e, EditError::Structural(_)),
            "src already on track must be a Structural error, got {e:?}"
        );
        assert_eq!(
            json(&ed),
            before,
            "rejected overwrite must not mutate (no double-attach)"
        );
        assert!(!ed.can_undo(), "a rejected overwrite records no undo entry");
    }

    #[test]
    fn three_point_with_src_already_on_track_errors_and_does_not_mutate() {
        // MEDIUM-2 (inherited path): three_point sets src.source_range THEN calls
        // overwrite_in_tx, which carries the same double-attach guard. The set must
        // roll back with the overwrite leg so the doc is untouched.
        let (mut ed, track, ids) = timeline_with_media(&[(0.0, 10.0), (0.0, 10.0)], 20.0);
        let before = json(&ed);
        let e = ops::three_point(
            &mut ed,
            track,
            ids[0],
            RationalTime::new(2.0, 24.0),
            RationalTime::new(6.0, 24.0),
            RationalTime::new(5.0, 24.0),
        )
        .unwrap_err();
        assert!(
            matches!(e, EditError::Structural(_)),
            "three_point src already on track must be Structural, got {e:?}"
        );
        assert_eq!(
            json(&ed),
            before,
            "rejected three_point rolls back the source_range set too"
        );
    }

    #[test]
    fn overwrite_splitting_a_gap_normalizes_tail_start_to_zero() {
        // LOW-4: a single Gap [0,20) overwritten in the middle (src dur 4 @ t=8)
        // splits into head gap [0,8) and a TAIL gap whose source_range.start must
        // be canonical 0 — not `csr.start + tail_drop` (=12), which would emit a
        // non-canonical gap. (For a clip the tail keeps its real media offset; the
        // is_gap branch is the only one normalized.)
        let mut doc = Document::new();
        let (tl, stack) = build::timeline(&mut doc, "T");
        doc.set_root(tl);
        let track = build::track(&mut doc, "V", "Video");
        let _ = doc.append_child(stack, track);
        let g = build::gap(&mut doc, tr(0.0, 20.0));
        let _ = doc.append_child(track, g);
        let mut ed = Editor::new(doc);
        let src = standalone_clip(&mut ed, 4.0);

        ops::overwrite(&mut ed, track, src, RationalTime::new(8.0, 24.0)).unwrap();

        let kids = ed
            .document()
            .node(track)
            .unwrap()
            .children("children")
            .to_vec();
        assert_eq!(kids.len(), 3, "head gap, src, tail gap");
        assert!(is_gap(ed.document(), kids[0]), "head is a gap");
        assert_eq!(kids[1], src, "src occupies the window");
        assert!(is_gap(ed.document(), kids[2]), "tail is a gap");
        // The crux: the tail gap is canonical (start == 0), not (12, 8).
        assert_eq!(
            sr_of(&ed, kids[2]),
            (0.0, 8.0),
            "tail gap source_range normalized to start 0"
        );
        // Head gap is unaffected (its start was already 0).
        assert_eq!(sr_of(&ed, kids[0]), (0.0, 8.0), "head gap [0,8)");
    }

    // ---- invariant locks: footprint-preserving ops -------------------------

    /// Total track footprint = end_time_exclusive of the last child (0 if empty).
    fn footprint(ed: &Editor, track: otio_core::NodeId) -> f64 {
        otio_algo::range_of_all_children(ed.document(), track)
            .unwrap()
            .last()
            .map(|(_, r)| r.end_time_exclusive().value())
            .unwrap_or(0.0)
    }

    #[test]
    fn lift_preserves_track_footprint() {
        // Invariant: lift swaps a clip for a same-duration gap, so the track's
        // total layout footprint is unchanged (the hole does not ripple).
        let (mut ed, track) = timeline_with_clips(); // 3×(0,10) → footprint 30
        let before = footprint(&ed, track);
        assert_eq!(before, 30.0);
        ops::lift(&mut ed, track, 1).unwrap();
        assert_eq!(
            footprint(&ed, track),
            before,
            "lift keeps the footprint (gap == clip dur)"
        );
        assert!(
            is_gap(ed.document(), child(&ed, track, 1)),
            "the lifted slot is a gap"
        );
    }

    #[test]
    fn slide_preserves_track_footprint_across_deltas() {
        // Invariant (deterministic table, no randomness): sliding a middle clip
        // between two clip neighbours rolls A's tail by +δ and C's head by -δ, so
        // the summed footprint A+B+C = 30 is preserved for every valid δ. Each
        // case starts from a fresh editor so the cases are independent.
        // Bounds for [A(0,10) B(0,10) C(0,10)] @ avail 20: 0 < δ < 10 (A end =
        // 10+δ ≤ 20, C head dur = 10-δ > 0, C start = δ ≥ avail.start 0).
        for &d in &[1.0_f64, 2.0, 5.0, 8.0, 9.0] {
            let (mut ed, track, ids) =
                timeline_with_media(&[(0.0, 10.0), (0.0, 10.0), (0.0, 10.0)], 20.0);
            let b = ids[1];
            let before = footprint(&ed, track);
            assert_eq!(before, 30.0, "precondition footprint");
            ops::slide(&mut ed, b, RationalTime::new(d, 24.0)).unwrap();
            assert_eq!(
                footprint(&ed, track),
                before,
                "slide by +{d} must preserve the footprint (roll both edges)"
            );
            // And it is genuinely footprint-preserving by reshaping neighbours, not
            // by no-op: A grew and C shrank.
            assert_eq!(sr_of(&ed, ids[0]), (0.0, 10.0 + d), "A tail grew by +{d}");
            assert_eq!(sr_of(&ed, ids[2]), (d, 10.0 - d), "C head shrank by +{d}");
        }
    }
}
