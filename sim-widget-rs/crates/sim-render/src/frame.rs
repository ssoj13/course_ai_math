//! Immutable frame bridge and full-stamp asynchronous freshness.
use crate::{
    RenderError, Result,
    error::invalid,
    geometry::{CpuGeometry, compile},
    hierarchy::{WorldTransform, hierarchy},
};
use glam::DVec3;
use sim_model::*;
use sim_runtime::{FrameSnapshot, PresentationFrame};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameStamp {
    pub generation: GenerationId,
    pub document: DocumentId,
    pub session: SessionId,
    pub epoch: u64,
    pub document_revision: u64,
    pub revision: u64,
}
impl FrameStamp {
    pub fn from_frame(frame: &FrameSnapshot) -> Self {
        Self {
            generation: frame.generation().clone(),
            document: frame.document_id().clone(),
            session: frame.session_id().clone(),
            epoch: frame.epoch(),
            document_revision: frame.document_revision(),
            revision: frame.revision(),
        }
    }
}
/// Presentation may be interpolated but must belong to this exact target publication.
pub struct StampedPresentation<'a> {
    pub stamp: FrameStamp,
    pub frame: &'a PresentationFrame,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedObject {
    pub id: ObjectId,
    pub geometry: CpuGeometry,
    pub transform: WorldTransform,
    pub visible: bool,
    pub pickable: bool,
    pub source_values: BTreeSet<ValueId>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedScene {
    pub stamp: FrameStamp,
    pub scene_revision: u64,
    pub objects: BTreeMap<ObjectId, PreparedObject>,
    pub full_resync: bool,
    pub dirty_geometry: BTreeSet<ObjectId>,
    pub removed: BTreeSet<ObjectId>,
    pub metadata_dirty: bool,
}
/// Candidate compilation is atomic; invalid input leaves current untouched.
#[derive(Default)]
pub struct SceneCompiler {
    current: Option<PreparedScene>,
}
impl SceneCompiler {
    pub fn current(&self) -> Option<&PreparedScene> {
        self.current.as_ref()
    }
    pub fn compile(
        &mut self,
        frame: &FrameSnapshot,
        scene: &SceneSpec,
        presentation: Option<StampedPresentation<'_>>,
    ) -> Result<&PreparedScene> {
        let stamp = FrameStamp::from_frame(frame);
        if let Some(p) = &presentation {
            if p.stamp != stamp || p.frame.target_revision() != frame.revision() {
                return Err(RenderError::Stale("foreign presentation target".into()));
            }
        }
        if let Some(previous) = &self.current {
            if previous.stamp.document == stamp.document
                && previous.stamp.session == stamp.session
                && previous.stamp.generation == stamp.generation
                && (stamp.epoch < previous.stamp.epoch
                    || stamp.epoch == previous.stamp.epoch
                        && (stamp.document_revision < previous.stamp.document_revision
                            || stamp.revision < previous.stamp.revision))
            {
                return Err(RenderError::Stale("old publication".into()));
            }
        }
        scene
            .validate(
                frame.document_id(),
                frame.document_revision(),
                frame.objects(),
                4096,
            )
            .map_err(|e| invalid("scene", e.to_string()))?;
        let transforms = hierarchy(scene)?;
        let full_resync = self.current.as_ref().is_none_or(|p| {
            p.stamp.generation != stamp.generation
                || p.stamp.document != stamp.document
                || p.stamp.session != stamp.session
                || p.stamp.epoch != stamp.epoch
                || p.stamp.revision.checked_add(1) != Some(stamp.revision)
                || p.scene_revision != scene.revision
                || frame.changes().structural
        });
        let mut objects = BTreeMap::new();
        let mut dirty_geometry = BTreeSet::new();
        for object in &scene.objects {
            let mut sources = BTreeSet::new();
            for p in object.primitive.positions() {
                if let ScenePosition::Property(property) = p {
                    let address = PropertyAddress {
                        object: object.object.clone(),
                        property: property.clone(),
                    };
                    let resolved = frame.properties().get(&address).ok_or_else(|| {
                        invalid(
                            format!("{}/{}", object.object.as_str(), property.as_str()),
                            "current frame missing resolved property",
                        )
                    })?;
                    sources.insert(resolved.source.clone());
                }
            }
            let resolve = |p: &ScenePosition| match p {
                ScenePosition::Literal(v) => Ok(DVec3::from_array(v.map(Finite::get))),
                ScenePosition::Property(property) => {
                    let address = PropertyAddress {
                        object: object.object.clone(),
                        property: property.clone(),
                    };
                    let resolved = frame.properties().get(&address).ok_or_else(|| {
                        invalid(
                            format!("{}/{}", object.object.as_str(), property.as_str()),
                            "current frame missing property",
                        )
                    })?;
                    let value = if let Some(presentation) = &presentation {
                        presentation
                            .frame
                            .shown()
                            .get(&resolved.source)
                            .ok_or_else(|| {
                                invalid(
                                    resolved.source.as_str(),
                                    "presentation missing bound value",
                                )
                            })?
                    } else {
                        &resolved.value
                    };
                    match value {
                        Value::Vector(v) if v.len() == 3 => {
                            Ok(DVec3::new(v[0].get(), v[1].get(), v[2].get()))
                        }
                        _ => Err(invalid(property.as_str(), "position must be vector3")),
                    }
                }
            };
            let candidate = PreparedObject {
                id: object.object.clone(),
                geometry: compile(&object.primitive, resolve)?,
                transform: *transforms
                    .get(&object.object)
                    .ok_or_else(|| invalid(object.object.as_str(), "missing transform"))?,
                visible: object.visible,
                pickable: object.pickable,
                source_values: sources,
            };
            if self
                .current
                .as_ref()
                .and_then(|p| p.objects.get(&object.object))
                .is_none_or(|old| {
                    old.geometry != candidate.geometry
                        || old.transform != candidate.transform
                        || old.visible != candidate.visible
                })
            {
                dirty_geometry.insert(object.object.clone());
            }
            objects.insert(object.object.clone(), candidate);
        }
        let removed = self
            .current
            .as_ref()
            .map(|p| {
                p.objects
                    .keys()
                    .filter(|id| !objects.contains_key(*id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let metadata_dirty = !frame.changes().provenance_only.is_empty()
            || !frame.changes().changed_values.is_empty();
        self.current = Some(PreparedScene {
            stamp,
            scene_revision: scene.revision,
            objects,
            full_resync,
            dirty_geometry,
            removed,
            metadata_dirty,
        });
        self.current
            .as_ref()
            .ok_or_else(|| invalid("compiler", "missing published candidate"))
    }
    pub fn reset(&mut self) {
        self.current = None;
    }
}
