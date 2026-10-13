//! Checked RH y-up transform DAG with explicit pivot and normal conventions.
use crate::{Result, error::invalid};
use glam::{DMat4, DQuat, DVec3};
use sim_model::{Finite, ObjectId, SceneSpec, SceneTransform, SceneUpAxis};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldTransform {
    pub world: DMat4,
    pub normal: DMat4,
    pub negative_winding: bool,
}
pub fn local(transform: &SceneTransform) -> Result<DMat4> {
    let t = DVec3::from_array(transform.translation.map(Finite::get));
    let pivot = DVec3::from_array(transform.pivot.map(Finite::get));
    let scale = DVec3::from_array(transform.scale.map(Finite::get));
    let q = DQuat::from_array(transform.rotation_xyzw.map(Finite::get));
    if scale.abs().min_element() < 1e-12 || (q.length_squared() - 1.0).abs() > 1e-6 {
        return Err(invalid("transform", "singular scale or nonunit rotation"));
    }
    let matrix = DMat4::from_translation(t)
        * DMat4::from_translation(pivot)
        * DMat4::from_quat(q)
        * DMat4::from_scale(scale)
        * DMat4::from_translation(-pivot);
    if !matrix.is_finite() {
        return Err(invalid("transform", "matrix overflow"));
    }
    Ok(matrix)
}
pub fn hierarchy(scene: &SceneSpec) -> Result<BTreeMap<ObjectId, WorldTransform>> {
    let source = scene
        .objects
        .iter()
        .map(|o| (&o.object, o))
        .collect::<BTreeMap<_, _>>();
    let axis = match scene.up_axis {
        SceneUpAxis::Y => DMat4::IDENTITY,
        SceneUpAxis::Z => DMat4::from_rotation_x(-std::f64::consts::FRAC_PI_2),
    };
    let root = axis * DMat4::from_scale(DVec3::splat(scene.metres_per_unit.get()));
    let mut output = BTreeMap::new();
    for object in &scene.objects {
        let mut path = Vec::new();
        let mut id = Some(&object.object);
        while let Some(current) = id {
            if path
                .iter()
                .any(|o: &&sim_model::RenderObjectSpec| &o.object == current)
            {
                return Err(invalid(current.as_str(), "parent cycle"));
            }
            if path.len() > 64 {
                return Err(invalid(current.as_str(), "hierarchy depth exceeds64"));
            }
            let o = source
                .get(current)
                .ok_or_else(|| invalid(current.as_str(), "missing parent"))?;
            path.push(*o);
            id = o.parent.as_ref();
        }
        let mut world = root;
        for o in path.into_iter().rev() {
            world *= local(&o.transform)?;
        }
        if !world.is_finite() || world.determinant().abs() < 1e-15 {
            return Err(invalid(
                object.object.as_str(),
                "singular or nonfinite world",
            ));
        }
        output.insert(
            object.object.clone(),
            WorldTransform {
                world,
                normal: world.inverse().transpose(),
                negative_winding: world.determinant() < 0.0,
            },
        );
    }
    Ok(output)
}
