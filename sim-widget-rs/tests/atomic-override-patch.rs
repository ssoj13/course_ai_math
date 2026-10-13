include!("fixtures/runtime/support.rs");
fn patch(
    s: &mut Session,
    entries: serde_json::Value,
    playback: serde_json::Value,
) -> Result<DispatchResult> {
    let mut wire = serde_json::to_value(s.prepare_command(CommandKind::Seek {
        time: s.snapshot().time(),
    }))
    .unwrap();
    wire["kind"] =
        serde_json::json!({"kind":"patch_live_overrides","entries":entries,"playback":playback});
    let command: Command = serde_json::from_value(wire)
        .map_err(|e| Diagnostic::new(DiagnosticCode::Schema, "command", e.to_string()))?;
    s.dispatch(command)
}
fn value_json(x: f64) -> serde_json::Value {
    serde_json::to_value(scalar(x)).unwrap()
}
#[test]
fn mixed_set_clear_evaluates_only_the_valid_final_candidate() {
    let mut d = base();
    d.parameters[1].default = scalar(3.);
    d.constraints = vec![Constraint::ScalarBounds {
        parameter: p("a"),
        min: Some(p("b")),
        max: Some(p("b")),
    }];
    let mut s = session(d, Default::default());
    command(
        &mut s,
        CommandKind::SetLiveOverrides {
            values: BTreeMap::from([(p("a"), scalar(6.)), (p("b"), scalar(6.))]),
        },
    )
    .unwrap();
    s.seek(s.revision(), TimeTicks::new(127008000000)).unwrap();
    let before = capture(&s);
    assert!(s.return_to_guide(s.revision(), &[p("a")]).is_err());
    assert_eq!(capture(&s), before);
    assert!(
        s.set_live_override(s.revision(), p("b"), scalar(3.))
            .is_err()
    );
    assert_eq!(capture(&s), before);
    patch(
        &mut s,
        serde_json::json!([["a", null], ["b", value_json(3.)]]),
        serde_json::json!("preserve"),
    )
    .unwrap();
    assert!(!s.live_overrides().contains_key(&p("a")));
    assert_eq!(s.live_overrides()[&p("b")], scalar(3.));
    assert_eq!(n(&s, "a"), 3.);
    assert_eq!(n(&s, "b"), 3.);
    assert_eq!(
        s.snapshot().provenance()[&v("a")].origin,
        InputOrigin::Default
    );
    assert_eq!(
        s.snapshot().provenance()[&v("b")].origin,
        InputOrigin::Override
    );
    assert_eq!(s.snapshot().time(), before.frame.time());
    assert_eq!(s.snapshot().generation(), before.frame.generation());
    assert_eq!(s.history_len(), 0);
}
#[test]
fn equal_number_clear_is_provenance_only_and_compensation_is_monotonic() {
    let mut s = session(base(), Default::default());
    s.set_live_override(0, p("a"), scalar(3.)).unwrap();
    let before = s.snapshot();
    let sequence = s.checkpoint().sequence;
    patch(
        &mut s,
        serde_json::json!([["a", null]]),
        serde_json::json!("preserve"),
    )
    .unwrap();
    assert!(s.snapshot().changes().changed_values.is_empty());
    assert!(s.snapshot().changes().provenance_only.contains(&v("a")));
    assert_eq!(s.revision(), before.revision() + 1);
    assert_eq!(s.checkpoint().sequence, sequence + 1);
    assert_eq!(before.provenance()[&v("a")].origin, InputOrigin::Override);
    let saved = s.snapshot();
    patch(
        &mut s,
        serde_json::json!([["a", value_json(3.)]]),
        serde_json::json!("pause"),
    )
    .unwrap();
    assert_eq!(s.revision(), saved.revision() + 1);
    assert_eq!(s.snapshot().generation(), saved.generation());
    assert_eq!(
        s.snapshot().provenance()[&v("a")].origin,
        InputOrigin::Override
    );
}
#[test]
fn no_op_patch_preserves_complete_state_and_author_redo() {
    let mut s = session(base(), Default::default());
    s.set_parameter(0, p("a"), scalar(5.)).unwrap();
    s.undo(s.revision()).unwrap();
    let before = capture(&s);
    for entries in [serde_json::json!([]), serde_json::json!([["a", null]])] {
        let result = patch(&mut s, entries, serde_json::json!("preserve")).unwrap();
        assert!(result.changes == ChangeSet::default());
        assert_eq!(capture(&s), before);
        assert!(s.can_redo());
    }
}
#[test]
fn actual_saved_v1_migrates_to_v2_and_replays_exact_time_absence_and_intent() {
    assert_eq!(SESSION_VERSION, 2);
    let doc = Document::decode(
        include_bytes!("fixtures/runtime/session-v1-document.json"),
        DecodeLimits::default(),
    )
    .unwrap();
    let cp = SessionCheckpoint::decode(
        include_bytes!("fixtures/runtime/session-v1-checkpoint.json"),
        DecodeLimits::default(),
    )
    .unwrap();
    assert_eq!(cp.version, 2);
    assert_eq!(cp.time.ticks(), i64::MAX);
    assert!(!cp.overrides.contains_key(&p("b")));
    let events = SessionEnvelope::decode(
        include_bytes!("fixtures/runtime/session-v1-events.json"),
        DecodeLimits::default(),
    )
    .unwrap();
    assert_eq!(events.version, 2);
    assert_eq!(events.baseline.version, 2);
    assert!(events.events.iter().all(|e| e.version == 2));
    let loaded = Session::from_checkpoint(
        doc.clone(),
        OperatorRegistry::standard().unwrap(),
        Default::default(),
        cp.session.clone(),
        cp.epoch,
        cp.clone(),
    )
    .unwrap();
    let replay = Session::from_events(
        doc,
        OperatorRegistry::standard().unwrap(),
        Default::default(),
        cp.session.clone(),
        cp.epoch,
        events,
    )
    .unwrap();
    assert_eq!(loaded.checkpoint(), cp);
    assert_eq!(replay.checkpoint(), cp);
    assert_eq!(replay.snapshot().truth(), loaded.snapshot().truth());
    assert_eq!(
        replay.snapshot().provenance(),
        loaded.snapshot().provenance()
    );
    assert!(!loaded.is_playing());
    assert!(!replay.is_playing());
    assert!(replay.checkpoint().playback_intent);
}

