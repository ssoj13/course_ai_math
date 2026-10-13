//! Explicit packed GPU ABI; never cast the donor's Rust struct.
use crate::{Result, error::invalid};
use standard_surface_bsdf::SurfaceInputs;
#[cfg(not(target_arch = "wasm32"))]
use standard_surface_bsdf::{Environment, ShadingFrame};
/// Standard Surface inputs plus explicit raster coverage.
#[derive(Clone, Copy, Debug)]
pub struct Material {
    pub surface: SurfaceInputs,
    pub alpha: f32,
    pub cutoff: f32,
}
impl Default for Material {
    fn default() -> Self {
        Self {
            surface: SurfaceInputs::MATERIALX_DEFAULT,
            alpha: 1.0,
            cutoff: 0.0,
        }
    }
}
impl Material {
    /// Validate once before CPU/GPU shading. Volume transport is not silently approximated.
    pub fn validate(&self) -> Result<()> {
        self.surface
            .validate()
            .map_err(|e| invalid("material", e.to_string()))?;
        if self.surface.subsurface != 0.0 {
            return Err(invalid(
                "material.subsurface",
                "curvature-dependent raster diffusion requires curvature-enabled profile",
            ));
        }
        if self.alpha < 1.0 {
            return Err(invalid(
                "material.alpha",
                "baseline opaque profile requiresalpha1; explicit blend profile required",
            ));
        }
        if self.surface.transmission != 0.0 {
            return Err(invalid(
                "material.transmission",
                "raster refraction/interior transport unsupported; use compute profile",
            ));
        }
        if !self.alpha.is_finite()
            || !(0.0..=1.0).contains(&self.alpha)
            || !self.cutoff.is_finite()
            || !(0.0..=1.0).contains(&self.cutoff)
        {
            return Err(invalid("material.alpha", "alpha/cutoff must lie in [0,1]"));
        }
        Ok(())
    }
    /// Evaluate donor CPU BSDF at the same frame, light and environment used by the GPU.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn evaluate(
        &self,
        frame: &ShadingFrame,
        wo: [f32; 3],
        wi: [f32; 3],
        irradiance: [f32; 3],
        env: &Environment,
    ) -> Result<[f32; 3]> {
        self.validate()?;
        let light = standard_surface_bsdf::eval_light_materialx(&self.surface, frame, wo, wi);
        let ambient = standard_surface_bsdf::eval_environment(&self.surface, frame, wo, env);
        let emission = standard_surface_bsdf::eval_emission(&self.surface, frame, wo);
        Ok(std::array::from_fn(|k| {
            (light.base[k] + light.specular[k] + light.transmission[k]) * irradiance[k]
                + ambient.base[k]
                + ambient.specular[k]
                + ambient.transmission[k]
                + emission[k]
        }))
    }
}
/// Thirteen sixteen-byte slots: all49 channels, mode, raster alpha and cutoff.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MaterialGpu {
    pub slots: [[f32; 4]; 13],
}
impl MaterialGpu {
    pub const CHANNEL_NAMES: [&'static str; 49] = [
        "base",
        "base_color.r",
        "base_color.g",
        "base_color.b",
        "diffuse_roughness",
        "metalness",
        "specular",
        "specular_color.r",
        "specular_color.g",
        "specular_color.b",
        "specular_roughness",
        "specular_ior",
        "specular_anisotropy",
        "specular_rotation",
        "transmission",
        "transmission_color.r",
        "transmission_color.g",
        "transmission_color.b",
        "transmission_extra_roughness",
        "subsurface",
        "subsurface_color.r",
        "subsurface_color.g",
        "subsurface_color.b",
        "subsurface_radius.r",
        "subsurface_radius.g",
        "subsurface_radius.b",
        "subsurface_scale",
        "subsurface_anisotropy",
        "sheen",
        "sheen_color.r",
        "sheen_color.g",
        "sheen_color.b",
        "sheen_roughness",
        "coat",
        "coat_color.r",
        "coat_color.g",
        "coat_color.b",
        "coat_roughness",
        "coat_anisotropy",
        "coat_rotation",
        "coat_ior",
        "coat_affect_color",
        "coat_affect_roughness",
        "thin_film_thickness",
        "thin_film_ior",
        "emission",
        "emission_color.r",
        "emission_color.g",
        "emission_color.b",
    ];
    /// Every field is mapped explicitly, in donor declaration order.
    pub fn encode(material: &Material) -> Result<Self> {
        material.validate()?;
        let i = &material.surface;
        let channels = [
            i.base,
            i.base_color[0],
            i.base_color[1],
            i.base_color[2],
            i.diffuse_roughness,
            i.metalness,
            i.specular,
            i.specular_color[0],
            i.specular_color[1],
            i.specular_color[2],
            i.specular_roughness,
            i.specular_ior,
            i.specular_anisotropy,
            i.specular_rotation,
            i.transmission,
            i.transmission_color[0],
            i.transmission_color[1],
            i.transmission_color[2],
            i.transmission_extra_roughness,
            i.subsurface,
            i.subsurface_color[0],
            i.subsurface_color[1],
            i.subsurface_color[2],
            i.subsurface_radius[0],
            i.subsurface_radius[1],
            i.subsurface_radius[2],
            i.subsurface_scale,
            i.subsurface_anisotropy,
            i.sheen,
            i.sheen_color[0],
            i.sheen_color[1],
            i.sheen_color[2],
            i.sheen_roughness,
            i.coat,
            i.coat_color[0],
            i.coat_color[1],
            i.coat_color[2],
            i.coat_roughness,
            i.coat_anisotropy,
            i.coat_rotation,
            i.coat_ior,
            i.coat_affect_color,
            i.coat_affect_roughness,
            i.thin_film_thickness,
            i.thin_film_ior,
            i.emission,
            i.emission_color[0],
            i.emission_color[1],
            i.emission_color[2],
            i.thin_film_energy.code() as f32,
            material.alpha,
            material.cutoff,
        ];
        Ok(Self {
            slots: std::array::from_fn(|row| std::array::from_fn(|col| channels[row * 4 + col])),
        })
    }
}
/// Finite/range checked visual narrowing does not modify runtime truth.
pub fn narrow(value: f64, address: &str) -> Result<f32> {
    let out = value as f32;
    if !value.is_finite() || !out.is_finite() {
        Err(invalid(
            address,
            "value cannot be represented as finite f32",
        ))
    } else {
        Ok(out)
    }
}
