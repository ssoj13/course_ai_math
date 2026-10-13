# cam-viewport

GPU-agnostic viewport math: pixel ↔ NDC ↔ ray conversions for a right-handed WebGPU
clip space. No wgpu dependency — just `glam`. Copied from `gitnexus-rs`'s cam-viewport.

The foundation `cam-controls` builds on; kept separate so the pure coordinate math can
be reused without pulling in the camera rigs.
