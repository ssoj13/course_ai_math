include!("fixtures/runtime/support.rs");
fn equivalent(s: &Session) {
    let full = s.full_evaluation().unwrap();
    assert_eq!(s.snapshot().truth(), full.truth());
    assert_eq!(s.snapshot().provenance(), full.provenance());
}
#[test]
fn incremental_operation_trace_matches_fresh_full_eval_and_skips_unaffected_branch() {
    let mut s = session(base(), Default::default());
    let old = s.snapshot();
    s.set_live_override(0, p("a"), scalar(5.)).unwrap();
    equivalent(&s);
    let trace = s
        .snapshot()
        .evaluated_operations()
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect::<Vec<_>>();
    assert!(trace.contains(&"square-a".to_owned()));
    assert!(!trace.contains(&"square-b".to_owned()));
    assert!(!trace.contains(&"cos".to_owned()));
    assert_eq!(old.truth()[&v("a")], scalar(3.));
    for ticks in [254016000000, 0, 127008000000, 0] {
        s.seek(s.revision(), TimeTicks::new(ticks)).unwrap();
        equivalent(&s);
    }
    s.return_to_guide(s.revision(), &[p("a")]).unwrap();
    equivalent(&s);
    s.set_parameter(s.revision(), p("b"), scalar(7.)).unwrap();
    equivalent(&s);
}
#[test]
fn same_bits_changed_origin_is_provenance_only_and_old_snapshots_are_immutable() {
    let mut s = session(base(), Default::default());
    let old = s.snapshot();
    s.set_live_override(0, p("a"), scalar(3.)).unwrap();
    equivalent(&s);
    let f = s.snapshot();
    assert!(f.changes().changed_values.is_empty());
    assert_eq!(
        f.changes().provenance_only,
        std::collections::BTreeSet::from([v("a")])
    );
    assert_eq!(f.provenance()[&v("a")].origin, InputOrigin::Override);
    assert_eq!(old.provenance()[&v("a")].origin, InputOrigin::Default);
    assert!(f.evaluated_operations().is_empty());
    assert!(f.changes().changed_properties.is_empty());
    s.return_to_guide(s.revision(), &[p("a")]).unwrap();
    assert!(s.snapshot().changes().provenance_only.contains(&v("a")));
}
#[test]
fn dependent_bound_track_override_and_return_fail_before_evaluator_publication() {
    let mut d = base();
    d.constraints.push(Constraint::ScalarBounds {
        parameter: p("a"),
        min: None,
        max: Some(p("b")),
    });
    d.tracks.push(ScalarTrack {
        parameter: p("b"),
        keys: vec![
            ScalarKey {
                time: TimeTicks::ZERO,
                value: Finite::new(4.).unwrap(),
                interpolation: Interpolation::Linear,
            },
            ScalarKey {
                time: TimeTicks::new(254016000000),
                value: Finite::new(2.).unwrap(),
                interpolation: Interpolation::Linear,
            },
        ],
    });
    let mut s = session(d, Default::default());
    s.set_playing(0, true).unwrap();
    let old = capture(&s);
    let e = s
        .seek(s.revision(), TimeTicks::new(254016000000))
        .unwrap_err();
    assert_eq!(e.code, DiagnosticCode::Range);
    assert_eq!(e.path, "constraints/0");
    assert!(e.affected.contains(&"a".to_owned()));
    assert_eq!(capture(&s), old);
    command(
        &mut s,
        CommandKind::SetLiveOverrides {
            values: BTreeMap::from([(p("a"), scalar(1.)), (p("b"), scalar(5.))]),
        },
    )
    .unwrap();
    s.seek(s.revision(), TimeTicks::new(254016000000)).unwrap();
    s.set_live_override(s.revision(), p("a"), scalar(4.))
        .unwrap();
    let old = capture(&s);
    assert!(s.return_to_guide(s.revision(), &[p("b")]).is_err());
    assert_eq!(capture(&s), old);
    equivalent(&s);
}
#[test]
fn vector_norm_constraints_and_two_resolved_properties_are_live_and_bounded() {
    let mut d = base();
    d.tracks.clear();
    d.operations.clear();
    d.constants.clear();
    d.objects.clear();
    d.parameters.truncate(2);
    d.parameters[0].default = Value::vector(vec![3., 4.]).unwrap();
    d.parameters[0].port.shape = Shape::Vector { len: 2 };
    d.parameters[0].range = None;
    d.parameters[1].default = scalar(5.);
    d.constraints = vec![Constraint::NormBounds {
        parameter: p("a"),
        min: None,
        max: Some(p("b")),
    }];
    let property = PropertyId::new("position").unwrap();
    let port = d.parameters[0].port.clone();
    for name in ["object1", "object2"] {
        d.objects.push(SceneObject {
            id: ObjectId::new(name).unwrap(),
            label: name.into(),
            properties: BTreeMap::from([(property.clone(), port.clone())]),
            bindings: BTreeMap::from([(property.clone(), v("a"))]),
            assets: vec![],
        });
    }
    let mut s = session(d, Default::default());
    let old = capture(&s);
    assert!(
        s.set_live_override(0, p("a"), Value::vector(vec![6., 8.]).unwrap())
            .is_err()
    );
    assert_eq!(capture(&s), old);
    s.set_live_override(0, p("a"), Value::vector(vec![0., 5.]).unwrap())
        .unwrap();
    assert_eq!(s.snapshot().changes().changed_properties.len(), 2);
    assert_eq!(s.snapshot().properties().len(), 2);
    equivalent(&s);
    let first = s.document().document().objects[0].id.clone();
    command(
        &mut s,
        CommandKind::EditDocument {
            expected_document_revision: 0,
            edits: vec![DocumentEdit::DeleteObject { id: first.clone() }],
        },
    )
    .unwrap();
    assert!(s.snapshot().changes().removed_objects.contains(&first));
    assert_eq!(s.snapshot().changes().removed_properties.len(), 1);
    assert_eq!(s.snapshot().properties().len(), 1);
}
#[test]
fn incremental_cache_rejects_foreign_compiled_contract_and_constant_ease_stays_exact() {
    let registry = OperatorRegistry::standard().unwrap();
    let doc = base().validate(DecodeLimits::default()).unwrap();
    let graph = registry.compile(doc.clone()).unwrap();
    let inputs = doc
        .document()
        .parameters
        .iter()
        .map(|p| (p.id.clone(), p.default.clone()))
        .collect::<BTreeMap<_, _>>();
    let origins = inputs
        .keys()
        .map(|id| (id.clone(), InputOrigin::Default))
        .collect::<BTreeMap<_, _>>();
    let old = graph.evaluate(&inputs, &origins).unwrap();
    let mut raw = base();
    raw.revision = 1;
    let next = registry
        .compile(raw.validate(DecodeLimits::default()).unwrap())
        .unwrap();
    assert_eq!(
        next.evaluate_incremental(&inputs, &origins, &old)
            .unwrap_err()
            .code,
        DiagnosticCode::StaleRevision
    );
    let mut raw = base();
    raw.tracks[0].keys.iter_mut().for_each(|k| {
        k.value = Finite::new(1.).unwrap();
        k.interpolation = Interpolation::EasyEase;
    });
    raw.parameters[2].default = scalar(1.);
    raw.parameters[2].range = Some(NumericRange {
        min: Finite::new(1.).unwrap(),
        max: Finite::new(1.).unwrap(),
    });
    let mut s = session(raw, Default::default());
    for time in [0, 2540160000, 254016000000, 127008000000, 2540160000, 0] {
        s.seek(s.revision(), TimeTicks::new(time)).unwrap();
        assert_eq!(n(&s, "theta").to_bits(), 1f64.to_bits());
        equivalent(&s);
    }
}

