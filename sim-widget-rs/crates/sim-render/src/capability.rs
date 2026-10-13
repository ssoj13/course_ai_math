//! Adapter preflight: no optional extension is silently requested.
use crate::{RenderError, Result};
use serde::{Deserialize, Serialize};
/// Observed adapter profile, distinct from executed acceptance.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub name: String,
    pub backend: String,
    pub driver: String,
    pub max_bind_groups: u32,
    pub max_texture_dimension_2d: u32,
    pub max_buffer_size: u64,
    pub max_sampled_textures_per_shader_stage: u32,
    pub max_samplers_per_shader_stage: u32,
    pub timestamp_query: bool,
}
impl CapabilityReport {
    /// Inspect adapter data without claiming a successful draw.
    pub fn inspect(adapter: &wgpu::Adapter) -> Self {
        let info = adapter.get_info();
        let limits = adapter.limits();
        Self {
            name: info.name,
            backend: format!("{:?}", info.backend),
            driver: format!("{} {}", info.driver, info.driver_info),
            max_bind_groups: limits.max_bind_groups,
            max_texture_dimension_2d: limits.max_texture_dimension_2d,
            max_buffer_size: limits.max_buffer_size,
            max_sampled_textures_per_shader_stage: limits.max_sampled_textures_per_shader_stage,
            max_samplers_per_shader_stage: limits.max_samplers_per_shader_stage,
            timestamp_query: adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY),
        }
    }
    /// Baseline shaders use four uniform groups; no POLYGON_MODE_LINE/MULTIVIEW.
    pub fn validate_baseline(&self) -> Result<()> {
        if self.max_bind_groups < 4 {
            return Err(RenderError::Capability {
                name: "maxBindGroups".into(),
                required: 4,
                observed: self.max_bind_groups.into(),
            });
        }
        if self.max_buffer_size < 1024 {
            return Err(RenderError::Capability {
                name: "maxBufferSize".into(),
                required: 1024,
                observed: self.max_buffer_size,
            });
        }
        Ok(())
    }
}
/// Bound dimensions and total target bytes before allocation.
pub fn target_bytes(
    width: u32,
    height: u32,
    samples: u32,
    max_dimension: u32,
    resident: u64,
    budget: u64,
) -> Result<u64> {
    if width == 0 || height == 0 || ![1, 2, 4, 8, 16].contains(&samples) {
        return Err(crate::error::invalid(
            "target",
            "invalid dimensions or sample count",
        ));
    }
    if width > max_dimension || height > max_dimension {
        return Err(RenderError::Budget(
            "texture dimension exceeds adapter".into(),
        ));
    }
    let multisample_bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(8))
        .and_then(|n| n.checked_mul(u64::from(samples)))
        .ok_or_else(|| RenderError::Budget("target byte multiplication overflow".into()))?;
    let resolve_bytes = if samples > 1 {
        u64::from(width) * u64::from(height) * 4
    } else {
        0
    };
    let bytes = multisample_bytes
        .checked_add(resolve_bytes)
        .ok_or_else(|| RenderError::Budget("resolve bytes overflow".into()))?;
    if resident.checked_add(bytes).is_none_or(|n| n > budget) {
        return Err(RenderError::Budget(
            "total resident targets exceed budget".into(),
        ));
    }
    Ok(bytes)
}
