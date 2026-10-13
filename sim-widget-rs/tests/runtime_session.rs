include!("fixtures/runtime/support.rs");
fn identified(config: SessionConfig) -> Session {
    Session::new_with_identity(
        base().validate(config.decode_limits).unwrap(),
        OperatorRegistry::standard().unwrap(),
        config,
        SessionId::new("replay-session").unwrap(),
    )
    .unwrap()
}
#[test]
fn snapshot_event_suffix_replays_values_origins_time_and_imports_paused() {
    let mut source = identified(Default::default());
    source.set_playing(0, true).unwrap();
    source
        .set_live_override(source.revision(), p("a"), scalar(5.))
        .unwrap();
    source
        .seek(source.revision(), TimeTicks::new(127008000000))
        .unwrap();
    source.set_playing(source.revision(), true).unwrap();
    let envelope = source.export_events().unwrap();
    let bytes = serde_json::to_vec(&envelope).unwrap();
    let envelope: SessionEnvelope = serde_json::from_slice(&bytes).unwrap();
    let mut target = identified(Default::default());
    target.replay(0, envelope).unwrap();
    assert_eq!(target.checkpoint(), source.checkpoint());
    assert_eq!(target.snapshot().truth(), source.snapshot().truth());
    assert_eq!(
        target.snapshot().provenance(),
        source.snapshot().provenance()
    );
    assert!(!target.is_playing());
    assert!(target.checkpoint().playback_intent);
    let snapshot = source.export_snapshot().unwrap();
    let mut restored = identified(Default::default());
    restored.restore_snapshot(0, snapshot).unwrap();
    assert_eq!(restored.snapshot().truth(), source.snapshot().truth());
    assert!(!restored.is_playing());
    assert_eq!(restored.history_len(), 0);
    source.seek(source.revision(), TimeTicks::ZERO).unwrap();
    source
        .seek(source.revision(), TimeTicks::new(127008000000))
        .unwrap();
    assert_eq!(source.snapshot().truth(), target.snapshot().truth());
    assert_eq!(
        source.snapshot().provenance(),
        target.snapshot().provenance()
    );
}
#[test]
fn bounded_suffix_baseline_survives_eviction_and_zero_retention_exports_snapshot() {
    let config = SessionConfig {
        event_capacity: 2,
        ..Default::default()
    };
    let mut source = identified(config);
    source.set_live_override(0, p("a"), scalar(5.)).unwrap();
    for t in [10, 20, 30, 40] {
        source.seek(source.revision(), TimeTicks::new(t)).unwrap();
    }
    let envelope = source.export_events().unwrap();
    assert_eq!(envelope.events.len(), 2);
    assert_eq!(envelope.baseline.sequence, 3);
    assert_eq!(envelope.retained_start, 4);
    let mut target = identified(config);
    target.replay(0, envelope.clone()).unwrap();
    assert_eq!(target.snapshot().truth(), source.snapshot().truth());
    assert_eq!(target.checkpoint(), source.checkpoint());
    let mut missing = envelope;
    missing.baseline.sequence = 0;
    missing.baseline.revision = 0;
    let mut target = identified(config);
    let before = capture(&target);
    assert!(target.replay(0, missing).is_err());
    assert_eq!(capture(&target), before);
    let mut s = identified(SessionConfig {
        event_capacity: 0,
        ..Default::default()
    });
    s.set_live_override(0, p("a"), scalar(6.)).unwrap();
    assert!(s.events().is_empty());
    assert_eq!(s.export_snapshot().unwrap().overrides[&p("a")], scalar(6.));
    assert!(s.export_events().unwrap().events.is_empty());
}
#[test]
fn replay_rejects_foreign_future_duplicate_gapped_and_reordered_events_atomically() {
    let mut source = identified(Default::default());
    for t in [10, 20, 30] {
        source.seek(source.revision(), TimeTicks::new(t)).unwrap();
    }
    let good = source.export_events().unwrap();
    for mode in 0..9 {
        let mut bad = good.clone();
        match mode {
            // Version 2 is supported after the Task04 session protocol migration.
            0 => bad.version = SESSION_VERSION + 1,
            1 => bad.events[0].version = SESSION_VERSION + 1,
            2 => bad.events[1].sequence = bad.events[0].sequence,
            3 => bad.events.swap(0, 1),
            4 => {
                bad.events.remove(0);
                bad.retained_start = bad.events[0].sequence;
            }
            5 => bad.baseline.document_revision = 99,
            6 => bad.events[1].session = SessionId::new("foreign").unwrap(),
            7 => bad.baseline.epoch = 99,
            8 => bad.retained_end = 999,
            _ => unreachable!(),
        }
        let mut target = identified(Default::default());
        let before = capture(&target);
        assert!(target.replay(0, bad).is_err(), "mode {mode}");
        assert_eq!(capture(&target), before);
    }
    let mut target = identified(Default::default());
    target.set_parameter(0, p("a"), scalar(7.)).unwrap();
    let before = capture(&target);
    assert!(target.replay(target.revision(), good).is_err());
    assert_eq!(capture(&target), before);
    assert_eq!(target.epoch(), 1);
}
#[test]
fn view_hover_selection_guide_and_camera_have_no_mathematical_effect() {
    let mut s = session(base(), Default::default());
    s.set_live_override(0, p("a"), scalar(5.)).unwrap();
    let before = capture(&s);
    let mut view = ViewState::new(ViewId::new("view-1").unwrap());
    let guide = HighlightTarget::Value(v("b"));
    let selection = HighlightTarget::Value(v("a"));
    let hover = HighlightTarget::Object(ObjectId::new("tri").unwrap());
    view.set_guide(vec![guide.clone()]).unwrap();
    view.set_selection(vec![selection.clone()]).unwrap();
    view.set_hover(vec![hover.clone()]).unwrap();
    assert_eq!(view.highlighted(), &[hover]);
    view.set_hover(vec![]).unwrap();
    assert_eq!(view.highlighted(), &[selection]);
    view.set_selection(vec![]).unwrap();
    assert_eq!(view.highlighted(), &[guide]);
    view.set_camera(Some(CameraState {
        schema: "camera.perspective.v1".into(),
        fields: BTreeMap::from([("position".into(), Value::vector(vec![1., 2., 3.]).unwrap())]),
    }))
    .unwrap();
    assert_eq!(capture(&s), before);
    assert_eq!(n(&s, "a"), 5.);
    let other = ViewState::new(ViewId::new("view-2").unwrap());
    assert!(other.highlighted().is_empty());
    assert!(other.camera().is_none());
    let old = view.clone();
    assert!(
        view.set_camera(Some(CameraState {
            schema: "".into(),
            fields: BTreeMap::new()
        }))
        .is_err()
    );
    assert_eq!(view, old);
}
#[test]
fn session_full_i64_time_decimal_wire_and_malformed_time_are_checked() {
    let mut s = identified(Default::default());
    for ticks in [i64::MIN, i64::MAX] {
        s.seek(s.revision(), TimeTicks::new(ticks)).unwrap();
        let bytes = serde_json::to_vec(&s.export_snapshot().unwrap()).unwrap();
        let round: SessionCheckpoint = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(round.time.ticks(), ticks);
    }
    let mut raw = serde_json::to_value(s.export_snapshot().unwrap()).unwrap();
    for malformed in [
        serde_json::json!(1),
        serde_json::json!("NaN"),
        serde_json::json!("inf"),
        serde_json::json!("9223372036854775808"),
        serde_json::json!("1.5"),
        serde_json::json!("+1"),
    ] {
        raw["time"] = malformed;
        assert!(serde_json::from_value::<SessionCheckpoint>(raw.clone()).is_err());
    }
    assert!(TimeTicks::from_rational(1, 1, 0).is_err());
    assert!(TimeTicks::from_rational(i64::MAX, 1, 1).is_err());
}
#[test]
fn restore_cannot_rewind_active_session_counters() {
    let mut s = identified(Default::default());
    let checkpoint = s.export_snapshot().unwrap();
    s.seek(0, TimeTicks::new(10)).unwrap();
    let before = capture(&s);
    assert!(s.restore_snapshot(s.revision(), checkpoint).is_err());
    assert_eq!(capture(&s), before);
}