#[test]
fn fix_r4_object_dirties_compare_previous_publication_not_committed_graph() {
    let mut s = session(base(), Default::default());
    let original = s.document().document().objects[0].clone();
    let id = original.id.clone();
    let tx = s.begin_transaction(0).unwrap();
    let mut preview = original.clone();
    preview.label = "Preview label".into();
    command(
        &mut s,
        CommandKind::UpdateDocumentTransaction {
            id: tx,
            expected_document_revision: 0,
            edits: vec![DocumentEdit::PutObject { object: preview }],
        },
    )
    .unwrap();
    let prior = s.snapshot();
    s.cancel_transaction(s.revision(), tx).unwrap();
    assert_eq!(
        s.snapshot().changes().changed_objects,
        std::collections::BTreeSet::from([id.clone()])
    );
    assert!(prior.changes().changed_objects.contains(&id));
}

#[test]
fn fix_r4_created_deleted_reverted_and_committed_previews_emit_exact_object_dirties() {
    let mut s = session(base(), Default::default());
    let original = s.snapshot().objects()[0].clone();
    let original_frame = s.snapshot();
    let tx = s.begin_transaction(0).unwrap();
    let mut created = original.clone();
    created.id = ObjectId::new("temporary").unwrap();
    let update = |s: &mut Session, edits| {
        command(
            s,
            CommandKind::UpdateDocumentTransaction {
                id: tx,
                expected_document_revision: 0,
                edits,
            },
        )
    };
    update(
        &mut s,
        vec![
            DocumentEdit::PutObject {
                object: created.clone(),
            },
            DocumentEdit::DeleteObject {
                id: original.id.clone(),
            },
        ],
    )
    .unwrap();
    let preview = s.snapshot();
    assert_eq!(
        preview.changes().changed_objects,
        std::collections::BTreeSet::from([created.id.clone()])
    );
    assert_eq!(
        preview.changes().removed_objects,
        std::collections::BTreeSet::from([original.id.clone()])
    );
    update(
        &mut s,
        vec![
            DocumentEdit::PutObject {
                object: original.clone(),
            },
            DocumentEdit::DeleteObject {
                id: created.id.clone(),
            },
        ],
    )
    .unwrap();
    assert_eq!(
        s.snapshot().changes().changed_objects,
        std::collections::BTreeSet::from([original.id.clone()])
    );
    assert_eq!(
        s.snapshot().changes().removed_objects,
        std::collections::BTreeSet::from([created.id.clone()])
    );
    let mut renamed = original.clone();
    renamed.label = "Saved preview".into();
    update(
        &mut s,
        vec![DocumentEdit::PutObject {
            object: renamed.clone(),
        }],
    )
    .unwrap();
    let last_preview = s.snapshot();
    s.commit_transaction(s.revision(), tx).unwrap();
    assert!(s.snapshot().changes().changed_objects.is_empty());
    assert!(s.snapshot().changes().removed_objects.is_empty());
    assert_eq!(s.snapshot().objects(), &[renamed]);
    assert!(s.snapshot().changes().structural);
    assert_eq!(original_frame.objects(), &[original]);
    assert_eq!(preview.objects(), &[created]);
    assert_eq!(last_preview.objects()[0].label, "Saved preview");
}
