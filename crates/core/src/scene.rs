//! Scene graph: layers with stable IDs.
//!
//! Layers live in a map keyed by [`LayerId`]; the hierarchy is stored as
//! ordered child lists (`root` and each group's `children`). IDs are
//! allocated monotonically and never reused, so references from masks,
//! behaviours, template parameters and undo commands stay valid when layers
//! are reordered or regrouped.
//!
//! Coordinates are canvas pixels with the origin at the canvas centre and
//! +y pointing down. Child lists are ordered bottom-to-top: index 0 is drawn
//! first.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use glam::{Affine2, Vec2};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum nesting depth honoured when traversing the hierarchy.
const MAX_DEPTH: usize = 256;

/// Stable identifier of a layer. Never reused within a project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u64);

impl fmt::Display for LayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SceneError {
    #[error("layer {0} does not exist")]
    NotFound(LayerId),
    #[error("layer {0} already exists")]
    DuplicateId(LayerId),
    #[error("layer {0} is not a group")]
    NotAGroup(LayerId),
    #[error("layer {0} cannot be moved into itself or one of its descendants")]
    Cycle(LayerId),
    #[error("index {index} is out of bounds (length {len})")]
    IndexOutOfBounds { index: usize, len: usize },
    #[error("a layer cannot change between group and non-group")]
    KindChange,
    #[error("scene is inconsistent: {0}")]
    Corrupt(String),
}

/// 2D transform of a layer relative to its parent.
///
/// A content point `q` (content centred at the origin) maps to
/// `position + R(rotation) * S(scale) * (q - anchor)`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform2D {
    pub position: Vec2,
    pub scale: Vec2,
    pub rotation_deg: f32,
    /// Pivot in content coordinates (0,0 = content centre).
    pub anchor: Vec2,
}

impl Default for Transform2D {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            scale: Vec2::ONE,
            rotation_deg: 0.0,
            anchor: Vec2::ZERO,
        }
    }
}

impl Transform2D {
    pub fn to_affine(&self) -> Affine2 {
        Affine2::from_scale_angle_translation(
            self.scale,
            self.rotation_deg.to_radians(),
            self.position,
        ) * Affine2::from_translation(-self.anchor)
    }

    /// Rotation and translation only (the frame in which scale is applied).
    pub fn rotation_translation(&self) -> Affine2 {
        Affine2::from_angle_translation(self.rotation_deg.to_radians(), self.position)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LayerKind {
    Rectangle {
        size: Vec2,
        /// Linear-light RGBA, straight alpha.
        color: [f32; 4],
        corner_radius: f32,
    },
    Image {
        path: PathBuf,
        /// Natural size in pixels.
        size: Vec2,
    },
    Group,
}

impl LayerKind {
    /// Size of the drawable content, `None` for groups.
    pub fn content_size(&self) -> Option<Vec2> {
        match self {
            Self::Rectangle { size, .. } | Self::Image { size, .. } => Some(*size),
            Self::Group => None,
        }
    }

    pub fn is_group(&self) -> bool {
        matches!(self, Self::Group)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Rectangle { .. } => "Rectangle",
            Self::Image { .. } => "Image",
            Self::Group => "Group",
        }
    }
}

/// Editable properties of a layer (everything except hierarchy).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerProps {
    pub name: String,
    pub visible: bool,
    /// 0.0 ..= 1.0
    pub opacity: f32,
    pub transform: Transform2D,
    pub kind: LayerKind,
}

impl LayerProps {
    pub fn new(name: impl Into<String>, kind: LayerKind) -> Self {
        Self {
            name: name.into(),
            visible: true,
            opacity: 1.0,
            transform: Transform2D::default(),
            kind,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    id: LayerId,
    parent: Option<LayerId>,
    children: Vec<LayerId>,
    pub props: LayerProps,
}

impl Layer {
    pub fn id(&self) -> LayerId {
        self.id
    }

    pub fn parent(&self) -> Option<LayerId> {
        self.parent
    }

    /// Children bottom-to-top (empty for non-groups).
    pub fn children(&self) -> &[LayerId] {
        &self.children
    }
}

/// A removed subtree, held by undo commands to restore it later.
#[derive(Clone, Debug, PartialEq)]
pub struct RemovedSubtree {
    parent: Option<LayerId>,
    index: usize,
    /// Pre-order; `layers[0]` is the subtree root.
    layers: Vec<Layer>,
}

/// A drawable layer with its accumulated world transform and opacity.
#[derive(Clone, Copy, Debug)]
pub struct DrawItem<'a> {
    pub id: LayerId,
    /// Content space → canvas space.
    pub world: Affine2,
    pub opacity: f32,
    pub kind: &'a LayerKind,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    layers: BTreeMap<LayerId, Layer>,
    root: Vec<LayerId>,
    next_id: u64,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserves a fresh ID. IDs are never handed out twice, even after undo.
    pub fn allocate_id(&mut self) -> LayerId {
        self.next_id = self.next_id.saturating_add(1);
        LayerId(self.next_id)
    }

    pub fn len(&self) -> usize {
        self.layers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.get(&id)
    }

    pub fn contains(&self, id: LayerId) -> bool {
        self.layers.contains_key(&id)
    }

    pub fn layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers.values()
    }

