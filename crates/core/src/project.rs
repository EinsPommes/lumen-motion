use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::scene::{Scene, SceneError};
use crate::time::{FrameRange, FrameRate, Time, TimeError};

/// Current on-disk format version. Bump on breaking changes and add a
/// migration path in [`Project::from_ron`].
pub const PROJECT_FORMAT_VERSION: u32 = 1;

/// Largest supported canvas edge in pixels.
pub const MAX_DIMENSION: u32 = 16_384;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("invalid resolution {width}×{height}")]
    InvalidResolution { width: u32, height: u32 },
    #[error("project duration must be at least one frame")]
    InvalidDuration,
    #[error(transparent)]
    Time(#[from] TimeError),
    #[error(transparent)]
    Scene(#[from] SceneError),
    #[error("project format version {found} is not supported (max {supported})")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("failed to read project: {0}")]
    Parse(#[from] ron::error::SpannedError),
    #[error("failed to write project: {0}")]
    Serialize(#[from] ron::Error),
}

/// Global settings of a project (canvas, timing, background).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub width: u32,
    pub height: u32,
    pub frame_rate: FrameRate,
    /// Stored in seconds so a frame-rate change keeps the duration.
    pub duration: Time,
    /// Linear-light RGBA, straight alpha.
    pub background: [f32; 4],
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            frame_rate: FrameRate::FPS_30,
            duration: Time::from_secs(10),
            background: [0.012, 0.013, 0.018, 1.0],
        }
    }
}

impl ProjectSettings {
    pub fn validate(&self) -> Result<(), ProjectError> {
        let dim_ok = |v: u32| (1..=MAX_DIMENSION).contains(&v);
        if !dim_ok(self.width) || !dim_ok(self.height) {
            return Err(ProjectError::InvalidResolution {
                width: self.width,
                height: self.height,
            });
        }
        if self.duration.to_frame_ceil(self.frame_rate) < 1 {
            return Err(ProjectError::InvalidDuration);
        }
        Ok(())
    }

    /// Number of frames, rounding a partial last frame up.
    pub fn frame_count(&self) -> i64 {
        self.duration.to_frame_ceil(self.frame_rate).max(1)
    }

    pub fn frame_range(&self) -> FrameRange {
        FrameRange::new(0, self.frame_count()).unwrap_or_else(|_| FrameRange::single(0))
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }
}

/// The project document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub name: String,
    pub settings: ProjectSettings,
    #[serde(default)]
    pub scene: Scene,
}

impl Default for Project {
    fn default() -> Self {
        Self::new("Untitled")
    }
}

#[derive(Deserialize)]
#[serde(rename = "Project")]
struct VersionProbe {
    format_version: u32,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format_version: PROJECT_FORMAT_VERSION,
            name: name.into(),
            settings: ProjectSettings::default(),
            scene: Scene::default(),
        }
    }

    pub fn to_ron(&self) -> Result<String, ProjectError> {
        let cfg = ron::ser::PrettyConfig::default().struct_names(true);
        Ok(ron::ser::to_string_pretty(self, cfg)?)
    }

    pub fn from_ron(text: &str) -> Result<Self, ProjectError> {
        // Check the version first: a newer format may not parse at all.
        let probe: VersionProbe = ron::from_str(text)?;
        if probe.format_version > PROJECT_FORMAT_VERSION {
            return Err(ProjectError::UnsupportedVersion {
                found: probe.format_version,
                supported: PROJECT_FORMAT_VERSION,
            });
        }
        let project: Project = ron::from_str(text)?;
        project.settings.validate()?;
        project.scene.validate()?;
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_round_trip() {
        let mut p = Project::new("Test");
        p.settings.frame_rate = FrameRate::FPS_29_97;
        p.settings.duration = Time::new(7, 2).unwrap();
        let text = p.to_ron().unwrap();
        let back = Project::from_ron(&text).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn ron_round_trip_with_scene() {
        use crate::scene::{LayerKind, LayerProps};
        let mut p = Project::new("Scene");
        let g = p.scene.allocate_id();
        p.scene
            .insert(g, None, 0, LayerProps::new("Group", LayerKind::Group))
            .unwrap();
        let r = p.scene.allocate_id();
        let mut props = LayerProps::new(
            "Rect",
            LayerKind::Rectangle {
                size: glam::Vec2::new(100.0, 50.0),
                color: [1.0, 0.5, 0.0, 1.0],
                corner_radius: 8.0,
            },
        );
        props.transform.rotation_deg = 30.0;
        p.scene.insert(r, Some(g), 0, props).unwrap();
        let back = Project::from_ron(&p.to_ron().unwrap()).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn file_without_scene_loads_empty_scene() {
        let text = "Project(format_version: 1, name: \"Old\", settings: ProjectSettings(\
            width: 1920, height: 1080, frame_rate: FrameRate(num: 25, den: 1), \
            duration: Time(num: 10, den: 1), background: (0.0, 0.0, 0.0, 1.0)))";
        let p = Project::from_ron(text).unwrap();
        assert!(p.scene.is_empty());
    }

    #[test]
    fn newer_version_is_rejected() {
        let mut p = Project::new("Future");
        p.format_version = PROJECT_FORMAT_VERSION + 1;
        let text = p.to_ron().unwrap();
        assert!(matches!(
            Project::from_ron(&text),
            Err(ProjectError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn invalid_settings_are_rejected() {
        let mut s = ProjectSettings::default();
        assert!(s.validate().is_ok());
        s.width = 0;
        assert!(s.validate().is_err());
        s.width = 1920;
        s.duration = Time::ZERO;
        assert!(matches!(s.validate(), Err(ProjectError::InvalidDuration)));
    }

    #[test]
    fn frame_range_follows_rate() {
        let mut s = ProjectSettings::default();
        assert_eq!(s.frame_range().len(), 300);
        s.frame_rate = FrameRate::FPS_60;
        assert_eq!(s.frame_range().len(), 600);
    }
}
