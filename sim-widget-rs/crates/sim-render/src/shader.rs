//! Public BSDF composition and validated registered shader resources.
#[cfg(not(target_arch = "wasm32"))]
use crate::{Result, error::invalid};
use std::collections::BTreeSet;
/// Donor source is preserved byte for byte as prefix; wrapper owns all resource bindings.
pub fn material_source() -> String {
    format!(
        "{}\n{}",
        standard_surface_bsdf::wgsl_source(),
        include_str!("../shaders/material.wgsl")
    )
}
/// Resource signature extracted by Naga, not by textual group substitution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShaderContract {
    pub bindings: BTreeSet<(u32, u32)>,
    pub entrypoints: BTreeSet<String>,
}
/// Validate source and require explicit entrypoints and at most four groups.
#[cfg(not(target_arch = "wasm32"))]
pub fn validate(source: &str, vertex: &str, fragment: &str) -> Result<ShaderContract> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| invalid("shader", e.emit_to_string(source)))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| invalid("shader", e.to_string()))?;
    let mut bindings = BTreeSet::new();
    for (_, v) in module.global_variables.iter() {
        if let Some(b) = v.binding.as_ref() {
            if b.group >= 4 {
                return Err(invalid("shader.binding", "bind group must be0..3"));
            }
            bindings.insert((b.group, b.binding));
        }
    }
    let entrypoints = module
        .entry_points
        .iter()
        .map(|e| e.name.clone())
        .collect::<BTreeSet<_>>();
    for (name, stage) in [
        (vertex, naga::ShaderStage::Vertex),
        (fragment, naga::ShaderStage::Fragment),
    ] {
        if !module
            .entry_points
            .iter()
            .any(|e| e.name == name && e.stage == stage)
        {
            return Err(invalid(
                format!("shader.{name}"),
                "missing entrypoint or wrong stage",
            ));
        }
    }
    Ok(ShaderContract {
        bindings,
        entrypoints,
    })
}
