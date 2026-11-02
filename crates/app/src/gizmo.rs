use lumen_core::glam::{Affine2, Vec2};
use lumen_core::{LayerId, LayerProps, Transform2D};

/// Pick radius of handles in screen pixels.
pub const HANDLE_RADIUS: f32 = 7.0;
/// Distance of the rotate handle above the top edge, in screen pixels.
pub const ROTATE_OFFSET: f32 = 28.0;
/// Rotation snapping step with Shift held.
pub const ROTATE_SNAP_DEG: f32 = 15.0;
/// Smallest allowed absolute scale.
const MIN_SCALE: f32 = 0.001;

/// Unit corners in content space: top-left, top-right, bottom-right, bottom-left.
pub const CORNERS: [Vec2; 4] = [
    Vec2::new(-0.5, -0.5),
    Vec2::new(0.5, -0.5),
    Vec2::new(0.5, 0.5),
    Vec2::new(-0.5, 0.5),
];

/// Mapping between canvas pixels and screen points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Screen position of the canvas origin (canvas centre).
    pub center: Vec2,
    /// Screen points per canvas pixel.
    pub scale: f32,
}

impl View {
    pub fn to_screen(self, p: Vec2) -> Vec2 {
        self.center + p * self.scale
    }

    pub fn to_canvas(self, s: Vec2) -> Vec2 {
        (s - self.center) / self.scale.max(1e-6)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragMode {
    Move,
    /// `corner` in content space (unit corner × content size).
    Scale { corner: Vec2 },
    Rotate,
}

/// State of an active gizmo drag.
#[derive(Clone, Debug)]
pub struct GizmoDrag {
    pub id: LayerId,
    pub mode: DragMode,
    /// Stored properties at drag start (edits are written relative to these).
    pub start_props: LayerProps,
    /// Transform and opacity evaluated at the current time at drag start.
    pub start_transform: Transform2D,
    pub start_opacity: f32,
    /// Pointer at drag start, canvas space.
    pub start_pointer: Vec2,
    /// World transform of the layer's parent at drag start.
    pub parent_world: Affine2,
}

/// Screen-space geometry of the gizmo for a layer with content.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub corners: [Vec2; 4],
    pub top_mid: Vec2,
    pub rotate_handle: Vec2,
    pub pivot: Vec2,
}

pub fn shape(world: Affine2, size: Vec2, anchor: Vec2, view: View) -> Shape {
    let corners = CORNERS.map(|c| view.to_screen(world.transform_point2(c * size)));
    let center = (corners[0] + corners[2]) * 0.5;
    let top_mid = (corners[0] + corners[1]) * 0.5;
    let dir = (top_mid - center).try_normalize().unwrap_or(Vec2::NEG_Y);
    Shape {
        corners,
        top_mid,
        rotate_handle: top_mid + dir * ROTATE_OFFSET,
        pivot: view.to_screen(world.transform_point2(anchor)),
    }
}

/// Returns the handle under `pointer` (screen space), if any.
pub fn pick_handle(shape: &Shape, size: Vec2, pointer: Vec2) -> Option<DragMode> {
    if shape.rotate_handle.distance(pointer) <= HANDLE_RADIUS {
        return Some(DragMode::Rotate);
    }
    shape
        .corners
        .iter()
        .zip(CORNERS)
        .find(|(c, _)| c.distance(pointer) <= HANDLE_RADIUS)
        .map(|(_, unit)| DragMode::Scale {
            corner: unit * size,
        })
}

/// New transform for the current pointer position (canvas space).
pub fn apply_drag(drag: &GizmoDrag, pointer: Vec2, shift: bool) -> Transform2D {
    let t = &drag.start_transform;
    match drag.mode {
        DragMode::Move => drag_move(t, drag.parent_world, drag.start_pointer, pointer, shift),
        DragMode::Scale { corner } => drag_scale(t, drag.parent_world, corner, pointer, shift),
        DragMode::Rotate => drag_rotate(t, drag.parent_world, drag.start_pointer, pointer, shift),
    }
}

/// Translates by the pointer delta (converted into parent space).
/// `constrain` locks movement to the dominant axis.
pub fn drag_move(
    t: &Transform2D,
    parent_world: Affine2,
    start: Vec2,
    pointer: Vec2,
    constrain: bool,
) -> Transform2D {
    let mut delta = pointer - start;
    if constrain {
        if delta.x.abs() >= delta.y.abs() {
            delta.y = 0.0;
        } else {
            delta.x = 0.0;
        }
    }
    let local = if parent_world.matrix2.determinant().abs() > 1e-8 {
        parent_world.inverse().transform_vector2(delta)
    } else {
        delta
    };
    Transform2D {
        position: t.position + local,
        ..t.clone()
    }
}

fn clamp_scale(v: f32) -> f32 {
    if !v.is_finite() {
        return 1.0;
    }
    if v.abs() < MIN_SCALE {
        MIN_SCALE.copysign(if v == 0.0 { 1.0 } else { v })
    } else {
        v
    }
}

/// Scales so that `corner` follows the pointer; the anchor stays fixed.
/// `uniform` keeps the aspect ratio.
pub fn drag_scale(
    t: &Transform2D,
    parent_world: Affine2,
    corner: Vec2,
    pointer: Vec2,
    uniform: bool,
) -> Transform2D {
    let frame = parent_world * t.rotation_translation();
    if frame.matrix2.determinant().abs() < 1e-8 {
        return t.clone();
    }
    // Pointer in the layer's rotated frame, relative to the pivot.
    let v = frame.inverse().transform_point2(pointer);
    // Corner offset from the pivot, unscaled.
    let c = corner - t.anchor;
    let mut scale = t.scale;
    if uniform {
        let start = c * t.scale;
        let len2 = start.length_squared();
        if len2 > 1e-8 {
            let k = v.dot(start) / len2;
            scale = t.scale * k;
        }
    } else {
        if c.x.abs() > 1e-6 {
            scale.x = v.x / c.x;
        }
        if c.y.abs() > 1e-6 {
            scale.y = v.y / c.y;
        }
    }
    Transform2D {
        scale: Vec2::new(clamp_scale(scale.x), clamp_scale(scale.y)),
        ..t.clone()
    }
}

/// Rotates around the anchor by the pointer's angular change.
pub fn drag_rotate(
    t: &Transform2D,
    parent_world: Affine2,
    start: Vec2,
    pointer: Vec2,
    snap: bool,
) -> Transform2D {
    let pivot = parent_world.transform_point2(t.position);
    let a0 = start - pivot;
    let a1 = pointer - pivot;
    if a0.length_squared() < 1e-6 || a1.length_squared() < 1e-6 {
        return t.clone();
    }
    let delta = (a1.y.atan2(a1.x) - a0.y.atan2(a0.x)).to_degrees();
    let mut rotation = t.rotation_deg + delta;
    if snap {
        rotation = (rotation / ROTATE_SNAP_DEG).round() * ROTATE_SNAP_DEG;
    }
    // Keep the value tidy in the -180..180 range.
    rotation = (rotation + 180.0).rem_euclid(360.0) - 180.0;
    Transform2D {
        rotation_deg: rotation,
        ..t.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn view_round_trip() {
        let v = View {
            center: Vec2::new(400.0, 300.0),
            scale: 0.5,
        };
        let p = Vec2::new(100.0, -40.0);
        assert!(approx(v.to_canvas(v.to_screen(p)), p));
        assert!(approx(v.to_screen(Vec2::ZERO), v.center));
    }

    #[test]
    fn move_respects_parent_scale_and_constraint() {
        let t = Transform2D::default();
        let parent = Affine2::from_scale(Vec2::splat(2.0));
        let moved = drag_move(&t, parent, Vec2::ZERO, Vec2::new(10.0, 4.0), false);
        assert!(approx(moved.position, Vec2::new(5.0, 2.0)));
        let locked = drag_move(&t, Affine2::IDENTITY, Vec2::ZERO, Vec2::new(10.0, 4.0), true);
        assert!(approx(locked.position, Vec2::new(10.0, 0.0)));
    }

    #[test]
    fn scale_follows_corner() {
        let t = Transform2D::default();
        let corner = Vec2::new(50.0, 25.0); // bottom-right of a 100×50 layer
        let s = drag_scale(&t, Affine2::IDENTITY, corner, Vec2::new(100.0, 25.0), false);
        assert!(approx(s.scale, Vec2::new(2.0, 1.0)));
        let u = drag_scale(&t, Affine2::IDENTITY, corner, Vec2::new(100.0, 50.0), true);
        assert!(approx(u.scale, Vec2::splat(2.0)));
    }

    #[test]
    fn scale_works_on_rotated_layers() {
        let t = Transform2D {
            rotation_deg: 90.0,
            ..Transform2D::default()
        };
        // Content +x points to canvas +y after rotation.
        let s = drag_scale(&t, Affine2::IDENTITY, Vec2::new(50.0, 0.0), Vec2::new(0.0, 150.0), false);
        assert!((s.scale.x - 3.0).abs() < 1e-3);
    }

    #[test]
    fn scale_never_reaches_zero() {
        let t = Transform2D::default();
        let s = drag_scale(&t, Affine2::IDENTITY, Vec2::new(50.0, 25.0), Vec2::ZERO, false);
        assert!(s.scale.x.abs() >= MIN_SCALE && s.scale.y.abs() >= MIN_SCALE);
    }

    #[test]
    fn rotate_by_quarter_turn_and_snap() {
        let t = Transform2D::default();
        let r = drag_rotate(&t, Affine2::IDENTITY, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0), false);
        assert!((r.rotation_deg - 90.0).abs() < 1e-3);
        let s = drag_rotate(&t, Affine2::IDENTITY, Vec2::new(10.0, 0.0), Vec2::new(10.0, 2.0), true);
        assert!((s.rotation_deg - 15.0).abs() < 1e-3);
    }

    #[test]
    fn handles_are_picked() {
        let view = View {
            center: Vec2::ZERO,
            scale: 1.0,
        };
        let size = Vec2::new(100.0, 50.0);
        let sh = shape(Affine2::IDENTITY, size, Vec2::ZERO, view);
        assert!(approx(sh.corners[2], Vec2::new(50.0, 25.0)));
        assert!(approx(sh.rotate_handle, Vec2::new(0.0, -25.0 - ROTATE_OFFSET)));
        assert_eq!(
            pick_handle(&sh, size, Vec2::new(51.0, 24.0)),
            Some(DragMode::Scale {
                corner: Vec2::new(50.0, 25.0)
            })
        );
        assert_eq!(pick_handle(&sh, size, sh.rotate_handle), Some(DragMode::Rotate));
        assert_eq!(pick_handle(&sh, size, Vec2::ZERO), None);
    }
}