fn typed_patch(
    s: &mut Session,
    entries: Vec<(ParameterId, Option<Value>)>,
    playback: OverridePlaybackPolicy,
) -> Result<DispatchResult> {
    command(s, CommandKind::PatchLiveOverrides { entries, playback })
}
fn learner_document() -> Document {
    let mut d = base();
    for p in &mut d.parameters {
        p.metadata.learner_exposed = true;
    }
    d
}
#[test]
fn playback_policies_and_imported_preserve_do_not_accidentally_activate() {
    let mut source = session(base(), Default::default());
    source.seek(0, TimeTicks::new(127008000000)).unwrap();
    source.set_playing(source.revision(), true).unwrap();
    typed_patch(
        &mut source,
        vec![(p("a"), Some(scalar(5.)))],
        OverridePlaybackPolicy::Preserve,
    )
    .unwrap();
    assert!(source.is_playing());
    assert!(source.playback_intent());
    typed_patch(
        &mut source,
        vec![(p("a"), None)],
        OverridePlaybackPolicy::Pause,
    )
    .unwrap();
    assert!(!source.is_playing());
    assert!(!source.playback_intent());
    typed_patch(&mut source, vec![], OverridePlaybackPolicy::SetIntent(true)).unwrap();
    assert!(source.is_playing());
    assert!(source.playback_intent());
    let cp = source.export_snapshot().unwrap();
    let doc = source.document().clone();
    let mut imported = Session::from_checkpoint(
        doc.clone(),
        OperatorRegistry::standard().unwrap(),
        Default::default(),
        cp.session.clone(),
        cp.epoch,
        cp,
    )
    .unwrap();
    assert!(!imported.is_playing());
    assert!(imported.playback_intent());
    let time = imported.snapshot().time();
    typed_patch(
        &mut imported,
        vec![(p("b"), Some(scalar(6.)))],
        OverridePlaybackPolicy::Preserve,
    )
    .unwrap();
    typed_patch(
        &mut imported,
        vec![(p("b"), None)],
        OverridePlaybackPolicy::Preserve,
    )
    .unwrap();
    assert!(!imported.is_playing());
    assert!(imported.playback_intent());
    assert_eq!(imported.snapshot().time(), time);
    let events = imported.export_events().unwrap();
    let restored = Session::from_events(
        doc,
        OperatorRegistry::standard().unwrap(),
        Default::default(),
        events.baseline.session.clone(),
        events.baseline.epoch,
        events,
    )
    .unwrap();
    assert_eq!(restored.checkpoint(), imported.checkpoint());
    assert!(!restored.is_playing());
    assert!(restored.playback_intent());
    typed_patch(
        &mut imported,
        vec![],
        OverridePlaybackPolicy::SetIntent(true),
    )
    .unwrap();
    assert!(imported.is_playing());
    typed_patch(
        &mut imported,
        vec![],
        OverridePlaybackPolicy::SetIntent(false),
    )
    .unwrap();
    assert!(!imported.is_playing());
    assert!(!imported.playback_intent());
}
#[test]
fn patch_eviction_and_zero_retention_replay_exact_absence_track_source_and_unrelated_keys() {
    for capacity in [0, 1, 2, 10] {
        let config = SessionConfig {
            event_capacity: capacity,
            ..Default::default()
        };
        let mut s = session(base(), config);
        s.seek(0, TimeTicks::new(127008000000)).unwrap();
        typed_patch(
            &mut s,
            vec![(p("a"), Some(scalar(5.))), (p("theta"), Some(scalar(1.)))],
            OverridePlaybackPolicy::SetIntent(true),
        )
        .unwrap();
        let a = s.live_overrides()[&p("a")].clone();
        typed_patch(
            &mut s,
            vec![(p("b"), Some(scalar(7.))), (p("theta"), None)],
            OverridePlaybackPolicy::Preserve,
        )
        .unwrap();
        typed_patch(
            &mut s,
            vec![(p("b"), None)],
            OverridePlaybackPolicy::Preserve,
        )
        .unwrap();
        assert_eq!(s.live_overrides()[&p("a")], a);
        assert!(!s.live_overrides().contains_key(&p("theta")));
        assert_eq!(
            s.snapshot().provenance()[&v("theta")].origin,
            InputOrigin::Track
        );
        let cp = s.export_snapshot().unwrap();
        let envelope = s.export_events().unwrap();
        assert_eq!(envelope.version, 2);
        assert!(envelope.events.len() <= capacity);
        let bytes = envelope.encode(1048576).unwrap();
        let decoded = SessionEnvelope::decode(&bytes, DecodeLimits::default()).unwrap();
        let replay = Session::from_events(
            s.document().clone(),
            OperatorRegistry::standard().unwrap(),
            config,
            cp.session.clone(),
            cp.epoch,
            decoded,
        )
        .unwrap();
        assert_eq!(replay.checkpoint(), cp);
        assert_eq!(replay.snapshot().truth(), s.snapshot().truth());
        assert_eq!(replay.snapshot().provenance(), s.snapshot().provenance());
        assert!(!replay.is_playing());
        assert!(replay.playback_intent());
    }
}
#[test]
fn actual_v1_evicted_and_zero_baselines_migrate_and_typed_constructors_normalize() {
    for (bytes, capacity) in [
        (
            include_bytes!("fixtures/runtime/session-v1-evicted-events.json").as_slice(),
            2,
        ),
        (
            include_bytes!("fixtures/runtime/session-v1-zero-events.json").as_slice(),
            0,
        ),
    ] {
        let raw: SessionEnvelope = serde_json::from_slice(bytes).unwrap();
        assert_eq!(raw.version, 1);
        assert_eq!(raw.baseline.version, 1);
        let normalized = SessionEnvelope::decode(bytes, DecodeLimits::default()).unwrap();
        assert_eq!(normalized.version, 2);
        let serialized = raw.encode(1048576).unwrap();
        let wire: serde_json::Value = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(wire["version"], 2);
        assert_eq!(wire["baseline"]["version"], 2);
        for event in wire["events"].as_array().unwrap() {
            assert_eq!(event["version"], 2);
        }
        let s = Session::from_events(
            base().validate(DecodeLimits::default()).unwrap(),
            OperatorRegistry::standard().unwrap(),
            SessionConfig {
                event_capacity: capacity,
                ..Default::default()
            },
            raw.baseline.session.clone(),
            raw.baseline.epoch,
            raw,
        )
        .unwrap();
        assert_eq!(s.checkpoint().version, 2);
        assert!(!s.is_playing());
        assert!(s.playback_intent());
        assert_eq!(s.history_len(), 0);
        if capacity == 2 {
            assert_eq!(s.snapshot().time().ticks(), i64::MIN);
            assert!(s.live_overrides().is_empty());
            assert_eq!(
                s.snapshot().provenance()[&v("a")].origin,
                InputOrigin::Default
            );
        } else {
            assert_eq!(s.snapshot().time().ticks(), -127008000000);
            assert_eq!(s.live_overrides()[&p("b")], scalar(6.));
        }
    }
    let raw: SessionCheckpoint = serde_json::from_slice(include_bytes!(
        "fixtures/runtime/session-v1-checkpoint.json"
    ))
    .unwrap();
    assert_eq!(raw.version, 1);
    let saved_doc = Document::decode(
        include_bytes!("fixtures/runtime/session-v1-document.json"),
        DecodeLimits::default(),
    )
    .unwrap();
    let loaded = Session::from_checkpoint(
        saved_doc.clone(),
        OperatorRegistry::standard().unwrap(),
        Default::default(),
        raw.session.clone(),
        raw.epoch,
        raw.clone(),
    )
    .unwrap();
    assert_eq!(loaded.checkpoint().version, 2);
    let mut forbidden_doc = saved_doc.document().clone();
    for item in &mut forbidden_doc.parameters {
        item.metadata.learner_exposed = false;
    }
    assert!(
        Session::from_checkpoint(
            forbidden_doc.validate(DecodeLimits::default()).unwrap(),
            OperatorRegistry::standard().unwrap(),
            SessionConfig {
                role: SessionRole::Learner,
                ..Default::default()
            },
            raw.session.clone(),
            raw.epoch,
            raw
        )
        .is_err()
    );
}
#[test]
fn patch_cannot_claim_v1_and_mixed_future_versions_are_rejected_before_import() {
    let mut s = session(base(), Default::default());
    typed_patch(
        &mut s,
        vec![(p("a"), Some(scalar(5.)))],
        OverridePlaybackPolicy::Pause,
    )
    .unwrap();
    let envelope = s.export_events().unwrap();
    let mut wire = serde_json::to_value(&envelope).unwrap();
    wire["version"] = serde_json::json!(1);
    wire["baseline"]["version"] = serde_json::json!(1);
    wire["events"][0]["version"] = serde_json::json!(1);
    let bytes = serde_json::to_vec(&wire).unwrap();
    assert!(serde_json::from_slice::<SessionEnvelope>(&bytes).is_err());
    assert!(SessionEnvelope::decode(&bytes, DecodeLimits::default()).is_err());
    let mut falsely_old = envelope.clone();
    falsely_old.version = 1;
    falsely_old.baseline.version = 1;
    falsely_old.events[0].version = 1;
    assert!(falsely_old.encode(1048576).is_err());
    assert!(
        Session::from_events(
            s.document().clone(),
            OperatorRegistry::standard().unwrap(),
            Default::default(),
            s.checkpoint().session.clone(),
            s.epoch(),
            falsely_old
        )
        .is_err()
    );
    for mode in 0..3 {
        let mut bad = envelope.clone();
        match mode {
            0 => bad.version = 3,
            1 => bad.baseline.version = 1,
            _ => bad.events[0].version = 1,
        };
        assert!(
            Session::from_events(
                s.document().clone(),
                OperatorRegistry::standard().unwrap(),
                Default::default(),
                s.checkpoint().session.clone(),
                s.epoch(),
                bad
            )
            .is_err()
        );
    }
}
#[test]
fn duplicate_unknown_type_unit_nonfinite_range_and_parameter_authority_deny_atomically() {
    let mut s = session(base(), Default::default());
    for entries in [
        serde_json::json!([["a", value_json(5.)], ["a", null]]),
        serde_json::json!([["missing", null]]),
        serde_json::json!([["a",{"kind":"bool","data":true}]]),
        serde_json::json!([["a",{"kind":"scalar","data":null}]]),
        serde_json::json!([["a",{"kind":"scalar","data":5.,"unit":"metres"}]]),
        serde_json::json!([["a", value_json(101.)]]),
    ] {
        let before = capture(&s);
        assert!(patch(&mut s, entries, serde_json::json!("preserve")).is_err());
        assert_eq!(capture(&s), before);
    }
    let before = capture(&s);
    assert_eq!(
        typed_patch(
            &mut s,
            vec![(p("a"), Some(scalar(5.))), (p("a"), None)],
            OverridePlaybackPolicy::Preserve
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Duplicate
    );
    assert_eq!(capture(&s), before);
    for mode in 0..3 {
        let mut d = learner_document();
        match mode {
            0 => d.parameters[0].readonly = true,
            1 => d.parameters[0].metadata.locked = true,
            _ => d.parameters[0].metadata.learner_exposed = false,
        }
        let mut s = session(
            d,
            SessionConfig {
                role: SessionRole::Learner,
                ..Default::default()
            },
        );
        for value in [Some(scalar(5.)), None] {
            let before = capture(&s);
            assert_eq!(
                typed_patch(
                    &mut s,
                    vec![(p("a"), value)],
                    OverridePlaybackPolicy::Preserve
                )
                .unwrap_err()
                .code,
                DiagnosticCode::Permission
            );
            assert_eq!(capture(&s), before);
        }
    }
}
#[test]
fn directional_permissions_and_explicit_playback_permission_are_not_weakened() {
    for mode in 0..3 {
        let mut d = learner_document();
        match mode {
            0 => d.learner_permissions.overrides = false,
            1 => d.learner_permissions.return_to_guide = false,
            _ => d.learner_permissions.playback = false,
        }
        let mut s = session(
            d,
            SessionConfig {
                role: SessionRole::Learner,
                ..Default::default()
            },
        );
        let before = capture(&s);
        let result = match mode {
            0 => typed_patch(
                &mut s,
                vec![(p("a"), Some(scalar(5.)))],
                OverridePlaybackPolicy::Preserve,
            ),
            1 => typed_patch(
                &mut s,
                vec![(p("a"), None)],
                OverridePlaybackPolicy::Preserve,
            ),
            _ => typed_patch(&mut s, vec![], OverridePlaybackPolicy::SetIntent(false)),
        };
        assert_eq!(result.unwrap_err().code, DiagnosticCode::Permission);
        assert_eq!(capture(&s), before);
        if mode == 2 {
            typed_patch(
                &mut s,
                vec![(p("a"), Some(scalar(5.)))],
                OverridePlaybackPolicy::Pause,
            )
            .unwrap();
            assert!(!s.is_playing());
        }
    }
    let mut d = base();
    d.parameters[0].metadata.locked = true;
    let mut s = session(d, Default::default());
    assert!(s.return_to_guide(0, &[p("a")]).is_ok());
    let before = capture(&s);
    assert!(
        typed_patch(
            &mut s,
            vec![(p("a"), None)],
            OverridePlaybackPolicy::Preserve
        )
        .is_err()
    );
    assert_eq!(capture(&s), before);
}
#[test]
fn patch_noop_at_counter_ceiling_and_invalid_identity_never_rewrite_history() {
    let mut s = session(base(), Default::default());
    let mut cp = s.export_snapshot().unwrap();
    cp.sequence = u64::MAX;
    cp.revision = u64::MAX;
    s.restore_snapshot(0, cp).unwrap();
    let before = capture(&s);
    typed_patch(
        &mut s,
        vec![(p("a"), None)],
        OverridePlaybackPolicy::Preserve,
    )
    .unwrap();
    assert_eq!(capture(&s), before);
    assert_eq!(
        typed_patch(
            &mut s,
            vec![(p("a"), Some(scalar(5.)))],
            OverridePlaybackPolicy::Preserve
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Overflow
    );
    assert_eq!(capture(&s), before);
    let mut s = session(base(), Default::default());
    let mut foreign = s.prepare_command(CommandKind::PatchLiveOverrides {
        entries: vec![(p("a"), Some(scalar(5.)))],
        playback: OverridePlaybackPolicy::Pause,
    });
    foreign.generation = GenerationId::new("old-generation").unwrap();
    let before = capture(&s);
    assert!(s.dispatch(foreign).is_err());
    assert_eq!(capture(&s), before);
    let tx = s.begin_transaction(s.revision()).unwrap();
    let before = capture(&s);
    assert!(
        typed_patch(
            &mut s,
            vec![(p("a"), Some(scalar(5.)))],
            OverridePlaybackPolicy::Pause
        )
        .is_err()
    );
    assert_eq!(capture(&s), before);
    s.cancel_transaction(s.revision(), tx).unwrap();
}
fn vector_document() -> Document {
    let mut d = base();
    d.operations.clear();
    d.objects.clear();
    d.tracks.clear();
    d.constants.clear();
    d.parameters.truncate(1);
    d.parameters[0].default = Value::vector(vec![1.]).unwrap();
    d.parameters[0].port.shape = Shape::Vector { len: 1 };
    d.parameters[0].range = None;
    d
}
#[test]
fn patch_typed_capacity_command_event_candidate_and_checkpoint_budgets_are_atomic() {
    let mut s = session(base(), Default::default());
    let mut entries = Vec::with_capacity(100000);
    entries.push((p("a"), Some(scalar(5.))));
    let before = capture(&s);
    assert_eq!(
        typed_patch(&mut s, entries, OverridePlaybackPolicy::Pause)
            .unwrap_err()
            .code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), before);
    let mut s = session(vector_document(), Default::default());
    let mut values = Vec::with_capacity(200000);
    values.push(Finite::new(2.).unwrap());
    let before = capture(&s);
    assert_eq!(
        typed_patch(
            &mut s,
            vec![(p("a"), Some(Value::Vector(std::sync::Arc::new(values))))],
            OverridePlaybackPolicy::Pause
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), before);
    let mut values = Vec::with_capacity(40000);
    values.push(Finite::new(2.).unwrap());
    typed_patch(
        &mut s,
        vec![(p("a"), Some(Value::Vector(std::sync::Arc::new(values))))],
        OverridePlaybackPolicy::Pause,
    )
    .unwrap();
    assert_eq!(s.live_overrides()[&p("a")].heap_bytes().unwrap(), 8);
    let cp = s.export_snapshot().unwrap();
    let events = s.export_events().unwrap();
    let replay = Session::from_events(
        s.document().clone(),
        OperatorRegistry::standard().unwrap(),
        Default::default(),
        cp.session.clone(),
        cp.epoch,
        events,
    )
    .unwrap();
    assert_eq!(replay.checkpoint(), cp);
    let mut s = session(
        base(),
        SessionConfig {
            max_event_bytes: 192,
            ..Default::default()
        },
    );
    let before = capture(&s);
    assert_eq!(
        typed_patch(
            &mut s,
            vec![(p("a"), Some(scalar(5.)))],
            OverridePlaybackPolicy::Pause
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), before);
    let mut s = session(
        vector_document(),
        SessionConfig {
            max_candidate_bytes: 9000,
            ..Default::default()
        },
    );
    let mut entries = Vec::with_capacity(100);
    entries.push((p("a"), Some(Value::vector(vec![2.]).unwrap())));
    let before = capture(&s);
    assert_eq!(
        typed_patch(&mut s, entries, OverridePlaybackPolicy::Pause)
            .unwrap_err()
            .code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), before);
    let mut d = vector_document();
    d.parameters[0].default = scalar(1.);
    d.parameters[0].port.shape = Shape::Scalar;
    let mut second = d.parameters[0].clone();
    d.parameters[0].id = p(&format!("p0_{}", "x".repeat(95)));
    d.parameters[0].value_id = v("first");
    second.id = p(&format!("p1_{}", "x".repeat(95)));
    second.value_id = v("second");
    d.parameters.push(second);
    let mut s = session(
        d,
        SessionConfig {
            max_command_bytes: 512,
            ..Default::default()
        },
    );
    let first = s.document().document().parameters[0].id.clone();
    let second = s.document().document().parameters[1].id.clone();
    typed_patch(
        &mut s,
        vec![(first, Some(scalar(2.)))],
        OverridePlaybackPolicy::Pause,
    )
    .unwrap();
    let before = capture(&s);
    assert_eq!(
        typed_patch(
            &mut s,
            vec![(second, Some(scalar(2.)))],
            OverridePlaybackPolicy::Pause
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), before);
}
#[test]
fn bounded_command_and_event_decoders_reject_payload_limits_duplicates_and_nonfinite() {
    let s = session(base(), Default::default());
    let command = s.prepare_command(CommandKind::PatchLiveOverrides {
        entries: vec![(p("a"), Some(scalar(5.)))],
        playback: OverridePlaybackPolicy::Pause,
    });
    let bytes = command.encode(1048576).unwrap();
    assert!(Command::decode(&bytes, DecodeLimits::default()).is_ok());
    for limits in [
        DecodeLimits {
            max_bytes: 4,
            ..Default::default()
        },
        DecodeLimits {
            max_nodes: 0,
            ..Default::default()
        },
        DecodeLimits {
            max_elements: 0,
            ..Default::default()
        },
        DecodeLimits {
            max_depth: 2,
            ..Default::default()
        },
    ] {
        assert!(Command::decode(&bytes, limits).is_err());
    }
    let mut wire = serde_json::to_value(command).unwrap();
    wire["kind"]["entries"] = serde_json::json!([["a", value_json(5.)], ["a", null]]);
    assert!(Command::decode(&serde_json::to_vec(&wire).unwrap(), DecodeLimits::default()).is_err());
    wire["kind"]["entries"] = serde_json::json!([["a", value_json(5.)]]);
    let text = serde_json::to_string(&wire)
        .unwrap()
        .replace("5.0", "1e309");
    assert!(Command::decode(text.as_bytes(), DecodeLimits::default()).is_err());
    let mut s = session(base(), Default::default());
    typed_patch(
        &mut s,
        vec![(p("a"), Some(scalar(5.)))],
        OverridePlaybackPolicy::Pause,
    )
    .unwrap();
    let envelope = s.export_events().unwrap();
    let bytes = envelope.encode(1048576).unwrap();
    for limits in [
        DecodeLimits {
            max_bytes: 4,
            ..Default::default()
        },
        DecodeLimits {
            max_nodes: 0,
            ..Default::default()
        },
        DecodeLimits {
            max_elements: 0,
            ..Default::default()
        },
    ] {
        assert!(SessionEnvelope::decode(&bytes, limits).is_err());
    }
    let mut wire = serde_json::to_value(envelope).unwrap();
    wire["events"][0]["kind"]["entries"] = serde_json::json!([["a", value_json(5.)], ["a", null]]);
    assert!(
        SessionEnvelope::decode(&serde_json::to_vec(&wire).unwrap(), DecodeLimits::default())
            .is_err()
    );
}

