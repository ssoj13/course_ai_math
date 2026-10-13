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
fn id(s: &str) -> ParameterId {
    ParameterId::new(s).unwrap()
}
fn val(s: &str) -> ValueId {
    ValueId::new(s).unwrap()
}
fn param(name: &str, value: Value) -> Parameter {
    Parameter {
        id: id(name),
        value_id: val(name),
        port: Port {
            shape: value.shape(),
            unit: Unit::DIMENSIONLESS,
        },
        default: value,
        range: None,
        readonly: false,
        metadata: ParameterMetadata {
            learner_exposed: true,
            ..Default::default()
        },
        source_refs: vec![],
    }
}
fn empty() -> Document {
    let mut d = base();
    d.parameters.clear();
    d.constants.clear();
    d.operations.clear();
    d.tracks.clear();
    d.objects.clear();
    d.constraints.clear();
    d
}
#[test]
fn archived_v1_migrates_to_current_schema() {
    let d = Document::decode(
        include_bytes!("fixtures/cosines.json"),
        DecodeLimits::default(),
    )
    .unwrap();
    assert_eq!(d.document().schema_version, 2);
    assert_eq!(
        d.document(),
        Document::decode(
            include_bytes!("fixtures/runtime/cosines-v2.json"),
            DecodeLimits::default()
        )
        .unwrap()
        .document()
    );
    let bytes = d.encode(1048576).unwrap();
    assert_eq!(
        Document::decode(&bytes, DecodeLimits::default()).unwrap(),
        d
    );
    assert!(d.encode(4).is_err());
    let object = &d.document().objects[0];
    let property = PropertyId::new("opposite_length").unwrap();
    assert_eq!(object.bindings[&property], val("c"));
    assert_eq!(object.properties[&property].unit, Unit::LENGTH);
    let mut s = Session::new(
        d,
        OperatorRegistry::standard().unwrap(),
        SessionConfig::default(),
    )
    .unwrap();
    for (time, expected) in [(0, 13.), (127008000000, 25.), (254016000000, 37.)] {
        s.seek(s.revision(), TimeTicks::new(time)).unwrap();
        assert!((s.snapshot().truth()[&val("c2")].as_scalar().unwrap() - expected).abs() < 1e-12);
    }
}
#[test]
fn eight_parameter_kinds_roundtrip_exact_integer_file_color_enum() {
    let mut d = empty();
    let asset = AssetId::new("image").unwrap();
    d.assets.push(Asset {
        id: asset.clone(),
        uri: "image.png".into(),
    });
    d.parameters = vec![
        param("real", Value::scalar(1.25).unwrap()),
        param("integer", Value::Integer(9007199254740993)),
        param("boolean", Value::Bool(true)),
        param("vector", Value::vector(vec![1., 2.]).unwrap()),
        param("matrix", Value::matrix(2, 2, vec![1., 2., 3., 4.]).unwrap()),
        param("choice", Value::Enum("stable-a".into())),
        param(
            "color",
            Value::Color {
                space: ColorSpace::Srgb,
                channels: [Finite::new(0.5).unwrap(); 4],
            },
        ),
        param("file", Value::File(Some(asset))),
    ];
    d.parameters[1].metadata.integer_range = Some(IntegerRange {
        min: 9007199254740993,
        max: 9007199254740993,
    });
    d.parameters[1].metadata.integer_step = Some(1);
    d.parameters[5].metadata.enum_choices = vec!["stable-a".into(), "stable-b".into()];
    let valid = d.clone().validate(DecodeLimits::default()).unwrap();
    assert_eq!(
        Document::decode(&valid.encode(1048576).unwrap(), DecodeLimits::default())
            .unwrap()
            .document(),
        &d
    );
    assert!(
        d.parameters[1]
            .validate_value(&Value::Integer(9007199254740992))
            .is_err()
    );
    assert!(
        d.parameters[5]
            .validate_value(&Value::Enum("display label".into()))
            .is_err()
    );
    let mut bad = d.clone();
    bad.parameters[7].default = Value::File(Some(AssetId::new("missing").unwrap()));
    let e = bad.validate(DecodeLimits::default()).unwrap_err();
    assert_eq!(e.code, DiagnosticCode::Reference);
    assert!(e.path.contains("file"));
    let mut bad = d.clone();
    bad.parameters[6].default = Value::Color {
        space: ColorSpace::Srgb,
        channels: [Finite::new(1.1).unwrap(); 4],
    };
    assert!(bad.validate(DecodeLimits::default()).is_err());
    let mut bad = d.clone();
    bad.parameters[4].default = Value::Matrix {
        rows: usize::MAX,
        cols: 2,
        values: std::sync::Arc::new(vec![]),
    };
    assert!(bad.validate(DecodeLimits::default()).is_err());
    let mut bad = d.clone();
    bad.parameters[5]
        .metadata
        .enum_choices
        .push("stable-a".into());
    assert_eq!(
        bad.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Duplicate
    );
}
#[test]
fn checked_metadata_display_units_and_shape_units() {
    let d = base();
    for x in [0., -1.] {
        let mut bad = d.clone();
        bad.parameters[0].metadata.step = Some(Finite::new(x).unwrap());
        assert!(
            bad.validate(DecodeLimits::default())
                .unwrap_err()
                .path
                .contains("a")
        );
    }
    let mut bad = d.clone();
    bad.parameters[0].metadata.precision = Some(16);
    assert!(bad.validate(DecodeLimits::default()).is_err());
    let mut bad = d.clone();
    bad.parameters[0].metadata.display_unit = Some(DisplayUnit::Degrees);
    assert_eq!(
        bad.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Unit
    );
    assert_eq!(
        DisplayUnit::Centimetres
            .to_canonical(Finite::new(300.).unwrap(), Unit::LENGTH)
            .unwrap()
            .get(),
        3.
    );
    assert_eq!(
        DisplayUnit::Millimetres
            .from_canonical(Finite::new(3.).unwrap(), Unit::LENGTH)
            .unwrap()
            .get(),
        3000.
    );
    assert_eq!(
        DisplayUnit::Milliseconds
            .to_canonical(
                Finite::new(1000.).unwrap(),
                Unit {
                    length: 0,
                    time: 1,
                    angle: 0
                }
            )
            .unwrap()
            .get(),
        1.
    );
    assert_eq!(
        DisplayUnit::Degrees
            .to_canonical(Finite::new(180.).unwrap(), Unit::ANGLE)
            .unwrap()
            .get(),
        std::f64::consts::PI
    );
    let derived = Unit::LENGTH.multiply(Unit::LENGTH).unwrap();
    let unit = DisplayUnit::Derived {
        unit: derived,
        scale: Finite::new(0.0001).unwrap(),
    };
    assert_eq!(
        unit.to_canonical(Finite::new(10000.).unwrap(), derived)
            .unwrap()
            .get(),
        1.
    );
    let mut integer = param("i", Value::Integer(i64::MAX));
    integer.metadata.integer_range = Some(IntegerRange {
        min: i64::MAX,
        max: i64::MAX,
    });
    assert!(integer.validate_value(&Value::Integer(i64::MAX)).is_ok());
    assert!(
        integer
            .validate_value(&Value::Integer(i64::MAX - 1))
            .is_err()
    );
    let mut bool_param = param("flag", Value::Bool(false));
    bool_param.range = Some(NumericRange {
        min: Finite::new(0.).unwrap(),
        max: Finite::new(1.).unwrap(),
    });
    assert!(bool_param.validate_value(&Value::Bool(false)).is_err());
}
#[test]
fn typed_bindings_reject_missing_shape_unit_and_duplicate_wire_targets() {
    let d = base();
    let property = PropertyId::new("opposite_length").unwrap();
    let mut bad = d.clone();
    bad.objects[0].properties.clear();
    assert_eq!(
        bad.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Reference
    );
    let mut bad = d.clone();
    bad.objects[0].properties.get_mut(&property).unwrap().shape = Shape::Bool;
    assert!(bad.validate(DecodeLimits::default()).is_err());
    let mut bad = d.clone();
    bad.objects[0].properties.get_mut(&property).unwrap().unit = Unit::DIMENSIONLESS;
    assert_eq!(
        bad.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Unit
    );
    let mut raw = String::from_utf8(
        Document::decode(
            include_bytes!("fixtures/cosines.json"),
            DecodeLimits::default(),
        )
        .unwrap()
        .encode(1048576)
        .unwrap(),
    )
    .unwrap();
    raw = raw.replace(
        "\"opposite_length\":\"c\"",
        "\"opposite_length\":\"c\",\"opposite_length\":\"a\"",
    );
    assert!(Document::decode(raw.as_bytes(), DecodeLimits::default()).is_err());
    let property = PropertyId::new("value").unwrap();
    let port = Port {
        shape: Shape::Scalar,
        unit: Unit::LENGTH,
    };
    let mut d = base();
    d.objects.push(SceneObject {
        id: ObjectId::new("second").unwrap(),
        label: "Second".into(),
        properties: BTreeMap::from([(property.clone(), port)]),
        bindings: BTreeMap::from([(property, val("a"))]),
        assets: vec![],
    });
    assert!(d.validate(DecodeLimits::default()).is_ok());
}
#[test]
fn schema_budgets_authority_future_versions_and_v1_unknown_fields() {
    let mut d = base();
    d.authority = Authority::LegacyTs;
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Capability
    );
    let mut d = base();
    d.schema_version = 3;
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Schema
    );
    let mut d = base();
    d.parameters[0].metadata.enum_choices = vec!["a".repeat(129)];
    assert!(d.validate(DecodeLimits::default()).is_err());
    let d = base();
    assert_eq!(
        d.clone()
            .validate(DecodeLimits {
                max_bytes: 50,
                ..DecodeLimits::default()
            })
            .unwrap_err()
            .code,
        DiagnosticCode::Limit
    );
    assert!(
        d.clone()
            .validate(DecodeLimits {
                max_edges: 1,
                ..DecodeLimits::default()
            })
            .is_err()
    );
    assert!(
        d.clone()
            .validate(DecodeLimits {
                max_keys: 1,
                ..DecodeLimits::default()
            })
            .is_err()
    );
    let mut raw: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/cosines.json")).unwrap();
    raw["parameters"][0]["metadata"] = serde_json::json!({});
    assert!(Document::decode(&serde_json::to_vec(&raw).unwrap(), DecodeLimits::default()).is_err());
}

#[test]
fn reserved_capacity_cannot_bypass_document_storage_budget() {
    let mut d = empty();
    let mut xs = Vec::with_capacity(200000);
    xs.push(Finite::new(1.).unwrap());
    d.parameters
        .push(param("buffer", Value::Vector(std::sync::Arc::new(xs))));
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Limit
    );
    let mut d = empty();
    let mut name = String::with_capacity(2 * 1048576);
    name.push_str("a.png");
    d.assets.push(Asset {
        id: AssetId::new("large-capacity").unwrap(),
        uri: name,
    });
    assert_eq!(
        d.validate(DecodeLimits::default()).unwrap_err().code,
        DiagnosticCode::Limit
    );
}
