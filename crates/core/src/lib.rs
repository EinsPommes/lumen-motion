//! Core data model of Lumen Motion.
//!
//! * [`time`]: rational time, frame rates, timecode and playback stepping.
//! * [`scene`]: scene graph with stable layer IDs.
//! * [`project`]: versioned, serialisable project document.
//! * [`command`] / [`layer_commands`]: undo/redo via the command pattern.

#![forbid(unsafe_code)]

pub mod command;
pub mod layer_commands;
pub mod project;
pub mod scene;
pub mod time;

pub use command::{Command, CommandError, History, SetProjectSettings};
pub use glam;
pub use layer_commands::{AddLayer, ModifyLayer, MoveLayer, RemoveLayer};
pub use project::{Project, ProjectError, ProjectSettings, PROJECT_FORMAT_VERSION};
pub use scene::{
    DrawItem, Layer, LayerId, LayerKind, LayerProps, Scene, SceneError, Transform2D,
};
pub use time::{FrameRange, FrameRate, LoopMode, Time, TimeError};
