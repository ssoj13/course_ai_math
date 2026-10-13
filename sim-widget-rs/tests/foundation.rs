use sim_widget_rs::{math::OperatorRegistry, model::*, runtime::*};
use std::collections::BTreeMap;

fn doc() -> ValidatedDocument {
    Document::decode(
        include_bytes!("fixtures/cosines.json"),
        DecodeLimits::default(),
    )
    .unwrap()
}
fn session() -> Session {
    Session::new(
        doc(),
        OperatorRegistry::standard().unwrap(),
        SessionConfig::default(),
    )
    .unwrap()
}
fn p(id: &str) -> ParameterId {
    ParameterId::new(id).unwrap()
}
fn v(id: &str) -> ValueId {
    ValueId::new(id).unwrap()
}
fn scalar(x: f64) -> Value {
    Value::scalar(x).unwrap()
}
fn number(frame: &FrameSnapshot, id: &str) -> f64 {
    frame.truth()[&v(id)].as_scalar().unwrap()
}

#[test]
fn time_is_exact_string_and_checked_rational() {
    let t = TimeTicks::new(9_007_199_254_740_993);
    assert_eq!(serde_json::to_string(&t).unwrap(), "\"9007199254740993\"");
    assert_eq!(
        serde_json::from_str::<TimeTicks>("\"9007199254740993\"").unwrap(),
        t
    );
    assert!(serde_json::from_str::<TimeTicks>("9007199254740993").is_err());
    assert!(serde_json::from_str::<TimeTicks>("\"01\"").is_err());
    assert_eq!(
        TimeTicks::from_rational(1, 1, 24).unwrap().ticks(),
        10_584_000_000
    );
    assert_eq!(
        TimeTicks::from_rational(1, 1001, 30000).unwrap().ticks(),
        8_475_667_200
    );
    assert!(TimeTicks::from_rational(1, 1, 11).is_err());
    assert!(TimeTicks::from_rational(1, 1, 0).is_err());
    assert!(TimeTicks::from_rational(i64::MAX, i64::MAX, 1).is_err());
    assert!(
        TimeTicks::new(i64::MAX)
            .checked_add(TimeTicks::new(1))
            .is_err()
    );
    assert!(
        TimeTicks::new(i64::MIN)
            .checked_sub(TimeTicks::new(1))
            .is_err()
    );
}

#[test]
fn malformed_documents_are_addressable_and_bounded() {
    assert!(Value::scalar(f64::NAN).is_err());
    assert!(Value::vector(vec![1., f64::INFINITY]).is_err());
    assert!(Value::matrix(2, 2, vec![1., 2.]).is_err());
    assert!(DocumentId::new("display label").is_err());
    let bytes = include_bytes!("fixtures/cosines.json");
    let limits = DecodeLimits {
        max_bytes: 2,
        ..DecodeLimits::default()
    };
    assert_eq!(
        Document::decode(bytes, limits).unwrap_err().code,
        DiagnosticCode::Limit
    );
    let limits = DecodeLimits {
        max_nodes: 1,
        ..DecodeLimits::default()
    };
    assert_eq!(
        Document::decode(bytes, limits).unwrap_err().code,
        DiagnosticCode::Limit
    );
    let mut raw: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    raw["schema_version"] = 99.into();
    assert_eq!(
        Document::decode(&serde_json::to_vec(&raw).unwrap(), DecodeLimits::default())
            .unwrap_err()
            .code,
        DiagnosticCode::Schema
    );
    raw["schema_version"] = 1.into();
    raw["unexpected"] = true.into();
    assert!(Document::decode(&serde_json::to_vec(&raw).unwrap(), DecodeLimits::default()).is_err());
}

#[test]
fn cycles_duplicate_references_and_dimensions_are_rejected() {
    let mut d = doc().document().clone();
    d.operations[0].inputs[0] = v("c");
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Cycle
    );
    let mut d = doc().document().clone();
    d.operations[0].output = v("a");
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Duplicate
    );
    let mut d = doc().document().clone();
    d.operations[0].inputs[0] = v("missing");
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Reference
    );
    let mut d = doc().document().clone();
    d.parameters[0].port.shape = Shape::Vector { len: 3 };
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Type
    );
    let mut d = doc().document().clone();
    d.operations[6].output_port.unit = Unit::DIMENSIONLESS;
    assert_eq!(
        Session::new(
            d.validate(DecodeLimits::default()).unwrap(),
            OperatorRegistry::standard().unwrap(),
            SessionConfig::default()
        )
        .unwrap_err()
        .code,
        DiagnosticCode::Unit
    );
}

#[test]
fn cosine_tracks_seek_and_provenance_use_actual_values() {
    let mut s = session();
    let initial = s.snapshot();
    assert!((number(&initial, "c2") - 13.).abs() < 1e-12);
    for (ticks, want) in [
        (254_016_000_000, 37.),
        (0, 13.),
        (127_008_000_000, 25.),
        (0, 13.),
    ] {
        s.seek(s.revision(), TimeTicks::new(ticks)).unwrap();
        let f = s.snapshot();
        assert!((number(&f, "c2") - want).abs() < 1e-12);
        let record = &f.provenance()[&v("c2")];
        assert_eq!(record.operation.as_ref().unwrap().as_str(), "subtract");
        assert_eq!(record.operands[0].id, v("sum"));
        assert_eq!(record.operands[0].value, scalar(25.));
        assert_eq!(record.result, f.truth()[&v("c2")]);
    }
    assert!((number(&initial, "c2") - 13.).abs() < 1e-12);
    assert_eq!(s.document().document().revision, 0);
    assert_eq!(s.history_len(), 0);
}