    /// Top-level layers, bottom-to-top.
    pub fn root(&self) -> &[LayerId] {
        &self.root
    }

    pub fn children_of(&self, parent: Option<LayerId>) -> Result<&[LayerId], SceneError> {
        match parent {
            None => Ok(&self.root),
            Some(p) => {
                let l = self.layers.get(&p).ok_or(SceneError::NotFound(p))?;
                if l.props.kind.is_group() {
                    Ok(&l.children)
                } else {
                    Err(SceneError::NotAGroup(p))
                }
            }
        }
    }

    fn children_mut(&mut self, parent: Option<LayerId>) -> Result<&mut Vec<LayerId>, SceneError> {
        match parent {
            None => Ok(&mut self.root),
            Some(p) => {
                let l = self.layers.get_mut(&p).ok_or(SceneError::NotFound(p))?;
                if l.props.kind.is_group() {
                    Ok(&mut l.children)
                } else {
                    Err(SceneError::NotAGroup(p))
                }
            }
        }
    }

    /// Parent and index within the parent's child list.
    pub fn position_of(&self, id: LayerId) -> Result<(Option<LayerId>, usize), SceneError> {
        let l = self.layers.get(&id).ok_or(SceneError::NotFound(id))?;
        let index = self
            .children_of(l.parent)?
            .iter()
            .position(|&c| c == id)
            .ok_or_else(|| SceneError::Corrupt(format!("{id} missing from its parent")))?;
        Ok((l.parent, index))
    }

    /// `true` if `ancestor` is a (transitive) parent of `id`.
    pub fn is_ancestor(&self, ancestor: LayerId, id: LayerId) -> bool {
        let mut cur = self.layers.get(&id).and_then(|l| l.parent);
        for _ in 0..MAX_DEPTH {
            match cur {
                Some(p) if p == ancestor => return true,
                Some(p) => cur = self.layers.get(&p).and_then(|l| l.parent),
                None => return false,
            }
        }
        false
    }

    pub(crate) fn insert(
        &mut self,
        id: LayerId,
        parent: Option<LayerId>,
        index: usize,
        props: LayerProps,
    ) -> Result<(), SceneError> {
        if self.layers.contains_key(&id) {
            return Err(SceneError::DuplicateId(id));
        }
        let siblings = self.children_mut(parent)?;
        if index > siblings.len() {
            return Err(SceneError::IndexOutOfBounds {
                index,
                len: siblings.len(),
            });
        }
        siblings.insert(index, id);
        self.layers.insert(
            id,
            Layer {
                id,
                parent,
                children: Vec::new(),
                props,
            },
        );
        self.next_id = self.next_id.max(id.0);
        Ok(())
    }

