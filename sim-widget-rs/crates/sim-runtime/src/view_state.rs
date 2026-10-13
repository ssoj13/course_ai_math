use serde::{Deserialize, Serialize};
use sim_model::*;
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HighlightTarget {
    Value(ValueId),
    Object(ObjectId),
    Parameter(ParameterId),
    Operation(OperationId),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraState {
    pub schema: String,
    pub fields: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewState {
    id: ViewId,
    hover: Vec<HighlightTarget>,
    selection: Vec<HighlightTarget>,
    guide: Vec<HighlightTarget>,
    camera: Option<CameraState>,
}
impl ViewState {
    pub fn new(id: ViewId) -> Self {
        Self {
            id,
            hover: vec![],
            selection: vec![],
            guide: vec![],
            camera: None,
        }
    }
    pub fn id(&self) -> &ViewId {
        &self.id
    }
    pub fn highlighted(&self) -> &[HighlightTarget] {
        if !self.hover.is_empty() {
            &self.hover
        } else if !self.selection.is_empty() {
            &self.selection
        } else {
            &self.guide
        }
    }
    fn check(ids: &Vec<HighlightTarget>) -> Result<()> {
        if ids.capacity() > 4096 {
            return Err(Diagnostic::new(
                DiagnosticCode::Limit,
                "view",
                "too many highlights",
            ));
        }
        Ok(())
    }
    pub fn set_hover(&mut self, ids: Vec<HighlightTarget>) -> Result<()> {
        Self::check(&ids)?;
        self.hover = ids;
        Ok(())
    }
    pub fn set_selection(&mut self, ids: Vec<HighlightTarget>) -> Result<()> {
        Self::check(&ids)?;
        self.selection = ids;
        Ok(())
    }
    pub fn set_guide(&mut self, ids: Vec<HighlightTarget>) -> Result<()> {
        Self::check(&ids)?;
        self.guide = ids;
        Ok(())
    }
    pub fn camera(&self) -> Option<&CameraState> {
        self.camera.as_ref()
    }
    pub fn set_camera(&mut self, camera: Option<CameraState>) -> Result<()> {
        if let Some(c) = &camera {
            if c.schema.is_empty() || c.schema.capacity() > 128 || c.fields.len() > 1024 {
                return Err(Diagnostic::new(
                    DiagnosticCode::Limit,
                    "view/camera",
                    "invalid camera schema or field count",
                ));
            }
            bounded_size(c, 65536)?;
            let mut bytes = c
                .fields
                .len()
                .checked_mul(std::mem::size_of::<(String, Value)>() + 64)
                .ok_or_else(|| {
                    Diagnostic::new(DiagnosticCode::Limit, "view/camera", "map byte overflow")
                })?;
            for (key, value) in &c.fields {
                bytes = bytes
                    .checked_add(key.capacity())
                    .and_then(|n| value.heap_bytes().ok().and_then(|m| n.checked_add(m)))
                    .filter(|n| *n <= 65536)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            DiagnosticCode::Limit,
                            "view/camera",
                            "camera retained allocation exceeds budget",
                        )
                    })?;
            }
            for v in c.fields.values() {
                v.validate()?;
            }
        }
        self.camera = camera;
        Ok(())
    }
}
