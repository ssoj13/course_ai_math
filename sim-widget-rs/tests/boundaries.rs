use sim_widget_rs::{math::*, model::*, runtime::*};
use std::collections::BTreeMap;

fn base() -> Document {
    Document::decode(
        include_bytes!("fixtures/cosines.json"),
        DecodeLimits::default(),
    )
    .unwrap()
    .document()
    .clone()
}
fn p(s: &str) -> ParameterId {
    ParameterId::new(s).unwrap()
}
fn v(s: &str) -> ValueId {
    ValueId::new(s).unwrap()
}
fn scalar(x: f64) -> Value {
    Value::scalar(x).unwrap()
}
fn session(d: Document, config: SessionConfig) -> Session {
    Session::new(
        d.validate(DecodeLimits::default()).unwrap(),
        OperatorRegistry::standard().unwrap(),
        config,
    )
    .unwrap()
}
fn n(s: &Session, id: &str) -> f64 {
    s.snapshot().truth()[&v(id)].as_scalar().unwrap()
}

#[test]
fn source_capability_and_json_depth_validation_precede_loading() {
    let mut d = base();
    d.required_capabilities.push("arbitrary.script.v1".into());
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Capability
    );
    let mut d = base();
    d.parameters[0]
        .source_refs
        .push(SourceRef::Asset(AssetId::new("missing").unwrap()));
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Reference
    );
    let limits = DecodeLimits {
        max_depth: 2,
        ..DecodeLimits::default()
    };
    assert_eq!(
        Document::decode(br#"{"nested":[[[[0]]]]}"#, limits)
            .unwrap_err()
            .code,
        DiagnosticCode::Limit
    );
    assert!(
        Document::decode(
            br#"{"schema_version":1,"schema_version":2}"#,
            DecodeLimits::default()
        )
        .is_err()
    );
}

#[test]
fn decoded_budget_accounts_for_derived_vector_storage() {
    let mut d = base();
    d.parameters.clear();
    d.constants.clear();
    d.operations.clear();
    d.tracks.clear();
    d.objects.clear();
    d.constants.push(Constant {
        id: v("x"),
        value: Value::vector(vec![3., 4.]).unwrap(),
        port: Port {
            shape: Shape::Vector { len: 2 },
            unit: Unit::LENGTH,
        },
        source_refs: vec![],
    });
    d.constants.push(Constant {
        id: v("scale"),
        value: scalar(2.),
        port: Port {
            shape: Shape::Scalar,
            unit: Unit::DIMENSIONLESS,
        },
        source_refs: vec![],
    });
    d.operations.push(Operation {
        id: OperationId::new("scaled").unwrap(),
        operator: OperatorId::new("vector.scale").unwrap(),
        inputs: vec![v("x"), v("scale")],
        output: v("scaled"),
        output_port: Port {
            shape: Shape::Vector { len: 2 },
            unit: Unit::LENGTH,
        },
        source_refs: vec![],
    });
    let limits = DecodeLimits {
        max_elements: 3,
        ..DecodeLimits::default()
    };
    assert_eq!(d.validate(limits).unwrap_err().code, DiagnosticCode::Limit);
}

#[test]
fn vector_graph_uses_real_dot_norm_and_component_operations() {
    let mut d = base();
    d.parameters.clear();
    d.constants.clear();
    d.operations.clear();
    d.tracks.clear();
    d.objects.clear();
    for (id, values) in [("x", vec![3., 4.]), ("y", vec![2., -1.])] {
        d.constants.push(Constant {
            id: v(id),
            value: Value::vector(values).unwrap(),
            port: Port {
                shape: Shape::Vector { len: 2 },
                unit: Unit::LENGTH,
            },
            source_refs: vec![],
        });
    }
    for (id, operator, inputs, shape, unit) in [
        (
            "dot",
            "vector.dot",
            vec![v("x"), v("y")],
            Shape::Scalar,
            Unit {
                length: 2,
                time: 0,
                angle: 0,
            },
        ),
        (
            "norm",
            "vector.norm",
            vec![v("x")],
            Shape::Scalar,
            Unit::LENGTH,
        ),
        (
            "sum",
            "vector.add",
            vec![v("x"), v("y")],
            Shape::Vector { len: 2 },
            Unit::LENGTH,
        ),
        (
            "diff",
            "vector.sub",
            vec![v("x"), v("y")],
            Shape::Vector { len: 2 },
            Unit::LENGTH,
        ),
    ] {
        d.operations.push(Operation {
            id: OperationId::new(id).unwrap(),
            operator: OperatorId::new(operator).unwrap(),
            inputs,
            output: v(id),
            output_port: Port { shape, unit },
            source_refs: vec![],
        });
    }
    let s = session(d, SessionConfig::default());
    assert_eq!(n(&s, "dot"), 2.);
    assert_eq!(n(&s, "norm"), 5.);
    assert_eq!(
        s.snapshot().truth()[&v("sum")],
        Value::vector(vec![5., 3.]).unwrap()
    );
    assert_eq!(
        s.snapshot().truth()[&v("diff")],
        Value::vector(vec![1., 5.]).unwrap()
    );
    assert_eq!(
        s.snapshot().provenance()[&v("dot")].operands[1].value,
        Value::vector(vec![2., -1.]).unwrap()
    );
}