    pub(crate) fn remove_subtree(&mut self, id: LayerId) -> Result<RemovedSubtree, SceneError> {
        let (parent, index) = self.position_of(id)?;
        let mut order = Vec::new();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            order.push(cur);
            if let Some(l) = self.layers.get(&cur) {
                stack.extend(l.children.iter().rev());
            }
        }
        self.children_mut(parent)?.remove(index);
        let layers = order.iter().filter_map(|i| self.layers.remove(i)).collect();
        Ok(RemovedSubtree {
            parent,
            index,
            layers,
        })
    }

    pub(crate) fn restore_subtree(&mut self, removed: RemovedSubtree) -> Result<(), SceneError> {
        let root_id = removed
            .layers
            .first()
            .map(|l| l.id)
            .ok_or_else(|| SceneError::Corrupt("empty subtree".to_owned()))?;
        if let Some(l) = removed.layers.iter().find(|l| self.layers.contains_key(&l.id)) {
            return Err(SceneError::DuplicateId(l.id));
        }
        let siblings = self.children_mut(removed.parent)?;
        if removed.index > siblings.len() {
            return Err(SceneError::IndexOutOfBounds {
                index: removed.index,
                len: siblings.len(),
            });
        }
        siblings.insert(removed.index, root_id);
        for l in removed.layers {
            self.next_id = self.next_id.max(l.id.0);
            self.layers.insert(l.id, l);
        }
        Ok(())
    }

    /// Moves `id` to `new_parent` at `new_index`; the index refers to the
    /// target list *after* `id` has been removed from its old position.
    pub(crate) fn move_layer(
        &mut self,
        id: LayerId,
        new_parent: Option<LayerId>,
        new_index: usize,
    ) -> Result<(), SceneError> {
        if let Some(p) = new_parent {
            if p == id || self.is_ancestor(id, p) {
                return Err(SceneError::Cycle(id));
            }
        }
        let (old_parent, old_index) = self.position_of(id)?;
        let target_len = self.children_of(new_parent)?.len();
        let len_after = if old_parent == new_parent {
            target_len.saturating_sub(1)
        } else {
            target_len
        };
        if new_index > len_after {
            return Err(SceneError::IndexOutOfBounds {
                index: new_index,
                len: len_after,
            });
        }
        self.children_mut(old_parent)?.remove(old_index);
        self.children_mut(new_parent)?.insert(new_index, id);
        if let Some(l) = self.layers.get_mut(&id) {
            l.parent = new_parent;
        }
        Ok(())
    }

    /// Replaces a layer's properties and returns the previous ones.
    pub(crate) fn replace_props(
        &mut self,
        id: LayerId,
        props: LayerProps,
    ) -> Result<LayerProps, SceneError> {
        let l = self.layers.get_mut(&id).ok_or(SceneError::NotFound(id))?;
        if l.props.kind.is_group() != props.kind.is_group() {
            return Err(SceneError::KindChange);
        }
        Ok(std::mem::replace(&mut l.props, props))
    }

    /// Content space → canvas space for `id`.
    pub fn world_transform(&self, id: LayerId) -> Option<Affine2> {
        let layer = self.layers.get(&id)?;
        let mut m = layer.props.transform.to_affine();
        let mut cur = layer.parent;
        for _ in 0..MAX_DEPTH {
            let Some(p) = cur else { break };
            let l = self.layers.get(&p)?;
            m = l.props.transform.to_affine() * m;
            cur = l.parent;
        }
        Some(m)
    }

    /// World transform of the parent (identity for top-level layers).
    pub fn parent_world(&self, id: LayerId) -> Affine2 {
        self.layers
            .get(&id)
            .and_then(|l| l.parent)
            .and_then(|p| self.world_transform(p))
            .unwrap_or(Affine2::IDENTITY)
    }

    /// Visible drawable layers in draw order (bottom first).
    pub fn flatten(&self) -> Vec<DrawItem<'_>> {
        let mut out = Vec::new();
        self.flatten_into(&self.root, Affine2::IDENTITY, 1.0, 0, &mut out);
        out
    }

    fn flatten_into<'a>(
        &'a self,
        ids: &[LayerId],
        parent: Affine2,
        parent_opacity: f32,
        depth: usize,
        out: &mut Vec<DrawItem<'a>>,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        for id in ids {
            let Some(l) = self.layers.get(id) else {
                continue;
            };
            if !l.props.visible {
                continue;
            }
            let world = parent * l.props.transform.to_affine();
            let opacity = parent_opacity * l.props.opacity.clamp(0.0, 1.0);
            if l.props.kind.is_group() {
                self.flatten_into(&l.children, world, opacity, depth + 1, out);
            } else {
                out.push(DrawItem {
                    id: *id,
                    world,
                    opacity,
                    kind: &l.props.kind,
                });
            }
        }
    }

    /// Topmost visible layer whose content contains canvas point `p`.
    pub fn hit_test(&self, p: Vec2) -> Option<LayerId> {
        self.flatten()
            .iter()
            .rev()
            .find(|item| {
                let Some(size) = item.kind.content_size() else {
                    return false;
                };
                if item.world.matrix2.determinant().abs() < 1e-8 {
                    return false;
                }
                let q = item.world.inverse().transform_point2(p);
                q.x.abs() <= size.x * 0.5 && q.y.abs() <= size.y * 0.5
            })
            .map(|item| item.id)
    }

    /// Layers for a list view: topmost first, depth-first, with depth.
    pub fn display_order(&self) -> Vec<(LayerId, usize)> {
        let mut out = Vec::new();
        self.display_into(&self.root, 0, &mut out);
        out
    }

    fn display_into(&self, ids: &[LayerId], depth: usize, out: &mut Vec<(LayerId, usize)>) {
        if depth > MAX_DEPTH {
            return;
        }
        for id in ids.iter().rev() {
            out.push((*id, depth));
            if let Some(l) = self.layers.get(id) {
                self.display_into(&l.children, depth + 1, out);
            }
        }
    }

    /// Checks structural consistency (used after loading a file).
    pub fn validate(&self) -> Result<(), SceneError> {
        let corrupt = |m: String| Err(SceneError::Corrupt(m));
        let mut seen = BTreeSet::new();
        let mut stack: Vec<(Option<LayerId>, &[LayerId], usize)> = vec![(None, &self.root, 0)];
        while let Some((parent, list, depth)) = stack.pop() {
            if depth > MAX_DEPTH {
                return corrupt("hierarchy too deep".to_owned());
            }
            for &id in list {
                let Some(l) = self.layers.get(&id) else {
                    return corrupt(format!("{id} is listed but does not exist"));
                };
                if l.id != id {
                    return corrupt(format!("{id} stored under the wrong key"));
                }
                if l.parent != parent {
                    return corrupt(format!("{id} has a wrong parent link"));
                }
                if !seen.insert(id) {
                    return corrupt(format!("{id} is listed more than once"));
                }
                if !l.props.kind.is_group() && !l.children.is_empty() {
                    return corrupt(format!("{id} has children but is not a group"));
                }
                if id.0 > self.next_id {
                    return corrupt(format!("{id} is above the ID counter"));
                }
                stack.push((Some(id), &l.children, depth + 1));
            }
        }
        if seen.len() != self.layers.len() {
            return corrupt("scene contains unreachable layers".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(name: &str, w: f32, h: f32) -> LayerProps {
        LayerProps::new(
            name,
            LayerKind::Rectangle {
                size: Vec2::new(w, h),
                color: [1.0; 4],
                corner_radius: 0.0,
            },
        )
    }

    fn group(name: &str) -> LayerProps {
        LayerProps::new(name, LayerKind::Group)
    }

    fn add(scene: &mut Scene, parent: Option<LayerId>, props: LayerProps) -> LayerId {
        let id = scene.allocate_id();
        let index = scene.children_of(parent).unwrap().len();
        scene.insert(id, parent, index, props).unwrap();
        id
    }

    #[test]
    fn ids_are_unique_and_never_reused() {
        let mut s = Scene::new();
        let a = add(&mut s, None, rect("a", 1.0, 1.0));
        let removed = s.remove_subtree(a).unwrap();
        let b = s.allocate_id();
        assert_ne!(a, b);
        s.restore_subtree(removed).unwrap();
        assert!(s.contains(a));
        s.validate().unwrap();
    }

    #[test]
    fn insert_validates_parent_and_index() {
        let mut s = Scene::new();
        let r = add(&mut s, None, rect("r", 1.0, 1.0));
        let id = s.allocate_id();
        assert_eq!(s.insert(id, Some(r), 0, rect("x", 1.0, 1.0)), Err(SceneError::NotAGroup(r)));
        assert!(matches!(
            s.insert(id, None, 5, rect("x", 1.0, 1.0)),
            Err(SceneError::IndexOutOfBounds { .. })
        ));
        assert_eq!(s.insert(r, None, 0, rect("x", 1.0, 1.0)), Err(SceneError::DuplicateId(r)));
    }

    #[test]
    fn remove_and_restore_subtree_keeps_order() {
        let mut s = Scene::new();
        let a = add(&mut s, None, rect("a", 1.0, 1.0));
        let g = add(&mut s, None, group("g"));
        let c1 = add(&mut s, Some(g), rect("c1", 1.0, 1.0));
        let c2 = add(&mut s, Some(g), rect("c2", 1.0, 1.0));
        let b = add(&mut s, None, rect("b", 1.0, 1.0));
        let before = s.clone();

        let removed = s.remove_subtree(g).unwrap();
        assert_eq!(s.root(), &[a, b]);
        assert_eq!(s.len(), 2);
        assert!(!s.contains(c1) && !s.contains(c2));

        s.restore_subtree(removed).unwrap();
        assert_eq!(s, before);
        s.validate().unwrap();
    }

    #[test]
    fn move_within_and_between_parents() {
        let mut s = Scene::new();
        let a = add(&mut s, None, rect("a", 1.0, 1.0));
        let b = add(&mut s, None, rect("b", 1.0, 1.0));
        let g = add(&mut s, None, group("g"));
        s.move_layer(a, None, 1).unwrap();
        assert_eq!(s.root(), &[b, a, g]);
        s.move_layer(a, Some(g), 0).unwrap();
        assert_eq!(s.root(), &[b, g]);
        assert_eq!(s.layer(g).unwrap().children(), &[a]);
        assert_eq!(s.layer(a).unwrap().parent(), Some(g));
        s.validate().unwrap();
    }

    #[test]
    fn move_rejects_cycles() {
        let mut s = Scene::new();
        let g = add(&mut s, None, group("g"));
        let inner = add(&mut s, Some(g), group("inner"));
        assert_eq!(s.move_layer(g, Some(inner), 0), Err(SceneError::Cycle(g)));
        assert_eq!(s.move_layer(g, Some(g), 0), Err(SceneError::Cycle(g)));
        assert!(s.is_ancestor(g, inner));
        assert!(!s.is_ancestor(inner, g));
    }

    #[test]
    fn replace_props_cannot_change_group_ness() {
        let mut s = Scene::new();
        let g = add(&mut s, None, group("g"));
        assert_eq!(s.replace_props(g, rect("r", 1.0, 1.0)), Err(SceneError::KindChange));
    }

    #[test]
    fn world_transform_composes_parents() {
        let mut s = Scene::new();
        let mut gp = group("g");
        gp.transform.position = Vec2::new(100.0, 0.0);
        gp.transform.scale = Vec2::splat(2.0);
        let g = add(&mut s, None, gp);
        let mut cp = rect("c", 10.0, 10.0);
        cp.transform.position = Vec2::new(5.0, 5.0);
        let c = add(&mut s, Some(g), cp);
        let w = s.world_transform(c).unwrap();
        let p = w.transform_point2(Vec2::ZERO);
        assert!((p - Vec2::new(110.0, 10.0)).length() < 1e-4);
        assert!((s.parent_world(c).transform_point2(Vec2::ZERO) - Vec2::new(100.0, 0.0)).length() < 1e-4);
    }

    #[test]
    fn transform_rotation_and_anchor() {
        let t = Transform2D {
            position: Vec2::new(10.0, 0.0),
            scale: Vec2::ONE,
            rotation_deg: 90.0,
            anchor: Vec2::new(5.0, 0.0),
        };
        // The anchor point maps to the position.
        let m = t.to_affine();
        assert!((m.transform_point2(Vec2::new(5.0, 0.0)) - Vec2::new(10.0, 0.0)).length() < 1e-4);
        // +x in content rotates to +y (y down → clockwise on screen).
        assert!((m.transform_point2(Vec2::new(6.0, 0.0)) - Vec2::new(10.0, 1.0)).length() < 1e-4);
    }

    #[test]
    fn flatten_skips_hidden_and_multiplies_opacity() {
        let mut s = Scene::new();
        let mut gp = group("g");
        gp.opacity = 0.5;
        let g = add(&mut s, None, gp);
        let mut cp = rect("c", 1.0, 1.0);
        cp.opacity = 0.5;
        let c = add(&mut s, Some(g), cp);
        let mut hp = rect("hidden", 1.0, 1.0);
        hp.visible = false;
        add(&mut s, None, hp);
        let top = add(&mut s, None, rect("top", 1.0, 1.0));

        let items = s.flatten();
        let ids: Vec<_> = items.iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![c, top]);
        assert!((items[0].opacity - 0.25).abs() < 1e-6);
    }

    #[test]
    fn hit_test_picks_topmost() {
        let mut s = Scene::new();
        let bottom = add(&mut s, None, rect("bottom", 100.0, 100.0));
        let mut tp = rect("top", 20.0, 20.0);
        tp.transform.position = Vec2::new(30.0, 0.0);
        let top = add(&mut s, None, tp);
        assert_eq!(s.hit_test(Vec2::new(30.0, 5.0)), Some(top));
        assert_eq!(s.hit_test(Vec2::new(-30.0, 0.0)), Some(bottom));
        assert_eq!(s.hit_test(Vec2::new(500.0, 0.0)), None);
    }

    #[test]
    fn display_order_is_topmost_first() {
        let mut s = Scene::new();
        let a = add(&mut s, None, rect("a", 1.0, 1.0));
        let g = add(&mut s, None, group("g"));
        let c = add(&mut s, Some(g), rect("c", 1.0, 1.0));
        assert_eq!(s.display_order(), vec![(g, 0), (c, 1), (a, 0)]);
    }

    #[test]
    fn validate_detects_corruption() {
        let mut s = Scene::new();
        let a = add(&mut s, None, rect("a", 1.0, 1.0));
        s.root.push(a);
        assert!(s.validate().is_err());
        s.root.pop();
        s.next_id = 0;
        assert!(s.validate().is_err());
    }
}
