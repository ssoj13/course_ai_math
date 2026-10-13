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
fn v(s: &str) -> ValueId {
    ValueId::new(s).unwrap()
}
#[test]
fn real_ts_fp64_sequential_vector_dot_fixture_bits_match_rust() {
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/runtime/dot-fp64.json")).unwrap();
    assert_eq!(fixture["dtype"], "fp64");
    assert_eq!(fixture["order"], "sequential");
    for case in fixture["cases"].as_array().unwrap() {
        let parse = |key: &str| {
            case[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| f64::from_bits(u64::from_str_radix(x.as_str().unwrap(), 16).unwrap()))
                .collect::<Vec<_>>()
        };
        let a = parse("a_bits");
        let b = parse("b_bits");
        let expected = u64::from_str_radix(case["result_bits"].as_str().unwrap(), 16).unwrap();
        let mut raw = base();
        raw.parameters.clear();
        raw.constants.clear();
        raw.operations.clear();
        raw.tracks.clear();
        raw.objects.clear();
        for (name, xs) in [("a", a), ("b", b)] {
            raw.constants.push(Constant {
                id: v(name),
                value: Value::vector(xs.clone()).unwrap(),
                port: Port {
                    shape: Shape::Vector { len: xs.len() },
                    unit: Unit::DIMENSIONLESS,
                },
                source_refs: vec![],
            });
        }
        raw.operations.push(Operation {
            id: OperationId::new("dot").unwrap(),
            operator: OperatorId::new("vector.dot").unwrap(),
            inputs: vec![v("a"), v("b")],
            output: v("dot"),
            output_port: Port {
                shape: Shape::Scalar,
                unit: Unit::DIMENSIONLESS,
            },
            source_refs: vec![],
        });
        let graph = OperatorRegistry::standard()
            .unwrap()
            .compile(raw.validate(DecodeLimits::default()).unwrap())
            .unwrap();
        let evaluation = graph.evaluate(&BTreeMap::new(), &BTreeMap::new()).unwrap();
        assert_eq!(
            evaluation.truth()[&v("dot")].as_scalar().unwrap().to_bits(),
            expected,
            "{}",
            case["name"]
        );
    }
}
#[test]
fn matrix_storage_edits_are_real_but_uninstalled_matrix_operator_fails_atomically() {
    let mut raw = base();
    raw.operations.clear();
    raw.tracks.clear();
    raw.objects.clear();
    raw.parameters.truncate(1);
    raw.constants.clear();
    raw.parameters[0].default = Value::matrix(2, 2, vec![1., 2., 3., 4.]).unwrap();
    raw.parameters[0].port.shape = Shape::Matrix { rows: 2, cols: 2 };
    raw.parameters[0].range = None;
    let mut s = Session::new(
        raw.validate(DecodeLimits::default()).unwrap(),
        OperatorRegistry::standard().unwrap(),
        Default::default(),
    )
    .unwrap();
    s.set_parameter(
        0,
        ParameterId::new("a").unwrap(),
        Value::matrix(2, 2, vec![4., 3., 2., 1.]).unwrap(),
    )
    .unwrap();
    assert_eq!(
        s.snapshot().truth()[&v("a")].components(),
        vec![4., 3., 2., 1.]
    );
    let old = s.snapshot();
    let op = Operation {
        id: OperationId::new("inverse").unwrap(),
        operator: OperatorId::new("matrix.inverse").unwrap(),
        inputs: vec![v("a")],
        output: v("inverse"),
        output_port: s.document().document().parameters[0].port.clone(),
        source_refs: vec![],
    };
    let kind = CommandKind::EditDocument {
        expected_document_revision: 1,
        edits: vec![DocumentEdit::PutOperation { operation: op }],
    };
    assert_eq!(
        s.dispatch(s.prepare_command(kind)).unwrap_err().code,
        DiagnosticCode::Capability
    );
    assert_eq!(s.snapshot(), old);
}
#[test]
fn discrete_presentation_always_cuts_and_truth_is_exact_for_every_progress() {
    let mut raw = base();
    raw.operations.clear();
    raw.tracks.clear();
    raw.objects.clear();
    raw.constants.clear();
    raw.parameters.clear();
    raw.assets.push(Asset {
        id: AssetId::new("file-a").unwrap(),
        uri: "a.png".into(),
    });
    raw.assets.push(Asset {
        id: AssetId::new("file-b").unwrap(),
        uri: "b.png".into(),
    });
    for (name, value) in [
        ("enum", Value::Enum("a".into())),
        ("bool", Value::Bool(false)),
        ("file", Value::File(Some(AssetId::new("file-a").unwrap()))),
        (
            "color",
            Value::Color {
                space: ColorSpace::LinearSrgb,
                channels: [Finite::new(0.).unwrap(); 4],
            },
        ),
    ] {
        let mut metadata = ParameterMetadata::default();
        if name == "enum" {
            metadata.enum_choices = vec!["a".into(), "b".into()];
        }
        raw.parameters.push(Parameter {
            id: ParameterId::new(name).unwrap(),
            value_id: v(name),
            port: Port {
                shape: value.shape(),
                unit: Unit::DIMENSIONLESS,
            },
            default: value,
            range: None,
            readonly: false,
            metadata,
            source_refs: vec![],
        });
    }
    let mut s = Session::new(
        raw.validate(DecodeLimits::default()).unwrap(),
        OperatorRegistry::standard().unwrap(),
        Default::default(),
    )
    .unwrap();
    let from = PresentationFrame::cut(&s.snapshot());
    let values = BTreeMap::from([
        (ParameterId::new("enum").unwrap(), Value::Enum("b".into())),
        (ParameterId::new("bool").unwrap(), Value::Bool(true)),
        (
            ParameterId::new("file").unwrap(),
            Value::File(Some(AssetId::new("file-b").unwrap())),
        ),
        (
            ParameterId::new("color").unwrap(),
            Value::Color {
                space: ColorSpace::LinearSrgb,
                channels: [Finite::new(1.).unwrap(); 4],
            },
        ),
    ]);
    let kind = CommandKind::SetParameters {
        expected_document_revision: 0,
        values,
    };
    s.dispatch(s.prepare_command(kind)).unwrap();
    let truth = s.snapshot();
    for t in [0., 0.01, 0.5, 0.99, 1.] {
        let shown = PresentationFrame::interpolate(&from, &truth, Finite::new(t).unwrap()).unwrap();
        assert_eq!(shown.shown(), truth.truth());
        assert_eq!(s.snapshot(), truth);
        let retarget =
            PresentationFrame::interpolate(&shown, &truth, Finite::new(0.25).unwrap()).unwrap();
        assert_eq!(retarget.shown(), truth.truth());
    }
    assert_eq!(PresentationFrame::cut(&truth).shown(), truth.truth());
}
