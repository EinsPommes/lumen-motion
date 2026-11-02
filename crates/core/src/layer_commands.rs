use std::any::Any;

use crate::command::{Command, CommandError};
use crate::project::Project;
use crate::scene::{LayerId, LayerProps, RemovedSubtree};

/// Inserts a new layer with a pre-allocated ID.
#[derive(Debug, Clone)]
pub struct AddLayer {
    id: LayerId,
    parent: Option<LayerId>,
    index: usize,
    props: LayerProps,
}

impl AddLayer {
    pub fn new(id: LayerId, parent: Option<LayerId>, index: usize, props: LayerProps) -> Self {
        Self {
            id,
            parent,
            index,
            props,
        }
    }
}

impl Command for AddLayer {
    fn label(&self) -> String {
        format!("Add {}", self.props.kind.type_name())
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        project
            .scene
            .insert(self.id, self.parent, self.index, self.props.clone())?;
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        project.scene.remove_subtree(self.id)?;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Removes a layer including its children.
#[derive(Debug)]
pub struct RemoveLayer {
    id: LayerId,
    removed: Option<RemovedSubtree>,
}

impl RemoveLayer {
    pub fn new(id: LayerId) -> Self {
        Self { id, removed: None }
    }
}

impl Command for RemoveLayer {
    fn label(&self) -> String {
        "Delete Layer".to_owned()
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        self.removed = Some(project.scene.remove_subtree(self.id)?);
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        let removed = self
            .removed
            .take()
            .ok_or_else(|| CommandError::Invalid("nothing to restore".to_owned()))?;
        project.scene.restore_subtree(removed)?;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Replaces a layer's properties. Consecutive edits of the same layer
/// within one gesture merge into one undo step.
#[derive(Debug, Clone)]
pub struct ModifyLayer {
    id: LayerId,
    before: LayerProps,
    after: LayerProps,
}

impl ModifyLayer {
    pub fn new(id: LayerId, before: LayerProps, after: LayerProps) -> Self {
        Self { id, before, after }
    }
}

impl Command for ModifyLayer {
    fn label(&self) -> String {
        format!("Edit {}", self.after.name)
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        project.scene.replace_props(self.id, self.after.clone())?;
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        project.scene.replace_props(self.id, self.before.clone())?;
        Ok(())
    }

    fn merge(&mut self, next: &dyn Command) -> bool {
        match next.as_any().downcast_ref::<Self>() {
            Some(n) if n.id == self.id => {
                self.after = n.after.clone();
                true
            }
            _ => false,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Moves a layer within the hierarchy.
#[derive(Debug, Clone)]
pub struct MoveLayer {
    id: LayerId,
    to_parent: Option<LayerId>,
    to_index: usize,
    from: Option<(Option<LayerId>, usize)>,
}

impl MoveLayer {
    /// `to_index` refers to the target list after removing the layer.
    pub fn new(id: LayerId, to_parent: Option<LayerId>, to_index: usize) -> Self {
        Self {
            id,
            to_parent,
            to_index,
            from: None,
        }
    }
}

impl Command for MoveLayer {
    fn label(&self) -> String {
        "Move Layer".to_owned()
    }

    fn apply(&mut self, project: &mut Project) -> Result<(), CommandError> {
        let from = project.scene.position_of(self.id)?;
        project
            .scene
            .move_layer(self.id, self.to_parent, self.to_index)?;
        self.from = Some(from);
        Ok(())
    }

    fn revert(&mut self, project: &mut Project) -> Result<(), CommandError> {
        let (parent, index) = self
            .from
            .ok_or_else(|| CommandError::Invalid("move was never applied".to_owned()))?;
        project.scene.move_layer(self.id, parent, index)?;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::History;
    use crate::scene::LayerKind;
    use glam::Vec2;

    fn rect(name: &str) -> LayerProps {
        LayerProps::new(
            name,
            LayerKind::Rectangle {
                size: Vec2::splat(10.0),
                color: [1.0; 4],
                corner_radius: 0.0,
            },
        )
    }

    fn add(p: &mut Project, h: &mut History, parent: Option<LayerId>, props: LayerProps) -> LayerId {
        let id = p.scene.allocate_id();
        let index = p.scene.children_of(parent).unwrap().len();
        h.execute(p, Box::new(AddLayer::new(id, parent, index, props))).unwrap();
        h.seal();
        id
    }

    #[test]
    fn add_remove_undo_redo() {
        let mut p = Project::default();
        let mut h = History::default();
        let a = add(&mut p, &mut h, None, rect("a"));
        let b = add(&mut p, &mut h, None, rect("b"));
        let snapshot = p.clone();

        h.execute(&mut p, Box::new(RemoveLayer::new(a))).unwrap();
        assert_eq!(p.scene.root(), &[b]);
        h.undo(&mut p).unwrap();
        assert_eq!(p, snapshot);
        h.redo(&mut p).unwrap();
        assert_eq!(p.scene.root(), &[b]);
        h.undo(&mut p).unwrap();
        h.undo(&mut p).unwrap();
        h.undo(&mut p).unwrap();
        assert!(p.scene.is_empty());
        p.scene.validate().unwrap();
    }

    #[test]
    fn modify_merges_per_layer() {
        let mut p = Project::default();
        let mut h = History::default();
        let a = add(&mut p, &mut h, None, rect("a"));
        let b = add(&mut p, &mut h, None, rect("b"));
        let original = p.scene.layer(a).unwrap().props.clone();

        for x in [1.0, 2.0, 3.0] {
            let before = p.scene.layer(a).unwrap().props.clone();
            let mut after = before.clone();
            after.transform.position.x = x;
            h.execute(&mut p, Box::new(ModifyLayer::new(a, before, after))).unwrap();
        }
        assert_eq!(h.undo_len(), 3, "two adds + one merged modify");

        // A different layer in the same gesture does not merge.
        let before = p.scene.layer(b).unwrap().props.clone();
        let mut after = before.clone();
        after.opacity = 0.5;
        h.execute(&mut p, Box::new(ModifyLayer::new(b, before, after))).unwrap();
        assert_eq!(h.undo_len(), 4);

        h.undo(&mut p).unwrap();
        h.undo(&mut p).unwrap();
        assert_eq!(p.scene.layer(a).unwrap().props, original);
    }

    #[test]
    fn move_undo_restores_position() {
        let mut p = Project::default();
        let mut h = History::default();
        let a = add(&mut p, &mut h, None, rect("a"));
        let g = add(&mut p, &mut h, None, LayerProps::new("g", LayerKind::Group));
        let b = add(&mut p, &mut h, None, rect("b"));
        h.execute(&mut p, Box::new(MoveLayer::new(a, Some(g), 0))).unwrap();
        assert_eq!(p.scene.root(), &[g, b]);
        h.undo(&mut p).unwrap();
        assert_eq!(p.scene.root(), &[a, g, b]);
        assert_eq!(p.scene.layer(a).unwrap().parent(), None);
        p.scene.validate().unwrap();
    }

    #[test]
    fn invalid_move_is_not_recorded() {
        let mut p = Project::default();
        let mut h = History::default();
        let g = add(&mut p, &mut h, None, LayerProps::new("g", LayerKind::Group));
        let len = h.undo_len();
        assert!(h.execute(&mut p, Box::new(MoveLayer::new(g, Some(g), 0))).is_err());
        assert_eq!(h.undo_len(), len);
    }
}
