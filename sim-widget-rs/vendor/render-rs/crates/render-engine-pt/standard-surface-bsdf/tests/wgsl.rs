//! Structural tests of the WGSL twin: naga validation, constant bits, the twin-name registry
//! and the naming rules that let consumers prepend the library to their own shaders.

use std::collections::BTreeSet;

use standard_surface_bsdf::consts::{WGSL_CONSTANTS, WgslConst, wgsl_header};
use standard_surface_bsdf::wgsl::{TWIN_FUNCTIONS, WGSL_FILES, wgsl_source};

fn parse(source: &str) -> naga::Module {
    match naga::front::wgsl::parse_str(source) {
        Ok(module) => module,
        Err(e) => panic!("WGSL parse error:\n{}", e.emit_to_string(source)),
    }
}

/// Every WGSL function name declared at module scope (`fn name(`), in source order.
fn wgsl_fn_names(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| line.strip_prefix("fn "))
        .map(|rest| rest.split('(').next().unwrap_or_default().trim().to_owned())
        .collect()
}

/// Every `pub fn`/`pub const fn` name in a Rust source file.
fn rust_fn_names(source: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim_start)
        .filter_map(|line| {
            line.strip_prefix("pub fn ")
                .or_else(|| line.strip_prefix("pub const fn "))
        })
        .map(|rest| {
            rest.split(['(', '<'])
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned()
        })
        .collect()
}

#[test]
fn wgsl_library_validates_with_naga() {
    let source = wgsl_source();
    let module = parse(source);
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    if let Err(e) = validator.validate(&module) {
        panic!("WGSL validation error:\n{}", e.emit_to_string(source));
    }
    let registered: usize = TWIN_FUNCTIONS.iter().map(|(_, names)| names.len()).sum();
    assert_eq!(module.functions.len(), registered, "naga function count");
}

/// Recursively reads the scalar bits of a constant initialiser.
fn const_bits(module: &naga::Module, expr: naga::Handle<naga::Expression>) -> Vec<u32> {
    match &module.global_expressions[expr] {
        naga::Expression::Literal(naga::Literal::F32(v)) => vec![v.to_bits()],
        naga::Expression::Literal(naga::Literal::I32(v)) => vec![*v as u32],
        naga::Expression::Literal(naga::Literal::U32(v)) => vec![*v],
        naga::Expression::Compose { components, .. } => components
            .iter()
            .flat_map(|c| const_bits(module, *c))
            .collect(),
        naga::Expression::Unary {
            op: naga::UnaryOperator::Negate,
            expr,
        } => const_bits(module, *expr)
            .into_iter()
            .map(|b| b ^ 0x8000_0000)
            .collect(),
        other => panic!("unexpected constant expression {other:?}"),
    }
}

#[test]
fn constants_have_identical_bits_in_wgsl() {
    let header = wgsl_header();
    let module = parse(&header);
    assert_eq!(module.constants.len(), WGSL_CONSTANTS.len());
    for (name, value) in WGSL_CONSTANTS {
        let (_, constant) = module
            .constants
            .iter()
            .find(|(_, c)| c.name.as_deref() == Some(*name))
            .unwrap_or_else(|| panic!("constant {name} missing from the header"));
        let expected: Vec<u32> = match *value {
            WgslConst::F32(v) => vec![v.to_bits()],
            WgslConst::I32(v) => vec![v as u32],
            WgslConst::U32(v) => vec![v],
            WgslConst::Vec2(v) => v.iter().map(|x| x.to_bits()).collect(),
            WgslConst::Vec3(v) => v.iter().map(|x| x.to_bits()).collect(),
            WgslConst::Vec4(v) => v.iter().map(|x| x.to_bits()).collect(),
        };
        assert_eq!(
            const_bits(&module, constant.init),
            expected,
            "constant {name}"
        );
    }
}

