include!("fixtures/runtime/support.rs");
#[test]
fn batch_connect_rebind_undo_and_explicit_delete_repairs() {
    let mut s = session(base(), SessionConfig::default());
    let before = s.snapshot();
    let prop = PropertyId::new("opposite_length").unwrap();
    let mut op = s.document().document().operations.last().unwrap().clone();
    op.operator = OperatorId::new("scalar.add").unwrap();
    op.inputs = vec![v("a"), v("b")];
    op.output_port.unit = Unit::LENGTH;
    let mut object = s.document().document().objects[0].clone();
    object.bindings.insert(prop.clone(), v("a"));
    command(
        &mut s,
        CommandKind::EditDocument {
            expected_document_revision: 0,
            edits: vec![
                DocumentEdit::PutOperation { operation: op },
                DocumentEdit::PutObject { object },
            ],
        },
    )
    .unwrap();
    assert_eq!(n(&s, "c"), 7.);
    assert_eq!(
        s.snapshot().properties().values().next().unwrap().value,
        scalar(3.)
    );
    assert!(s.snapshot().changes().structural);
    assert!((before.truth()[&v("c")].as_scalar().unwrap() - 13f64.sqrt()).abs() < 1e-12);
    s.undo(s.revision()).unwrap();
    assert_eq!(s.snapshot().properties(), before.properties());
    s.redo(s.revision()).unwrap();
    assert_eq!(n(&s, "c"), 7.);
    let current = capture(&s);
    assert!(
        command(
            &mut s,
            CommandKind::EditDocument {
                expected_document_revision: 3,
                edits: vec![DocumentEdit::DeleteParameter { id: p("a") }]
            }
        )
        .is_err()
    );
    assert_eq!(capture(&s), current);
    let ops = s
        .document()
        .document()
        .operations
        .iter()
        .map(|o| DocumentEdit::DeleteOperation { id: o.id.clone() })
        .collect::<Vec<_>>();
    let mut edits = ops;
    edits.push(DocumentEdit::DeleteParameter { id: p("a") });
    edits.push(DocumentEdit::DeleteObject {
        id: s.document().document().objects[0].id.clone(),
    });
    command(
        &mut s,
        CommandKind::EditDocument {
            expected_document_revision: 3,
            edits,
        },
    )
    .unwrap();
    assert!(!s.snapshot().truth().contains_key(&v("a")));
    assert!(s.snapshot().properties().is_empty());
    assert!(!s.snapshot().changes().removed_properties.is_empty());
}
#[test]
fn role_locked_readonly_and_derived_write_paths_reject_atomically() {
    let mut learner = session(
        base(),
        SessionConfig {
            role: SessionRole::Learner,
            ..Default::default()
        },
    );
    let before = capture(&learner);
    assert_eq!(
        learner
            .set_parameter(0, p("a"), scalar(4.))
            .unwrap_err()
            .code,
        DiagnosticCode::Permission
    );
    assert!(learner.begin_transaction(0).is_err());
    assert!(
        command(
            &mut learner,
            CommandKind::EditDocument {
                expected_document_revision: 0,
                edits: vec![DocumentEdit::Unlock { parameter: p("a") }]
            }
        )
        .is_err()
    );
    assert_eq!(capture(&learner), before);
    for readonly in [true, false] {
        let mut d = base();
        d.parameters[0].readonly = readonly;
        d.parameters[0].metadata.locked = !readonly;
        let mut s = session(d, SessionConfig::default());
        let old = capture(&s);
        assert!(s.set_parameter(0, p("a"), scalar(5.)).is_err());
        assert!(s.set_live_override(0, p("a"), scalar(5.)).is_err());
        assert!(
            command(
                &mut s,
                CommandKind::SetParameters {
                    expected_document_revision: 0,
                    values: BTreeMap::from([(p("a"), scalar(5.))])
                }
            )
            .is_err()
        );
        assert_eq!(capture(&s), old);
        let tx = s.begin_transaction(0).unwrap();
        let old = capture(&s);
        assert!(
            s.update_transaction(s.revision(), tx, BTreeMap::from([(p("a"), scalar(5.))]))
                .is_err()
        );
        assert_eq!(capture(&s), old);
        s.cancel_transaction(s.revision(), tx).unwrap();
        if !readonly {
            command(
                &mut s,
                CommandKind::EditDocument {
                    expected_document_revision: 0,
                    edits: vec![DocumentEdit::Unlock { parameter: p("a") }],
                },
            )
            .unwrap();
            s.set_parameter(s.revision(), p("a"), scalar(5.)).unwrap();
        }
    }
    let mut d = base();
    d.parameters[0].metadata.learner_exposed = false;
    let mut s = session(
        d,
        SessionConfig {
            role: SessionRole::Learner,
            ..Default::default()
        },
    );
    assert!(s.set_live_override(0, p("a"), scalar(4.)).is_err());
    let mut s = session(base(), Default::default());
    let old = capture(&s);
    assert!(s.set_parameter(0, p("c"), scalar(4.)).is_err());
    assert_eq!(capture(&s), old);
}
#[test]
fn multi_edit_operator_failure_stale_document_and_transaction_preserve_envelope() {
    let mut s = session(base(), Default::default());
    s.set_playing(0, true).unwrap();
    let old = capture(&s);
    assert!(
        command(
            &mut s,
            CommandKind::SetParameters {
                expected_document_revision: 0,
                values: BTreeMap::from([(p("a"), scalar(9.)), (p("b"), scalar(-1.))])
            }
        )
        .is_err()
    );
    assert_eq!(capture(&s), old);
    assert!(
        command(
            &mut s,
            CommandKind::EditDocument {
                expected_document_revision: 1,
                edits: vec![]
            }
        )
        .is_err()
    );
    assert_eq!(capture(&s), old);
    let mut op = s.document().document().operations[0].clone();
    op.operator = OperatorId::new("scalar.div").unwrap();
    op.inputs = vec![v("a"), v("zero")];
    op.output_port.unit = Unit::LENGTH;
    let zero = Constant {
        id: v("zero"),
        value: scalar(0.),
        port: Port {
            shape: Shape::Scalar,
            unit: Unit::DIMENSIONLESS,
        },
        source_refs: vec![],
    };
    // Reuse the independent final operation; remove all obsolete graph consumers atomically.
    let mut edits = s
        .document()
        .document()
        .operations
        .iter()
        .map(|o| DocumentEdit::DeleteOperation { id: o.id.clone() })
        .collect::<Vec<_>>();
    edits.push(DocumentEdit::DeleteObject {
        id: s.document().document().objects[0].id.clone(),
    });
    edits.push(DocumentEdit::PutConstant { constant: zero });
    edits.push(DocumentEdit::PutOperation { operation: op });
    let error = command(
        &mut s,
        CommandKind::EditDocument {
            expected_document_revision: 0,
            edits,
        },
    )
    .unwrap_err();
    assert_eq!(error.code, DiagnosticCode::Singular);
    assert!(error.path == "operations/square-a");
    assert_eq!(capture(&s), old);
    let tx = s.begin_transaction(s.revision()).unwrap();
    s.update_transaction(s.revision(), tx, BTreeMap::from([(p("a"), scalar(7.))]))
        .unwrap();
    let old = capture(&s);
    assert!(s.update_transaction(0, tx, BTreeMap::new()).is_err());
    assert!(s.set_playing(s.revision(), false).is_err());
    assert_eq!(capture(&s), old);
    s.cancel_transaction(s.revision(), tx).unwrap();
    assert_eq!(n(&s, "a"), 3.);
}
#[test]
fn runtime_reset_defaults_is_distinct_from_return_to_animated_guide() {
    let mut s = session(base(), Default::default());
    s.seek(0, TimeTicks::new(254016000000)).unwrap();
    assert!((n(&s, "c2") - 37.).abs() < 1e-12);
    s.set_playing(s.revision(), true).unwrap();
    command(
        &mut s,
        CommandKind::ResetParameters {
            expected_document_revision: 0,
            parameters: vec![p("theta"), p("a")],
        },
    )
    .unwrap();
    assert!(!s.is_playing());
    assert!((n(&s, "c2") - 13.).abs() < 1e-12);
    assert!(matches!(
        s.events().back().unwrap().kind,
        EventKind::ResetDefaults { .. }
    ));
    let old = capture(&s);
    command(
        &mut s,
        CommandKind::ResetParameters {
            expected_document_revision: 0,
            parameters: vec![p("theta"), p("a")],
        },
    )
    .unwrap();
    assert_eq!(capture(&s), old);
    s.return_to_guide(s.revision(), &[p("theta")]).unwrap();
    assert!(s.live_overrides().contains_key(&p("a")));
    assert!((n(&s, "c2") - 37.).abs() < 1e-12);
    let old = capture(&s);
    assert!(
        command(
            &mut s,
            CommandKind::ResetParameters {
                expected_document_revision: 0,
                parameters: vec![p("theta"), p("missing")]
            }
        )
        .is_err()
    );
    assert_eq!(capture(&s), old);
}
#[test]
fn history_event_command_candidate_budgets_and_revision_overflow_are_atomic() {
    let mut s = session(
        base(),
        SessionConfig {
            max_command_bytes: 512,
            ..Default::default()
        },
    );
    let old = capture(&s);
    assert_eq!(
        command(
            &mut s,
            CommandKind::EditDocument {
                expected_document_revision: 0,
                edits: vec![DocumentEdit::PutAsset {
                    asset: Asset {
                        id: AssetId::new("big").unwrap(),
                        uri: "x".repeat(1000)
                    }
                }]
            }
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), old);
    let mut s = session(
        base(),
        SessionConfig {
            max_batch_edits: 1,
            ..Default::default()
        },
    );
    let old = capture(&s);
    assert!(
        command(
            &mut s,
            CommandKind::SetLiveOverrides {
                values: BTreeMap::from([(p("a"), scalar(5.)), (p("b"), scalar(6.))])
            }
        )
        .is_err()
    );
    assert_eq!(capture(&s), old);
    let mut d = base();
    d.revision = u64::MAX;
    let mut s = session(d, Default::default());
    let old = capture(&s);
    assert_eq!(
        s.set_parameter(0, p("a"), scalar(5.)).unwrap_err().code,
        DiagnosticCode::Overflow
    );
    assert_eq!(capture(&s), old);
    let mut s = session(base(), Default::default());
    let mut c = s.export_snapshot().unwrap();
    c.revision = u64::MAX;
    c.sequence = u64::MAX;
    s.restore_snapshot(0, c).unwrap();
    let old = capture(&s);
    assert_eq!(
        s.seek(s.revision(), TimeTicks::ZERO).unwrap_err().code,
        DiagnosticCode::Overflow
    );
    assert_eq!(capture(&s), old);
    let mut s = session(
        base(),
        SessionConfig {
            max_history_bytes: 16000,
            ..Default::default()
        },
    );
    for x in [5., 6., 7., 8.] {
        s.set_parameter(s.revision(), p("a"), scalar(x)).unwrap();
    }
    assert_eq!(s.history_len(), 1);
    assert!(s.can_undo());
    s.undo(s.revision()).unwrap();
    assert_eq!(n(&s, "a"), 7.);
    s.redo(s.revision()).unwrap();
    assert_eq!(n(&s, "a"), 8.);
    let old = capture(&s);
    let expected_document_revision = s.document().document().revision;
    let mut huge = s.document().document().operations[0].clone();
    huge.output_port.shape = Shape::Matrix {
        rows: usize::MAX,
        cols: 2,
    };
    assert!(
        command(
            &mut s,
            CommandKind::EditDocument {
                expected_document_revision,
                edits: vec![DocumentEdit::PutOperation { operation: huge }]
            }
        )
        .is_err()
    );
    assert_eq!(capture(&s), old);
    assert!(
        Session::new(
            base().validate(DecodeLimits::default()).unwrap(),
            OperatorRegistry::standard().unwrap(),
            SessionConfig {
                max_candidate_bytes: 1,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn reserved_capacity_live_value_rejects_before_clone_and_preserves_all_state() {
    let mut d = base();
    d.operations.clear();
    d.objects.clear();
    d.tracks.clear();
    d.parameters.truncate(1);
    d.parameters[0].range = None;
    d.parameters[0].default = Value::vector(vec![1.]).unwrap();
    d.parameters[0].port.shape = Shape::Vector { len: 1 };
    let mut s = session(d, Default::default());
    let before = capture(&s);
    let mut xs = Vec::with_capacity(200000);
    xs.push(Finite::new(2.).unwrap());
    assert_eq!(
        s.set_live_override(0, p("a"), Value::Vector(std::sync::Arc::new(xs)))
            .unwrap_err()
            .code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), before);
}

#[test]
fn fix_r3_oversized_newest_history_group_rejects_with_undo_enabled() {
    let mut s = session(
        base(),
        SessionConfig {
            max_history_bytes: 10000,
            ..Default::default()
        },
    );
    let before = capture(&s);
    assert_eq!(
        s.set_parameter(0, p("a"), scalar(5.)).unwrap_err().code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), before);
    let mut disabled = session(
        base(),
        SessionConfig {
            history_capacity: 0,
            max_history_bytes: 0,
            ..Default::default()
        },
    );
    disabled.set_parameter(0, p("a"), scalar(5.)).unwrap();
    assert_eq!(disabled.history_len(), 0);
    assert_eq!(n(&disabled, "a"), 5.);
}
#[test]
fn fix_r5_frozen_registry_is_bounded_before_runtime_cloning() {
    let mut d = base();
    d.parameters.clear();
    d.constants.clear();
    d.operations.clear();
    d.tracks.clear();
    d.objects.clear();
    let descriptors = (0..200)
        .map(|i| OperatorDescriptor {
            id: OperatorId::new(format!("custom-{i}")).unwrap(),
            builtin: Builtin::Add,
            description: "x".repeat(4096),
        })
        .collect();
    let registry = OperatorRegistry::from_descriptors(descriptors).unwrap();
    assert_eq!(
        Session::new(
            d.validate(DecodeLimits::default()).unwrap(),
            registry,
            SessionConfig {
                max_candidate_bytes: 4096,
                ..Default::default()
            }
        )
        .expect_err("oversized registry must reject")
        .code,
        DiagnosticCode::Limit
    );
}

#[test]
fn fix_r3_history_budget_failure_preserves_redo_and_active_draft() {
    let mut s = session(
        base(),
        SessionConfig {
            max_history_bytes: 16000,
            ..Default::default()
        },
    );
    s.set_parameter(0, p("a"), scalar(5.)).unwrap();
    s.undo(s.revision()).unwrap();
    let tx = s.begin_transaction(s.revision()).unwrap();
    let mut object = s.snapshot().objects()[0].clone();
    object.label = "x".repeat(4000);
    let mut second = object.clone();
    second.id = ObjectId::new("second").unwrap();
    command(
        &mut s,
        CommandKind::UpdateDocumentTransaction {
            id: tx,
            expected_document_revision: 2,
            edits: vec![
                DocumentEdit::PutObject { object },
                DocumentEdit::PutObject { object: second },
            ],
        },
    )
    .unwrap();
    let before = capture(&s);
    assert_eq!(
        s.commit_transaction(s.revision(), tx).unwrap_err().code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), before);
    assert!(s.can_redo());
    s.cancel_transaction(s.revision(), tx).unwrap();
    s.redo(s.revision()).unwrap();
    assert_eq!(n(&s, "a"), 5.);
}
#[test]
fn fix_r5_shared_registry_within_budget_supports_transaction_and_failed_growth_is_atomic() {
    let config = SessionConfig {
        max_candidate_bytes: 150000,
        ..Default::default()
    };
    let mut s = session(base(), config);
    let tx = s.begin_transaction(0).unwrap();
    s.update_transaction(s.revision(), tx, BTreeMap::from([(p("a"), scalar(5.))]))
        .unwrap();
    s.commit_transaction(s.revision(), tx).unwrap();
    assert_eq!(n(&s, "a"), 5.);
    let template = s.snapshot().objects()[0].clone();
    let objects = (0..45)
        .map(|i| {
            let mut object = template.clone();
            object.id = ObjectId::new(format!("large-{i}")).unwrap();
            object.label = "x".repeat(3000);
            object
        })
        .collect::<Vec<_>>();
    let mut raw = s.document().document().clone();
    raw.objects.extend(objects.clone());
    raw.validate(DecodeLimits::default()).unwrap();
    let expected_document_revision = s.document().document().revision;
    let before = capture(&s);
    assert_eq!(
        command(
            &mut s,
            CommandKind::EditDocument {
                expected_document_revision,
                edits: objects
                    .into_iter()
                    .map(|object| DocumentEdit::PutObject { object })
                    .collect()
            }
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Limit
    );
    assert_eq!(capture(&s), before);
}

fn peak_document(objects: usize) -> Document {
    let mut d = base();
    d.parameters.clear();
    d.constants.clear();
    d.operations.clear();
    d.tracks.clear();
    d.objects.clear();
    for i in 0..objects {
        d.objects.push(SceneObject {
            id: ObjectId::new(format!("peak-{i}")).unwrap(),
            label: "x".repeat(4000),
            properties: BTreeMap::new(),
            bindings: BTreeMap::new(),
            assets: vec![],
        });
    }
    d
}
#[test]
fn fix_r5_f1_author_transition_rejects_old_plus_new_documents_before_copying() {
    for retention in [0, 4] {
        let config = SessionConfig {
            history_capacity: retention,
            event_capacity: retention,
            max_candidate_bytes: 150000,
            ..Default::default()
        };
        let mut s = session(peak_document(25), config);
        let mut object = s.snapshot().objects()[0].clone();
        object.label = "y".repeat(4000);
        let before = capture(&s);
        assert_eq!(
            command(
                &mut s,
                CommandKind::EditDocument {
                    expected_document_revision: 0,
                    edits: vec![DocumentEdit::PutObject { object }]
                }
            )
            .unwrap_err()
            .code,
            DiagnosticCode::Limit
        );
        assert_eq!(capture(&s), before);
    }
}
#[test]
fn fix_r5_f1_transaction_commit_preflight_preserves_draft_with_each_retention_mode() {
    for retention in [0, 4] {
        let config = SessionConfig {
            history_capacity: retention,
            event_capacity: retention,
            max_candidate_bytes: 150000,
            ..Default::default()
        };
        let mut s = session(peak_document(9), config);
        let tx = s.begin_transaction(0).unwrap();
        let mut object = s.snapshot().objects()[0].clone();
        object.label = "y".repeat(4000);
        command(
            &mut s,
            CommandKind::UpdateDocumentTransaction {
                id: tx,
                expected_document_revision: 0,
                edits: vec![DocumentEdit::PutObject { object }],
            },
        )
        .unwrap();
        let before = capture(&s);
        assert_eq!(
            s.commit_transaction(s.revision(), tx).unwrap_err().code,
            DiagnosticCode::Limit
        );
        assert_eq!(capture(&s), before);
        s.cancel_transaction(s.revision(), tx).unwrap();
        assert_eq!(s.snapshot().objects()[0].label, "x".repeat(4000));
    }
}
#[test]
fn fix_r5_f1_small_author_transitions_keep_history_and_epoch_with_each_retention_mode() {
    for retention in [0, 4] {
        let config = SessionConfig {
            history_capacity: retention,
            event_capacity: retention,
            max_candidate_bytes: 150000,
            ..Default::default()
        };
        let mut s = session(peak_document(1), config);
        let mut object = s.snapshot().objects()[0].clone();
        object.label = "y".repeat(4000);
        command(
            &mut s,
            CommandKind::EditDocument {
                expected_document_revision: 0,
                edits: vec![DocumentEdit::PutObject { object }],
            },
        )
        .unwrap();
        let tx = s.begin_transaction(s.revision()).unwrap();
        let mut object = s.snapshot().objects()[0].clone();
        object.label = "z".repeat(4000);
        command(
            &mut s,
            CommandKind::UpdateDocumentTransaction {
                id: tx,
                expected_document_revision: 1,
                edits: vec![DocumentEdit::PutObject { object }],
            },
        )
        .unwrap();
        s.commit_transaction(s.revision(), tx).unwrap();
        assert_eq!(s.epoch(), 2);
        assert_eq!(s.checkpoint().sequence, 4);
        assert_eq!(s.snapshot().objects()[0].label, "z".repeat(4000));
        if retention == 0 {
            assert_eq!(s.history_len(), 0);
            assert!(s.events().is_empty());
        } else {
            s.undo(s.revision()).unwrap();
            assert_eq!(s.snapshot().objects()[0].label, "y".repeat(4000));
            s.redo(s.revision()).unwrap();
            assert_eq!(s.snapshot().objects()[0].label, "z".repeat(4000));
        }
        assert!(s.export_events().unwrap().events.is_empty());
        assert_eq!(s.export_events().unwrap().baseline, s.checkpoint());
    }
}