#[test]
fn override_precedes_evaluation_and_return_restores_guide() {
    let mut s = session();
    s.set_playing(s.revision(), true).unwrap();
    s.set_live_override(
        s.revision(),
        p("theta"),
        scalar(std::f64::consts::FRAC_PI_2),
    )
    .unwrap();
    assert!(!s.is_playing());
    assert!((number(&s.snapshot(), "c2") - 25.).abs() < 1e-12);
    assert_eq!(
        s.snapshot().provenance()[&v("theta")].origin,
        InputOrigin::Override
    );
    s.seek(s.revision(), TimeTicks::new(254_016_000_000))
        .unwrap();
    assert!((number(&s.snapshot(), "c2") - 25.).abs() < 1e-12);
    s.return_to_guide(s.revision(), &[p("theta")]).unwrap();
    assert!((number(&s.snapshot(), "c2") - 37.).abs() < 1e-12);
    assert_eq!(s.history_len(), 0);
}

#[test]
fn commands_are_revision_checked_atomic_and_undoable() {
    let mut s = session();
    let rev = s.revision();
    s.set_parameter(rev, p("a"), scalar(6.)).unwrap();
    assert!((number(&s.snapshot(), "c2") - 28.).abs() < 1e-12);
    assert_eq!(s.document().document().revision, 1);
    let current = s.snapshot();
    assert_eq!(
        s.set_parameter(rev, p("a"), scalar(2.)).unwrap_err().code,
        DiagnosticCode::StaleRevision
    );
    assert!(s.set_parameter(s.revision(), p("a"), scalar(-1.)).is_err());
    assert_eq!(s.snapshot(), current);
    s.undo(s.revision()).unwrap();
    assert!((number(&s.snapshot(), "c2") - 13.).abs() < 1e-12);
    s.redo(s.revision()).unwrap();
    assert!((number(&s.snapshot(), "c2") - 28.).abs() < 1e-12);
    assert_eq!(s.document().document().revision, 3);
}

#[test]
fn drag_is_one_history_entry_cancel_restores_and_stale_update_does_not_write() {
    let mut s = session();
    let id = s.begin_transaction(s.revision()).unwrap();
    s.update_transaction(s.revision(), id, BTreeMap::from([(p("a"), scalar(5.))]))
        .unwrap();
    s.update_transaction(s.revision(), id, BTreeMap::from([(p("a"), scalar(6.))]))
        .unwrap();
    assert_eq!(s.history_len(), 0);
    assert_eq!(s.document().document().revision, 0);
    s.commit_transaction(s.revision(), id).unwrap();
    assert_eq!(s.history_len(), 1);
    s.undo(s.revision()).unwrap();
    assert!((number(&s.snapshot(), "c2") - 13.).abs() < 1e-12);
    let id = s.begin_transaction(s.revision()).unwrap();
    s.update_transaction(s.revision(), id, BTreeMap::from([(p("a"), scalar(10.))]))
        .unwrap();
    let current = s.snapshot();
    assert!(
        s.update_transaction(0, id, BTreeMap::from([(p("a"), scalar(12.))]))
            .is_err()
    );
    assert_eq!(s.snapshot(), current);
    s.cancel_transaction(s.revision(), id).unwrap();
    assert!((number(&s.snapshot(), "c2") - 13.).abs() < 1e-12);
    s.redo(s.revision()).unwrap();
    assert!((number(&s.snapshot(), "c2") - 28.).abs() < 1e-12);
}

#[test]
fn evaluation_failure_rolls_back_document_and_whole_transaction() {
    let mut d = doc().document().clone();
    d.operations[0].operator = OperatorId::new("scalar.div").unwrap();
    d.operations[0].output_port.unit = Unit::DIMENSIONLESS;
    d.operations = vec![d.operations[0].clone()];
    d.operations[0].inputs = vec![v("a"), v("theta")];
    d.operations[0].output_port.unit = Unit {
        length: 1,
        time: 0,
        angle: -1,
    };
    d.objects.clear();
    d.tracks.clear();
    d.parameters[2].range = None;
    let mut s = Session::new(
        d.validate(DecodeLimits::default()).unwrap(),
        OperatorRegistry::standard().unwrap(),
        SessionConfig::default(),
    )
    .unwrap();
    let before = s.snapshot();
    assert_eq!(
        s.set_parameter(s.revision(), p("theta"), scalar(0.))
            .unwrap_err()
            .code,
        DiagnosticCode::Singular
    );
    assert_eq!(s.snapshot(), before);
    let id = s.begin_transaction(s.revision()).unwrap();
    s.update_transaction(s.revision(), id, BTreeMap::from([(p("a"), scalar(5.))]))
        .unwrap();
    let before = s.snapshot();
    assert!(
        s.update_transaction(
            s.revision(),
            id,
            BTreeMap::from([(p("a"), scalar(9.)), (p("theta"), scalar(0.))])
        )
        .is_err()
    );
    assert_eq!(s.snapshot(), before);
    s.cancel_transaction(s.revision(), id).unwrap();
    assert_eq!(number(&s.snapshot(), "a"), 3.);
}