#[test]
fn unsupported_operators_and_wrong_arity_fail_before_evaluation() {
    let mut d = base();
    d.operations[0].operator = OperatorId::new("missing.plugin").unwrap();
    assert_eq!(
        Session::new(
            d.validate(DecodeLimits::default()).unwrap(),
            OperatorRegistry::standard().unwrap(),
            SessionConfig::default()
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Capability
    );
    let mut d = base();
    d.operations[0].inputs.pop();
    assert_eq!(
        Session::new(
            d.validate(DecodeLimits::default()).unwrap(),
            OperatorRegistry::standard().unwrap(),
            SessionConfig::default()
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Type
    );
}

#[test]
fn final_constraints_reject_seek_and_override_without_mutating_events_or_playback() {
    let mut d = base();
    d.parameters[2].range = None;
    d.tracks[0].keys[1].value = Finite::new(0.).unwrap();
    d.constraints.push(Constraint::NonZero {
        parameter: p("theta"),
    });
    let mut s = session(d, SessionConfig::default());
    s.set_playing(s.revision(), true).unwrap();
    let before = s.snapshot();
    let events = s.events().clone();
    assert_eq!(
        s.seek(s.revision(), TimeTicks::new(254_016_000_000))
            .unwrap_err()
            .code,
        DiagnosticCode::Range
    );
    assert!(
        s.set_live_override(s.revision(), p("theta"), scalar(0.))
            .is_err()
    );
    assert_eq!(s.snapshot(), before);
    assert_eq!(s.events(), &events);
    assert!(s.is_playing());
    assert!(s.live_overrides().is_empty());
    assert_eq!(
        s.snapshot().provenance()[&v("theta")].validated_constraints,
        vec![0]
    );
    assert!(
        s.set_live_override(s.revision(), p("c2"), scalar(0.))
            .is_err()
    );
}

#[test]
fn unchanged_author_parameter_does_not_erase_redo_or_advance_document_revision() {
    let mut s = session(base(), SessionConfig::default());
    s.set_parameter(s.revision(), p("a"), scalar(6.)).unwrap();
    s.undo(s.revision()).unwrap();
    let document_revision = s.document().document().revision;
    s.set_parameter(s.revision(), p("a"), scalar(3.)).unwrap();
    assert_eq!(s.document().document().revision, document_revision);
    assert_eq!(s.history_len(), 1);
    assert!(s.can_redo());
    s.redo(s.revision()).unwrap();
    assert_eq!(n(&s, "a"), 6.);
}

#[test]
fn bounded_history_and_event_log_preserve_last_edits_and_new_branch() {
    let config = SessionConfig {
        history_capacity: 2,
        event_capacity: 2,
        ..SessionConfig::default()
    };
    let mut s = session(base(), config);
    for x in [4., 5., 6.] {
        s.set_parameter(s.revision(), p("a"), scalar(x)).unwrap();
    }
    assert_eq!(s.history_len(), 2);
    assert_eq!(s.events().len(), 2);
    s.undo(s.revision()).unwrap();
    assert_eq!(n(&s, "a"), 5.);
    s.undo(s.revision()).unwrap();
    assert_eq!(n(&s, "a"), 4.);
    let before = s.snapshot();
    assert!(s.undo(s.revision()).is_err());
    assert_eq!(s.snapshot(), before);
    s.set_parameter(s.revision(), p("a"), scalar(7.)).unwrap();
    assert!(!s.can_redo());
    assert!(s.redo(s.revision()).is_err());
    assert_eq!(s.events().len(), 2);
}

#[test]
fn transactions_block_interleaving_and_noop_drag_preserves_redo() {
    let mut s = session(base(), SessionConfig::default());
    s.set_parameter(s.revision(), p("a"), scalar(6.)).unwrap();
    s.undo(s.revision()).unwrap();
    let id = s.begin_transaction(s.revision()).unwrap();
    let before = s.snapshot();
    assert_eq!(
        s.seek(s.revision(), TimeTicks::new(10)).unwrap_err().code,
        DiagnosticCode::Transaction
    );
    assert_eq!(s.snapshot(), before);
    s.update_transaction(s.revision(), id, BTreeMap::from([(p("a"), scalar(5.))]))
        .unwrap();
    s.update_transaction(s.revision(), id, BTreeMap::from([(p("a"), scalar(3.))]))
        .unwrap();
    s.commit_transaction(s.revision(), id).unwrap();
    assert_eq!(s.history_len(), 1);
    assert!(s.can_redo());
    s.redo(s.revision()).unwrap();
    assert_eq!(n(&s, "a"), 6.);
}

#[test]
fn presentation_interpolation_never_changes_truth_or_provenance() {
    let mut s = session(base(), SessionConfig::default());
    let shown = PresentationFrame::cut(&s.snapshot());
    s.set_parameter(s.revision(), p("a"), scalar(6.)).unwrap();
    let truth = s.snapshot();
    let half = PresentationFrame::interpolate(&shown, &truth, Finite::new(0.5).unwrap()).unwrap();
    assert_eq!(half.shown()[&v("a")], scalar(4.5));
    assert_eq!(half.target_revision(), truth.revision());
    assert_eq!(s.snapshot(), truth);
    assert_eq!(n(&s, "a"), 6.);
    assert!(PresentationFrame::interpolate(&shown, &truth, Finite::new(2.).unwrap()).is_err());
}

#[test]
fn rebased_track_adapter_preserves_large_absolute_times_and_reports_unsafe_span() {
    let start = 9_007_199_254_740_993i64;
    let track = ScalarTrack {
        parameter: p("theta"),
        keys: vec![
            ScalarKey {
                time: TimeTicks::new(start),
                value: Finite::new(10.).unwrap(),
                interpolation: Interpolation::Linear,
            },
            ScalarKey {
                time: TimeTicks::new(start + 254_016_000_000),
                value: Finite::new(20.).unwrap(),
                interpolation: Interpolation::Linear,
            },
        ],
    };
    let adapter = ScalarTrackAdapter::new(track).unwrap();
    assert_eq!(
        adapter
            .sample(TimeTicks::new(start + 127_008_000_000))
            .unwrap(),
        scalar(15.)
    );
    let track = ScalarTrack {
        parameter: p("theta"),
        keys: vec![
            ScalarKey {
                time: TimeTicks::ZERO,
                value: Finite::new(1.).unwrap(),
                interpolation: Interpolation::Linear,
            },
            ScalarKey {
                time: TimeTicks::new(i64::MAX),
                value: Finite::new(2.).unwrap(),
                interpolation: Interpolation::Linear,
            },
        ],
    };
    assert_eq!(
        ScalarTrackAdapter::new(track).unwrap_err().code,
        DiagnosticCode::Time
    );
}

#[test]
fn constant_easy_ease_adapter_preserves_exact_segment_value() {
    for value in [1., -1., 1e200] {
        let adapter = ScalarTrackAdapter::new(ScalarTrack {
            parameter: p("constant"),
            keys: vec![
                ScalarKey {
                    time: TimeTicks::ZERO,
                    value: Finite::new(value).unwrap(),
                    interpolation: Interpolation::EasyEase,
                },
                ScalarKey {
                    time: TimeTicks::new(TICKS_PER_SECOND),
                    value: Finite::new(value).unwrap(),
                    interpolation: Interpolation::EasyEase,
                },
            ],
        })
        .unwrap();
        for ticks in [
            TICKS_PER_SECOND / 100,
            TICKS_PER_SECOND / 3,
            TICKS_PER_SECOND * 99 / 100,
        ] {
            assert_eq!(
                adapter
                    .sample(TimeTicks::new(ticks))
                    .unwrap()
                    .as_scalar()
                    .unwrap()
                    .to_bits(),
                value.to_bits(),
            );
        }
    }
}

#[test]
fn constant_easy_ease_constrained_session_seeks_forward_and_backward() {
    let mut d = base();
    d.parameters[2].default = scalar(1.);
    d.parameters[2].range = Some(NumericRange {
        min: Finite::new(1.).unwrap(),
        max: Finite::new(1.).unwrap(),
    });
    for key in &mut d.tracks[0].keys {
        key.value = Finite::new(1.).unwrap();
        key.interpolation = Interpolation::EasyEase;
    }
    let mut fixed = d.parameters[2].clone();
    fixed.id = p("fixed_theta");
    fixed.value_id = v("fixed_theta");
    fixed.readonly = true;
    d.parameters.push(fixed);
    d.constraints.push(Constraint::Equal {
        left: p("theta"),
        right: p("fixed_theta"),
    });
    let mut s = session(d, SessionConfig::default());
    for ticks in [
        TICKS_PER_SECOND / 100,
        TICKS_PER_SECOND / 3,
        TICKS_PER_SECOND * 99 / 100,
        TICKS_PER_SECOND,
        TICKS_PER_SECOND / 100,
        0,
    ] {
        s.seek(s.revision(), TimeTicks::new(ticks)).unwrap();
        let frame = s.snapshot();
        assert_eq!(frame.time(), TimeTicks::new(ticks));
        assert_eq!(frame.truth()[&v("theta")], scalar(1.));
        assert_eq!(frame.truth()[&v("fixed_theta")], scalar(1.));
        assert_eq!(frame.provenance()[&v("theta")].origin, InputOrigin::Track);
        assert_eq!(
            frame.provenance()[&v("theta")].validated_constraints,
            vec![0],
        );
    }
    assert_eq!(s.document().document().revision, 0);
    assert_eq!(s.history_len(), 0);
    let before = s.snapshot();
    assert_eq!(
        s.set_live_override(s.revision(), p("theta"), scalar(1. + f64::EPSILON))
            .unwrap_err()
            .code,
        DiagnosticCode::Range,
    );
    assert_eq!(s.snapshot(), before);
}
