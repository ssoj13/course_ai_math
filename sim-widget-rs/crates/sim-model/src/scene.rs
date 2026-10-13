//! Portable scene descriptions. No GPU, DOM or mathematical evaluator lives here.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const SCENE_SCHEMA_VERSION: u32 = 1;
/// A runtime-bound position or explicit authored geometric coordinate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "source",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ScenePosition {
    Literal([Finite; 3]),
    Property(PropertyId),
}
/// Transform convention: T(translation) T(pivot) R S T(-pivot), then parent world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneTransform {
    pub translation: [Finite; 3],
    pub rotation_xyzw: [Finite; 4],
    pub scale: [Finite; 3],
    pub pivot: [Finite; 3],
}
impl Default for SceneTransform {
    fn default() -> Self {
        let zero = Finite::new(0.0).unwrap();
        let one = Finite::new(1.0).unwrap();
        Self {
            translation: [zero; 3],
            rotation_xyzw: [zero, zero, zero, one],
            scale: [one; 3],
            pivot: [zero; 3],
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneUpAxis {
    Y,
    Z,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScenePrimitive {
    Point {
        position: ScenePosition,
        diameter_css: Finite,
    },
    Segment {
        a: ScenePosition,
        b: ScenePosition,
        width_css: Finite,
    },
    Arrow {
        a: ScenePosition,
        b: ScenePosition,
        width_css: Finite,
        head_css: Finite,
    },
    Arc {
        center: ScenePosition,
        normal: [Finite; 3],
        from: [Finite; 3],
        angle_radians: Finite,
        radius: Finite,
        width_css: Finite,
    },
    Polygon {
        positions: Vec<ScenePosition>,
    },
    Grid {
        extent: Finite,
        step: Finite,
        width_css: Finite,
    },
    Mesh {
        positions: Vec<[Finite; 3]>,
        triangles: Vec<[u32; 3]>,
    },
    Trajectory {
        positions: Vec<ScenePosition>,
        width_css: Finite,
    },
}
impl ScenePrimitive {
    pub fn positions(&self) -> Vec<&ScenePosition> {
        match self {
            Self::Point { position, .. } => vec![position],
            Self::Segment { a, b, .. } | Self::Arrow { a, b, .. } => vec![a, b],
            Self::Arc { center, .. } => vec![center],
            Self::Polygon { positions } | Self::Trajectory { positions, .. } => {
                positions.iter().collect()
            }
            _ => Vec::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneAlphaMode {
    Opaque,
    Mask,
    Blend,
}
/// Named MaterialX channels; defaults are resolved only by the renderer's public BSDF.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneMaterial {
    #[serde(deserialize_with = "deserialize_unique_map")]
    pub channels: BTreeMap<String, Finite>,
    pub alpha: Finite,
    pub alpha_cutoff: Finite,
    pub alpha_mode: SceneAlphaMode,
}
impl Default for SceneMaterial {
    fn default() -> Self {
        Self {
            channels: BTreeMap::new(),
            alpha: Finite::new(1.0).unwrap(),
            alpha_cutoff: Finite::new(0.5).unwrap(),
            alpha_mode: SceneAlphaMode::Opaque,
        }
    }
}
/// Semantic label in CSS pixels; text is content, never a property identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneLabel {
    pub text: String,
    pub position: ScenePosition,
    pub priority: i32,
    pub numeric_columns: Option<u16>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderObjectSpec {
    pub object: ObjectId,
    pub parent: Option<ObjectId>,
    pub transform: SceneTransform,
    pub primitive: ScenePrimitive,
    pub material: SceneMaterial,
    pub visible: bool,
    pub pickable: bool,
    pub labels: Vec<SceneLabel>,
}
/// Scene revision belongs to presentation authoring. Runtime identity/revision must match exactly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneSpec {
    pub schema_version: u32,
    pub document: DocumentId,
    pub document_revision: u64,
    pub revision: u64,
    pub up_axis: SceneUpAxis,
    pub metres_per_unit: Finite,
    pub objects: Vec<RenderObjectSpec>,
}
/// Save/load envelope remains entirely model-only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderDocument {
    pub schema_version: u32,
    pub runtime: Document,
    pub scene: SceneSpec,
}
impl SceneSpec {
    /// Validate references and hierarchy against actual immutable runtime objects.
    pub fn validate(
        &self,
        document: &DocumentId,
        revision: u64,
        objects: &[SceneObject],
        limit: usize,
    ) -> Result<()> {
        let error =
            |path: String, message: &str| Diagnostic::new(DiagnosticCode::Reference, path, message);
        if self.schema_version != SCENE_SCHEMA_VERSION {
            return Err(Diagnostic::new(
                DiagnosticCode::Schema,
                "scene.schema_version",
                "unsupported scene schema",
            ));
        }
        if &self.document != document || self.document_revision != revision {
            return Err(error(
                "scene.document".into(),
                "runtime identity/revision mismatch",
            ));
        }
        if self.objects.len() > limit {
            return Err(Diagnostic::new(
                DiagnosticCode::Limit,
                "scene.objects",
                "object budget exceeded",
            ));
        }
        if self.metres_per_unit.get() <= 0.0 {
            return Err(Diagnostic::new(
                DiagnosticCode::Range,
                "scene.metres_per_unit",
                "scale must be positive",
            ));
        }
        let runtime = objects
            .iter()
            .map(|o| (&o.id, o))
            .collect::<BTreeMap<_, _>>();
        let mut ids = BTreeSet::new();
        for object in &self.objects {
            if !ids.insert(&object.object) {
                return Err(error(
                    object.object.as_str().into(),
                    "duplicate render object",
                ));
            }
            let contract = runtime
                .get(&object.object)
                .ok_or_else(|| error(object.object.as_str().into(), "missing runtime object"))?;
            match &object.primitive {
                ScenePrimitive::Polygon { positions }
                | ScenePrimitive::Trajectory { positions, .. }
                    if positions.len() > 65536 =>
                {
                    return Err(Diagnostic::new(
                        DiagnosticCode::Limit,
                        object.object.as_str(),
                        "path point budget exceeded",
                    ));
                }
                ScenePrimitive::Mesh {
                    positions,
                    triangles,
                } if positions.len() > 1000000 || triangles.len() > 1000000 => {
                    return Err(Diagnostic::new(
                        DiagnosticCode::Limit,
                        object.object.as_str(),
                        "mesh budget exceeded",
                    ));
                }
                _ => {}
            }
            let mut positions = object.primitive.positions();
            positions.extend(object.labels.iter().map(|l| &l.position));
            for position in positions {
                if let ScenePosition::Property(id) = position {
                    let port = contract.properties.get(id).ok_or_else(|| {
                        error(
                            format!("{}/{}", object.object.as_str(), id.as_str()),
                            "missing property contract",
                        )
                    })?;
                    if port.shape != (Shape::Vector { len: 3 }) {
                        return Err(Diagnostic::new(
                            DiagnosticCode::Type,
                            format!("{}/{}", object.object.as_str(), id.as_str()),
                            "position property must be vector3",
                        ));
                    }
                    if !contract.bindings.contains_key(id) {
                        return Err(error(
                            format!("{}/{}", object.object.as_str(), id.as_str()),
                            "unbound position property",
                        ));
                    }
                }
            }
            if object.labels.len() > 256
                || object
                    .labels
                    .iter()
                    .any(|l| l.text.len() > 4096 || l.numeric_columns.is_some_and(|n| n > 128))
            {
                return Err(Diagnostic::new(
                    DiagnosticCode::Limit,
                    object.object.as_str(),
                    "label budget exceeded",
                ));
            }
            let scale = object.transform.scale.map(Finite::get);
            if scale.iter().any(|s| s.abs() < 1e-12) {
                return Err(Diagnostic::new(
                    DiagnosticCode::Range,
                    object.object.as_str(),
                    "singular local scale",
                ));
            }
            let q = object.transform.rotation_xyzw.map(Finite::get);
            let norm = q.iter().map(|n| n * n).sum::<f64>();
            if (norm - 1.0).abs() > 1e-6 {
                return Err(Diagnostic::new(
                    DiagnosticCode::Range,
                    object.object.as_str(),
                    "rotation quaternion must be unit",
                ));
            }
            let scalar = |name: &str, value: Finite| {
                if value.get() <= 0.0 {
                    Err(Diagnostic::new(
                        DiagnosticCode::Range,
                        format!("{}/{name}", object.object.as_str()),
                        "must be positive",
                    ))
                } else {
                    Ok(())
                }
            };
            match &object.primitive {
                ScenePrimitive::Point { diameter_css, .. } => scalar("diameter", *diameter_css)?,
                ScenePrimitive::Segment { width_css, .. }
                | ScenePrimitive::Trajectory { width_css, .. } => scalar("width", *width_css)?,
                ScenePrimitive::Arrow {
                    width_css,
                    head_css,
                    ..
                } => {
                    scalar("width", *width_css)?;
                    scalar("head", *head_css)?;
                }
                ScenePrimitive::Arc {
                    radius,
                    width_css,
                    normal,
                    from,
                    ..
                } => {
                    scalar("radius", *radius)?;
                    scalar("width", *width_css)?;
                    if normal
                        .iter()
                        .chain(from)
                        .map(|n| n.get().abs())
                        .sum::<f64>()
                        < 1e-12
                    {
                        return Err(error(object.object.as_str().into(), "arc directions zero"));
                    }
                }
                ScenePrimitive::Grid {
                    extent,
                    step,
                    width_css,
                } => {
                    scalar("extent", *extent)?;
                    scalar("step", *step)?;
                    scalar("width", *width_css)?;
                }
                ScenePrimitive::Polygon { positions } => {
                    if positions.len() < 3 || positions.len() > 65536 {
                        return Err(error(
                            object.object.as_str().into(),
                            "polygon must have3..65536 points",
                        ));
                    }
                }
                ScenePrimitive::Mesh {
                    positions,
                    triangles,
                } => {
                    if positions.is_empty()
                        || positions.len() > 1000000
                        || triangles.is_empty()
                        || triangles.len() > 1000000
                        || triangles
                            .iter()
                            .flatten()
                            .any(|i| *i as usize >= positions.len())
                    {
                        return Err(error(
                            object.object.as_str().into(),
                            "invalid bounded indexed mesh",
                        ));
                    }
                }
            }
        }
        for object in &self.objects {
            let mut path = BTreeSet::new();
            let mut current = Some(&object.object);
            while let Some(id) = current {
                if !path.insert(id) {
                    return Err(error(object.object.as_str().into(), "parent cycle"));
                }
                let o = self
                    .objects
                    .iter()
                    .find(|o| &o.object == id)
                    .ok_or_else(|| error(id.as_str().into(), "missing parent render object"))?;
                current = o.parent.as_ref();
            }
        }
        Ok(())
    }
}
impl RenderDocument {
    pub fn validate(self, limits: DecodeLimits) -> Result<(ValidatedDocument, SceneSpec)> {
        if self.schema_version != SCENE_SCHEMA_VERSION {
            return Err(Diagnostic::new(
                DiagnosticCode::Schema,
                "render_document.schema_version",
                "unsupported envelope",
            ));
        }
        self.scene.validate(
            &self.runtime.id,
            self.runtime.revision,
            &self.runtime.objects,
            limits.max_nodes,
        )?;
        Ok((self.runtime.validate(limits)?, self.scene))
    }
}