#[test]
fn twin_registry_matches_wgsl_and_rust() {
    let rust_sources = [
        ("math", include_str!("../src/math.rs")),
        ("fresnel", include_str!("../src/fresnel.rs")),
        ("microfacet", include_str!("../src/microfacet.rs")),
        ("diffuse", include_str!("../src/diffuse.rs")),
        ("sheen", include_str!("../src/sheen.rs")),
        ("surface", include_str!("../src/surface.rs")),
        ("sampling", include_str!("../src/sampling.rs")),
        ("transmission", include_str!("../src/transmission.rs")),
        ("sample", include_str!("../src/sample.rs")),
    ];
    // Same module order in the registry, the WGSL files and the Rust sources.
    let registry_modules: Vec<&str> = TWIN_FUNCTIONS.iter().map(|(m, _)| *m).collect();
    let wgsl_modules: Vec<&str> = WGSL_FILES.iter().map(|(m, _)| *m).collect();
    let rust_modules: Vec<&str> = rust_sources.iter().map(|(m, _)| *m).collect();
    assert_eq!(registry_modules, wgsl_modules);
    assert_eq!(rust_modules, wgsl_modules);

    // The library as a whole declares each function once, and every one is registered.
    let all: Vec<String> = wgsl_fn_names(wgsl_source());
    let unique: BTreeSet<&str> = all.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), all.len(), "duplicate WGSL function");
    let registered: usize = TWIN_FUNCTIONS.iter().map(|(_, names)| names.len()).sum();
    assert_eq!(registered, all.len(), "registry vs WGSL function count");

    // Rust-only methods (validation, accessors, enum codes) have no WGSL twin.
    const RUST_ONLY: &[&str] = &["domain", "validate", "sum", "index", "code"];
    for (((module, registry), (_, wgsl_src)), (_, rust_src)) in
        TWIN_FUNCTIONS.iter().zip(WGSL_FILES).zip(&rust_sources)
    {
        let mut sorted = registry.to_vec();
        sorted.sort_unstable();
        assert_eq!(&sorted, registry, "{module}: WGSL_TWINS must stay sorted");
        let twins: BTreeSet<String> = wgsl_fn_names(wgsl_src).into_iter().collect();
        let listed: BTreeSet<String> = registry.iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(
            twins, listed,
            "{module}: wgsl/{module}.wgsl vs {module}::WGSL_TWINS"
        );

        let rust: BTreeSet<String> = rust_fn_names(rust_src).into_iter().collect();
        // WGSL -> Rust: every WGSL function has its Rust twin in the same module.
        for name in &twins {
            let rust_name = name.strip_prefix("ss_").unwrap_or(name);
            assert!(
                rust.contains(rust_name),
                "{module}: WGSL `{name}` has no Rust `{rust_name}`"
            );
        }
        // Rust -> WGSL: every Rust function is a twin, except the builtin stand-ins of
        // math.rs and the Rust-only methods.
        for name in &rust {
            let twin = if name.starts_with("mx_") {
                name.clone()
            } else {
                format!("ss_{name}")
            };
            let builtin_stand_in = *module == "math" && !name.starts_with("mx_");
            assert!(
                builtin_stand_in || RUST_ONLY.contains(&name.as_str()) || twins.contains(&twin),
                "{module}: Rust `{name}` has no WGSL twin `{twin}`"
            );
        }
    }
}

#[test]
fn wgsl_names_are_prefixed_and_distance_is_never_called() {
    let source = wgsl_source();
    let module = parse(source);
    for (_, function) in module.functions.iter() {
        let name = function.name.as_deref().unwrap_or_default();
        assert!(
            name.starts_with("mx_") || name.starts_with("ss_"),
            "function `{name}`"
        );
    }
    for (_, ty) in module.types.iter() {
        if let naga::TypeInner::Struct { .. } = ty.inner {
            let name = ty.name.as_deref().unwrap_or_default();
            assert!(
                name.starts_with("Mx") || name.starts_with("Ss"),
                "struct `{name}`"
            );
        }
    }
    for (_, constant) in module.constants.iter() {
        let name = constant.name.as_deref().unwrap_or_default();
        assert!(
            name.starts_with("MX_") || name.starts_with("SS_"),
            "const `{name}`"
        );
    }
    assert!(
        module.entry_points.is_empty(),
        "the library has no entry point"
    );
    assert!(
        module.global_variables.is_empty(),
        "the library has no bindings"
    );
    assert!(
        !source.contains("distance("),
        "the library never calls the builtin distance"
    );
}