#[test]
fn author_epoch_import_and_generation_bound_commands_are_validated() {
    let mut source = identified(Default::default());
    command(
        &mut source,
        CommandKind::SetParameters {
            expected_document_revision: 0,
            values: BTreeMap::from([(p("a"), scalar(5.))]),
        },
    )
    .unwrap();
    source.seek(source.revision(), TimeTicks::new(50)).unwrap();
    let saved = source.export_events().unwrap();
    let mut target = Session::new_with_epoch(
        source.document().clone(),
        OperatorRegistry::standard().unwrap(),
        Default::default(),
        SessionId::new("replay-session").unwrap(),
        source.epoch(),
    )
    .unwrap();
    let old_generation_command = source.prepare_command(CommandKind::Seek {
        time: TimeTicks::new(100),
    });
    target.replay(0, saved).unwrap();
    assert_eq!(target.checkpoint(), source.checkpoint());
    let before = capture(&target);
    assert_eq!(
        target.dispatch(old_generation_command).unwrap_err().code,
        DiagnosticCode::StaleRevision
    );
    assert_eq!(capture(&target), before);
    let snapshot = target.export_snapshot().unwrap();
    assert!(
        target
            .restore_snapshot(target.revision(), snapshot)
            .is_err()
    );
    assert_eq!(capture(&target), before);
    let mut fresh = identified(Default::default());
    let zero = fresh.export_snapshot().unwrap();
    fresh.restore_snapshot(0, zero.clone()).unwrap();
    let before = capture(&fresh);
    assert!(fresh.restore_snapshot(0, zero).is_err());
    assert_eq!(capture(&fresh), before);
    let mut draft = identified(Default::default());
    let zero = draft.export_snapshot().unwrap();
    draft.begin_transaction(0).unwrap();
    let before = capture(&draft);
    assert!(draft.restore_snapshot(draft.revision(), zero).is_err());
    assert_eq!(capture(&draft), before);
}

