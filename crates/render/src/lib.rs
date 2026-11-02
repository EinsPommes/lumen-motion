//! GPU rendering for Lumen Motion.
//!
//! The renderer composites the scene into an offscreen texture at project
//! resolution (independent of the UI). The app displays that texture; export
//! will read it back. Blend modes and the filter graph build on this.

#![forbid(unsafe_code)]

mod canvas;

pub use canvas::{CanvasRenderer, FrameParams, CANVAS_FORMAT};
pub use wgpu;
