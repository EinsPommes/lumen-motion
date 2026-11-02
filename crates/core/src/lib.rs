#![forbid(unsafe_code)]

pub mod animation;
pub mod command;
pub mod layer_commands;
pub mod project;
pub mod scene;
pub mod time;

pub use animation::{Animation, Channel, Curve, Interpolation, Keyframe, Tangent};
pub use command::{Command, CommandError, History, SetProjectSettings};
pub use glam;
pub use layer_commands::{AddLayer, ModifyLayer, MoveLayer, RemoveLayer};
pub use project::{Project, ProjectError, ProjectSettings, PROJECT_FORMAT_VERSION};
pub use scene::{
    DrawItem, Layer, LayerId, LayerKind, LayerProps, Resolver, Scene, SceneError, StaticValues,
    Transform2D,
};
pub use time::{FrameRange, FrameRate, LoopMode, Time, TimeError};
