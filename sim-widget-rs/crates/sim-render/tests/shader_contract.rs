use sim_render::shader::*;
#[test]
fn donor_prefix_and_resource_signature_are_unchanged() {
    let donor = standard_surface_bsdf::wgsl_source();
    let composed = material_source();
    assert!(composed.starts_with(&donor));
    assert_eq!(&composed.as_bytes()[..donor.len()], donor.as_bytes());
    let contract = validate(&composed, "widget_vs", "widget_fs").unwrap();
    assert_eq!(
        contract.bindings.into_iter().collect::<Vec<_>>(),
        vec![(0, 0), (1, 0), (2, 0), (3, 0)]
    );
}
#[test]
fn malformed_or_fifth_group_custom_shader_is_rejected() {
    assert!(validate("@vertexfn broken", "vs", "fs").is_err());
    let source = "@group(4)@binding(0)var<uniform>x:vec4<f32>;@vertex fn vs()->@builtin(position)vec4<f32>{return x;}@fragment fn fs()->@location(0)vec4<f32>{return vec4(1.0);}";
    assert!(
        validate(source, "vs", "fs")
            .unwrap_err()
            .to_string()
            .contains("group")
    );
    assert!(validate(&material_source(), "widget_fs", "widget_vs").is_err());
}