#[test]
fn session_decode_limits_duplicate_override_keys_and_event_bytes_reject() {
    let mut s = identified(Default::default());
    s.set_live_override(0, p("a"), scalar(4.)).unwrap();
    let checkpoint = s.export_snapshot().unwrap();
    let bytes = checkpoint.encode(1048576).unwrap();
    assert_eq!(
        SessionCheckpoint::decode(&bytes, DecodeLimits::default()).unwrap(),
        checkpoint
    );
    assert!(
        SessionCheckpoint::decode(
            &bytes,
            DecodeLimits {
                max_bytes: 4,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        SessionCheckpoint::decode(
            &bytes,
            DecodeLimits {
                max_elements: 0,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        SessionCheckpoint::decode(
            &bytes,
            DecodeLimits {
                max_depth: 2,
                ..Default::default()
            }
        )
        .is_err()
    );
    let envelope = s.export_events().unwrap();
    let bytes = envelope.encode(1048576).unwrap();
    assert_eq!(
        SessionEnvelope::decode(&bytes, DecodeLimits::default()).unwrap(),
        envelope
    );
    let text = String::from_utf8(checkpoint.encode(1048576).unwrap()).unwrap();
    let duplicate = text.replace(
        "\"overrides\":{",
        "\"overrides\":{\"a\":{\"kind\":\"scalar\",\"data\":5.0},",
    );
    assert!(SessionCheckpoint::decode(duplicate.as_bytes(), DecodeLimits::default()).is_err());
    let event_bytes = serde_json::to_vec(s.events().back().unwrap())
        .unwrap()
        .len();
    let mut tiny = identified(SessionConfig {
        max_event_bytes: event_bytes - 1,
        ..Default::default()
    });
    let before = capture(&tiny);
    assert_eq!(
        tiny.set_live_override(0, p("a"), scalar(4.))
            .unwrap_err()
            .code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&tiny), before);
}
#[test]
fn reset_replay_preserves_paused_import_policy_from_playing_baseline() {
    let mut source = identified(SessionConfig {
        event_capacity: 1,
        ..Default::default()
    });
    command(
        &mut source,
        CommandKind::ResetParameters {
            expected_document_revision: 0,
            parameters: vec![p("a")],
        },
    )
    .unwrap();
    source.set_playing(source.revision(), true).unwrap();
    command(
        &mut source,
        CommandKind::ResetParameters {
            expected_document_revision: 0,
            parameters: vec![p("a")],
        },
    )
    .unwrap();
    let exported = source.export_events().unwrap();
    assert!(exported.baseline.playback_intent);
    let mut target = identified(SessionConfig {
        event_capacity: 1,
        ..Default::default()
    });
    target.replay(0, exported).unwrap();
    assert_eq!(source.checkpoint(), target.checkpoint());
    assert!(!target.is_playing());
}

fn reopen_saved(
    doc: ValidatedDocument,
    config: SessionConfig,
    c: SessionCheckpoint,
) -> Result<Session> {
    Session::from_checkpoint(
        doc,
        OperatorRegistry::standard()?,
        config,
        c.session.clone(),
        c.epoch,
        c,
    )
}
#[test]
fn fix_r1_saved_override_masks_invalid_authored_default_before_initial_evaluation() {
    let mut d = base();
    d.parameters[0].range = None;
    d.operations.clear();
    d.objects.clear();
    d.tracks.clear();
    d.constraints = vec![Constraint::NonZero { parameter: p("a") }];
    let mut source = session(d, Default::default());
    source.set_live_override(0, p("a"), scalar(5.)).unwrap();
    source
        .set_parameter(source.revision(), p("a"), scalar(0.))
        .unwrap();
    source.set_playing(source.revision(), true).unwrap();
    let cp = source.export_snapshot().unwrap();
    let doc = Document::decode(
        &source.document().encode(1048576).unwrap(),
        DecodeLimits::default(),
    )
    .unwrap();
    let restored = reopen_saved(doc, Default::default(), cp.clone()).unwrap();
    assert_eq!(restored.checkpoint(), cp);
    assert_eq!(restored.snapshot().truth(), source.snapshot().truth());
    assert_eq!(
        restored.snapshot().provenance(),
        source.snapshot().provenance()
    );
    assert!(!restored.is_playing());
    assert_ne!(
        restored.snapshot().generation(),
        source.snapshot().generation()
    );
}
#[test]
fn fix_r2_current_checkpoint_budget_rejects_cumulative_ids_atomically() {
    let mut d = base();
    d.operations.clear();
    d.objects.clear();
    d.tracks.clear();
    d.constants.clear();
    let template = d.parameters[0].clone();
    d.parameters.clear();
    let names = (0..4)
        .map(|i| format!("p{i}_{}", "x".repeat(95)))
        .collect::<Vec<_>>();
    for name in &names {
        let mut item = template.clone();
        item.id = p(name);
        item.value_id = v(name);
        item.range = None;
        d.parameters.push(item);
    }
    let config = SessionConfig {
        max_command_bytes: 512,
        event_capacity: 1,
        ..Default::default()
    };
    let mut source = session(d, config);
    let mut rejected = false;
    for name in &names {
        let before = capture(&source);
        if source
            .set_live_override(source.revision(), p(name), scalar(5.))
            .is_err()
        {
            assert_eq!(capture(&source), before);
            rejected = true;
            break;
        }
    }
    assert!(rejected);
    let cp = source.export_snapshot().unwrap();
    assert!(cp.allocated_bytes().unwrap() <= 512);
    let restored = reopen_saved(source.document().clone(), config, cp.clone()).unwrap();
    assert_eq!(restored.checkpoint(), cp);
    let mut replay = Session::new_with_epoch(
        source.document().clone(),
        OperatorRegistry::standard().unwrap(),
        config,
        cp.session.clone(),
        cp.epoch,
    )
    .unwrap();
    replay.replay(0, source.export_events().unwrap()).unwrap();
    assert_eq!(replay.checkpoint(), cp);
}
#[test]
fn fix_r2_default_spare_capacity_snapshot_roundtrip_remains_valid() {
    let mut d = base();
    d.operations.clear();
    d.objects.clear();
    d.tracks.clear();
    d.constants.clear();
    let mut template = d.parameters[0].clone();
    template.range = None;
    template.default = Value::vector(vec![1.]).unwrap();
    template.port.shape = Shape::Vector { len: 1 };
    d.parameters.clear();
    for i in 0..4 {
        template.id = p(&format!("p{i}"));
        template.value_id = v(&format!("p{i}"));
        d.parameters.push(template.clone());
    }
    let mut source = session(d, Default::default());
    for i in 0..4 {
        let mut data = Vec::with_capacity(40000);
        data.push(Finite::new(2.).unwrap());
        source
            .set_live_override(
                source.revision(),
                p(&format!("p{i}")),
                Value::Vector(std::sync::Arc::new(data)),
            )
            .unwrap();
    }
    let cp = source.export_snapshot().unwrap();
    let restored = reopen_saved(source.document().clone(), Default::default(), cp.clone()).unwrap();
    assert_eq!(restored.checkpoint(), cp);
    assert_eq!(restored.snapshot().truth(), source.snapshot().truth());
}

#[test]
fn fix_r1_saved_nonzero_time_is_evaluated_before_any_truth_publication() {
    let mut d = base();
    d.operations.clear();
    d.objects.clear();
    d.tracks.clear();
    d.parameters[0].range = None;
    d.constraints = vec![Constraint::NonZero { parameter: p("a") }];
    let mut source = session(d, Default::default());
    source.seek(0, TimeTicks::new(100)).unwrap();
    let track = ScalarTrack {
        parameter: p("a"),
        keys: vec![
            ScalarKey {
                time: TimeTicks::ZERO,
                value: Finite::new(0.).unwrap(),
                interpolation: Interpolation::Linear,
            },
            ScalarKey {
                time: TimeTicks::new(100),
                value: Finite::new(2.).unwrap(),
                interpolation: Interpolation::Linear,
            },
        ],
    };
    command(
        &mut source,
        CommandKind::EditDocument {
            expected_document_revision: 0,
            edits: vec![DocumentEdit::PutTrack { track }],
        },
    )
    .unwrap();
    source.set_playing(source.revision(), true).unwrap();
    let cp = source.export_snapshot().unwrap();
    let doc = source.document().clone();
    let before = capture(&source);
    assert!(
        Session::new_with_epoch(
            doc.clone(),
            OperatorRegistry::standard().unwrap(),
            Default::default(),
            cp.session.clone(),
            cp.epoch
        )
        .is_err()
    );
    let loaded = reopen_saved(doc.clone(), Default::default(), cp.clone()).unwrap();
    assert_eq!(loaded.checkpoint(), cp);
    assert_eq!(loaded.snapshot().truth(), source.snapshot().truth());
    assert_eq!(
        loaded.snapshot().provenance(),
        source.snapshot().provenance()
    );
    assert!(!loaded.is_playing());
    assert_ne!(
        loaded.snapshot().generation(),
        source.snapshot().generation()
    );
    for mode in 0..5 {
        let mut invalid = cp.clone();
        match mode {
            0 => invalid.time = TimeTicks::ZERO,
            1 => invalid.epoch += 1,
            2 => invalid.document_revision += 1,
            3 => invalid.session = SessionId::new("foreign").unwrap(),
            _ => {
                invalid.overrides.insert(p("missing"), scalar(1.));
            }
        }
        assert!(
            Session::from_checkpoint(
                doc.clone(),
                OperatorRegistry::standard().unwrap(),
                Default::default(),
                cp.session.clone(),
                cp.epoch,
                invalid
            )
            .is_err()
        );
        assert_eq!(capture(&source), before);
    }
    let denied = SessionConfig {
        role: SessionRole::Learner,
        ..Default::default()
    };
    let mut restricted = doc.document().clone();
    restricted.learner_permissions.seek = false;
    assert!(
        reopen_saved(
            restricted.validate(DecodeLimits::default()).unwrap(),
            denied,
            cp
        )
        .is_err()
    );
}
#[test]
fn fix_r1_checkpoint_and_event_load_preserve_real_division_and_reject_old_generation() {
    let mut d = base();
    let mut op = d.operations[0].clone();
    d.operations.clear();
    d.objects.clear();
    d.tracks.clear();
    d.parameters[0].range = None;
    op.operator = OperatorId::new("scalar.div").unwrap();
    op.inputs = vec![v("b"), v("a")];
    op.output = v("quotient");
    op.output_port = Port {
        shape: Shape::Scalar,
        unit: Unit::DIMENSIONLESS,
    };
    d.operations.push(op);
    let mut source = session(
        d,
        SessionConfig {
            event_capacity: 1,
            ..Default::default()
        },
    );
    source.set_live_override(0, p("a"), scalar(5.)).unwrap();
    source
        .set_parameter(source.revision(), p("a"), scalar(0.))
        .unwrap();
    source.seek(source.revision(), TimeTicks::new(1)).unwrap();
    source.set_playing(source.revision(), true).unwrap();
    let cp = source.export_snapshot().unwrap();
    let events = source.export_events().unwrap();
    let old_command = source.prepare_command(CommandKind::Seek {
        time: TimeTicks::new(2),
    });
    assert_eq!(n(&source, "quotient"), 0.8);
    assert!(
        Session::new_with_epoch(
            source.document().clone(),
            OperatorRegistry::standard().unwrap(),
            Default::default(),
            cp.session.clone(),
            cp.epoch
        )
        .is_err()
    );
    for mut loaded in [
        reopen_saved(source.document().clone(), Default::default(), cp.clone()).unwrap(),
        Session::from_events(
            source.document().clone(),
            OperatorRegistry::standard().unwrap(),
            SessionConfig {
                event_capacity: 1,
                ..Default::default()
            },
            cp.session.clone(),
            cp.epoch,
            events.clone(),
        )
        .unwrap(),
    ] {
        assert_eq!(loaded.checkpoint(), cp);
        assert_eq!(loaded.snapshot().truth(), source.snapshot().truth());
        assert_eq!(
            loaded.snapshot().provenance(),
            source.snapshot().provenance()
        );
        assert!(!loaded.is_playing());
        let before = capture(&loaded);
        assert!(loaded.dispatch(old_command.clone()).is_err());
        assert!(
            loaded
                .restore_snapshot(loaded.revision(), cp.clone())
                .is_err()
        );
        assert!(loaded.replay(loaded.revision(), events.clone()).is_err());
        assert_eq!(capture(&loaded), before);
    }
    let mut bad = cp.clone();
    bad.overrides.clear();
    assert!(reopen_saved(source.document().clone(), Default::default(), bad).is_err());
    let mut bad = events;
    bad.events[0].sequence += 1;
    assert!(
        Session::from_events(
            source.document().clone(),
            OperatorRegistry::standard().unwrap(),
            Default::default(),
            cp.session.clone(),
            cp.epoch,
            bad
        )
        .is_err()
    );
}