#[test]
fn serialized_command_limit_and_event_allocation_limit_are_independent() {
    let mut d = vector_document();
    d.id = DocumentId::new("d".repeat(128)).unwrap();
    d.parameters[0].default = scalar(1.);
    d.parameters[0].port.shape = Shape::Scalar;
    d.parameters[0].id = p(&"p".repeat(128));
    let mut s = Session::new_with_identity(
        d.validate(DecodeLimits::default()).unwrap(),
        OperatorRegistry::standard().unwrap(),
        SessionConfig {
            max_command_bytes: 512,
            ..Default::default()
        },
        SessionId::new("s".repeat(128)).unwrap(),
    )
    .unwrap();
    let command = s.prepare_command(CommandKind::PatchLiveOverrides {
        entries: vec![(p(&"p".repeat(128)), None)],
        playback: OverridePlaybackPolicy::Preserve,
    });
    assert!(serde_json::to_vec(&command).unwrap().len() > 512);
    let before = capture(&s);
    assert!(s.dispatch(command).is_err());
    assert_eq!(capture(&s), before);
    let mut d = vector_document();
    let template = d.parameters[0].clone();
    d.parameters.clear();
    for i in 0..10 {
        let mut item = template.clone();
        item.default = scalar(1.);
        item.port.shape = Shape::Scalar;
        item.id = p(&format!("p{i}"));
        item.value_id = v(&format!("v{i}"));
        d.parameters.push(item);
    }
    let identity = SessionId::new("event-budget").unwrap();
    let make = |max_event_bytes| {
        Session::new_with_identity(
            d.clone().validate(DecodeLimits::default()).unwrap(),
            OperatorRegistry::standard().unwrap(),
            SessionConfig {
                max_event_bytes,
                ..Default::default()
            },
            identity.clone(),
        )
        .unwrap()
    };
    let entries = (0..10)
        .map(|i| (p(&format!("p{i}")), Some(scalar(2.))))
        .collect::<Vec<_>>();
    let mut probe = make(1048576);
    typed_patch(&mut probe, entries.clone(), OverridePlaybackPolicy::Pause).unwrap();
    let event = probe.events().back().unwrap();
    let encoded = serde_json::to_vec(event).unwrap().len();
    assert!(event.allocated_bytes().unwrap() > encoded + 4);
    let mut constrained = make(encoded + 4);
    let before = capture(&constrained);
    let error = typed_patch(&mut constrained, entries, OverridePlaybackPolicy::Pause).unwrap_err();
    assert_eq!(error.code, DiagnosticCode::Limit);
    assert!(error.message.contains("event allocation"));
    assert_eq!(capture(&constrained), before);
}
#[test]
fn imported_event_capacity_and_checkpoint_decoded_allocation_limits_are_enforced() {
    let mut source = session(base(), Default::default());
    typed_patch(
        &mut source,
        vec![(p("a"), Some(scalar(5.)))],
        OverridePlaybackPolicy::Pause,
    )
    .unwrap();
    let mut envelope = source.export_events().unwrap();
    let mut entries = Vec::with_capacity(100000);
    entries.push((p("a"), Some(scalar(5.))));
    envelope.events[0].kind = EventKind::PatchLiveOverrides {
        entries,
        playback: OverridePlaybackPolicy::Pause,
    };
    assert!(envelope.encode(1048576).is_err());
    let mut target = Session::new_with_identity(
        source.document().clone(),
        OperatorRegistry::standard().unwrap(),
        Default::default(),
        source.checkpoint().session.clone(),
    )
    .unwrap();
    let before = capture(&target);
    assert!(target.replay(0, envelope).is_err());
    assert_eq!(capture(&target), before);
    let mut d = vector_document();
    d.parameters[0].default = scalar(1.);
    d.parameters[0].port.shape = Shape::Scalar;
    let template = d.parameters[0].clone();
    d.parameters.clear();
    for i in 0..10 {
        let mut item = template.clone();
        item.id = p(&format!("p{i}"));
        item.value_id = v(&format!("v{i}"));
        d.parameters.push(item);
    }
    let mut s = session(d, Default::default());
    let entries = (0..10)
        .map(|i| (p(&format!("p{i}")), Some(scalar(2.))))
        .collect();
    typed_patch(&mut s, entries, OverridePlaybackPolicy::Pause).unwrap();
    let cp = s.export_snapshot().unwrap();
    let bytes = cp.encode(1048576).unwrap();
    assert!(cp.allocated_bytes().unwrap() > bytes.len());
    assert!(
        SessionCheckpoint::decode(
            &bytes,
            DecodeLimits {
                max_bytes: bytes.len(),
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn event_admission_rejects_large_compaction_payload_before_owning_event() {
    for event_capacity in [0, 2] {
        let mut d = base();
        d.operations.clear();
        d.objects.clear();
        d.tracks.clear();
        d.constants.clear();
        d.parameters.truncate(1);
        d.parameters[0].default = Value::vector(vec![1.; 8192]).unwrap();
        d.parameters[0].port.shape = Shape::Vector { len: 8192 };
        d.parameters[0].range = None;
        let mut s = session(
            d,
            SessionConfig {
                event_capacity,
                max_event_bytes: 192,
                ..Default::default()
            },
        );
        let mut data = Vec::with_capacity(16384);
        data.resize(8192, Finite::new(2.).unwrap());
        let before = capture(&s);
        let error = typed_patch(
            &mut s,
            vec![(p("a"), Some(Value::Vector(std::sync::Arc::new(data))))],
            OverridePlaybackPolicy::Preserve,
        )
        .unwrap_err();
        assert_eq!(error.code, DiagnosticCode::Limit);
        assert_eq!(error.message, "patch event allocation exceeds byte budget");
        assert_eq!(capture(&s), before);
    }
}
