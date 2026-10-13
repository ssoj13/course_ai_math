//! Encode / on-disk format for a clipboard slot.

use serde::{Deserialize, Serialize};

/// How a clip slot is encoded on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipFormat {
    Jpeg { quality: u8 },
    Png,
    Exr,
}

impl ClipFormat {
    pub fn jpeg(quality: u8) -> Self {
        Self::Jpeg {
            quality: quality.clamp(1, 100),
        }
    }

    /// File extension without the dot (`jpg` / `png` / `exr`).
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg { .. } => "jpg",
            Self::Png => "png",
            Self::Exr => "exr",
        }
    }

    pub fn is_exr(self) -> bool {
        matches!(self, Self::Exr)
    }

    /// True when fal / typical AI providers can ingest this format as a still.
    pub fn is_ai_friendly(self) -> bool {
        matches!(self, Self::Jpeg { .. } | Self::Png)
    }
}

impl Default for ClipFormat {
    fn default() -> Self {
        Self::jpeg(90)
    }
}
